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
//! - `linux`: AT-SPI (`accesskit_unix`), which the X11 and winit hosts use;
//! - `headless`: `FakeAccessibility`, which counts what was published while active.
//!
//! The adapters have no feature: every window of a host that has one hands it
//! out, and each builds on every target of its operating system. Android, iOS
//! and the Web have none yet: their tables name `Absent<Accessibility>`. The
//! Linux adapter comes with every Linux build, as the Linux hosts do
//! (ADR-0151 §15).

#[cfg(target_os = "macos")]
pub(crate) mod apple;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
mod bridge;
pub(crate) mod headless;
#[cfg(target_os = "linux")]
pub(crate) mod linux;
#[cfg(windows)]
pub(crate) mod windows;
