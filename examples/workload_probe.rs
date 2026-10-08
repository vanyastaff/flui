//! Self-driving desktop workload probe; see the desktop implementation for
//! measurement contracts and configuration.

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
#[path = "workload_probe/desktop.rs"]
mod desktop;

fn main() {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    desktop::run();
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    eprintln!("workload_probe requires a desktop application host");
}
