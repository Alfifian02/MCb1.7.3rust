//! Milestone 5 of the Photon rewrite: the post chain, a line-by-line port of SixthSurge's Photon (`photon-main/shaders/program`)
//! `c2_dof`, `c4_taa_exposure`, `c5_c10_bloom_downsample`, `c11_bloom_merge_buffers`, `c12/c13_bloom_gaussian`,
//! `c14_c18_bloom_upsample`, `c19_color_grading` (+ the ACES code of `include/post_processing/aces`), `c21_fxaa` and `final`.
//! Order, like `world0/composite*.fsh`: DOF -> TAA + exposure -> 6 bloom tiles down -> merge -> blur h, v -> 5 up -> grade +
//! tonemap -> FXAA -> CAS + dither onto the surface.
//! Input: the HDR scene colour (`Post::scene_view`, the volumetric light is already blended into it) and the depth buffer.
//! Settings are the `#define`s of `settings.glsl` (`Settings::default()` is the pack's default).
//! How GLSL maps to wgpu here (no change of maths):
//! - `uv` and texels are y-down in every pass (the bloom tile layout is just mirrored); NDC maths is y-up and converted at the
//!   few places that mix the two (reprojection). Depth is 0..1 (`perspective_rh`), so `ndc.z = depth`, no `* 2 - 1`.
//! - Iris' colortex0 main/alt flip is two explicit textures (`bloom`) used with the same read/write pattern, so photon's
//!   behaviour (tiles 3 and 1 read the horizontally-blurred copy as their destination) is kept bit for bit.
//! - The vertex-shader exposure (`c4.vsh`) is a 1x1 pass (`fs_exposure`) that also keeps `centerDepthSmooth` for DOF.
//! - `texelFetch` outside the texture is clamped to the edge (GLSL: undefined).
//! - Matrices of `color.glsl` / `aces/matrices.glsl` are multiplied here in f64 and written into the shader as literals.
//! Not ported (nothing in mc-rs feeds them, or off in the pack's defaults): TAAU + `c3_taau_prep` (render scale 1), motion blur
//! (`c20`), `BLOOMY_FOG` (needs the fog buffer of milestone 4), the histogram debug view, the `bloom_input` output of `c4` (dead
//! in the pack: `c5` reads the history), `MANUAL_EXPOSURE_USE_SCREEN_BRIGHTNESS`, `DISTANCE_VIEW`, `TONEMAP_COMPARISON`.
//! UNVERIFIED: not run on a device. The `lib.rs` wiring is in `render()`, the shader module is validated by a host test.

use bytemuck::{Pod, Zeroable};
use glam::{DMat3, DVec3, Mat3, Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

/// `AUTO_EXPOSURE_*`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exposure {
    Off,
    Simple,
    Histogram,
}

/// `tonemap_*` of `tonemap_operators.glsl` (`#define tonemap`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tonemap {
    AcesFit,
    AcesFull,
    Lottes,
    Hejl2015,
    HejlBurgess,
    Tech,
    Uncharted2,
    Ozius,
    Reinhard,
    ReinhardJodie,
}

impl Tonemap {
    fn func(self) -> &'static str {
        match self {
            Tonemap::AcesFit => "tonemap_aces_fit",
            Tonemap::AcesFull => "tonemap_aces_full",
            Tonemap::Lottes => "tonemap_lottes",
            Tonemap::Hejl2015 => "tonemap_hejl_2015",
            Tonemap::HejlBurgess => "tonemap_hejl_burgess",
            Tonemap::Tech => "tonemap_tech",
            Tonemap::Uncharted2 => "tonemap_uncharted_2",
            Tonemap::Ozius => "tonemap_ozius",
            Tonemap::Reinhard => "tonemap_reinhard",
            Tonemap::ReinhardJodie => "tonemap_reinhard_jodie",
        }
    }
}

/// The `#define`s of `settings.glsl` that the ported programs read. `Default` = the pack's defaults, except `tonemap`: the pack
/// ships `tonemap_lottes`, the rewrite asks for ACES (`AcesFull` = RRT + ODT; `Tonemap::Lottes` is the pack's own choice).
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub taa: bool,
    pub fxaa: bool,
    pub cas: bool,
    pub cas_intensity: f32,
    pub vignette: bool,
    pub vignette_intensity: f32,
    pub bloom: bool,
    pub bloom_intensity: f32,
    pub bloom_spread: f32,
    /// `BLOOM_UPSAMPLING_FILTER`: false = `texture`, true = `bicubic_filter`.
    pub bloom_bicubic: bool,
    pub dof: bool,
    pub dof_intensity: f32,
    pub dof_samples: i32,
    /// Blocks; negative = focus on the crosshair (`centerDepthSmooth`).
    pub dof_focus: f32,
    pub exposure: Exposure,
    pub exposure_bias: f32,
    pub exposure_min: f32,
    pub exposure_max: f32,
    pub rate_dim_to_bright: f32,
    pub rate_bright_to_dim: f32,
    pub manual_exposure: f32,
    pub histogram_bins: i32,
    pub histogram_target: f32,
    pub tonemap: Tonemap,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub white_balance: f32,
    pub orange_sat_boost: f32,
    pub teal_sat_boost: f32,
    pub green_sat_boost: f32,
    pub green_hue_shift: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            taa: true,
            fxaa: true,
            cas: true,
            cas_intensity: 0.5,
            vignette: true,
            vignette_intensity: 1.0,
            bloom: true,
            bloom_intensity: 1.0,
            bloom_spread: 1.0,
            bloom_bicubic: false,
            dof: false,
            dof_intensity: 1.0,
            dof_samples: 40,
            dof_focus: -1.0,
            exposure: Exposure::Off,
            exposure_bias: 0.0,
            exposure_min: -1.0,
            exposure_max: 0.0,
            rate_dim_to_bright: 2.0,
            rate_bright_to_dim: 1.0,
            manual_exposure: 0.0,
            histogram_bins: 32,
            histogram_target: 0.5,
            tonemap: Tonemap::AcesFull,
            brightness: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            white_balance: 6500.0,
            orange_sat_boost: 0.0,
            teal_sat_boost: 0.10,
            green_sat_boost: 0.0,
            green_hue_shift: 0.0,
        }
    }
}

