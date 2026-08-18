//! Shared platform infrastructure
//!
//! Components shared between platform implementations to reduce code
//! duplication.

// Adapter-side accessibility state, shared by the AT-SPI / UIA /
// NSAccessibility bridges. Gated to the targets that have one so the
// module is never dead code on Android or wasm.
#[cfg(all(
    feature = "a11y",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
pub(crate) mod accessibility_bridge;
pub mod gestures;
mod handlers;
pub mod keys;
pub mod keys_macos;
pub mod scroll;

pub use handlers::{PlatformHandlers, WindowCallbacks};
