//! Winit-based platform implementation

// `positions.rs` (the same-thread window creation) and the macOS display
// source need small `unsafe` FFI islands (documented at each site); the
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`). The other
// submodules in this backend have no `unsafe` of their own.
#![expect(unsafe_code)]

mod clipboard;
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
pub(crate) mod events;
#[cfg(target_os = "linux")]
pub(crate) mod linux_notices;
mod positions;
#[cfg(target_os = "linux")]
pub(crate) mod preferences;
pub(crate) mod window;
pub(crate) mod window_plan;

pub use clipboard::ArboardClipboard;
pub use data_transfer::WinitDataTransfer;
pub(crate) use display::WinitDisplaySource;
pub(crate) use window::WinitTextInput;
pub use window::WinitWindow;
