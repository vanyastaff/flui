//! Desktop accessibility probe; see the desktop implementation for its
//! controls and automation contract.

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
#[path = "a11y_probe/desktop.rs"]
mod desktop;

fn main() {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    desktop::run();
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    eprintln!("a11y_probe requires a desktop application host");
}
