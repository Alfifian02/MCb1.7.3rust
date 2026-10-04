mod engine;
mod game;
mod gamepad;
#[allow(dead_code)]
mod gl_raw;
mod input;
mod math;
mod renderer;
mod ui;
mod world;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(not(target_os = "android"))]
pub use desktop::run;

#[cfg(target_os = "android")]
mod android;
