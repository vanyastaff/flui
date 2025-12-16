//! Device-specific gesture settings.
//!
//! Different input devices (touch, mouse, stylus) need different tolerance
//! values for gesture recognition. This module provides configurable settings
//! that can be tuned per device type.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::settings::GestureSettings;
//! use ui_events::pointer::PointerType;
//!
//! // Get settings for touch input
//! let touch_settings = GestureSettings::for_device(PointerType::Touch);
//! assert_eq!(touch_settings.touch_slop(), 18.0);
//!
//! // Get settings for mouse input (more precise)
//! let mouse_settings = GestureSettings::for_device(PointerType::Mouse);
//! assert_eq!(mouse_settings.touch_slop(), 1.0);
//! ```

use std::time::Duration;
use ui_events::pointer::PointerType;

/// Default touch slop for touch devices (18 logical pixels).
///
/// Touch slop is the maximum distance a pointer can move before it's
/// considered a drag rather than a tap.
pub const DEFAULT_TOUCH_SLOP: f32 = 18.0;

/// Default touch slop for mouse devices (1 logical pixel).
///
/// Mouse input is more precise, so the slop is much smaller.
pub const DEFAULT_MOUSE_SLOP: f32 = 1.0;

/// Default touch slop for pen/stylus devices (8 logical pixels).
pub const DEFAULT_PEN_SLOP: f32 = 8.0;

/// Default pan slop (same as touch slop by default).
pub const DEFAULT_PAN_SLOP: f32 = 18.0;

/// Default scale slop (minimum scale factor change to start scaling).
pub const DEFAULT_SCALE_SLOP: f32 = 0.05;

/// Default double-tap distance tolerance (100 logical pixels).
pub const DEFAULT_DOUBLE_TAP_SLOP: f32 = 100.0;

/// Default double-tap timeout (300ms).
pub const DEFAULT_DOUBLE_TAP_TIMEOUT: Duration = Duration::from_millis(300);

/// Default long-press timeout (500ms).
pub const DEFAULT_LONG_PRESS_TIMEOUT: Duration = Duration::from_millis(500);

/// Default minimum fling velocity (50 pixels/second).
pub const DEFAULT_MIN_FLING_VELOCITY: f32 = 50.0;

/// Default maximum fling velocity (8000 pixels/second).
pub const DEFAULT_MAX_FLING_VELOCITY: f32 = 8000.0;

/// Device-specific gesture settings.
///
/// These settings control how gestures are recognized based on the input device.
/// Different devices have different precision levels, so the tolerance values
/// need to be adjusted accordingly.
///
/// # Device Differences
///
/// - **Touch**: Fingers are imprecise, so larger tolerance values are needed
/// - **Mouse**: Very precise, so small tolerances work well
/// - **Pen/Stylus**: Medium precision, between touch and mouse
///
/// # Example
///
/// ```rust,ignore
/// use flui_interaction::settings::GestureSettings;
///
/// let settings = GestureSettings::default();
///
/// // Check touch slop
/// if distance < settings.touch_slop() {
///     // Still considered a tap
/// } else {
///     // Now a drag
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct GestureSettings {
    /// Maximum distance for a tap gesture (device-specific).
    touch_slop: f32,

    /// Maximum distance for starting a pan gesture.
    pan_slop: f32,

    /// Minimum scale factor change to start scaling.
    scale_slop: f32,

    /// Maximum distance between taps for a double-tap.
    double_tap_slop: f32,

    /// Maximum time between taps for a double-tap.
    double_tap_timeout: Duration,

    /// Time to wait before recognizing a long-press.
    long_press_timeout: Duration,

    /// Minimum velocity to trigger a fling.
    min_fling_velocity: f32,

    /// Maximum velocity for a fling (clamped).
    max_fling_velocity: f32,
}

impl Default for GestureSettings {
    fn default() -> Self {
        Self::touch_defaults()
    }
}

