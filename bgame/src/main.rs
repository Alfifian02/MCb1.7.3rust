// Desktop dev entry point (Android uses android_main in lib.rs).
fn main() {
    bgame::run(winit::event_loop::EventLoop::new().unwrap());
}
