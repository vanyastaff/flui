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

use std::{cell::RefCell, rc::Rc, time::Duration};

use flui_foundation::geometry::{Offset, Size};
use flui_platform_api::pointer::PointerKind;
use flui_platform_api::{FlingSpeeds, GestureGeometry, GesturePreferences, TargetPlatform};

use crate::processing::VelocityEstimator;

/// Presentation-owned writer of the gesture projection of host preferences.
///
/// This capability has one owner. Consumers receive only [`Self::provider`].
/// Publishing invokes no callbacks and does not schedule frames; each recognizer
/// copies the current profile when admitting a new gesture.
#[derive(Debug)]
pub struct GestureSettingsSource {
    settings: Rc<RefCell<GestureSettings>>,
}

impl GestureSettingsSource {
    /// Seed the presentation's validated gesture profile.
    #[must_use]
    pub fn new(settings: GestureSettings) -> Self {
        Self {
            settings: Rc::new(RefCell::new(settings)),
        }
    }

    /// Grant a read-only owner-local capability to future admissions.
    #[must_use]
    pub fn provider(&self) -> GestureSettingsProvider {
        GestureSettingsProvider {
            profile: SettingsProfile::Live(self.settings.clone()),
        }
    }

    /// Replace the projection, returning whether its effective value changed.
    pub fn replace(&self, settings: GestureSettings) -> bool {
        let mut current = self.settings.borrow_mut();
        if *current == settings {
            return false;
        }
        *current = settings;
        true
    }
}

/// Read-only settings used by new gesture admissions.
///
/// A fixed authored profile remains independent of host updates. A live provider
/// shares one presentation's projection, retains its last value if the writer
/// retires, and cannot cross threads. Equality compares fixed values or the exact
/// live source identity, so publication does not replace an existing consumer.
#[derive(Debug, Clone, PartialEq)]
pub struct GestureSettingsProvider {
    profile: SettingsProfile,
}

#[derive(Debug, Clone)]
enum SettingsProfile {
    Fixed(Rc<GestureSettings>),
    Live(Rc<RefCell<GestureSettings>>),
}

impl PartialEq for SettingsProfile {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fixed(left), Self::Fixed(right)) => left == right,
            (Self::Live(left), Self::Live(right)) => Rc::ptr_eq(left, right),
            _ => false,
        }
    }
}

impl GestureSettingsProvider {
    /// Copy a profile without retaining a source borrow during user code.
    #[must_use]
    pub fn snapshot(&self) -> GestureSettings {
        match &self.profile {
            SettingsProfile::Fixed(settings) => settings.as_ref().clone(),
            SettingsProfile::Live(settings) => settings.borrow().clone(),
        }
    }
}

impl From<GestureSettings> for GestureSettingsProvider {
    fn from(settings: GestureSettings) -> Self {
        Self {
            profile: SettingsProfile::Fixed(Rc::new(settings)),
        }
    }
}

impl Default for GestureSettingsProvider {
    fn default() -> Self {
        GestureSettings::default().into()
    }
}

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

    /// Release-velocity policy captured when a gesture sequence begins.
    velocity_estimator: VelocityEstimator,
    native: NativeSettings,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct NativeSettings {
    mouse_interval: Option<Duration>,
    touch_interval: Option<Duration>,
    mouse_drag: Option<Size>,
    mouse_double_click: Option<Size>,
    touch: Option<TouchSlops>,
    touch_double_tap: Option<f64>,
    touch_fling: Option<FlingSpeeds>,
}

#[derive(Debug, Clone, PartialEq)]
struct TouchSlops {
    hit: f64,
    pan: f64,
    horizontal: f64,
    vertical: f64,
    span: f64,
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
    /// A baseline ratio and native distance have no finite representation.
    #[error(
        "native gesture setting `{field}` cannot represent baseline ratio {tier}/{hit} at {observed}"
    )]
    UnrepresentableProjection {
        /// Derived tier that could not be represented.
        field: &'static str,
        /// Authored tier distance.
        tier: f64,
        /// Authored hit distance.
        hit: f64,
        /// Observed native hit distance.
        observed: f64,
    },
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

