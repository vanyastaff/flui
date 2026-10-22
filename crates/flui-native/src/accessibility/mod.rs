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
//! - `linux`: AT-SPI (`accesskit_unix`), which the winit host uses;
//! - `headless`: `FakeAccessibility`, which counts what was published while active.
//!
//! The adapters exist with the `a11y` feature. Android, iOS and the Web have
//! none yet: their tables name `Absent<Accessibility>`. Linux has no host but
//! the winit one, so its adapter builds with `winit-backend`, and in tests,
//! which exercise the bridge through it.

#[cfg(all(target_os = "macos", feature = "a11y"))]
pub(crate) mod apple;
#[cfg(all(
    feature = "a11y",
    any(
        target_os = "windows",
        target_os = "macos",
        all(target_os = "linux", any(feature = "winit-backend", test))
    )
))]
mod bridge;
pub(crate) mod headless;
#[cfg(all(
    target_os = "linux",
    feature = "a11y",
    any(feature = "winit-backend", test)
))]
pub(crate) mod linux;
#[cfg(all(windows, feature = "a11y"))]
pub(crate) mod windows;
