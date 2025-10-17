//! Accessibility features and preferences
//!
//! This module provides types for managing accessibility features,
//! similar to Flutter's AccessibilityFeatures class.

use serde::{Deserialize, Serialize};

/// Accessibility features that can be enabled by the user or platform.
///
/// Similar to Flutter's `AccessibilityFeatures`.
/// These features help users with disabilities interact with the application.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityFeatures {
    /// Whether the user has requested bold text.
    ///
    /// Screen readers may use this to make text more readable.
    pub bold_text: bool,

    /// Whether the user has requested high contrast UI.
    ///
    /// High contrast mode makes UI elements more distinguishable.
    pub high_contrast: bool,

    /// Whether the user has requested to disable animations.
    ///
    /// Some users find animations distracting or disorienting.
    pub disable_animations: bool,

    /// Whether the user has requested inverted colors.
    ///
    /// Inverted colors can help users with light sensitivity.
    pub invert_colors: bool,

    /// Whether the user has enabled a screen reader.
    ///
    /// When true, the app should provide semantic labels for all UI elements.
    pub screen_reader: bool,

    /// Whether the user has requested reduced motion.
    ///
    /// Similar to disable_animations but may allow some subtle motion.
    pub reduce_motion: bool,

    /// Whether the user has requested larger text scale.
    ///
    /// The platform-requested text scale factor (1.0 is normal).
    pub text_scale_factor: f32,
}

impl Default for AccessibilityFeatures {
    fn default() -> Self {
        Self {
            bold_text: false,
            high_contrast: false,
            disable_animations: false,
            invert_colors: false,
            screen_reader: false,
            reduce_motion: false,
            text_scale_factor: 1.0,
        }
    }
}

impl AccessibilityFeatures {
    /// Create a new `AccessibilityFeatures` with all features disabled.
    pub fn none() -> Self {
        Self::default()
    }

    /// Create a new `AccessibilityFeatures` with all features enabled.
    pub fn all() -> Self {
        Self {
            bold_text: true,
            high_contrast: true,
            disable_animations: true,
            invert_colors: true,
            screen_reader: true,
            reduce_motion: true,
            text_scale_factor: 1.5,
        }
    }

    /// Check if any accessibility features are enabled.
    pub fn has_any_enabled(&self) -> bool {
        self.bold_text
            || self.high_contrast
            || self.disable_animations
            || self.invert_colors
            || self.screen_reader
            || self.reduce_motion
            || (self.text_scale_factor - 1.0).abs() > 0.01
    }

    /// Check if animations should be shown.
    ///
    /// Returns false if either disable_animations or reduce_motion is true.
    pub fn should_show_animations(&self) -> bool {
        !self.disable_animations && !self.reduce_motion
    }

    /// Get the effective text scale factor.
    ///
    /// Returns the text_scale_factor, clamped to a reasonable range.
    pub fn effective_text_scale(&self) -> f32 {
        self.text_scale_factor.clamp(0.5, 3.0)
    }

    /// Merge with another AccessibilityFeatures, taking the maximum of each field.
    ///
    /// This is useful for combining user preferences with platform settings.
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            bold_text: self.bold_text || other.bold_text,
            high_contrast: self.high_contrast || other.high_contrast,
            disable_animations: self.disable_animations || other.disable_animations,
            invert_colors: self.invert_colors || other.invert_colors,
            screen_reader: self.screen_reader || other.screen_reader,
            reduce_motion: self.reduce_motion || other.reduce_motion,
            text_scale_factor: self.text_scale_factor.max(other.text_scale_factor),
        }
    }
}