fn project_tier(
    field: &'static str,
    tier: f64,
    hit: f64,
    observed: f64,
) -> Result<f64, GestureSettingsError> {
    if hit == 0.0 {
        return Ok(tier);
    }
    if observed == 0.0 || tier == 0.0 {
        return Ok(0.0);
    }
    let candidates = [
        (observed / hit) * tier,
        (tier / hit) * observed,
        (observed * tier) / hit,
    ];
    if let Some(value) = candidates
        .into_iter()
        .find(|value| value.is_finite() && *value > 0.0)
    {
        return Ok(value);
    }
    Err(GestureSettingsError::UnrepresentableProjection {
        field,
        tier,
        hit,
        observed,
    })
}

fn exceeds_tolerance(delta: Offset<f64>, rectangle: Option<Size>, distance: f64) -> bool {
    if !delta.is_finite() {
        return true;
    }
    rectangle.map_or_else(
        || delta.dx.hypot(delta.dy) > distance,
        |area| delta.dx.abs() > area.width || delta.dy.abs() > area.height,
    )
}

impl GestureSettings {
    /// Resolve accepted host timing and exact presentation geometry over a baseline.
    ///
    /// Missing observations restore baseline fields. Raw native host geometry is
    /// comparison-only: callers pass the exact presentation's logical geometry.
    /// `None` means an accepted unsupported/unknown query, not a failed query.
    /// Touch pan and span tiers retain their authored ratio to hit slop; a zero
    /// baseline hit distance keeps the authored derived tiers unchanged.
    ///
    /// # Errors
    /// Returns [`GestureSettingsError::UnrepresentableProjection`] if a derived
    /// touch distance cannot be represented as a finite nonnegative value.
    pub fn resolve_preferences(
        baseline: &Self,
        preferences: &GesturePreferences,
        geometry: Option<&GestureGeometry>,
    ) -> Result<Self, GestureSettingsError> {
        let mut settings = baseline.clone();
        if let Some(timeout) = preferences.long_press_timeout() {
            settings.long_press_timeout = timeout;
        }
        if let Some(timeout) = preferences.double_click_interval() {
            settings.native.mouse_interval = Some(timeout);
        }
        if let Some(timeout) = preferences.double_tap_interval() {
            settings.native.touch_interval = Some(timeout);
        }
        if let Some(geometry) = geometry {
            if let Some(area) = geometry.mouse_double_click_area() {
                settings.native.mouse_double_click =
                    Some(Size::new(area.width / 2.0, area.height / 2.0));
            }
            if let Some(tolerance) = geometry.mouse_drag_tolerance() {
                settings.native.mouse_drag = Some(tolerance);
            }
            if let Some(slop) = geometry.touch_slop() {
                let hit = baseline.hit_slop(PointerKind::Touch);
                let observed = slop.get();
                settings.native.touch = Some(TouchSlops {
                    hit: observed,
                    pan: project_tier(
                        "pan_slop",
                        baseline.pan_slop_for(PointerKind::Touch),
                        hit,
                        observed,
                    )?,
                    horizontal: project_tier(
                        "pan_slop_horizontal",
                        baseline.pan_slop_horizontal_for(PointerKind::Touch),
                        hit,
                        observed,
                    )?,
                    vertical: project_tier(
                        "pan_slop_vertical",
                        baseline.pan_slop_vertical_for(PointerKind::Touch),
                        hit,
                        observed,
                    )?,
                    span: project_tier(
                        "span_slop",
                        baseline.span_slop_for(PointerKind::Touch),
                        hit,
                        observed,
                    )?,
                });
            }
            if let Some(slop) = geometry.touch_double_tap_slop() {
                settings.native.touch_double_tap = Some(slop.get());
            }
            if let Some(speeds) = geometry.fling_speeds() {
                settings.native.touch_fling = Some(speeds);
            }
        }
        Ok(settings)
    }

