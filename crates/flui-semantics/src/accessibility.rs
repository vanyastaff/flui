//! Platform accessibility feature flags.
//!
//! This type used to live behind the process-wide `SemanticsBinding`
//! singleton (retired — see `flui-app`'s `SharedEngineServices`, which now
//! owns the live `AccessibilityFeatures` value directly). `AccessibilityFeatures`
//! itself is plain data with no binding behavior, so it keeps living here,
//! independent of whichever owner holds the current value.

/// Platform accessibility features.
///
/// This struct represents the accessibility settings that the platform
/// has enabled, such as reduced motion or high contrast mode.
#[expect(clippy::struct_excessive_bools)] // One independent platform toggle per flag
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccessibilityFeatures {
    /// Whether accessible navigation is enabled.
    pub accessible_navigation: bool,

    /// Whether to invert colors.
    pub invert_colors: bool,

    /// Whether to disable animations.
    pub disable_animations: bool,

    /// Whether bold text is enabled.
    pub bold_text: bool,

    /// Whether to reduce motion.
    pub reduce_motion: bool,

    /// Whether high contrast mode is enabled.
    pub high_contrast: bool,

    /// Whether on/off labels should be shown on switches.
    pub on_off_switch_labels: bool,
}

impl AccessibilityFeatures {
    /// Creates new accessibility features with all options disabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether any accessibility feature is enabled.
    pub fn any_enabled(&self) -> bool {
        self.accessible_navigation
            || self.invert_colors
            || self.disable_animations
            || self.bold_text
            || self.reduce_motion
            || self.high_contrast
            || self.on_off_switch_labels
    }
}
