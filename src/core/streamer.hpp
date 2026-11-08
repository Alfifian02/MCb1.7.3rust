// Background chunk generation + meshing around the player. The main thread owns the World and all
// scheduling; workers get immutable shared_ptr<const Chunk> neighbourhoods, so no block data is locked.
#pragma once
#include <condition_variable>
#include <deque>
#include <mutex>
#include <thread>
#include <unordered_set>
#include <vector>
#include "mesher.hpp"
#include "world.hpp"

namespace mc {

struct MeshResult {
    int cx, cz;
    MeshData data;
};

class ChunkStreamer {
    struct Job {
        bool mesh;
        int cx, cz;
        Neighborhood nb;
    };
    struct Result {
        bool mesh;
        int cx, cz;
        ChunkPtr chunk;
        MeshData data;
    };

    World& world_;
    int64_t seed_;
    std::vector<std::thread> threads_;
    std::mutex mu_;
    std::condition_variable cv_;
    std::deque<Job> jobs_;
    std::deque<Result> results_;
    bool quit_ = false;

    std::unordered_set<int64_t> gen_pending_, mesh_pending_, meshed_;

    void worker();

public:
    ChunkStreamer(World& world, int64_t seed, int workers);
    ~ChunkStreamer();

    // Call once per frame on the main thread. Fills `new_meshes` with finished meshes and `dropped` with the
    // chunks that left the loaded area (their GPU meshes must be freed).
    void update(int pcx, int pcz, int radius, std::vector<MeshResult>& new_meshes,
                std::vector<std::pair<int, int>>& dropped);

    size_t pending() const { return gen_pending_.size() + mesh_pending_.size(); }
};

// b1.7.3-style spawn: the first column (on a spiral from the origin) whose top block is grass or sand above sea
// level. Generates chunks synchronously. Returns the box centre for the player.
struct Spawn { float x, y, z; };
Spawn find_spawn(int64_t seed);

}  // namespace mc