/// `shaders.properties`: `taa_offset = vec2(jitter_x / viewWidth, jitter_y / viewWidth)` (y is divided by the WIDTH in the pack,
/// kept), `jitter = frac(R2 * frameCounter + 0.5) * 2 - 1`. The vertex shaders add `taa_offset * 0.66 * clip.w` (no TAAU), which
/// `jitter()` already includes. `frameCounter` wraps at 720720 like Iris'. Computed in f64 (the pack's f32 loses its fraction late).
pub fn taa_offset(frame: u32, view_width: f32) -> Vec2 {
    let n = (frame % 720720) as f64;
    let j = |a: f64| ((a * n + 0.5).fract() * 2.0 - 1.0) as f32;
    Vec2::new(j(1.3247179572) / view_width, j(1.7548776662) / view_width)
}

/// Per-frame inputs from the game. `proj` is the UNJITTERED projection (`gbufferProjection`), `view` the camera's view matrix.
pub struct Frame {
    pub view: Mat4,
    pub proj: Mat4,
    pub cam_pos: Vec3,
    /// Seconds since the last frame (`frameTime`) and since start (`frameTimeCounter`).
    pub dt: f32,
    pub time: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct U {
    proj: [[f32; 4]; 4],
    proj_inv: [[f32; 4]; 4],
    mv_inv: [[f32; 4]; 4],
    prev_mv: [[f32; 4]; 4],
    prev_proj: [[f32; 4]; 4],
    /// xyz: cameraPosition, w: frameTime.
    cam: [f32; 4],
    /// xyz: previousCameraPosition, w: frameTimeCounter.
    prev_cam: [f32; 4],
    /// xy: view_res, zw: view_pixel_size.
    res: [f32; 4],
    /// aspectRatio, frameCounter, blindness, darknessFactor.
    misc: [f32; 4],
    /// biome_cave, -, -, -.
    misc2: [f32; 4],
    /// `white_balance_matrix` columns.
    wb: [[f32; 4]; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct P {
    /// Pixels: the viewport the pass draws into (a bloom tile, or the whole target).
    rect: [f32; 4],
    /// x: bloom tile index.
    i: [i32; 4],
}

/// Dynamic-offset stride of `P`.
const P_STRIDE: u64 = 256;
/// Photon's bloom is 6 tiles (`c5` .. `c10`).
const TILES: usize = 6;
const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const EXPO: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// `bloom_tile_scale` / `bloom_tile_offset` of the bloom programs, in unit coordinates.
fn tile_rect(i: usize) -> (f32, f32, f32) {
    let s = 0.5 * (-(i as f32)).exp2();
    (1.0 - (-(i as f32)).exp2(), (i & 1) as f32 * (1.0 - 0.5 * (-(i as f32)).exp2()), s)
}

// ---------------------------------------------------------------------------------------------------------------------------
//  Constants that photon computes with GLSL const expressions: done in f64 here and written into the shader as literals.
// ---------------------------------------------------------------------------------------------------------------------------

fn m3(a: [f64; 9]) -> DMat3 {
    DMat3::from_cols_array(&a)
}

fn xyz_to_rec709() -> DMat3 {
    m3([3.2406, -1.5372, -0.4986, -0.9689, 1.8758, 0.0415, 0.0557, -0.2040, 1.0570])
}
fn xyz_to_rec2020() -> DMat3 {
    m3([1.7166084, -0.3556621, -0.2533601, -0.6666829, 1.6164776, 0.0157685, 0.0176422, -0.0427763, 0.94222867])
}
fn rec2020_to_xyz() -> DMat3 {
    m3([0.6369736, 0.1446172, 0.1688585, 0.2627066, 0.6779996, 0.0592938, 0.0000000, 0.0280728, 1.0608437])
}
fn ap0_to_xyz() -> DMat3 {
    m3([0.9525523959, 0.0, 0.0000936786, 0.3439664498, 0.7281660966, -0.0721325464, 0.0, 0.0, 1.0088251844])
}
fn xyz_to_ap0() -> DMat3 {
    m3([1.0498110175, 0.0, -0.0000974845, -0.4959030231, 1.3733130458, 0.0982400361, 0.0, 0.0, 0.9912520182])
}
fn ap1_to_xyz() -> DMat3 {
    m3([0.6624541811, 0.1340042065, 0.1561876870, 0.2722287168, 0.6740817658, 0.0536895174, -0.0055746495, 0.0040607335, 1.0103391003])
}
fn xyz_to_ap1() -> DMat3 {
    m3([1.6410233797, -0.3248032942, -0.2364246952, -0.6636628587, 1.6153315917, 0.0167563477, 0.0117218943, -0.0082844420, 0.9883948585])
}
fn d60_to_d65() -> DMat3 {
    m3([0.9872240000, -0.0061132700, 0.0159533000, -0.0075983600, 1.0018600000, 0.0053300200, 0.0030725700, -0.0050959500, 1.0816800000])
}
fn d65_to_d60() -> DMat3 {
    m3([1.0130349240, 0.0061053089, -0.0149709632, 0.0076982300, 0.9981648318, -0.0050320341, -0.0028413125, 0.0046851556, 0.9245066529])
}

/// `blackbody()` of `color.glsl` for the rec. 2020 primaries (660, 550, 440 nm).
fn blackbody(temperature: f64) -> DVec3 {
    let lambda = DVec3::new(660.0, 550.0, 440.0);
    let lambda2 = lambda * lambda;
    let lambda5 = lambda2 * lambda2 * lambda;
    let (h, k, c) = (6.63e-16, 1.38e-5, 3.0e17);
    let a = lambda5 / (2.0 * h * c * c);
    let b = (h * c) / (k * lambda);
    let d = DVec3::new((b.x / temperature).exp(), (b.y / temperature).exp(), (b.z / temperature).exp());
    let rgb = a * d - a;
    DVec3::splat(rgb.min_element()) / rgb
}

/// `get_chromatic_adaptation_matrix()` of `aces/utility.glsl` (Bradford).
fn chromatic_adaptation(src_xyz: DVec3, dst_xyz: DVec3) -> DMat3 {
    let bradford = m3([0.89510, -0.75020, 0.03890, 0.26640, 1.71350, -0.06850, -0.16140, 0.03670, 1.02960]);
    let (src_lms, dst_lms) = (rv(src_xyz, bradford), rv(dst_xyz, bradford));
    let q = dst_lms / src_lms;
    (bradford * DMat3::from_diagonal(q)) * bradford.inverse()
}

/// `c19_color_grading.vsh`: only built when `GRADE_WHITE_BALANCE != 6500`, identity otherwise (the shader skips it).
fn white_balance_matrix(wb: f32) -> DMat3 {
    if wb == 6500.0 {
        return DMat3::IDENTITY;
    }
    let to_xyz = rec2020_to_xyz();
    chromatic_adaptation(rv(blackbody(wb as f64), to_xyz), rv(blackbody(6500.0), to_xyz))
}

/// GLSL `v * m` (row vector times matrix).
fn rv(v: DVec3, m: DMat3) -> DVec3 {
    m.transpose() * v
}

fn wgsl_mat3(name: &str, m: DMat3) -> String {
    let c = |v: DVec3| format!("vec3<f32>({:?}, {:?}, {:?})", v.x as f32, v.y as f32, v.z as f32);
    format!("const {name}: mat3x3<f32> = mat3x3<f32>({}, {}, {});\n", c(m.x_axis), c(m.y_axis), c(m.z_axis))
}

/// Everything the shader needs that photon gets from `#define`s and GLSL constant folding.
fn prelude(s: &Settings, srgb_surface: bool) -> String {
    let mut o = String::new();
    let (r2020_709, r2020_ap0, ap0_ap1, ap1_r2020) =
        (rec2020_to_xyz() * xyz_to_rec709(), rec2020_to_xyz() * d65_to_d60() * xyz_to_ap0(), ap0_to_xyz() * xyz_to_ap1(), ap1_to_xyz() * d60_to_d65() * xyz_to_rec2020());
    o += &wgsl_mat3("rec2020_to_rec709", r2020_709);
    o += &wgsl_mat3("rec2020_to_xyz", rec2020_to_xyz());
    o += &wgsl_mat3("xyz_to_rec2020", xyz_to_rec2020());
    o += &wgsl_mat3("rec2020_to_ap0", r2020_ap0);
    o += &wgsl_mat3("ap0_to_ap1", ap0_ap1);
    o += &wgsl_mat3("ap1_to_xyz", ap1_to_xyz());
    o += &wgsl_mat3("xyz_to_ap1", xyz_to_ap1());
    o += &wgsl_mat3("ap1_to_rec2020", ap1_r2020);
    // tonemap_lottes: `b` and `c` are const expressions in the pack.
    let (a, d, hdr_max, mid_in, mid_out) = (1.5f64, 0.91f64, 8.0f64, 0.26f64, 0.32f64);
    let den = (hdr_max.powf(a * d) - mid_in.powf(a * d)) * mid_out;
    let b = (-mid_in.powf(a) + hdr_max.powf(a) * mid_out) / den;
    let c = (hdr_max.powf(a * d) * mid_in.powf(a) - hdr_max.powf(a) * mid_in.powf(a * d) * mid_out) / den;
    // segmented_spline_c5_fwd: `log(vec2(..)) * rcp(log(10))` params.
    let l10 = |x: f64| x.log10() as f32;
    let (min_x, min_y) = (l10(0.18 * (-15.0f64).exp2()), l10(0.0001));
    let (mid_x, mid_y) = (l10(0.18), l10(4.8));
    let (max_x, max_y) = (l10(0.18 * 18.0f64.exp2()), l10(1000.0));
    // c4.vsh exposure constants.
    let calibration = (s.exposure_bias as f64).exp2() * 12.5 / 100.0 / 1.2;
    let min_lum = calibration / (-(s.exposure_min as f64)).exp2(); // get_luminance_from_exposure(get_exposure_from_ev_100(min))
    let max_lum = calibration / (-(s.exposure_max as f64)).exp2();
    let f = |x: f64| format!("{:?}", x as f32);
    o += &format!(
        "const TAA_ON: bool = {};\nconst FXAA_ON: bool = {};\nconst CAS_ON: bool = {};\nconst VIGNETTE_ON: bool = {};\nconst BLOOM_ON: bool = {};\nconst BLOOM_BICUBIC: bool = {};\nconst SRGB_SURFACE: bool = {};\n\
         const AUTO_EXPOSURE: i32 = {};\nconst MANUAL_EV: f32 = {};\nconst CALIBRATION: f32 = {};\nconst MIN_LUMINANCE: f32 = {};\nconst MAX_LUMINANCE: f32 = {};\nconst MIN_LOG_LUMINANCE: f32 = {};\nconst MAX_LOG_LUMINANCE: f32 = {};\n\
         const RATE_DIM_TO_BRIGHT: f32 = {};\nconst RATE_BRIGHT_TO_DIM: f32 = {};\nconst HISTOGRAM_BINS: i32 = {};\nconst HISTOGRAM_TARGET: f32 = {};\n\
         const BLOOM_INTENSITY: f32 = {};\nconst BLOOM_SPREAD: f32 = {};\nconst DOF_INTENSITY: f32 = {};\nconst DOF_SAMPLES: i32 = {};\nconst DOF_FOCUS: f32 = {};\n\
         const GRADE_BRIGHTNESS: f32 = {};\nconst GRADE_CONTRAST: f32 = {};\nconst GRADE_SATURATION: f32 = {};\nconst GRADE_WB_ON: bool = {};\n\
         const GRADE_ORANGE_SAT_BOOST: f32 = {};\nconst GRADE_TEAL_SAT_BOOST: f32 = {};\nconst GRADE_GREEN_SAT_BOOST: f32 = {};\nconst GRADE_GREEN_HUE_SHIFT: f32 = {};\n\
         const VIGNETTE_INTENSITY: f32 = {};\nconst CAS_SHARPNESS: f32 = {};\n\
         const LOTTES_B: f32 = {};\nconst LOTTES_C: f32 = {};\n\
         const C5_MIN_X: f32 = {:?};\nconst C5_MIN_Y: f32 = {:?};\nconst C5_MID_X: f32 = {:?};\nconst C5_MID_Y: f32 = {:?};\nconst C5_MAX_X: f32 = {:?};\nconst C5_MAX_Y: f32 = {:?};\n",
        s.taa,
        s.fxaa,
        s.cas,
        s.vignette,
        s.bloom,
        s.bloom_bicubic,
        srgb_surface,
        match s.exposure {
            Exposure::Off => 0,
            Exposure::Simple => 1,
            Exposure::Histogram => 2,
        },
        f(-(s.manual_exposure as f64)), // get_exposure_from_ev_100(v) = exp2(-v): the shader takes the exponent
        f(calibration),
        f(min_lum),
        f(max_lum),
        f(min_lum.log2()),
        f(max_lum.log2()),
        f(s.rate_dim_to_bright as f64),
        f(s.rate_bright_to_dim as f64),
        s.histogram_bins,
        f(s.histogram_target as f64),
        f(s.bloom_intensity as f64),
        f(s.bloom_spread as f64),
        f(s.dof_intensity as f64),
        s.dof_samples,
        f(s.dof_focus as f64),
        f(s.brightness as f64),
        f(s.contrast as f64),
        f(s.saturation as f64),
        s.white_balance != 6500.0,
        f(s.orange_sat_boost as f64),
        f(s.teal_sat_boost as f64),
        f(s.green_sat_boost as f64),
        f(s.green_hue_shift as f64 / 360.0), // `GRADE_GREEN_HUE_SHIFT / 360.0`
        f(s.vignette_intensity as f64),
        f((s.cas_intensity * 2.0 - 1.0) as f64),
        f(b),
        f(c),
        min_x,
        min_y,
        mid_x,
        mid_y,
        max_x,
        max_y,
    );
    o += &format!("const TONEMAP_NAME: i32 = {};\n", s.tonemap as i32);
    o
}

/// `tonemap` is a `#define` in the pack; WGSL has no function pointers, so the shader switches on `TONEMAP_NAME` (the constant
/// folds away).
fn tonemap_switch(t: Tonemap) -> String {
    let all = [
        Tonemap::AcesFit,
        Tonemap::AcesFull,
        Tonemap::Lottes,
        Tonemap::Hejl2015,
        Tonemap::HejlBurgess,
        Tonemap::Tech,
        Tonemap::Uncharted2,
        Tonemap::Ozius,
        Tonemap::Reinhard,
        Tonemap::ReinhardJodie,
    ];
    let mut o = String::from("fn tonemap(rgb: vec3<f32>) -> vec3<f32> {\n");
    for k in all {
        o += &format!("    if (TONEMAP_NAME == {}) {{ return {}(rgb); }}\n", k as i32, k.func());
    }
    let _ = t;
    o + "    return rgb;\n}\n"
}

fn shader_source(s: &Settings, srgb_surface: bool) -> String {
    format!("{}{}{}", prelude(s, srgb_surface), tonemap_switch(s.tonemap), include_str!("post.wgsl"))
}

// ---------------------------------------------------------------------------------------------------------------------------
//  The GPU side
// ---------------------------------------------------------------------------------------------------------------------------

/// Index of every pass in the `P` buffer (its viewport + tile) and in the bind-group table.
mod id {
    pub const DOF: usize = 0;
    pub const EXPOSURE: usize = 1;
    pub const TAA: usize = 2;
    pub const DOWN: usize = 3; // ..9: bloom tiles 0..6
    pub const MERGE: usize = 9;
    pub const GAUSS_H: usize = 10;
    pub const GAUSS_V: usize = 11;
    pub const UP: usize = 12; // ..17: tiles 4, 3, 2, 1, 0
    pub const GRADE: usize = 17;
    pub const FXAA: usize = 18;
    pub const FINAL: usize = 19;
    pub const MIP: usize = 20; // one per scene mip level >= 1
}

struct Targets {
    scene_mips: Vec<wgpu::TextureView>,
    dof: wgpu::TextureView,
    hist: [wgpu::TextureView; 2],
    bloom: [wgpu::TextureView; 2],
    graded: wgpu::TextureView,
    fxaa: wgpu::TextureView,
    expo: [wgpu::TextureView; 2],
    /// Bind groups per frame parity (`frame & 1`: history and exposure swap), indexed by pass id.
    groups: [Vec<wgpu::BindGroup>; 2],
    w: u32,
    h: u32,
}

struct Pipes {
    dof: wgpu::RenderPipeline,
    expo: wgpu::RenderPipeline,
    taa: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    merge: wgpu::RenderPipeline,
    gauss_h: wgpu::RenderPipeline,
    gauss_v: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    grade: wgpu::RenderPipeline,
    fxaa: wgpu::RenderPipeline,
    fin: wgpu::RenderPipeline,
    mip: wgpu::RenderPipeline,
}

pub struct Post {
    pub settings: Settings,
    pipes: Pipes,
    layout0: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    pbuf: wgpu::Buffer,
    pgroup: wgpu::BindGroup,
    samp: wgpu::Sampler,
    noise: wgpu::TextureView,
    dummy: wgpu::TextureView,
    scene_format: wgpu::TextureFormat,
    t: Targets,
    scene_view: wgpu::TextureView,
    frame: u32,
    prev_view: Mat4,
    prev_proj: Mat4,
    prev_cam: Vec3,
    first: bool,
}

fn make_tex(device: &wgpu::Device, label: &str, w: u32, h: u32, format: wgpu::TextureFormat, mips: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn view(t: &wgpu::Texture) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor::default())
}

fn mip_view(t: &wgpu::Texture, level: u32) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() })
}

