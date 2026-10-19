//! Shared platform infrastructure
//!
//! Components shared between platform implementations to reduce code
//! duplication.
//!
//! # Rule modules
//!
//! The cfg-free rule modules below (`#[doc(hidden)]`) hold one backend's
//! mapping as pure functions — key and motion decoding, the haptics and
//! availability tables, drag and drop translation, pointer assembly — so
//! Linux CI runs every backend's rules. Each is consumed by the backends of
//! some targets only, and a crate-private module would be dead code, with
//! its re-exports unused, on every other target; so they are `pub` for this
//! crate's own tests and hidden from its documented surface. They are not
//! API: nothing outside this crate may name them except its tests.
//! `events` and `window_installation` are the exceptions, named by the
//! composition root (`flui-app`).

// Adapter-side accessibility state, shared by the AT-SPI / UIA /
// NSAccessibility bridges. Gated to the targets that have one so the
// module is never dead code on Android or wasm.
#[cfg(all(
    feature = "a11y",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
pub(crate) mod accessibility_bridge;
// The Win32 clipboard lock shared by the Win32 and winit (`arboard`) backends.
#[cfg(windows)]
pub(crate) mod clipboard_lock;
// Rule module; consumer: the Android backend.
#[doc(hidden)]
pub mod android_motion;
// Rule module; consumer: the Android backend's `FluiView`.
#[doc(hidden)]
pub mod android_drag;
// Rule module; consumer: the Android backend's text input.
#[doc(hidden)]
pub mod android_ime;
// Rule module; consumer: the Android backend.
#[cfg(any(target_os = "android", test))]
pub(crate) mod android_display;
#[doc(hidden)]
pub mod android_keys;
#[cfg(any(target_os = "android", test))]
pub(crate) mod android_lifecycle;
#[cfg(any(target_os = "android", test))]
pub(crate) mod android_os_version;
#[cfg(any(target_os = "android", test))]
pub(crate) mod android_scroll;
#[cfg(any(target_os = "android", test))]
pub(crate) mod android_text_weight;
// The id registry and last snapshot every native display source keeps.
#[cfg(any(
    windows,
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    all(target_os = "linux", any(feature = "wayland", feature = "x11")),
    target_arch = "wasm32",
    test
))]
pub(crate) mod display_source;
// The keys the macOS and iOS display sources give a screen, the iOS version
// check and frame-rate request, and the period a display link publishes; `test`
// keeps these pure rules checked on every host.
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple_display_identity;
// What the AppKit window reports and is asked, as pure rules; `test` keeps them
// checked on every host.
#[cfg(any(target_os = "macos", test))]
pub(crate) mod appkit_window_geometry;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod appkit_window_mode;
#[cfg(any(target_os = "ios", test))]
pub(crate) mod apple_os_version;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod dispatch_memory_pressure;
#[cfg(any(target_os = "ios", test))]
pub(crate) mod display_link_rate;
pub mod events;
mod handlers;
pub(crate) mod owner_signal;
pub(crate) mod owner_slot;
#[cfg(any(target_os = "android", target_os = "macos", test))]
pub(crate) mod preference_read;
pub(crate) mod system_notice;
#[cfg(any(target_os = "macos", test))]
pub(crate) mod terminate_gate;
pub(crate) mod window_state;
// Rule module; consumer: the Win32 backend.
#[doc(hidden)]
pub mod hwnd_affinity;
// Rule module: every backend's haptics degradation table (ADR-0210 §7).
#[doc(hidden)]
pub mod haptic_degradation;
// Rule module; consumer: the Win32 DirectManipulation viewport.
#[doc(hidden)]
pub mod direct_manipulation;
// Rule module: every backend's availability table.
#[doc(hidden)]
pub mod input_availability;
#[cfg(any(
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "windows", target_os = "macos", target_os = "linux")
    )
))]
pub(crate) mod keyboard_adapter;
// Rule module; consumers: the Win32 and winit backends.
#[doc(hidden)]
pub mod keys;
// Rule module; consumer: the AppKit backend.
#[doc(hidden)]
pub mod keys_macos;
// Rule module; consumer: the Win32 OLE drag and drop.
#[doc(hidden)]
pub mod ole_drag_drop;
// Rule module: owner-callback containment, used by every native backend.
#[doc(hidden)]
pub mod panic_boundary;
// Rule module: every native backend assembles pointers through it.
#[doc(hidden)]
pub mod pointer_assembly;
// Rule module; consumer: the UIKit backend.
#[doc(hidden)]
pub mod screen_edges;
// Rule module; consumers: the Win32 and web backends.
#[doc(hidden)]
pub mod scroll;
// Rule module; consumer: the Win32 text services.
#[doc(hidden)]
pub mod text_geometry;
// Rule module; consumers: the Win32 and AppKit backends.
#[doc(hidden)]
pub mod visibility;
// Rule module; consumer: the web backend's wheel and gesture events.
#[doc(hidden)]
pub mod web_pinch;
// Rule module; consumer: the web backend's HTML drag and drop.
#[doc(hidden)]
pub mod html_drag_drop;
// Rule module; consumer: the web backend's text input.
#[doc(hidden)]
pub mod web_text;
// The macOS source turns its current-mode rate into a snapshot period here;
// `test` keeps the rule checked on every host.
#[cfg(any(target_os = "macos", test))]
pub(crate) mod display_period;
// The Windows display source reads rates and resolves monitors by these rules;
// `test` keeps them checked on every host.
#[cfg(any(windows, test))]
pub(crate) mod windows_display;
// The Win32 backend turns window limits, system messages and the low-memory
// event into notices by these rules; `test` keeps them checked on every host.
#[cfg(any(windows, test))]
pub(crate) mod windows_host_messages;
#[cfg(any(windows, test))]
pub(crate) mod windows_memory_watch;
#[cfg(any(windows, test))]
pub(crate) mod windows_window_limits;
// The browser's display rules: the refresh estimate from animation-frame
// timestamps, the pixel-ratio query, screen identity and feature detection. The
// Web backend consumes them; `test` keeps the rules checked on every host.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod dppx_query;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod refresh_estimate;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod screen_identity;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web_feature;
// UIKit consumes this acknowledgement and `flui-app` names it; its transport
// contract runs on every host.
pub mod window_installation;

#[cfg(target_os = "ios")]
pub(crate) use handlers::LifecycleEvent;
pub(crate) use handlers::impl_window_callback_setters;
pub use handlers::{InputCallback, InputWake, PlatformHandlers, WindowCallbacks};

#[cfg(target_os = "ios")]
pub(crate) mod apple_scene_error;
