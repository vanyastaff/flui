//! Device-specific gesture settings.
//!
//! Different input devices (touch, mouse, stylus) need different tolerance
//! values for gesture recognition. This module provides configurable settings
//! that can be tuned per device type.
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::settings::GestureSettings;
//! use flui_platform_api::pointer::PointerKind;
//!
//! // Get settings for touch input
//! let touch_settings = GestureSettings::for_device(PointerKind::Touch);
//! assert_eq!(touch_settings.touch_slop(), 18.0);
//!
//! // Get settings for mouse input (more precise)
//! let mouse_settings = GestureSettings::for_device(PointerKind::Mouse);
//! assert_eq!(mouse_settings.touch_slop(), 1.0);
//! ```

use std::time::Duration;

use flui_platform_api::TargetPlatform;
use flui_platform_api::pointer::PointerKind;

/// Default touch slop for touch devices (18 logical pixels).
///
/// Touch slop is the maximum distance a pointer can move before it's
/// considered a drag rather than a tap.
pub const DEFAULT_TOUCH_SLOP: f64 = 18.0;

/// Default touch slop for mouse devices (1 logical pixel).
///
/// Mouse input is more precise, so the slop is much smaller. This is the
/// threshold [`GestureSettings::hit_slop`] returns for the mouse, used by
/// the axis-constrained drag recognizers.
pub const DEFAULT_MOUSE_SLOP: f64 = 1.0;

/// Default pan slop for mouse devices (2 logical pixels).
///
/// The threshold [`GestureSettings::pan_slop_for`] returns for the mouse,
/// used by free-direction drag. Free movement gets double the
/// axis-constrained hit slop.
pub const DEFAULT_MOUSE_PAN_SLOP: f64 = DEFAULT_MOUSE_SLOP * 2.0;

/// Default touch slop for pen/stylus devices (8 logical pixels).
pub const DEFAULT_PEN_SLOP: f64 = 8.0;

/// Default pan slop (same as touch slop by default).
pub const DEFAULT_PAN_SLOP: f64 = 18.0;

/// Default vertical-only pan slop.
///
/// Same as the touch slop (18 logical px) for vertical drag. Same numeric
/// value as [`DEFAULT_PAN_SLOP`] by default
/// — the split exists so apps can tune vertical drag more aggressively than
/// free pan (or vice versa) without touching the other.
pub const DEFAULT_PAN_SLOP_VERTICAL: f64 = 18.0;

/// Default horizontal-only pan slop.
///
/// See [`DEFAULT_PAN_SLOP_VERTICAL`] for the rationale behind the per-axis
/// split.
pub const DEFAULT_PAN_SLOP_HORIZONTAL: f64 = 18.0;

/// Default scale slop (minimum scale factor change to start scaling).
///
/// A *ratio* tolerance, not a distance: the acceptance test reads
/// `max(a / b, b / a) > 1.05`, and this is that `0.05`. It is dimensionless, which is why — unlike every other slop here —
/// it has no per-kind variant: a 5% pinch is 5% whatever moved.
pub const DEFAULT_SCALE_SLOP: f64 = 0.05;

/// Default span slop for imprecise devices (18 logical pixels).
///
/// The threshold [`GestureSettings::span_slop_for`] returns for every kind but
/// mouse. This is the *absolute* distance the span between two pointers must
/// change by, the tier [`DEFAULT_SCALE_SLOP`]'s ratio cannot express: a pinch
/// starting from a wide span moves a long way before it moves 5%.
pub const DEFAULT_SPAN_SLOP: f64 = DEFAULT_TOUCH_SLOP;

/// Default span slop for mouse devices (1 logical pixel).
///
/// The same precise-pointer slop as [`DEFAULT_MOUSE_SLOP`]; what
/// [`GestureSettings::span_slop_for`] returns for the mouse.
pub const DEFAULT_MOUSE_SPAN_SLOP: f64 = DEFAULT_MOUSE_SLOP;

/// Default double-tap distance tolerance (100 logical pixels).
pub const DEFAULT_DOUBLE_TAP_SLOP: f64 = 100.0;

/// Default double-tap timeout (300ms).
pub const DEFAULT_DOUBLE_TAP_TIMEOUT: Duration = Duration::from_millis(300);

