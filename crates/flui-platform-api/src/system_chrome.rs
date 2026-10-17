//! System chrome a window's content can style: today the overlay style of the
//! system bars drawn over it (status bar icons).
//!
//! The framework reads the style from the composited scene after each frame
//! and calls [`PlatformSystemChrome::set_overlay_style`] only when the topmost
//! style at the window's top edge changes. Backends without system bars (the
//! desktop targets) return no capability; mobile backends implement it.

/// Whether the system bars over the content draw light or dark icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum SystemUiOverlayStyle {
    /// Light icons, for dark content under the bars.
    #[default]
    Light,
    /// Dark icons, for light content under the bars.
    Dark,
}

/// A window's system-bar styling capability.
///
/// Calls arrive on the UI thread, deduplicated: a style equal to the last one
/// delivered is not repeated until the presentation detaches and attaches
/// again. Applying it is best-effort.
pub trait PlatformSystemChrome: Send + Sync {
    /// Style the system bars drawn over this window.
    fn set_overlay_style(&self, style: SystemUiOverlayStyle);

    /// Downcast support for tests that reach a recording fake behind the
    /// trait object `PlatformWindow::system_chrome` returns.
    fn as_any(&self) -> &dyn std::any::Any;
}
