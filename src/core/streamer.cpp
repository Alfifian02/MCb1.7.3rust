#include "streamer.hpp"
#include <algorithm>
#include "climate.hpp"
#include "terrain.hpp"

namespace mc {

ChunkStreamer::ChunkStreamer(World& world, int64_t seed, int workers) : world_(world), seed_(seed) {
    for (int i = 0; i < workers; i++) threads_.emplace_back([this] { worker(); });
}

ChunkStreamer::~ChunkStreamer() {
    {
        std::lock_guard<std::mutex> lk(mu_);
        quit_ = true;
    }
    cv_.notify_all();
    for (auto& t : threads_) t.join();
}

void ChunkStreamer::worker() {
    // Each worker owns its generator: it holds a Random and a column cache and is not thread-safe.
    TerrainGenerator gen(seed_);
    ClimateManager cm(seed_);
    for (;;) {
        Job job;
        {
            std::unique_lock<std::mutex> lk(mu_);
            cv_.wait(lk, [this] { return quit_ || !jobs_.empty(); });
            if (quit_) return;
            job = std::move(jobs_.front());
            jobs_.pop_front();
        }
        Result r{job.mesh, job.cx, job.cz, nullptr, {}};
        if (job.mesh) {
            r.data = mesh_chunk(job.nb);
        } else {
            auto c = std::make_shared<Chunk>();
            c->cx = job.cx;
            c->cz = job.cz;
            c->blocks = gen.generate(job.cx, job.cz, cm);
            r.chunk = std::move(c);
        }
        {
            std::lock_guard<std::mutex> lk(mu_);
            results_.push_back(std::move(r));
        }
    }
}

void ChunkStreamer::update(int pcx, int pcz, int radius, std::vector<MeshResult>& new_meshes,
                           std::vector<std::pair<int, int>>& dropped) {
    // 1. Integrate finished jobs.
    std::deque<Result> done;
    {
        std::lock_guard<std::mutex> lk(mu_);
        done.swap(results_);
    }
    auto dist2 = [&](int cx, int cz) { return (cx - pcx) * (cx - pcx) + (cz - pcz) * (cz - pcz); };
    const int keep2 = (radius + 3) * (radius + 3);
    for (auto& r : done) {
        const int64_t key = chunk_key(r.cx, r.cz);
        if (r.mesh) {
            mesh_pending_.erase(key);
            if (world_.get_chunk(r.cx, r.cz)) {  // ignore a mesh whose chunk was unloaded meanwhile
                meshed_.insert(key);
                new_meshes.push_back({r.cx, r.cz, std::move(r.data)});
            }
        } else {
            gen_pending_.erase(key);
            if (dist2(r.cx, r.cz) <= keep2) world_.chunks[key] = std::move(r.chunk);
        }
    }

    // 2. Unload what is out of range.
    for (auto it = world_.chunks.begin(); it != world_.chunks.end();) {
        const int cx = chunk_key_x(it->first), cz = chunk_key_z(it->first);
        if (dist2(cx, cz) > keep2) {
            if (meshed_.erase(it->first)) dropped.push_back({cx, cz});
            it = world_.chunks.erase(it);
        } else {
            ++it;
        }
    }

    // 3. Schedule: keep only a few jobs outstanding so priorities (nearest first) stay fresh.
    const size_t cap = threads_.size() * 3 + 2;
    if (pending() >= cap) return;
    std::vector<Job> batch;

    // Meshing first (cheap to unblock), nearest first. A mesh needs all 8 neighbours for AO and culling.
    struct Cand { int d, cx, cz; };
    std::vector<Cand> cands;
    for (int dz = -radius; dz <= radius; dz++)
        for (int dx = -radius; dx <= radius; dx++) {
            const int cx = pcx + dx, cz = pcz + dz, d = dx * dx + dz * dz;
            if (d > radius * radius) continue;
            const int64_t key = chunk_key(cx, cz);
            if (!world_.get_chunk(cx, cz) || meshed_.count(key) || mesh_pending_.count(key)) continue;
            bool ok = true;
            for (int oz = -1; oz <= 1 && ok; oz++)
                for (int ox = -1; ox <= 1 && ok; ox++) ok = world_.get_chunk(cx + ox, cz + oz) != nullptr;
            if (ok) cands.push_back({d, cx, cz});
        }
    std::sort(cands.begin(), cands.end(), [](const Cand& a, const Cand& b) { return a.d < b.d; });
    size_t budget = cap - pending();
    for (const Cand& c : cands) {
        if (budget == 0) break;
        Job j{true, c.cx, c.cz, {}};
        for (int oz = -1; oz <= 1; oz++)
            for (int ox = -1; ox <= 1; ox++) j.nb[(oz + 1) * 3 + (ox + 1)] = world_.get_chunk(c.cx + ox, c.cz + oz);
        mesh_pending_.insert(chunk_key(c.cx, c.cz));
        batch.push_back(std::move(j));
        budget--;
    }

    // Generation for the ring one chunk wider than what is drawn (the mesher needs the neighbours).
    cands.clear();
    const int gr = radius + 1;
    for (int dz = -gr; dz <= gr; dz++)
        for (int dx = -gr; dx <= gr; dx++) {
            const int cx = pcx + dx, cz = pcz + dz, d = dx * dx + dz * dz;
            if (d > gr * gr) continue;
            const int64_t key = chunk_key(cx, cz);
            if (world_.get_chunk(cx, cz) || gen_pending_.count(key)) continue;
            cands.push_back({d, cx, cz});
        }
    std::sort(cands.begin(), cands.end(), [](const Cand& a, const Cand& b) { return a.d < b.d; });
    for (const Cand& c : cands) {
        if (budget == 0) break;
        gen_pending_.insert(chunk_key(c.cx, c.cz));
        batch.push_back(Job{false, c.cx, c.cz, {}});
        budget--;
    }

    if (!batch.empty()) {
        {
            std::lock_guard<std::mutex> lk(mu_);
            for (auto& j : batch) jobs_.push_back(std::move(j));
        }
        cv_.notify_all();
    }
}

Spawn find_spawn(int64_t seed) {
    TerrainGenerator gen(seed);
    ClimateManager cm(seed);
    // Square spiral of chunks; test the centre column of each.
    int x = 0, z = 0, dx = 0, dz = -1;
    for (int i = 0; i < 40 * 40; i++) {
        auto blocks = gen.generate(x, z, cm);
        for (int bx = 4; bx < 16; bx += 8)
            for (int bz = 4; bz < 16; bz += 8) {
                int top = -1;
                for (int y = CHUNK_H - 1; y >= 0; y--)
                    if (is_solid(blocks[cell(bx, y, bz)])) { top = y; break; }
                if (top > SEA_LEVEL && (blocks[cell(bx, top, bz)] == block::GRASS || blocks[cell(bx, top, bz)] == block::SAND))
                    return {static_cast<float>(x * 16 + bx) + 0.5f, static_cast<float>(top + 1) + 0.9f + 0.01f,
                            static_cast<float>(z * 16 + bz) + 0.5f};
            }
        if (x == z || (x < 0 && x == -z) || (x > 0 && x == 1 - z)) { int t = dx; dx = -dz; dz = t; }
        x += dx;
        z += dz;
    }
    return {0.5f, 100.0f, 0.5f};
}

}  // namespace mc
