// Photon Shaders by SixthSurge, ported to WGSL (see LICENSE-PHOTON). program/d4a_generate_sky_sh.csh: the sky map's light
// as 9 SH coefficients per channel (+ the irradiance facing up). Appended to sky.wgsl, which provides group 0 and the helpers.
//
// Differences from the pack: one 64-thread group of 4 samples each instead of 256 threads of one (the same 256 samples; 256
// x 9 x vec3 of shared memory is 27 KB, over the 16 KB every device has to offer), and the result goes to a storage buffer
// instead of column 191 of the sky map (a texture cannot be sampled and written in one dispatch): `sh[0..9]` = coefficients,
// `sh[9]` = irradiance up, all rgb (w unused).

@group(1) @binding(0) var sky_map: texture_2d<f32>;
@group(1) @binding(1) var<storage, read_write> sh: array<vec4<f32>, 10>;

const SAMPLES: u32 = 256u;
const THREADS: u32 = 64u;
var<workgroup> partial: array<f32, 1728>; // THREADS x 9 bands x 3 channels

fn r2(n: u32) -> vec2<f32> { // low-discrepancy R2 sequence, seed 0.5
    let phi2 = 1.3247179572;
    return fract(vec2<f32>(0.5) + f32(n) * (1.0 / vec2<f32>(phi2, phi2 * phi2)));
}

fn uniform_hemisphere_up(hash: vec2<f32>) -> vec3<f32> {
    let a = hash.x * TAU;
    let y = 2.0 * hash.y - 1.0;
    let d = vec3<f32>(vec2<f32>(sin(a), cos(a)) * sqrt(1.0 - y * y), y);
    return select(d, -d, d.y < 0.0);
}

@compute @workgroup_size(64)
fn cs_sh(@builtin(local_invocation_index) t: u32) {
    let boost = get_skylight_boost();
    var acc: array<vec3<f32>, 9>;
    for (var k = 0u; k < SAMPLES / THREADS; k++) {
        let direction = uniform_hemisphere_up(r2(t + k * THREADS));
        let radiance = textureSampleLevel(sky_map, lut_samp, project_sky(direction), 0.0).rgb * boost;
        let coeff = sh_coeff_order_2(direction);
        for (var b = 0u; b < 9u; b++) { acc[b] += radiance * coeff[b] * (TAU / f32(SAMPLES)); }
    }
    for (var b = 0u; b < 9u; b++) {
        partial[t * 27u + b * 3u] = acc[b].x;
        partial[t * 27u + b * 3u + 1u] = acc[b].y;
        partial[t * 27u + b * 3u + 2u] = acc[b].z;
    }
    workgroupBarrier();
    // parallel reduction
    for (var stride = THREADS / 2u; stride > 0u; stride /= 2u) {
        if t < stride {
            for (var i = 0u; i < 27u; i++) { partial[t * 27u + i] += partial[(t + stride) * 27u + i]; }
        }
        workgroupBarrier();
    }
    if t == 0u {
        var coeffs: array<vec3<f32>, 9>;
        for (var b = 0u; b < 9u; b++) {
            coeffs[b] = vec3<f32>(partial[b * 3u], partial[b * 3u + 1u], partial[b * 3u + 2u]);
            sh[b] = vec4<f32>(coeffs[b], 0.0);
        }
        sh[9] = vec4<f32>(sh_evaluate_irradiance(coeffs, vec3<f32>(0.0, 1.0, 0.0), 1.0), 0.0);
    }
}