impl GestureSettings {
    /// Create settings with custom values.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        touch_slop: f32,
        pan_slop: f32,
        scale_slop: f32,
        double_tap_slop: f32,
        double_tap_timeout: Duration,
        long_press_timeout: Duration,
        min_fling_velocity: f32,
        max_fling_velocity: f32,
    ) -> Self {
        Self {
            touch_slop,
            pan_slop,
            scale_slop,
            double_tap_slop,
            double_tap_timeout,
            long_press_timeout,
            min_fling_velocity,
            max_fling_velocity,
        }
    }

    /// Create settings optimized for touch input.
    ///
    /// Uses larger tolerance values since fingers are imprecise.
    pub fn touch_defaults() -> Self {
        Self {
            touch_slop: DEFAULT_TOUCH_SLOP,
            pan_slop: DEFAULT_PAN_SLOP,
            scale_slop: DEFAULT_SCALE_SLOP,
            double_tap_slop: DEFAULT_DOUBLE_TAP_SLOP,
            double_tap_timeout: DEFAULT_DOUBLE_TAP_TIMEOUT,
            long_press_timeout: DEFAULT_LONG_PRESS_TIMEOUT,
            min_fling_velocity: DEFAULT_MIN_FLING_VELOCITY,
            max_fling_velocity: DEFAULT_MAX_FLING_VELOCITY,
        }
    }

    /// Create settings optimized for mouse input.
    ///
    /// Uses smaller tolerance values since mouse is precise.
    pub fn mouse_defaults() -> Self {
        Self {
            touch_slop: DEFAULT_MOUSE_SLOP,
            pan_slop: DEFAULT_MOUSE_SLOP,
            scale_slop: DEFAULT_SCALE_SLOP,
            double_tap_slop: DEFAULT_DOUBLE_TAP_SLOP,
            double_tap_timeout: DEFAULT_DOUBLE_TAP_TIMEOUT,
            long_press_timeout: DEFAULT_LONG_PRESS_TIMEOUT,
            min_fling_velocity: DEFAULT_MIN_FLING_VELOCITY,
            max_fling_velocity: DEFAULT_MAX_FLING_VELOCITY,
        }
    }

    /// Create settings optimized for pen/stylus input.
    ///
    /// Uses medium tolerance values.
    pub fn pen_defaults() -> Self {
        Self {
            touch_slop: DEFAULT_PEN_SLOP,
            pan_slop: DEFAULT_PEN_SLOP,
            scale_slop: DEFAULT_SCALE_SLOP,
            double_tap_slop: DEFAULT_DOUBLE_TAP_SLOP,
            double_tap_timeout: DEFAULT_DOUBLE_TAP_TIMEOUT,
            long_press_timeout: DEFAULT_LONG_PRESS_TIMEOUT,
            min_fling_velocity: DEFAULT_MIN_FLING_VELOCITY,
            max_fling_velocity: DEFAULT_MAX_FLING_VELOCITY,
        }
    }

    /// Get settings appropriate for a device type.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_interaction::settings::GestureSettings;
    /// use ui_events::pointer::PointerType;
    ///
    /// let settings = GestureSettings::for_device(PointerType::Touch);
    /// ```
    pub fn for_device(device_kind: PointerType) -> Self {
        match device_kind {
            PointerType::Touch => Self::touch_defaults(),
            PointerType::Mouse => Self::mouse_defaults(),
            PointerType::Pen => Self::pen_defaults(),
            _ => Self::touch_defaults(), // Default to touch for unknown
        }
    }

    // ========================================================================
    // Getters
    // ========================================================================

    /// Get the touch slop (maximum movement for a tap).
    #[inline]
    pub fn touch_slop(&self) -> f32 {
        self.touch_slop
    }

    /// Get the pan slop (minimum movement to start panning).
    #[inline]
    pub fn pan_slop(&self) -> f32 {
        self.pan_slop
    }

    /// Get the scale slop (minimum scale change to start scaling).
    #[inline]
    pub fn scale_slop(&self) -> f32 {
        self.scale_slop
    }

    /// Get the double-tap slop (maximum distance between taps).
    #[inline]
    pub fn double_tap_slop(&self) -> f32 {
        self.double_tap_slop
    }

    /// Get the double-tap timeout.
    #[inline]
    pub fn double_tap_timeout(&self) -> Duration {
        self.double_tap_timeout
    }

    /// Get the long-press timeout.
    #[inline]
    pub fn long_press_timeout(&self) -> Duration {
        self.long_press_timeout
    }

    /// Get the minimum fling velocity.
    #[inline]
    pub fn min_fling_velocity(&self) -> f32 {
        self.min_fling_velocity
    }

    /// Get the maximum fling velocity.
    #[inline]
    pub fn max_fling_velocity(&self) -> f32 {
        self.max_fling_velocity
    }

    // ========================================================================
    // Builder-style setters
    // ========================================================================

    /// Set the touch slop.
    #[inline]
    pub fn with_touch_slop(mut self, slop: f32) -> Self {
        self.touch_slop = slop;
        self
    }

    /// Set the pan slop.
    #[inline]
    pub fn with_pan_slop(mut self, slop: f32) -> Self {
        self.pan_slop = slop;
        self
    }

    /// Set the scale slop.
    #[inline]
    pub fn with_scale_slop(mut self, slop: f32) -> Self {
        self.scale_slop = slop;
        self
    }

    /// Set the double-tap slop.
    #[inline]
    pub fn with_double_tap_slop(mut self, slop: f32) -> Self {
        self.double_tap_slop = slop;
        self
    }

    /// Set the double-tap timeout.
    #[inline]
    pub fn with_double_tap_timeout(mut self, timeout: Duration) -> Self {
        self.double_tap_timeout = timeout;
        self
    }

    /// Set the long-press timeout.
    #[inline]
    pub fn with_long_press_timeout(mut self, timeout: Duration) -> Self {
        self.long_press_timeout = timeout;
        self
    }

    /// Set the minimum fling velocity.
    #[inline]
    pub fn with_min_fling_velocity(mut self, velocity: f32) -> Self {
        self.min_fling_velocity = velocity;
        self
    }

    /// Set the maximum fling velocity.
    #[inline]
    pub fn with_max_fling_velocity(mut self, velocity: f32) -> Self {
        self.max_fling_velocity = velocity;
        self
    }

    // ========================================================================
    // Utility methods
    // ========================================================================

    /// Check if a distance exceeds the touch slop.
    #[inline]
    pub fn exceeds_touch_slop(&self, distance: f32) -> bool {
        distance > self.touch_slop
    }

    /// Check if a distance exceeds the pan slop.
    #[inline]
    pub fn exceeds_pan_slop(&self, distance: f32) -> bool {
        distance > self.pan_slop
    }

    /// Check if a scale factor exceeds the scale slop.
    ///
    /// Scale slop is applied symmetrically around 1.0.
    #[inline]
    pub fn exceeds_scale_slop(&self, scale: f32) -> bool {
        (scale - 1.0).abs() > self.scale_slop
    }

    /// Clamp a fling velocity to the configured range.
    #[inline]
    pub fn clamp_fling_velocity(&self, velocity: f32) -> f32 {
        velocity.clamp(self.min_fling_velocity, self.max_fling_velocity)
    }

    /// Check if a velocity is fast enough for a fling.
    #[inline]
    pub fn is_fling_velocity(&self, velocity: f32) -> bool {
        velocity.abs() >= self.min_fling_velocity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_touch() {
        let default = GestureSettings::default();
        let touch = GestureSettings::touch_defaults();
        assert_eq!(default, touch);
    }

    #[test]
    fn test_touch_defaults() {
        let settings = GestureSettings::touch_defaults();
        assert_eq!(settings.touch_slop(), DEFAULT_TOUCH_SLOP);
        assert_eq!(settings.pan_slop(), DEFAULT_PAN_SLOP);
        assert_eq!(settings.long_press_timeout(), DEFAULT_LONG_PRESS_TIMEOUT);
    }

    #[test]
    fn test_mouse_defaults() {
        let settings = GestureSettings::mouse_defaults();
        assert_eq!(settings.touch_slop(), DEFAULT_MOUSE_SLOP);
        assert_eq!(settings.pan_slop(), DEFAULT_MOUSE_SLOP);
    }

    #[test]
    fn test_pen_defaults() {
        let settings = GestureSettings::pen_defaults();
        assert_eq!(settings.touch_slop(), DEFAULT_PEN_SLOP);
        assert_eq!(settings.pan_slop(), DEFAULT_PEN_SLOP);
    }

    #[test]
    fn test_for_device() {
        let touch = GestureSettings::for_device(PointerType::Touch);
        let mouse = GestureSettings::for_device(PointerType::Mouse);
        let pen = GestureSettings::for_device(PointerType::Pen);

        assert_eq!(touch.touch_slop(), DEFAULT_TOUCH_SLOP);
        assert_eq!(mouse.touch_slop(), DEFAULT_MOUSE_SLOP);
        assert_eq!(pen.touch_slop(), DEFAULT_PEN_SLOP);
    }

    #[test]
    fn test_builder_pattern() {
        let settings = GestureSettings::default()
            .with_touch_slop(24.0)
            .with_pan_slop(24.0)
            .with_long_press_timeout(Duration::from_millis(800));

        assert_eq!(settings.touch_slop(), 24.0);
        assert_eq!(settings.pan_slop(), 24.0);
        assert_eq!(settings.long_press_timeout(), Duration::from_millis(800));
    }

    #[test]
    fn test_exceeds_touch_slop() {
        let settings = GestureSettings::default();

        assert!(!settings.exceeds_touch_slop(10.0));
        assert!(!settings.exceeds_touch_slop(18.0)); // Equal is not exceeded
        assert!(settings.exceeds_touch_slop(19.0));
    }

    #[test]
    fn test_exceeds_scale_slop() {
        let settings = GestureSettings::default();

        assert!(!settings.exceeds_scale_slop(1.0)); // No change
        assert!(!settings.exceeds_scale_slop(1.03)); // Within slop
        assert!(settings.exceeds_scale_slop(1.1)); // Beyond slop
        assert!(settings.exceeds_scale_slop(0.9)); // Beyond slop (zoom out)
    }

    #[test]
    fn test_clamp_fling_velocity() {
        let settings = GestureSettings::default();

        // Below minimum
        assert_eq!(
            settings.clamp_fling_velocity(10.0),
            DEFAULT_MIN_FLING_VELOCITY
        );

        // Within range
        assert_eq!(settings.clamp_fling_velocity(500.0), 500.0);

        // Above maximum
        assert_eq!(
            settings.clamp_fling_velocity(10000.0),
            DEFAULT_MAX_FLING_VELOCITY
        );
    }

    #[test]
    fn test_is_fling_velocity() {
        let settings = GestureSettings::default();

        assert!(!settings.is_fling_velocity(10.0));
        assert!(settings.is_fling_velocity(100.0));
        assert!(settings.is_fling_velocity(-100.0)); // Negative velocity
    }
}
