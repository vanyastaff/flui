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
// `pub` for the same off-target-consumed reason as `hwnd_affinity` below
// (consumers: the winit, Win32, and AppKit event-conversion backends).
pub mod events;
pub mod gestures;
mod handlers;
// `pub`, not `pub(crate)`, for the same reason `keys`/`keys_macos` are:
// these cfg-free rule modules are consumed only by one target's backend
// (here Win32), so on every other target a crate-private visibility flags
// them dead — `pub` is what keeps a Linux-tested, Windows-consumed rule
// module warning-free everywhere. (`accessibility_bridge` can afford
// `pub(crate)` only because Linux production code consumes it too.)
pub mod hwnd_affinity;
pub mod keys;
pub mod keys_macos;
pub mod panic_boundary;
pub mod scroll;
// `pub` for the same Linux-tested/off-target-consumed reason as
// `hwnd_affinity` above (consumers: the Win32 and AppKit backends).
pub mod visibility;

pub(crate) use handlers::impl_window_callback_setters;
pub use handlers::{PlatformHandlers, WindowCallbacks};
