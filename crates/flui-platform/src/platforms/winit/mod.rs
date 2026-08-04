//! Winit-based platform implementation

// `platform.rs` needs a small `unsafe` FFI island (documented at each site);
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`). The other
// submodules in this backend have no `unsafe` of their own.
#![allow(unsafe_code)]

mod clipboard;
mod control;
mod data_transfer;
mod display;
mod events;
mod platform;

pub use clipboard::ArboardClipboard;
pub use data_transfer::WinitDataTransfer;
pub use display::WinitDisplay;
pub use platform::WinitPlatform;
