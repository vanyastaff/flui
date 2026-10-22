//! The displays: the outputs a platform reports, their geometry, scale and
//! refresh period, and the snapshot a host publishes of them (ADR-0151 §10).
//!
//! The model is `source`: the id registry and last snapshot every native
//! display source keeps, and when that snapshot is read from the operating
//! system again. Beside it is one file per platform, each reading its
//! operating system's own source:
//!
//! - `windows`: the monitors from `QueryDisplayConfig` and
//!   `EnumDisplayMonitors`, and the system's display broadcasts;
//! - `apple`: `NSScreen` on macOS and `UIScreen` on iOS, with what the two
//!   share (how a screen is keyed);
//! - `android`: the Activity's `Display` and the choreographer's refresh rate;
//! - `web`: the browser's screens, pixel ratio and animation-frame cadence;
//! - `linux`: the outputs of a Wayland compositor or an X server, read on a
//!   connection of its own;
//! - `winit`: the winit host's source on Linux, over `linux`; on Windows and
//!   macOS the winit host reads `windows` and `apple`, and never winit's
//!   monitor list (ADR-0236);
//! - `headless`: the displays a test lays out and changes.
//!
//! A loop holds its platform's displays through the hook its `owner` module
//! declares, which the host's table fills; nothing in `owner` names this
//! module.
//!
//! The halves of a platform file are inline modules. The pure one (keys,
//! periods, estimates) is `cfg(any(<os>, test))` with `forbid(unsafe_code)`,
//! so the Linux CI host runs its tests; the one that calls the operating system
//! is `cfg(<os>)`. A platform file has a child file only for a mechanism of one
//! operating-system object, for a split the 3 000-line limit forces (`linux`),
//! or for a module's tests.

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
pub(crate) mod source;

#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
pub(crate) mod headless;
#[cfg(any(target_os = "linux", test))]
pub(crate) mod linux;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web;
#[cfg(any(windows, test))]
pub(crate) mod windows;
#[cfg(all(feature = "winit-backend", target_os = "linux"))]
pub(crate) mod winit;
