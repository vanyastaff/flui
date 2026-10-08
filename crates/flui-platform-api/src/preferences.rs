//! Validated observations of system preferences (ADR-0172).
//!
//! Unknown values remain absent. A consumer chooses its fallback and application
//! policy; constructing this value does not install either one.

use std::sync::Arc;
use std::time::Duration;

use crate::Locale;

mod geometry;
pub use geometry::{
    Distance, FlingSpeeds, GestureGeometry, NativeMouseGeometry, NativeTouchGeometry,
    PreferenceQueryError,
};

/// An invalid numeric observation refused before snapshot publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InvalidPreference {
    /// Native sampling scale must be finite and strictly positive.
    #[error("preference sampling pixel ratio must be finite and strictly positive")]
    PixelRatio,
    /// Text sizing factors must be in the supported accessibility range.
    #[error("text scale must be between 1/64 and 64 inclusive")]
    TextScale,
    /// Motion duration factors must be finite and nonnegative.
    #[error("motion duration scale must be finite and nonnegative")]
    DurationScale,
    /// Rectangle dimensions and drag half-extents must be finite and nonnegative.
    #[error("gesture area must have finite nonnegative dimensions")]
    GestureArea,
    /// Logical distances must be finite and nonnegative.
    #[error("gesture distance must be finite and nonnegative")]
    Distance,
    /// Fling speeds must be finite and strictly positive.
    #[error("fling speed must be finite and strictly positive")]
    Speed,
    /// Minimum fling speed cannot exceed maximum fling speed.
    #[error("minimum fling speed exceeds maximum fling speed")]
    FlingRange,
}

/// A finite, strictly positive multiplier of an animation's duration.
///
/// This is a duration observation, not a playback rate. Consumers must handle
/// intermediate overflow when resolving it against their own durations.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct DurationScale(f64);

impl DurationScale {
    /// The observed duration multiplier.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// The operating system's motion preference, before application policy.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum MotionPreference {
    /// The system places no motion restriction on the application.
    NoPreference,
    /// The user requested reduced motion.
    Reduce,
    /// The system requested an animation-duration multiplier.
    Scaled(DurationScale),
}

impl MotionPreference {
    /// Interpret an OS duration-scale observation.
    ///
    /// Both signs of zero mean reduced motion; exactly one means no preference.
    /// Every other finite positive value, including subnormals, remains a scale.
    ///
    /// # Errors
    /// Rejects negative, NaN and infinite values.
    pub fn from_duration_scale(value: f64) -> Result<Self, InvalidPreference> {
        if !value.is_finite() || value < 0.0 {
            return Err(InvalidPreference::DurationScale);
        }
        Ok(if value == 0.0 {
            Self::Reduce
        } else if value == 1.0 {
            Self::NoPreference
        } else {
            Self::Scaled(DurationScale(value))
        })
    }
}

/// How a vertical discrete wheel notch is interpreted by the system.
///
/// Pixel-based scrolling already carries its distance and does not use this
/// preference. A zero line count explicitly disables discrete wheel scrolling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WheelStep {
    /// This many text lines per notch.
    Lines(u32),
    /// One viewport page per notch.
    Page,
}

/// Observed gesture timing; absent values leave the consumer's policy in charge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GesturePreferences {
    double_click_interval: Option<Duration>,
    double_tap_interval: Option<Duration>,
    long_press_timeout: Option<Duration>,
    native_mouse_geometry: Option<NativeMouseGeometry>,
    native_touch_geometry: Option<NativeTouchGeometry>,
}

impl GesturePreferences {
    /// Observed mouse double-click interval; this is not a touch double-tap policy.
    #[must_use]
    pub const fn double_click_interval(&self) -> Option<Duration> {
        self.double_click_interval
    }

    /// Record an observed double-click interval.
    #[must_use]
    pub const fn with_double_click_interval(mut self, value: Duration) -> Self {
        self.double_click_interval = Some(value);
        self
    }

    /// Observed touch double-tap interval, independently of mouse click timing.
    #[must_use]
    pub const fn double_tap_interval(&self) -> Option<Duration> {
        self.double_tap_interval
    }

    /// Record an observed touch double-tap interval.
    #[must_use]
    pub const fn with_double_tap_interval(mut self, value: Duration) -> Self {
        self.double_tap_interval = Some(value);
        self
    }

    /// Window-independent mouse geometry used for comparison and invalidation.
    ///
    /// Exact presentation geometry is queried separately from `PlatformWindow`.
    #[must_use]
    pub const fn native_mouse_geometry(&self) -> Option<NativeMouseGeometry> {
        self.native_mouse_geometry
    }

    /// Record a native observation without projecting it through another window.
    #[must_use]
    pub const fn with_native_mouse_geometry(mut self, value: NativeMouseGeometry) -> Self {
        self.native_mouse_geometry = Some(value);
        self
    }

    /// Native touch-context observation used only for comparison and invalidation.
    #[must_use]
    pub const fn native_touch_geometry(&self) -> Option<NativeTouchGeometry> {
        self.native_touch_geometry
    }

    /// Record physical touch measurements without projecting another presentation.
    #[must_use]
    pub const fn with_native_touch_geometry(mut self, value: NativeTouchGeometry) -> Self {
        self.native_touch_geometry = Some(value);
        self
    }

    /// Observed minimum duration for a long press.
    #[must_use]
    pub const fn long_press_timeout(&self) -> Option<Duration> {
        self.long_press_timeout
    }

