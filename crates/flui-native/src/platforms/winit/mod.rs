//! Winit-based platform implementation

// `positions.rs` (the same-thread window creation) needs a small `unsafe` FFI
// island (documented at its site); the workspace lint `unsafe_code = "warn"`
// is opted out here, at the module boundary, rather than for the whole crate
// (see `lib.rs`). The other submodules in this backend have no `unsafe` of
// their own.
#![expect(unsafe_code)]

pub(crate) mod events;
#[cfg(target_os = "linux")]
pub(crate) mod linux_unreported;
mod positions;
pub(crate) mod window;
pub(crate) mod window_plan;

pub use window::WinitWindow;