/// Default long-press timeout (500ms).
pub const DEFAULT_LONG_PRESS_TIMEOUT: Duration = Duration::from_millis(500);

/// Default minimum fling velocity (50 pixels/second).
pub const DEFAULT_MIN_FLING_VELOCITY: f64 = 50.0;

/// Default maximum fling velocity (8000 pixels/second).
pub const DEFAULT_MAX_FLING_VELOCITY: f64 = 8000.0;

/// Device-specific gesture settings.
///
/// These settings control how gestures are recognized based on the input
/// device. Different devices have different precision levels, so the tolerance
/// values need to be adjusted accordingly.
///
/// # Device Differences
///
/// - **Touch**: Fingers are imprecise, so larger tolerance values are needed
/// - **Mouse**: Very precise, so small tolerances work well
/// - **Pen/Stylus**: Medium precision, between touch and mouse
///
/// # Validation
///
/// Every slop, ratio and velocity is finite and not negative, and the
/// minimum fling velocity never exceeds the maximum. These invariants hold
/// for every value of the type: the built-in profiles satisfy them, and
/// [`Self::try_new`] and the `try_with_*` builders reject a value that would
/// break one with a [`GestureSettingsError`] instead of storing it.
///
/// # Example
///
/// ```rust
/// use flui_interaction::settings::GestureSettings;
///
/// let settings = GestureSettings::default();
/// let distance = 10.0;
///
/// // Check touch slop
/// if distance < settings.touch_slop() {
///     // Still considered a tap
/// } else {
///     // Now a drag
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GestureSettings {
    /// Maximum distance for a tap gesture (device-specific).
    touch_slop: f64,

    /// Maximum distance for starting a pan gesture (free direction).
    pan_slop: f64,

    /// Maximum vertical-only distance to start a vertical drag.
    ///
    /// Per-axis split lets the recogniser use a different tolerance for
    /// vertical-only drags than for free pans. Defaults to
    /// [`DEFAULT_PAN_SLOP_VERTICAL`].
    pan_slop_vertical: f64,

    /// Maximum horizontal-only distance to start a horizontal drag.
    ///
    /// See [`Self::pan_slop_vertical`] — same rationale, horizontal axis.
    pan_slop_horizontal: f64,

    /// Minimum scale factor change to start scaling.
    scale_slop: f64,

    /// Maximum distance between taps for a double-tap.
    double_tap_slop: f64,

    /// Maximum time between taps for a double-tap.
    double_tap_timeout: Duration,

    /// Time to wait before recognizing a long-press.
    long_press_timeout: Duration,

    /// Minimum velocity to trigger a fling.
    min_fling_velocity: f64,

    /// Maximum velocity for a fling (clamped).
    max_fling_velocity: f64,
}

impl Default for GestureSettings {
    fn default() -> Self {
        Self::touch_defaults()
    }
}

/// Why a [`GestureSettings`] value was rejected.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum GestureSettingsError {
    /// A slop, ratio or velocity was NaN, infinite or negative.
    #[error("gesture setting `{field}` must be finite and not negative, got {value}")]
    InvalidValue {
        /// The setting's name.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// The minimum fling velocity exceeded the maximum.
    #[error("minimum fling velocity {min} px/s exceeds the maximum {max} px/s")]
    InvertedFlingRange {
        /// The requested minimum, px/s.
        min: f64,
        /// The requested maximum, px/s.
        max: f64,
    },
}

/// `value` if it is a usable distance, ratio or speed (finite and not
/// negative), else the error naming `field`.
fn checked(field: &'static str, value: f64) -> Result<f64, GestureSettingsError> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(GestureSettingsError::InvalidValue { field, value })
    }
}

/// A validated `(min, max)` fling velocity pair.
fn checked_fling_range(min: f64, max: f64) -> Result<(f64, f64), GestureSettingsError> {
    let min = checked("min_fling_velocity", min)?;
    let max = checked("max_fling_velocity", max)?;
    if min > max {
        return Err(GestureSettingsError::InvertedFlingRange { min, max });
    }
    Ok((min, max))
}

