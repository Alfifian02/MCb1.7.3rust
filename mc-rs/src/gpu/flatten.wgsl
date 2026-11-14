// Flat chunk draws (gpu/flat.rs): copies each chunk run's indices into one
// index list, in draw order, offset by the run's base vertex.
//
// From Starling's gfx/flatten.wgsl. Changed for mc-rs: every run indexes quads
// (the shared quad index buffer, `embeddium::quad_index`: v, v+1, v+2, v+2,
// v+3, v), so Starling's sorted-index source (`sorted`, `first`) and phase 2's
// `flatten_seen` (GPU occlusion culling) are gone, and `quad` follows the
// pattern of `embeddium::quad_index` instead of Starling's 0 1 2 0 2 3.
struct Range {
    count: u32,
    base: u32,
    // First index in the list.
    dst: u32,
}
const GROUP: u32 = 64u;
@group(0) @binding(0) var<storage, read> ranges: array<Range>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

// Index `k` of a run: quad k / 6's vertices v, v+1, v+2, v+2, v+3, v.
fn quad(k: u32) -> u32 {
    let c = k % 6u;
    return (k / 6u) * 4u + select(select(c - 1u, 0u, c == 5u), c, c < 3u);
}

@compute @workgroup_size(64)
fn flatten(@builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) lane: u32) {
    let r = group.y * 65535u + group.x;
    if r >= arrayLength(&ranges) {
        return;
    }
    let d = ranges[r];
    for (var k = lane; k < d.count; k += GROUP) {
        out[d.dst + k] = d.base + quad(k);
    }
}
