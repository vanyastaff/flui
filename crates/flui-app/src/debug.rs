//! Debug utilities for the application layer.

/// Feature flags for enabling debug overlays and diagnostics.
#[derive(Debug, Clone, Default)]
pub struct DebugFlags {
    /// Show the widget inspector overlay.
    pub show_inspector: bool,
    /// Show the performance overlay (frame times, GPU stats).
    pub show_performance_overlay: bool,
    /// Show layout debug borders around widgets.
    pub show_layout_borders: bool,
    /// Show repaint regions (flashing).
    pub show_repaint_regions: bool,
}

impl DebugFlags {
    /// All flags disabled.
    pub fn none() -> Self {
        Self::default()
    }

    /// All flags enabled.
    pub fn all() -> Self {
        Self {
            show_inspector: true,
            show_performance_overlay: true,
            show_layout_borders: true,
            show_repaint_regions: true,
        }
    }
}
