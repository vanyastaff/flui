//! What the headless "operating system" refuses or clamps, so a test can see a
//! window snapshot report the outcome rather than the request.

use flui_foundation::geometry::Size;

/// What the headless host imposes on its windows.
///
/// A window command is a request (ADR-0237): the host may clamp a size or
/// decline a mode, and the window's snapshot shows what happened. The default
/// limits impose nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct HeadlessWindowPolicy {
    /// The largest window the host allows, in logical pixels. A larger
    /// resize request ends at this size.
    pub max_window_size: Option<Size<f64>>,
    /// Whether the host declines to maximize a window: the maximize request
    /// is accepted and the window stays as it was.
    pub refuses_maximize: bool,
}

impl HeadlessWindowPolicy {
    /// Cap windows at `size`.
    #[must_use]
    pub const fn with_max_window_size(mut self, size: Size<f64>) -> Self {
        self.max_window_size = Some(size);
        self
    }

    /// Make the host decline to maximize windows.
    #[must_use]
    pub const fn refusing_maximize(mut self) -> Self {
        self.refuses_maximize = true;
        self
    }
}