    /// Whether a displacement exceeds the admitted device's tap/hold tolerance.
    /// Mouse native tolerances are closed axis rectangles, never radial estimates.
    #[must_use]
    pub fn exceeds_hit_slop(&self, kind: PointerKind, delta: Offset<f64>) -> bool {
        exceeds_tolerance(delta, self.mouse_drag(kind), self.hit_slop(kind))
    }

    /// Whether free-plane movement exceeds the admitted pan tolerance.
    #[must_use]
    pub fn exceeds_pan_slop_for(&self, kind: PointerKind, delta: Offset<f64>) -> bool {
        exceeds_tolerance(delta, self.mouse_drag(kind), self.pan_slop_for(kind))
    }

    /// Whether the next contact lies outside its candidate's double-tap tolerance.
    #[must_use]
    pub fn exceeds_double_tap_slop(&self, kind: PointerKind, delta: Offset<f64>) -> bool {
        let rectangle = (kind == PointerKind::Mouse)
            .then_some(self.native.mouse_double_click)
            .flatten();
        let distance = if kind == PointerKind::Touch {
            self.native.touch_double_tap.unwrap_or(self.double_tap_slop)
        } else {
            self.double_tap_slop
        };
        exceeds_tolerance(delta, rectangle, distance)
    }

    fn mouse_drag(&self, kind: PointerKind) -> Option<Size> {
        if kind == PointerKind::Mouse {
            self.native.mouse_drag
        } else {
            None
        }
    }

    /// Per-kind double-tap interval, distinct from an authored universal timeout.
    #[must_use]
    pub fn double_tap_timeout_for(&self, kind: PointerKind) -> Duration {
        match kind {
            PointerKind::Mouse => self
                .native
                .mouse_interval
                .unwrap_or(self.double_tap_timeout),
            PointerKind::Touch => self
                .native
                .touch_interval
                .unwrap_or(self.double_tap_timeout),
            _ => self.double_tap_timeout,
        }
    }

    pub(crate) fn double_tap_uses_down_time(&self, kind: PointerKind) -> bool {
        kind == PointerKind::Mouse && self.native.mouse_interval.is_some()
    }

    pub(crate) fn resolve_fling_velocity(
        &self,
        kind: PointerKind,
        velocity: crate::Velocity,
    ) -> crate::Velocity {
        let (min, max) = if kind == PointerKind::Touch {
            self.native.touch_fling.map_or(
                (self.min_fling_velocity, self.max_fling_velocity),
                |speeds| (speeds.min(), speeds.max()),
            )
        } else {
            (self.min_fling_velocity, self.max_fling_velocity)
        };
        if velocity.magnitude() < min {
            crate::Velocity::ZERO
        } else {
            velocity.clamp_magnitude(0.0, max)
        }
    }
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
            velocity_estimator: VelocityEstimator::LeastSquares,
            native: NativeSettings::default(),
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
            velocity_estimator: VelocityEstimator::LeastSquares,
            native: NativeSettings::default(),
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
            velocity_estimator: VelocityEstimator::LeastSquares,
            native: NativeSettings::default(),
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
            velocity_estimator: VelocityEstimator::LeastSquares,
            native: NativeSettings::default(),
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
            velocity_estimator: VelocityEstimator::LeastSquares,
            native: NativeSettings::default(),
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
            velocity_estimator: VelocityEstimator::LeastSquares,
            native: NativeSettings::default(),
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

    /// Release-velocity algorithm captured by a new gesture sequence.
    #[must_use]
    pub const fn velocity_estimator(&self) -> VelocityEstimator {
        self.velocity_estimator
    }

