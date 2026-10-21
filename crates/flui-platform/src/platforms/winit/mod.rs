//! Winit-based platform implementation

// `platform.rs` needs a small `unsafe` FFI island (documented at each site);
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`). The other
// submodules in this backend have no `unsafe` of their own.
#![expect(unsafe_code)]

mod clipboard;
mod control;
mod data_transfer;
// The display source is the one the OS offers, never winit's monitor list:
// each file reads its OS's native source (ADR-0236).
#[cfg(target_os = "linux")]
#[path = "display_linux.rs"]
mod display;
#[cfg(target_os = "macos")]
#[path = "display_macos.rs"]
mod display;
#[cfg(windows)]
#[path = "display_windows.rs"]
mod display;
mod events;
mod platform;
#[cfg(target_os = "linux")]
mod preferences;
mod window;
mod window_plan;

pub use clipboard::ArboardClipboard;
pub use data_transfer::WinitDataTransfer;
pub use platform::WinitPlatform;
pub use window::WinitWindow;