/// User-configurable accessibility preferences.
///
/// This extends AccessibilityFeatures with additional app-specific settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessibilityPreferences {
    /// Base accessibility features from platform/user.
    pub features: AccessibilityFeatures,

    /// Whether to show tooltips on hover.
    pub show_tooltips: bool,

    /// Minimum touch target size (in pixels).
    ///
    /// Flutter Material Design recommends 48x48 for touch targets.
    pub min_touch_target_size: f32,

    /// Whether to enable keyboard navigation.
    pub keyboard_navigation: bool,

    /// Whether to announce UI changes via screen reader.
    pub announce_changes: bool,

    /// Focus indicator style intensity (0.0 = subtle, 1.0 = strong).
    pub focus_indicator_strength: f32,
}

impl Default for AccessibilityPreferences {
    fn default() -> Self {
        Self {
            features: AccessibilityFeatures::default(),
            show_tooltips: true,
            min_touch_target_size: 48.0,
            keyboard_navigation: true,
            announce_changes: false,
            focus_indicator_strength: 0.5,
        }
    }
}

impl AccessibilityPreferences {
    /// Create preferences optimized for screen reader users.
    pub fn for_screen_reader() -> Self {
        Self {
            features: AccessibilityFeatures {
                screen_reader: true,
                bold_text: true,
                high_contrast: true,
                ..Default::default()
            },
            show_tooltips: true,
            min_touch_target_size: 48.0,
            keyboard_navigation: true,
            announce_changes: true,
            focus_indicator_strength: 1.0,
        }
    }

    /// Create preferences optimized for users with motion sensitivity.
    pub fn for_motion_sensitivity() -> Self {
        Self {
            features: AccessibilityFeatures {
                disable_animations: true,
                reduce_motion: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create preferences optimized for users with visual impairments.
    pub fn for_visual_impairment() -> Self {
        Self {
            features: AccessibilityFeatures {
                bold_text: true,
                high_contrast: true,
                text_scale_factor: 1.5,
                ..Default::default()
            },
            show_tooltips: true,
            min_touch_target_size: 56.0,
            focus_indicator_strength: 1.0,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accessibility_features_default() {
        let features = AccessibilityFeatures::default();
        assert!(!features.has_any_enabled());
        assert!(features.should_show_animations());
        assert_eq!(features.effective_text_scale(), 1.0);
    }

    #[test]
    fn test_accessibility_features_all() {
        let features = AccessibilityFeatures::all();
        assert!(features.has_any_enabled());
        assert!(!features.should_show_animations());
        assert!(features.bold_text);
        assert!(features.screen_reader);
    }

    #[test]
    fn test_accessibility_features_merge() {
        let a = AccessibilityFeatures {
            bold_text: true,
            high_contrast: false,
            text_scale_factor: 1.2,
            ..Default::default()
        };

        let b = AccessibilityFeatures {
            bold_text: false,
            high_contrast: true,
            text_scale_factor: 1.5,
            ..Default::default()
        };

        let merged = a.merge(&b);
        assert!(merged.bold_text);
        assert!(merged.high_contrast);
        assert_eq!(merged.text_scale_factor, 1.5);
    }

    #[test]
    fn test_accessibility_preferences_presets() {
        let screen_reader = AccessibilityPreferences::for_screen_reader();
        assert!(screen_reader.features.screen_reader);
        assert!(screen_reader.announce_changes);

        let motion = AccessibilityPreferences::for_motion_sensitivity();
        assert!(motion.features.disable_animations);
        assert!(motion.features.reduce_motion);

        let visual = AccessibilityPreferences::for_visual_impairment();
        assert!(visual.features.bold_text);
        assert!(visual.features.high_contrast);
        assert!(visual.features.text_scale_factor > 1.0);
    }

    #[test]
    fn test_effective_text_scale_clamping() {
        let mut features = AccessibilityFeatures::default();

        features.text_scale_factor = 0.1; // Too small
        assert_eq!(features.effective_text_scale(), 0.5);

        features.text_scale_factor = 10.0; // Too large
        assert_eq!(features.effective_text_scale(), 3.0);

        features.text_scale_factor = 1.5; // Normal range
        assert_eq!(features.effective_text_scale(), 1.5);
    }
}