    /// Choose how a new gesture estimates its release velocity.
    ///
    /// Existing contacts keep their captured settings. All built-in settings
    /// profiles use least squares unless the caller explicitly selects another
    /// algorithm; platform thresholds and algorithm selection are independent.
    #[must_use]
    pub const fn with_velocity_estimator(mut self, estimator: VelocityEstimator) -> Self {
        self.velocity_estimator = estimator;
        self
    }

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
    /// Touch uses its observed logical hit distance when available. Pen,
    /// trackpad and unknown input retain the authored touch tier. Mouse returns
    /// its scalar fallback; its observed axis rectangle cannot be expressed as
    /// one distance. Recognizers compare movement through
    /// [`Self::exceeds_hit_slop`] to preserve that rectangle.
    #[inline]
    #[must_use]
    pub fn hit_slop(&self, kind: PointerKind) -> f64 {
        match kind {
            PointerKind::Mouse => DEFAULT_MOUSE_SLOP,
            PointerKind::Touch => self
                .native
                .touch
                .as_ref()
                .map_or(self.touch_slop, |slops| slops.hit),
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
            PointerKind::Touch => self
                .native
                .touch
                .as_ref()
                .map_or(self.pan_slop, |slops| slops.pan),
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

    /// Vertical pan distance, using the native mouse rectangle's vertical axis.
    #[must_use]
    pub fn pan_slop_vertical_for(&self, kind: PointerKind) -> f64 {
        match kind {
            PointerKind::Mouse => self
                .native
                .mouse_drag
                .map_or(DEFAULT_MOUSE_SLOP, |area| area.height),
            PointerKind::Touch => self
                .native
                .touch
                .as_ref()
                .map_or(self.pan_slop_vertical, |slops| slops.vertical),
            _ => self.pan_slop_vertical,
        }
    }

    /// Get the horizontal-only pan slop (per-axis tolerance).
    ///
    /// See [`Self::pan_slop_vertical`] — same rationale, horizontal axis.
    #[inline]
    pub fn pan_slop_horizontal(&self) -> f64 {
        self.pan_slop_horizontal
    }

    /// Horizontal pan distance, using the native mouse rectangle's horizontal axis.
    #[must_use]
    pub fn pan_slop_horizontal_for(&self, kind: PointerKind) -> f64 {
        match kind {
            PointerKind::Mouse => self
                .native
                .mouse_drag
                .map_or(DEFAULT_MOUSE_SLOP, |area| area.width),
            PointerKind::Touch => self
                .native
                .touch
                .as_ref()
                .map_or(self.pan_slop_horizontal, |slops| slops.horizontal),
            _ => self.pan_slop_horizontal,
        }
    }

    /// Get the scale slop (minimum scale change to start scaling).
    ///
    /// A ratio, not a distance — see [`DEFAULT_SCALE_SLOP`]. Compare through
    /// [`Self::exceeds_scale_slop`] rather than against a measured distance.
    #[inline]
    pub fn scale_slop(&self) -> f64 {
        self.scale_slop
    }

    /// The span slop for `kind` — how far the mean distance of the contacts from
    /// their focal point must change, in logical pixels, before a scale is recognised.
    ///
    /// Mouse retains its precise scalar tier. An observed touch hit distance
    /// scales the baseline touch span tier by the same ratio as the other touch
    /// distances. Other kinds retain their baseline tier.
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
            PointerKind::Touch => self
                .native
                .touch
                .as_ref()
                .map_or(DEFAULT_SPAN_SLOP, |slops| slops.span),
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
        self.native.touch = None;
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
        self.native.touch = None;
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
        self.native.touch = None;
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
        self.native.touch = None;
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
        self.native.mouse_double_click = None;
        self.native.touch_double_tap = None;
        Ok(self)
    }

    /// Set the double-tap timeout.
    #[inline]
    pub fn with_double_tap_timeout(mut self, timeout: Duration) -> Self {
        self.double_tap_timeout = timeout;
        self.native.mouse_interval = None;
        self.native.touch_interval = None;
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
        self.native.touch_fling = None;
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
