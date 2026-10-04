// Desktop dev entry point (Android uses android_main in android.rs).
fn main() {
    #[cfg(not(target_os = "android"))]
    bgame::run();
}