impl GestureSettings {
    /// Create settings with custom values.
    ///
    /// The per-axis pan slops start equal to `pan_slop`.
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] when a slop, ratio or velocity
    /// is NaN, infinite or negative; [`GestureSettingsError::InvertedFlingRange`]
    /// when `min_fling_velocity > max_fling_velocity`.
    #[expect(clippy::too_many_arguments)]
    pub fn try_new(
        touch_slop: f64,
        pan_slop: f64,
        scale_slop: f64,
        double_tap_slop: f64,
        double_tap_timeout: Duration,
        long_press_timeout: Duration,
        min_fling_velocity: f64,
        max_fling_velocity: f64,
    ) -> Result<Self, GestureSettingsError> {
        let pan_slop = checked("pan_slop", pan_slop)?;
        let (min_fling_velocity, max_fling_velocity) =
            checked_fling_range(min_fling_velocity, max_fling_velocity)?;
        Ok(Self {
            touch_slop: checked("touch_slop", touch_slop)?,
            pan_slop,
            pan_slop_vertical: pan_slop,
            pan_slop_horizontal: pan_slop,
            scale_slop: checked("scale_slop", scale_slop)?,
            double_tap_slop: checked("double_tap_slop", double_tap_slop)?,
            double_tap_timeout,
            long_press_timeout,
            min_fling_velocity,
            max_fling_velocity,
        })
    }

    /// Create settings optimized for touch input.
    ///
    /// Uses larger tolerance values since fingers are imprecise.
    pub fn touch_defaults() -> Self {
        Self {
            touch_slop: DEFAULT_TOUCH_SLOP,
            pan_slop: DEFAULT_PAN_SLOP,
            pan_slop_vertical: DEFAULT_PAN_SLOP_VERTICAL,
            pan_slop_horizontal: DEFAULT_PAN_SLOP_HORIZONTAL,
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
            // Free-direction pan uses the doubled precise-pointer constant
            // (`kPrecisePointerPanSlop`); vertical/horizontal drag stays on
            // the plain hit-slop tier (`kPrecisePointerHitSlop`) — see
            // `DEFAULT_MOUSE_PAN_SLOP`'s doc comment.
            pan_slop: DEFAULT_MOUSE_PAN_SLOP,
            pan_slop_vertical: DEFAULT_MOUSE_SLOP,
            pan_slop_horizontal: DEFAULT_MOUSE_SLOP,
            scale_slop: DEFAULT_SCALE_SLOP,
            double_tap_slop: DEFAULT_DOUBLE_TAP_SLOP,
            double_tap_timeout: DEFAULT_DOUBLE_TAP_TIMEOUT,
            long_press_timeout: DEFAULT_LONG_PRESS_TIMEOUT,
            min_fling_velocity: DEFAULT_MIN_FLING_VELOCITY,
            max_fling_velocity: DEFAULT_MAX_FLING_VELOCITY,
        }
    }

    /// Platform-faithful settings for a **runtime** [`TargetPlatform`].
    ///
    /// This is the primary platform-adaptation entry point: the platform is a *value*, not a
    /// compile-time fact, because the compile target alone is wrong in
    /// several real configurations —
    ///
    /// - **web/wasm**: one binary serves iOS Safari and Android Chrome; the
    ///   feel must be chosen from the user agent at runtime;
    /// - **tests**: widget tests exercise Android and iOS behavior on a
    ///   desktop host;
    /// - **ChromeOS / iPad-on-macOS**: the app's nominal platform and the
    ///   input hardware disagree.
    ///
    /// Use [`Self::native`] when the compile target *is* the right answer
    /// (a plain mobile/desktop build).
    ///
    /// Mapping: `Android`/`Fuchsia` → [`Self::android_defaults`] (Fuchsia is
    /// treated as Android-like); `iOS` →
    /// [`Self::ios_defaults`]; desktop and `Unknown` →
    /// [`Self::touch_defaults`] (the universal 18 px touch-slop
    /// baseline — per-device precision is layered on top via
    /// [`Self::for_device`]).
    #[must_use]
    pub fn for_platform(platform: TargetPlatform) -> Self {
        match platform {
            TargetPlatform::Android | TargetPlatform::Fuchsia => Self::android_defaults(),
            TargetPlatform::iOS => Self::ios_defaults(),
            // Desktop, Unknown, and any future `#[non_exhaustive]` variant:
            // the universal touch baseline is the safe feel.
            _ => Self::touch_defaults(),
        }
    }

    /// Settings for the compile-time platform
    /// ([`TargetPlatform::current()`], `cfg(target_os)`-seeded).
    ///
    /// Convenience over [`Self::for_platform`] for plain native builds.
    /// Anything that can host more than one platform feel (web, tests,
    /// embedders with platform override) must resolve a runtime
    /// [`TargetPlatform`] and call [`Self::for_platform`] instead.
    #[must_use]
    pub fn native() -> Self {
        Self::for_platform(TargetPlatform::current())
    }

    /// Native Android feel: values from AOSP `ViewConfiguration`
    /// (`frameworks/base/core/java/android/view/ViewConfiguration.java`),
    /// in dp ≡ logical px.
    ///
    /// Differences from [`Self::touch_defaults`] (the 18 px baseline):
    /// Android's native scroll-disambiguation slop is
    /// **8 dp** — noticeably more eager to scroll — and the double-tap
    /// window is 300 ms. Pan slop uses `PAGING_TOUCH_SLOP` (2× touch slop).
    pub fn android_defaults() -> Self {
        Self {
            touch_slop: 8.0,
            pan_slop: 16.0,
            pan_slop_vertical: 16.0,
            pan_slop_horizontal: 16.0,
            scale_slop: DEFAULT_SCALE_SLOP,
            double_tap_slop: 100.0,
            double_tap_timeout: Duration::from_millis(300),
            long_press_timeout: Duration::from_millis(400),
            min_fling_velocity: 50.0,
            max_fling_velocity: 8000.0,
        }
    }

    /// Native iOS feel.
    ///
    /// `touch_slop` is `UIGestureRecognizer.allowableMovement`'s 10 pt
    /// default — the only value Apple publishes. The remaining values are
    /// extrapolated (Apple does not document `UIScrollView` internals):
    /// pan slop 2× touch slop, Android-equivalent double-tap window, and the
    /// conventional 500 ms long-press.
    pub fn ios_defaults() -> Self {
        Self {
            touch_slop: 10.0,
            pan_slop: 20.0,
            pan_slop_vertical: 20.0,
            pan_slop_horizontal: 20.0,
            scale_slop: DEFAULT_SCALE_SLOP,
            double_tap_slop: 100.0,
            double_tap_timeout: Duration::from_millis(300),
            long_press_timeout: Duration::from_millis(500),
            min_fling_velocity: 50.0,
            max_fling_velocity: 8000.0,
        }
    }

    /// Create settings optimized for pen/stylus input.
    ///
    /// Uses medium tolerance values.
    pub fn pen_defaults() -> Self {
        Self {
            touch_slop: DEFAULT_PEN_SLOP,
            pan_slop: DEFAULT_PEN_SLOP,
            pan_slop_vertical: DEFAULT_PEN_SLOP,
            pan_slop_horizontal: DEFAULT_PEN_SLOP,
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
    /// ```rust
    /// use flui_interaction::settings::GestureSettings;
    /// use flui_platform_api::pointer::PointerKind;
    ///
    /// let settings = GestureSettings::for_device(PointerKind::Touch);
    /// ```
    pub fn for_device(device_kind: PointerKind) -> Self {
        match device_kind {
            PointerKind::Mouse => Self::mouse_defaults(),
            PointerKind::Pen { .. } => Self::pen_defaults(),
            // Touch, trackpad gestures and unknown kinds use touch defaults.
            _ => Self::touch_defaults(),
        }
    }

    // ========================================================================
    // Getters
    // ========================================================================

    /// Get the touch slop (maximum movement for a tap).
    #[inline]
    pub fn touch_slop(&self) -> f64 {
        self.touch_slop
    }

    /// Get the pan slop (minimum movement to start panning).
    #[inline]
    pub fn pan_slop(&self) -> f64 {
        self.pan_slop
    }

    /// The hit slop for `kind` — how far a pointer of that kind may drift
    /// before a gesture is rejected.
    ///
    /// [`PointerKind::Mouse`] is precise, so it gets a fixed small constant
    /// that no profile customises. Every other kind — `Pen` (tip or eraser),
    /// `Touch`, `Trackpad` and `Unknown` — resolves through this settings object's touch tier.
    /// **A pen is not precise under this rule**: that is deliberate (a
    /// stylus gets the touch tier), not an omission.
    ///
    /// **Read this rather than `touch_slop()` wherever a recognizer checks
    /// drift against the hit slop.** The rule was implemented separately in two
    /// recognizers before this existed, and a third copy would have been the
    /// point where they drifted: no *built-in* profile makes the two tiers
    /// coincide (a caller can of course build one with
    /// [`Self::try_with_touch_slop`]), so a recognizer that forgets the
    /// distinction looks fine in a default-profile test and is wrong on every
    /// shipped platform.
    #[inline]
    #[must_use]
    pub fn hit_slop(&self, kind: PointerKind) -> f64 {
        match kind {
            PointerKind::Mouse => DEFAULT_MOUSE_SLOP,
            _ => self.touch_slop(),
        }
    }

    /// The pan slop for `kind` — how far a pointer of that kind must move
    /// before a pan is recognised.
    ///
    /// Same rule as [`Self::hit_slop`], one tier up.
    ///
    /// The two tiers differ on every built-in platform profile —
    /// `android_defaults` is 8 against 16, `ios_defaults` 10 against 20 — and
    /// coincide only under `touch_defaults` (18 and 18). A recognizer reading
    /// the wrong one is therefore invisible in a default-profile test and
    /// wrong in production.
    #[inline]
    #[must_use]
    pub fn pan_slop_for(&self, kind: PointerKind) -> f64 {
        match kind {
            PointerKind::Mouse => DEFAULT_MOUSE_PAN_SLOP,
            _ => self.pan_slop(),
        }
    }

    /// Get the vertical-only pan slop (per-axis tolerance).
    ///
    /// Used by the vertical-drag recogniser to decide when a vertical
    /// drag crosses the acceptance threshold. Returns the same value as
    /// [`Self::pan_slop`] unless explicitly set via [`Self::try_with_pan_slop_vertical`].
    #[inline]
    pub fn pan_slop_vertical(&self) -> f64 {
        self.pan_slop_vertical
    }

    /// Get the horizontal-only pan slop (per-axis tolerance).
    ///
    /// See [`Self::pan_slop_vertical`] — same rationale, horizontal axis.
    #[inline]
    pub fn pan_slop_horizontal(&self) -> f64 {
        self.pan_slop_horizontal
    }

    /// Get the scale slop (minimum scale change to start scaling).
    ///
    /// A ratio, not a distance — see [`DEFAULT_SCALE_SLOP`]. Compare through
    /// [`Self::exceeds_scale_slop`] rather than against a measured distance.
    #[inline]
    pub fn scale_slop(&self) -> f64 {
        self.scale_slop
    }

    /// The span slop for `kind` — how far the *distance between two pointers*
    /// must change, in logical pixels, before a scale is recognised.
    ///
    /// Like [`Self::hit_slop`], it special-cases exactly the mouse as
    /// precise; unlike it, it reads no settings at all, so neither arm here
    /// is configurable.
    ///
    /// Distinct from [`Self::scale_slop`], which is a dimensionless ratio. The
    /// two are separate acceptance criteria, not two spellings of one: a pinch
    /// crosses this tier on absolute movement from a wide starting span long
    /// before it reaches 5%, and a small pinch crosses the ratio tier while
    /// barely moving.
    #[inline]
    pub fn span_slop_for(&self, kind: PointerKind) -> f64 {
        match kind {
            PointerKind::Mouse => DEFAULT_MOUSE_SPAN_SLOP,
            _ => DEFAULT_SPAN_SLOP,
        }
    }

    /// Get the double-tap slop (maximum distance between taps).
    #[inline]
    pub fn double_tap_slop(&self) -> f64 {
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
    pub fn min_fling_velocity(&self) -> f64 {
        self.min_fling_velocity
    }

    /// Get the maximum fling velocity. Never below
    /// [`Self::min_fling_velocity`].
    #[inline]
    pub fn max_fling_velocity(&self) -> f64 {
        self.max_fling_velocity
    }

    // ========================================================================
    // Builder-style setters
    // ========================================================================

    /// Set the touch slop.
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] for a NaN, infinite or negative
    /// slop.
    #[inline]
    pub fn try_with_touch_slop(mut self, slop: f64) -> Result<Self, GestureSettingsError> {
        self.touch_slop = checked("touch_slop", slop)?;
        Ok(self)
    }

    /// Set the pan slop.
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] for a NaN, infinite or negative
    /// slop.
    #[inline]
    pub fn try_with_pan_slop(mut self, slop: f64) -> Result<Self, GestureSettingsError> {
        self.pan_slop = checked("pan_slop", slop)?;
        Ok(self)
    }

    /// Set the vertical-only pan slop (per-axis tolerance).
    ///
    /// Independent of [`Self::try_with_pan_slop`] so callers can tune vertical
    /// drag without affecting free pan. Use this in vertical-only widgets
    /// (e.g. scroll views).
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] for a NaN, infinite or negative
    /// slop.
    #[inline]
    pub fn try_with_pan_slop_vertical(mut self, slop: f64) -> Result<Self, GestureSettingsError> {
        self.pan_slop_vertical = checked("pan_slop_vertical", slop)?;
        Ok(self)
    }

    /// Set the horizontal-only pan slop (per-axis tolerance).
    ///
    /// See [`Self::try_with_pan_slop_vertical`] — same rationale, horizontal
    /// axis.
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] for a NaN, infinite or negative
    /// slop.
    #[inline]
    pub fn try_with_pan_slop_horizontal(mut self, slop: f64) -> Result<Self, GestureSettingsError> {
        self.pan_slop_horizontal = checked("pan_slop_horizontal", slop)?;
        Ok(self)
    }

    /// Set the scale slop.
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] for a NaN, infinite or negative
    /// slop.
    #[inline]
    pub fn try_with_scale_slop(mut self, slop: f64) -> Result<Self, GestureSettingsError> {
        self.scale_slop = checked("scale_slop", slop)?;
        Ok(self)
    }

    /// Set the double-tap slop.
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] for a NaN, infinite or negative
    /// slop.
    #[inline]
    pub fn try_with_double_tap_slop(mut self, slop: f64) -> Result<Self, GestureSettingsError> {
        self.double_tap_slop = checked("double_tap_slop", slop)?;
        Ok(self)
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

    /// Set the fling velocity range, in px/s. The two bounds are set
    /// together so that no intermediate state has `min > max`.
    ///
    /// # Errors
    ///
    /// [`GestureSettingsError::InvalidValue`] for a NaN, infinite or negative
    /// bound; [`GestureSettingsError::InvertedFlingRange`] when `min > max`.
    #[inline]
    pub fn try_with_fling_velocity(
        mut self,
        min: f64,
        max: f64,
    ) -> Result<Self, GestureSettingsError> {
        (self.min_fling_velocity, self.max_fling_velocity) = checked_fling_range(min, max)?;
        Ok(self)
    }

    // ========================================================================
    // Utility methods
    // ========================================================================

    /// Check if a distance exceeds the touch slop.
    #[inline]
    pub fn exceeds_touch_slop(&self, distance: f64) -> bool {
        distance > self.touch_slop
    }

    /// Check if a distance exceeds the pan slop.
    #[inline]
    pub fn exceeds_pan_slop(&self, distance: f64) -> bool {
        distance > self.pan_slop
    }

    /// Check if a scale factor exceeds the scale slop.
    ///
    /// Scale slop is applied symmetrically around 1.0.
    #[inline]
    pub fn exceeds_scale_slop(&self, scale: f64) -> bool {
        (scale - 1.0).abs() > self.scale_slop
    }

    /// Clamp a signed release velocity (px/s along one axis) to at most
    /// [`Self::max_fling_velocity`] in magnitude, keeping its sign.
    ///
    /// Slow velocities are returned unchanged: whether a release is a fling
    /// at all is [`Self::is_fling_velocity`]'s question, and raising a slow
    /// release to the minimum would turn a gentle lift into a fling. A NaN
    /// velocity is no velocity (`0.0`); an infinite one is clamped like any
    /// other. Never panics.
    #[inline]
    #[must_use]
    pub fn clamp_fling_velocity(&self, velocity: f64) -> f64 {
        if velocity.is_nan() {
            return 0.0;
        }
        let max = self.max_fling_velocity();
        velocity.clamp(-max, max)
    }

    /// Check if a velocity is fast enough for a fling.
    #[inline]
    pub fn is_fling_velocity(&self, velocity: f64) -> bool {
        velocity.abs() >= self.min_fling_velocity
    }
}