fn mat(m: Mat4) -> [[f32; 4]; 4] {
    m.to_cols_array_2d()
}

impl Post {
    /// `scene_format` is the colour format the world is drawn in (what the chunk, sky and vl pipelines target: the surface format
    /// until the G-buffer milestone makes it HDR), `surface_format` the swapchain, `depth` the game's depth view (rebuilt, with
    /// this, on every resize).
    #[allow(clippy::too_many_arguments)]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, scene_format: wgpu::TextureFormat, surface_format: wgpu::TextureFormat, depth: &wgpu::TextureView, settings: Settings) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("post"), source: wgpu::ShaderSource::Wgsl(shader_source(&settings, surface_format.is_srgb()).into()) });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let tex = |filterable| wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post_layout0"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(2, tex(true)),
                entry(3, tex(true)),
                entry(4, tex(false)), // the 1x1 exposure texture is rgba32float
                entry(5, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(6, tex(true)),
            ],
        });
        let layout1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post_layout1"),
            entries: &[entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: None })],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("post"), bind_group_layouts: &[&layout0, &layout1], push_constant_ranges: &[] });
        let pipe = |fs: &str, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_full"), buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let pipes = Pipes {
            dof: pipe("fs_dof", HDR),
            expo: pipe("fs_exposure", EXPO),
            taa: pipe("fs_taa", HDR),
            down: pipe("fs_bloom_down", HDR),
            merge: pipe("fs_bloom_merge", HDR),
            gauss_h: pipe("fs_bloom_gauss_h", HDR),
            gauss_v: pipe("fs_bloom_gauss_v", HDR),
            up: pipe("fs_bloom_up", HDR),
            grade: pipe("fs_grade", HDR),
            fxaa: pipe("fs_fxaa", HDR),
            fin: pipe("fs_final", surface_format),
            mip: pipe("fs_mip", scene_format),
        };
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("post_u"), size: std::mem::size_of::<U>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let pbuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("post_p"), size: P_STRIDE * (id::MIP as u64 + 20), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let samp = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, mipmap_filter: wgpu::FilterMode::Linear, ..Default::default() });
        // `noise.png`'s blue channel (the only one `c2_dof` reads), 512 x 512.
        let noise = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("post_noise"),
                size: wgpu::Extent3d { width: 512, height: 512, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            NOISE,
        );
        let dummy = view(&make_tex(device, "post_dummy", 1, 1, HDR, 1));
        let noise = view(&noise);
        let (t, scene_view) = Self::targets(device, &layout0, &ubuf, &samp, &noise, &dummy, depth, scene_format, w, h, &settings);
        let pgroup = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post_p"),
            layout: &layout1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &pbuf, offset: 0, size: wgpu::BufferSize::new(std::mem::size_of::<P>() as u64) }) }],
        });
        let post = Self { settings, pipes, layout0, ubuf, pbuf, pgroup, samp, noise, dummy, scene_format, t, scene_view, frame: 0, prev_view: Mat4::IDENTITY, prev_proj: Mat4::IDENTITY, prev_cam: Vec3::ZERO, first: true };
        post.write_passes(queue);
        post
    }

    /// The HDR scene colour: the world (and the volumetric light) is drawn into this, mip 0.
    pub fn scene_view(&self) -> &wgpu::TextureView {
        &self.scene_view
    }

    /// Clip-space offset the geometry passes add (`clip.xy += jitter * clip.w`) this frame: `taa_offset * 0.66` of the pack
    /// (`dh_terrain.vsh`, no TAAU). Zero when TAA is off.
    pub fn jitter(&self) -> Vec2 {
        if self.settings.taa { taa_offset(self.frame, self.t.w as f32) * 0.66 } else { Vec2::ZERO }
    }

    /// The projection the world is drawn with this frame.
    pub fn jittered(&self, proj: Mat4) -> Mat4 {
        let j = self.jitter();
        Mat4::from_translation(Vec3::new(j.x, j.y, 0.0)) * proj
    }

    pub fn resize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, depth: &wgpu::TextureView) {
        let (t, sv) = Self::targets(device, &self.layout0, &self.ubuf, &self.samp, &self.noise, &self.dummy, depth, self.scene_format, w, h, &self.settings);
        self.t = t;
        self.scene_view = sv;
        self.first = true;
        self.write_passes(queue);
    }

    #[allow(clippy::too_many_arguments)]
    fn targets(device: &wgpu::Device, l0: &wgpu::BindGroupLayout, ubuf: &wgpu::Buffer, samp: &wgpu::Sampler, noise: &wgpu::TextureView, dummy: &wgpu::TextureView, depth: &wgpu::TextureView, scene_format: wgpu::TextureFormat, w: u32, h: u32, s: &Settings) -> (Targets, wgpu::TextureView) {
        let (w, h) = (w.max(1), h.max(1));
        // The exposure pass reads the scene at a coarse mip: only then the scene has a chain (`fs_mip` fills it).
        let mips = if s.exposure == Exposure::Off { 1 } else { 32 - w.max(h).leading_zeros() };
        let scene = make_tex(device, "post_scene", w, h, scene_format, mips);
        let scene_mips: Vec<_> = (0..mips).map(|l| mip_view(&scene, l)).collect();
        let scene_all = view(&scene);
        let mk = |n: &str, f| view(&make_tex(device, n, w, h, f, 1));
        let dof = mk("post_dof", HDR);
        let hist = [mk("post_hist0", HDR), mk("post_hist1", HDR)];
        let bloom = [mk("post_bloom0", HDR), mk("post_bloom1", HDR)];
        let graded = mk("post_graded", HDR);
        let fxaa = mk("post_fxaa", HDR);
        let e1 = |n: &str| view(&make_tex(device, n, 1, 1, EXPO, 1));
        let expo = [e1("post_expo0"), e1("post_expo1")];
        let bg = |a: &wgpu::TextureView, b: &wgpu::TextureView, c: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post_bind"),
                layout: l0,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(samp) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(a) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(b) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(c) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(noise) },
                ],
            })
        };
        // What the next pass after the scene reads: DOF output when DOF is on.
        let input = if s.dof { &dof } else { &scene_all };
        let groups = [0usize, 1].map(|f| {
            let (cur, prev) = (&hist[f], &hist[1 - f]);
            let mut g: Vec<wgpu::BindGroup> = Vec::new();
            g.push(bg(&scene_all, dummy, &expo[1 - f])); // DOF: centerDepthSmooth of the last frame
            // ponytail: the exposure pass reads the scene (the only texture with mips), also when DOF is on
            g.push(bg(&scene_all, dummy, &expo[1 - f])); // exposure
            g.push(bg(input, prev, &expo[f])); // TAA
            for i in 0..TILES {
                // Iris' main/alt flip: tile i writes bloom[(i & 1) ^ 1] and reads what tile i - 1 wrote (tile 0 reads the history)
                g.push(if i == 0 { bg(cur, dummy, &expo[f]) } else { bg(&bloom[i & 1], dummy, &expo[f]) });
            }
            g.push(bg(&bloom[0], dummy, &expo[f])); // merge: odd tiles live in bloom[0]
            g.push(bg(&bloom[1], dummy, &expo[f])); // gaussian h
            g.push(bg(&bloom[0], dummy, &expo[f])); // gaussian v
            for j in 0..5 {
                g.push(bg(&bloom[1 - (j & 1)], dummy, &expo[f])); // upsample
            }
            g.push(bg(&bloom[0], cur, &expo[f])); // grade: bloom + TAA output
            g.push(bg(&graded, dummy, &expo[f])); // fxaa
            g.push(bg(if s.fxaa { &fxaa } else { &graded }, dummy, &expo[f])); // final
            for l in 1..scene_mips.len() {
                g.push(bg(&scene_mips[l - 1], dummy, &expo[f]));
            }
            g
        });
        (Targets { scene_mips: scene_mips.clone(), dof, hist, bloom, graded, fxaa, expo, groups, w, h }, scene_mips[0].clone())
    }

    /// The viewport of every pass never changes between frames: written once per (re)size.
    fn write_passes(&self, queue: &wgpu::Queue) {
        let (w, h) = (self.t.w as f32, self.t.h as f32);
        let mut rects: Vec<([f32; 4], i32)> = vec![([0.0, 0.0, w, h], 0); id::MIP];
        rects[id::EXPOSURE] = ([0.0, 0.0, 1.0, 1.0], 0);
        for i in 0..TILES {
            let (x, y, s) = tile_rect(i);
            rects[id::DOWN + i] = ([x * w, y * h, s * w, s * h], i as i32);
        }
        for j in 0..5 {
            let i = 4 - j;
            let (x, y, s) = tile_rect(i);
            rects[id::UP + j] = ([x * w, y * h, s * w, s * h], i as i32);
        }
        for l in 1..self.t.scene_mips.len() as u32 {
            rects.push(([0.0, 0.0, (self.t.w >> l).max(1) as f32, (self.t.h >> l).max(1) as f32], 0));
        }
        let mut bytes = vec![0u8; rects.len() * P_STRIDE as usize];
        for (k, (rect, tile)) in rects.iter().enumerate() {
            let p = P { rect: *rect, i: [*tile, 0, 0, 0] };
            bytes[k * P_STRIDE as usize..][..std::mem::size_of::<P>()].copy_from_slice(bytemuck::bytes_of(&p));
        }
        queue.write_buffer(&self.pbuf, 0, &bytes);
    }

    /// Run the whole chain: scene -> `out` (the swapchain view). Call after the world and the volumetric light are in
    /// `scene_view`; the HUD is drawn over `out` afterwards (it is not tonemapped, like the pack's HUD).
    pub fn draw(&mut self, queue: &wgpu::Queue, enc: &mut wgpu::CommandEncoder, out: &wgpu::TextureView, f: &Frame) {
        let s = self.settings;
        let (w, h) = (self.t.w as f32, self.t.h as f32);
        let rot = |m: Mat4| Mat4::from_mat3(Mat3::from_mat4(m)); // scene space = world - camera
        let (mv, prev_mv, prev_proj, prev_cam) = if self.first { (rot(f.view), rot(f.view), f.proj, f.cam_pos) } else { (rot(f.view), rot(self.prev_view), self.prev_proj, self.prev_cam) };
        let wb = white_balance_matrix(s.white_balance);
        let col = |v: DVec3| [v.x as f32, v.y as f32, v.z as f32, 0.0];
        let u = U {
            proj: mat(f.proj),
            proj_inv: mat(f.proj.inverse()),
            mv_inv: mat(mv.inverse()),
            prev_mv: mat(prev_mv),
            prev_proj: mat(prev_proj),
            cam: [f.cam_pos.x, f.cam_pos.y, f.cam_pos.z, f.dt],
            prev_cam: [prev_cam.x, prev_cam.y, prev_cam.z, f.time],
            res: [w, h, 1.0 / w, 1.0 / h],
            misc: [w / h, (self.frame % 720720) as f32, 0.0, 0.0], // aspectRatio, frameCounter, blindness, darknessFactor
            misc2: [0.0; 4],                                       // biome_cave
            wb: [col(wb.x_axis), col(wb.y_axis), col(wb.z_axis)],
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::bytes_of(&u));

        let fi = (self.frame & 1) as usize;
        let t = &self.t;
        let pipes = &self.pipes;
        let (g, pg) = (&t.groups[fi], &self.pgroup);
        let run = |enc: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, pass: usize, target: &wgpu::TextureView, viewport: Option<[f32; 4]>, load: bool| {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: if load { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) }, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some(v) = viewport {
                rp.set_viewport(v[0], v[1], v[2], v[3], 0.0, 1.0);
            }
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &g[pass], &[]);
            rp.set_bind_group(1, pg, &[(pass as u64 * P_STRIDE) as u32]);
            rp.draw(0..3, 0..1);
        };
        let tile = |i: usize| {
            let (x, y, sc) = tile_rect(i);
            Some([x * w, y * h, sc * w, sc * h])
        };
        // Scene mip chain (auto exposure only), then c2 .. c21 in the order of `world0/composite*.fsh`.
        for l in 1..t.scene_mips.len() {
            run(enc, &pipes.mip, id::MIP + l - 1, &t.scene_mips[l], None, false);
        }
        if s.dof {
            run(enc, &pipes.dof, id::DOF, &t.dof, None, false);
        }
        run(enc, &pipes.expo, id::EXPOSURE, &t.expo[fi], None, false);
        run(enc, &pipes.taa, id::TAA, &t.hist[fi], None, false);
        if s.bloom {
            // Iris' colortex0 main/alt flip, spelled out: tile i is written to bloom[(i & 1) ^ 1] and read back from bloom[i & 1]
            // by the next one; merge copies the odd tiles over to the even ones' buffer.
            for i in 0..TILES {
                run(enc, &pipes.down, id::DOWN + i, &t.bloom[(i & 1) ^ 1], tile(i), true);
            }
            run(enc, &pipes.merge, id::MERGE, &t.bloom[1], None, true);
            run(enc, &pipes.gauss_h, id::GAUSS_H, &t.bloom[0], None, true);
            run(enc, &pipes.gauss_v, id::GAUSS_V, &t.bloom[1], None, false);
            for j in 0..5 {
                run(enc, &pipes.up, id::UP + j, &t.bloom[j & 1], tile(4 - j), true);
            }
        }
        run(enc, &pipes.grade, id::GRADE, &t.graded, None, false);
        if s.fxaa {
            run(enc, &pipes.fxaa, id::FXAA, &t.fxaa, None, false);
        }
        run(enc, &pipes.fin, id::FINAL, out, None, false);

        self.prev_view = f.view;
        self.prev_proj = f.proj;
        self.prev_cam = f.cam_pos;
        self.first = false;
        self.frame = self.frame.wrapping_add(1);
    }
}