    /// Record an observed long-press timeout.
    #[must_use]
    pub const fn with_long_press_timeout(mut self, value: Duration) -> Self {
        self.long_press_timeout = Some(value);
        self
    }
}

/// Observed discrete-wheel distances, before a scrollable resolves its metrics.
///
/// Horizontal distances are character counts, not vertical line counts. Zero
/// disables that axis. Pixel-based scrolling does not use these preferences.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WheelPreferences {
    vertical: Option<WheelStep>,
    horizontal_characters: Option<u32>,
}

impl WheelPreferences {
    /// Observed vertical line count or page behavior.
    #[must_use]
    pub const fn vertical(&self) -> Option<WheelStep> {
        self.vertical
    }

    /// Record observed vertical discrete-wheel behavior.
    #[must_use]
    pub const fn with_vertical(mut self, value: WheelStep) -> Self {
        self.vertical = Some(value);
        self
    }

    /// Observed horizontal character count per notch.
    #[must_use]
    pub const fn horizontal_characters(&self) -> Option<u32> {
        self.horizontal_characters
    }

    /// Record observed horizontal discrete-wheel behavior.
    #[must_use]
    pub const fn with_horizontal_characters(mut self, value: u32) -> Self {
        self.horizontal_characters = Some(value);
        self
    }
}

/// Host-wide system observations, independent of a user window's lifetime.
///
/// Start with [`Default`] (unknown observations) and add values with the owned
/// `with_*` methods. This value is immutable once shared; updating a producer
/// creates another snapshot. Fields with no observation stay `None`, including
/// a preference that the backend cannot obtain on this operating system.
///
/// Text scale is the observed user preference, not a claim that native font
/// metrics can be reproduced by multiplying every authored font size. The
/// consuming text policy resolves it separately from authored styles.
/// Native pointer geometry is presentation-context dependent and is not guessed
/// from a representative window by this host-wide value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemPreferences {
    text_scale: Option<f64>,
    motion: Option<MotionPreference>,
    high_contrast: Option<bool>,
    bold_text: Option<bool>,
    locales: Option<Arc<[Locale]>>,
    gestures: GesturePreferences,
    wheel: WheelPreferences,
}

impl SystemPreferences {
    /// An observed text-size preference in `1/64..=64`; no observation is `None`.
    #[must_use]
    pub const fn text_scale(&self) -> Option<f64> {
        self.text_scale
    }

    /// Record an observed text-size preference.
    ///
    /// The supported range is `1/64..=64`, inclusive. This is an accessibility
    /// multiplier, not an arbitrary geometric zoom. Bounding both ends keeps
    /// extreme native observations from overflowing or underflowing text
    /// sizing when consumers narrow to their shaping representation. Authored
    /// font sizes and other text metrics still require their own validation.
    ///
    /// # Errors
    /// Rejects values outside `1/64..=64`, including NaN and infinities.
    pub fn with_text_scale(mut self, value: f64) -> Result<Self, InvalidPreference> {
        if !(1.0 / 64.0..=64.0).contains(&value) {
            return Err(InvalidPreference::TextScale);
        }
        self.text_scale = Some(value);
        Ok(self)
    }

    /// The observed motion preference, before application policy.
    #[must_use]
    pub const fn motion(&self) -> Option<MotionPreference> {
        self.motion
    }

    /// Record an observed motion preference.
    #[must_use]
    pub const fn with_motion(mut self, value: MotionPreference) -> Self {
        self.motion = Some(value);
        self
    }

    /// Whether high contrast was observed as enabled or disabled.
    #[must_use]
    pub const fn high_contrast(&self) -> Option<bool> {
        self.high_contrast
    }

    /// Record an observed high-contrast setting.
    #[must_use]
    pub const fn with_high_contrast(mut self, value: bool) -> Self {
        self.high_contrast = Some(value);
        self
    }

    /// Whether bold text was observed as enabled or disabled.
    #[must_use]
    pub const fn bold_text(&self) -> Option<bool> {
        self.bold_text
    }

    /// Record an observed bold-text setting.
    #[must_use]
    pub const fn with_bold_text(mut self, value: bool) -> Self {
        self.bold_text = Some(value);
        self
    }

    /// Preferred UI locales, in system priority order; unknown is `None`.
    #[must_use]
    pub fn locales(&self) -> Option<&[Locale]> {
        self.locales.as_deref()
    }

    /// Record the complete ordered locale observation.
    #[must_use]
    pub fn with_locales(mut self, value: impl Into<Arc<[Locale]>>) -> Self {
        self.locales = Some(value.into());
        self
    }

    /// Observed gesture timing, before recognizer policy and authored overrides.
    #[must_use]
    pub const fn gestures(&self) -> &GesturePreferences {
        &self.gestures
    }

    /// Record the gesture observations in this snapshot.
    #[must_use]
    pub fn with_gestures(mut self, value: GesturePreferences) -> Self {
        self.gestures = value;
        self
    }

    /// Observed discrete-wheel behavior, before scrollable metrics are applied.
    #[must_use]
    pub const fn wheel(&self) -> &WheelPreferences {
        &self.wheel
    }

    /// Record the wheel observations in this snapshot.
    #[must_use]
    pub fn with_wheel(mut self, value: WheelPreferences) -> Self {
        self.wheel = value;
        self
    }
}
