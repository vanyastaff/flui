//! Multi-window desktop example.

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
#[path = "multi_window_demo/desktop.rs"]
mod desktop;

fn main() {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    desktop::run();
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    eprintln!("multi_window_demo requires a desktop application host");
}
