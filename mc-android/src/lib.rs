//! Cangkang Android minimal (APK-1): menjalankan `mc_core::selftest` lalu menggambar hasilnya di layar
//! lewat buffer window perangkat lunak (tanpa GL/Skia). Kotak hijau = semua lulus, merah = ada yang gagal.
//! Setelah ini, layar ini diganti renderer GLES (lihat ROADMAP).

mod font;

use android_activity::{AndroidApp, MainEvent, PollEvent};
use core::ffi::c_void;
use std::time::Duration;

#[repr(C)]
struct WindowBuffer {
    width: i32,
    height: i32,
    stride: i32,
    format: i32,
    bits: *mut c_void,
    reserved: [u32; 6],
}

#[link(name = "android")]
extern "C" {
    fn ANativeWindow_setBuffersGeometry(window: *mut c_void, width: i32, height: i32, format: i32) -> i32;
    fn ANativeWindow_lock(window: *mut c_void, out: *mut WindowBuffer, dirty: *mut c_void) -> i32;
    fn ANativeWindow_unlockAndPost(window: *mut c_void) -> i32;
}

const WINDOW_FORMAT_RGBA_8888: i32 = 1;

// Piksel RGBA8888 dibaca sebagai u32 little-endian: 0xAABBGGRR.
const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

const BG: u32 = rgb(10, 12, 16);
const WHITE: u32 = rgb(235, 235, 235);
const GRAY: u32 = rgb(150, 155, 165);
const GREEN: u32 = rgb(64, 224, 112);
const RED: u32 = rgb(240, 70, 70);

struct Line {
    text: String,
    color: u32,
}

fn build_report() -> (Vec<Line>, bool) {
    let results = mc_core::selftest::run_all();
    let total = results.len();
    let passed = results.iter().filter(|r| r.ok).count();
    let mut lines = vec![
        Line { text: "MINECRAFT B1.7.3 - RUST".into(), color: WHITE },
        Line { text: "APK-1: uji mandiri mc-core".into(), color: GRAY },
        Line { text: String::new(), color: GRAY },
    ];
    for r in &results {
        if r.ok {
            lines.push(Line { text: format!("[OK]    {}", r.name), color: GREEN });
        } else {
            lines.push(Line { text: format!("[GAGAL] {}", r.name), color: RED });
            lines.push(Line { text: format!("        {}", r.detail), color: RED });
        }
    }
    lines.push(Line { text: String::new(), color: GRAY });
    let all = passed == total;
    lines.push(Line {
        text: format!("{} dari {} lulus", passed, total),
        color: if all { GREEN } else { RED },
    });
    (lines, all)
}

fn wrap(text: &str, cols: usize) -> Vec<String> {
    if cols == 0 || text.chars().count() <= cols {
        return vec![text.to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(cols).map(|c| c.iter().collect()).collect()
}

fn draw_text(px: &mut [u32], stride: usize, w: usize, h: usize, x0: usize, y0: usize, scale: usize, text: &str, color: u32) {
    for (i, ch) in text.chars().enumerate() {
        let code = ch as usize;
        let glyph = if (32..127).contains(&code) { &font::GLYPHS[code - 32] } else { &font::GLYPHS['?' as usize - 32] };
        let gx = x0 + i * font::W * scale;
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..font::W {
                if bits & (1 << (font::W - 1 - col)) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = gx + col * scale + dx;
                            let y = y0 + row * scale + dy;
                            if x < w && y < h {
                                px[y * stride + x] = color;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn draw(app: &AndroidApp, lines: &[Line], all_ok: bool) {
    let window = match app.native_window() {
        Some(w) => w,
        None => return,
    };
    let ptr = window.ptr().as_ptr() as *mut c_void;
    unsafe {
        ANativeWindow_setBuffersGeometry(ptr, 0, 0, WINDOW_FORMAT_RGBA_8888);
        let mut buf = WindowBuffer {
            width: 0,
            height: 0,
            stride: 0,
            format: 0,
            bits: core::ptr::null_mut(),
            reserved: [0; 6],
        };
        if ANativeWindow_lock(ptr, &mut buf, core::ptr::null_mut()) != 0 || buf.bits.is_null() {
            return;
        }
        let (w, h, stride) = (buf.width.max(0) as usize, buf.height.max(0) as usize, buf.stride.max(0) as usize);
        let px = std::slice::from_raw_parts_mut(buf.bits as *mut u32, stride * h);
        for p in px.iter_mut() {
            *p = BG;
        }
        // Penanda status di atas: hijau/merah
        let bar = (h / 40).max(8);
        for y in 0..bar.min(h) {
            for x in 0..w {
                px[y * stride + x] = if all_ok { GREEN } else { RED };
            }
        }
        let scale = (w / (46 * font::W)).max(1);
        let margin = font::W * scale;
        let cols = (w / (font::W * scale)).saturating_sub(2);
        // Turun dari status bar / lekukan layar
        let mut y = bar + (h / 14).max(margin);
        for line in lines {
            for part in wrap(&line.text, cols) {
                draw_text(px, stride, w, h, margin, y, scale, &part, line.color);
                y += (font::H + 3) * scale;
            }
        }
        ANativeWindow_unlockAndPost(ptr);
    }
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    let (lines, all_ok) = build_report();
    let mut running = true;
    let mut redraw = true;
    while running {
        app.poll_events(Some(Duration::from_millis(250)), |event| {
            if let PollEvent::Main(main_event) = event {
                match main_event {
                    MainEvent::InitWindow { .. } | MainEvent::WindowResized { .. } | MainEvent::RedrawNeeded { .. } => {
                        redraw = true;
                    }
                    MainEvent::Destroy { .. } => running = false,
                    _ => {}
                }
            }
        });
        if redraw {
            draw(&app, &lines, all_ok);
            redraw = false;
        }
    }
}
