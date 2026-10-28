//! Accessibility: the tree a window hands an assistive technology, through an
//! AccessKit adapter of its operating system (ADR-0151 §10).
//!
//! The model is `bridge`, the state an adapter shares with the handlers its
//! platform library calls: activation, the retained tree and the listeners.
//! Beside it, one file per platform holds the adapter a window hands out,
//! the type the accessibility position of its table names:
//!
//! - `windows`: UI Automation (`accesskit_windows`);
//! - `apple`: NSAccessibility on macOS (`accesskit_macos`);
//! - `linux`: AT-SPI (`accesskit_unix`), which every Linux host's windows
//!   hand out (the Wayland, X11 and winit hosts');
//! - `headless`: `FakeAccessibility`, which counts what was published while active.
//!
//! The adapters have no feature: every window of a host that has one hands it
//! out, and each compiles wherever a host of its operating system does
//! (ADR-0151 §15): on Linux, with any of the Wayland, X11 and winit hosts.
//! Android and the Web have none yet: their tables name
//! `Absent<Accessibility>`.

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) mod apple;
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "ios",
    linux_host
))]
mod bridge;
pub(crate) mod headless;
#[cfg(linux_host)]
pub(crate) mod linux;
#[cfg(windows)]
pub(crate) mod windows;
