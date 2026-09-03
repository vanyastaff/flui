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
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `AccessibilityFeatures` from dart:ui.
#[expect(clippy::struct_excessive_bools)] // Mirrors Flutter's AccessibilityFeatures flags
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accessibility_features_any_enabled() {
        let features = AccessibilityFeatures::default();
        assert!(!features.any_enabled());

        let features = AccessibilityFeatures {
            bold_text: true,
            ..Default::default()
        };
        assert!(features.any_enabled());
    }

    #[test]
    fn new_matches_default() {
        assert_eq!(
            AccessibilityFeatures::new(),
            AccessibilityFeatures::default()
        );
    }
}