const NOISE: &[u8] = include_bytes!("../../assets/noise_b.r8");

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(s: &Settings, srgb: bool) {
        let src = shader_source(s, srgb);
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    }

    #[test]
    fn shader_validates_in_every_configuration() {
        validate(&Settings::default(), true);
        validate(&Settings { exposure: Exposure::Simple, dof: true, white_balance: 5000.0, bloom_bicubic: true, ..Settings::default() }, false);
        validate(&Settings { exposure: Exposure::Histogram, histogram_bins: 128, taa: false, cas: false, ..Settings::default() }, true);
        for t in [Tonemap::AcesFit, Tonemap::AcesFull, Tonemap::Lottes, Tonemap::Hejl2015, Tonemap::HejlBurgess, Tonemap::Tech, Tonemap::Uncharted2, Tonemap::Ozius, Tonemap::Reinhard, Tonemap::ReinhardJodie] {
            validate(&Settings { tonemap: t, ..Settings::default() }, true);
        }
    }

    #[test]
    fn bloom_tiles_fit_and_do_not_overlap() {
        let r: Vec<_> = (0..TILES).map(|i| { let (x, y, s) = tile_rect(i); (x, y, x + s, y + s) }).collect();
        for (i, a) in r.iter().enumerate() {
            assert!(a.2 <= 1.0 && a.3 <= 1.0, "tile {i} fits");
            for b in &r[i + 1..] {
                assert!(a.2 <= b.0 + 1e-6 || b.2 <= a.0 + 1e-6 || a.3 <= b.1 + 1e-6 || b.3 <= a.1 + 1e-6, "tiles overlap");
            }
        }
    }

    #[test]
    fn jitter_stays_inside_half_a_pixel_and_the_matrices_are_the_pack_s() {
        for n in 0..2000 {
            let j = taa_offset(n, 1000.0);
            assert!(j.x.abs() <= 1.0 / 1000.0 && j.y.abs() <= 1.0 / 1000.0);
        }
        // rec. 2020 -> rec. 709 maps white to white
        let m = rec2020_to_xyz() * xyz_to_rec709();
        let w = rv(DVec3::ONE, m);
        assert!((w - DVec3::ONE).abs().max_element() < 2e-3, "{w:?}");
        // the white balance at 6500 K is the identity
        assert_eq!(white_balance_matrix(6500.0), DMat3::IDENTITY);
    }
}
