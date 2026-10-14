//! Gesture behavior values: platform presets, host observations, overrides.
//!
//! A value resolves in ADR-0212's order: the OS observation where the host has
//! one, else [`GestureSettings::preset`] for the platform and pointer kind, then
//! an explicit subtree override; the OS accessibility timings are floors last.
//!
//! # Example
//!
//! ```rust
//! use flui_interaction::gesture::GestureSettings;
//! use flui_platform_api::{TargetPlatform, pointer::PointerKind};
//!
//! // AOSP `ViewConfiguration.TOUCH_SLOP`.
//! let touch = GestureSettings::preset(TargetPlatform::Android, PointerKind::Touch);
//! assert_eq!(touch.touch_slop(), 8.0);
//!
//! // A mouse falls back to Android's fine-pointer row.
//! let mouse = GestureSettings::preset(TargetPlatform::Android, PointerKind::Mouse);
//! assert_eq!(mouse.touch_slop(), 1.0);
//! ```

use std::{cell::RefCell, num::NonZeroU32, rc::Rc, time::Duration};

use flui_foundation::geometry::{Offset, Size};
use flui_platform_api::pointer::PointerKind;
use flui_platform_api::{FlingSpeeds, GestureGeometry, GesturePreferences, TargetPlatform};

use crate::gesture::{OneEuroTuning, VelocityEstimator};

/// The gesture values a presentation resolves for each kind of pointer
/// (ADR-0212 §4).
///
/// A recognizer admits a contact with its kind's profile, and a hit test
/// sizes an expanded area to its kind's minimum target, so a mouse and a
/// finger on the same window each get their own values.
///
/// ```rust
/// use flui_interaction::gesture::{GestureProfiles, GestureSettings};
/// use flui_platform_api::{TargetPlatform, pointer::PointerKind};
///
/// let profiles = GestureProfiles::preset(TargetPlatform::Android);
/// assert_eq!(
///     profiles.for_kind(PointerKind::Mouse),
///     &GestureSettings::preset(TargetPlatform::Android, PointerKind::Mouse),
/// );
/// assert!(
///     profiles.for_kind(PointerKind::Touch).min_target()
///         > profiles.for_kind(PointerKind::Mouse).min_target()
/// );
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct GestureProfiles {
    by_kind: Box<[GestureSettings; KIND_SLOTS]>,
}

const KIND_SLOTS: usize = 6;

/// One representative kind per slot: a pen's tip and eraser share one row.
const SLOT_KINDS: [PointerKind; KIND_SLOTS] = [
    PointerKind::Mouse,
    PointerKind::Touch,
    PointerKind::Pen {
        tool: flui_platform_api::pointer::PenTool::Tip,
    },
    PointerKind::Trackpad,
    PointerKind::Virtual,
    PointerKind::Unknown,
];

const fn kind_slot(kind: PointerKind) -> usize {
    match kind {
        PointerKind::Mouse => 0,
        PointerKind::Touch => 1,
        PointerKind::Pen { .. } => 2,
        PointerKind::Trackpad => 3,
        PointerKind::Virtual => 4,
        // Unknown, and a kind added later until it gets a row of its own.
        _ => 5,
    }
}

impl GestureProfiles {
    /// [`GestureSettings::preset`] for `platform` and every pointer kind.
    #[must_use]
    pub fn preset(platform: TargetPlatform) -> Self {
        Self {
            by_kind: Box::new(SLOT_KINDS.map(|kind| GestureSettings::preset(platform, kind))),
        }
    }

    /// One profile for every kind: an explicit choice, as a headless test
    /// that names its preset makes.
    #[must_use]
    pub fn uniform(settings: GestureSettings) -> Self {
        Self {
            by_kind: Box::new(std::array::from_fn(|_| settings.clone())),
        }
    }

    /// The profile a pointer of `kind` is admitted with.
    #[must_use]
    pub fn for_kind(&self, kind: PointerKind) -> &GestureSettings {
        &self.by_kind[kind_slot(kind)]
    }

    /// Every kind's profile with the host's observations applied
    /// ([`GestureSettings::resolve_preferences`]).
    ///
    /// # Errors
    ///
    /// The first [`GestureSettingsError`] any kind's resolution reports.
    pub fn resolve_preferences(
        &self,
        preferences: &GesturePreferences,
        geometry: Option<&GestureGeometry>,
    ) -> Result<Self, GestureSettingsError> {
        let mut resolved = self.clone();
        for settings in resolved.by_kind.iter_mut() {
            *settings = GestureSettings::resolve_preferences(settings, preferences, geometry)?;
        }
        Ok(resolved)
    }
}

impl From<GestureSettings> for GestureProfiles {
    fn from(settings: GestureSettings) -> Self {
        Self::uniform(settings)
    }
}

/// Presentation-owned writer of the gesture projection of host preferences.
///
/// This capability has one owner. Consumers receive only [`Self::provider`].
/// Publishing invokes no callbacks and does not schedule frames; each recognizer
/// copies its contact kind's current profile when admitting a new gesture.
#[derive(Debug)]
pub struct GestureSettingsSource {
    profiles: Rc<RefCell<GestureProfiles>>,
}

impl GestureSettingsSource {
    /// Seed the presentation's validated gesture profiles.
    #[must_use]
    pub fn new(profiles: impl Into<GestureProfiles>) -> Self {
        Self {
            profiles: Rc::new(RefCell::new(profiles.into())),
        }
    }

    /// Grant a read-only owner-local capability to future admissions.
    #[must_use]
    pub fn provider(&self) -> GestureSettingsProvider {
        GestureSettingsProvider {
            profile: SettingsProfile::Live(self.profiles.clone()),
        }
    }

    /// Replace the projection, returning whether its effective value changed.
    pub fn replace(&self, profiles: impl Into<GestureProfiles>) -> bool {
        let profiles = profiles.into();
        let mut current = self.profiles.borrow_mut();
        if *current == profiles {
            return false;
        }
        *current = profiles;
        true
    }
}

/// Read-only settings used by new gesture admissions.
///
/// A fixed authored profile remains independent of host updates. A live provider
/// shares one presentation's projection, retains its last value if the writer
/// retires, and cannot cross threads. Equality compares fixed values or the exact
/// live source identity, so publication does not replace an existing consumer.
/// Every read names the pointer kind it is for (ADR-0212 §4).
#[derive(Debug, Clone, PartialEq)]
pub struct GestureSettingsProvider {
    profile: SettingsProfile,
}

#[derive(Debug, Clone)]
enum SettingsProfile {
    Fixed(Rc<GestureProfiles>),
    Live(Rc<RefCell<GestureProfiles>>),
    /// An explicit subtree override over the provider it replaces, which still
    /// supplies the accessibility floors.
    Override {
        authored: Rc<SettingsProfile>,
        inherited: Rc<SettingsProfile>,
    },
}

impl PartialEq for SettingsProfile {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fixed(left), Self::Fixed(right)) => left == right,
            (Self::Live(left), Self::Live(right)) => Rc::ptr_eq(left, right),
            (
                Self::Override {
                    authored: left_authored,
                    inherited: left_inherited,
                },
                Self::Override {
                    authored: right_authored,
                    inherited: right_inherited,
                },
            ) => left_authored == right_authored && left_inherited == right_inherited,
            _ => false,
        }
    }
}

impl SettingsProfile {
    /// The floors are timings only, so an override's target is its authored one.
    fn min_target(&self, kind: PointerKind) -> f64 {
        match self {
            Self::Fixed(profiles) => profiles.for_kind(kind).min_target,
            Self::Live(profiles) => profiles.borrow().for_kind(kind).min_target,
            Self::Override { authored, .. } => authored.min_target(kind),
        }
    }

    fn snapshot(&self, kind: PointerKind) -> GestureSettings {
        match self {
            Self::Fixed(profiles) => profiles.for_kind(kind).clone(),
            Self::Live(profiles) => profiles.borrow().for_kind(kind).clone(),
            Self::Override {
                authored,
                inherited,
            } => {
                let floors = inherited.snapshot(kind).floors;
                let mut settings = authored.snapshot(kind);
                settings.floors = settings.floors.max(floors);
                settings
            }
        }
    }
}

impl GestureSettingsProvider {
    /// Copy the profile a contact of `kind` is admitted with, without
    /// retaining a source borrow during user code.
    #[must_use]
    pub fn snapshot_for(&self, kind: PointerKind) -> GestureSettings {
        self.profile.snapshot(kind)
    }

    /// The current [`GestureSettings::min_target`] for a pointer of `kind`,
    /// read without copying the profile, so a hit test can follow live host
    /// updates.
    #[must_use]
    pub fn min_target_for(&self, kind: PointerKind) -> f64 {
        self.profile.min_target(kind)
    }

    /// An explicit subtree override that still meets this provider's
    /// accessibility timing floors (ADR-0212).
    ///
    /// Every value comes from `authored`; the long-press and double-tap/click
    /// floors the host observed through `self` raise a shorter authored timing
    /// for each new admission, and follow later host observations.
    #[must_use]
    pub fn overridden_by(&self, authored: impl Into<Self>) -> Self {
        Self {
            profile: SettingsProfile::Override {
                authored: Rc::new(authored.into().profile),
                inherited: Rc::new(self.profile.clone()),
            },
        }
    }
}

impl From<GestureSettings> for GestureSettingsProvider {
    /// One authored profile for every pointer kind.
    fn from(settings: GestureSettings) -> Self {
        GestureProfiles::uniform(settings).into()
    }
}

impl From<GestureProfiles> for GestureSettingsProvider {
    fn from(profiles: GestureProfiles) -> Self {
        Self {
            profile: SettingsProfile::Fixed(Rc::new(profiles)),
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
/// use flui_interaction::gesture::GestureSettings;
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
    /// Post-recognition smoothing of reported positions, captured with the sequence.
    position_smoothing: Option<OneEuroTuning>,
    native: NativeSettings,

    /// Side of the smallest target a pointer of this class can hit reliably.
    min_target: f64,
    /// Dwell over a drop target before it opens (spring-loading).
    spring_loading_delay: Duration,
    /// Width of the edge strip that starts an edge swipe.
    swipe_edge_width: f64,
    /// When a scroll remainder moves to an ancestor (ADR-0209 §6).
    remainder_handoff: RemainderHandoff,
    /// Idle time that ends a contactless scroll sequence (ADR-0209 §5).
    contactless_latch_idle: Duration,
    /// Idle time that ends a phased contactless sequence (ADR-0209 §5).
    phased_latch_idle: Duration,
    /// Ceiling on the summed deadlines of one composition path (ADR-0208).
    max_resolution_delay: Duration,
    /// Refresh intervals a frame-queued haptic waits (ADR-0210 §4).
    haptic_frame_deadline_factor: NonZeroU32,
    /// OS accessibility timings no authored value may shorten.
    floors: TimingFloors,
}

/// Where a scroll remainder goes once the latched leaf reaches its edge.
///
/// A preset value (ADR-0209 §6, ADR-0212): the remainder rule is a closed
/// policy, never a callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RemainderHandoff {
    /// The latch holds for the whole input sequence; an ancestor moves on the
    /// next sequence.
    NextSequence,
    /// A latched leaf that consumes nothing at its edge re-latches the chain to
    /// the nearest consuming ancestor within the same sequence, never back.
    AtEdgeWithinSequence,
}

/// The precision class a preset falls back to when its platform has no row
/// for a pointer kind (ADR-0212 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PointerClass {
    /// A finger, or a device the platform did not name.
    Coarse,
    /// A mouse, pen or trackpad.
    Fine,
}

impl PointerClass {
    /// The class of `kind`: touch, virtual and unknown devices are coarse
    /// (a gamepad cursor or an XR ray aims no better than a finger); mouse,
    /// pen and trackpad are fine.
    #[must_use]
    pub const fn of(kind: PointerKind) -> Self {
        match kind {
            PointerKind::Mouse | PointerKind::Pen { .. } | PointerKind::Trackpad => Self::Fine,
            // Touch, Virtual and Unknown; a kind added later is coarse until
            // it is classified above.
            _ => Self::Coarse,
        }
    }
}

/// OS-published accessibility timings that act as lower bounds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct TimingFloors {
    long_press: Option<Duration>,
    double_tap: Option<Duration>,
    double_click: Option<Duration>,
}

impl TimingFloors {
    fn max(self, other: Self) -> Self {
        let max = |left: Option<Duration>, right: Option<Duration>| match (left, right) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        Self {
            long_press: max(self.long_press, other.long_press),
            double_tap: max(self.double_tap, other.double_tap),
            double_click: max(self.double_click, other.double_click),
        }
    }
}

fn floored(value: Duration, floor: Option<Duration>) -> Duration {
    floor.map_or(value, |floor| value.max(floor))
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
    /// The platform-neutral touch preset,
    /// `preset(TargetPlatform::Unknown, PointerKind::Touch)`. Never the host's
    /// platform, so a value built from it is the same on every machine.
    fn default() -> Self {
        Self::preset(TargetPlatform::Unknown, PointerKind::Touch)
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
    /// The resolution order is ADR-0212's: an OS observation replaces the
    /// baseline (the preset) value, and an explicit override applied to the
    /// result replaces both. The observed long-press, double-tap and
    /// double-click timings are also accessibility floors: a later override
    /// may lengthen them but a shorter one is raised to the floor.
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
        settings.floors = TimingFloors {
            long_press: preferences.long_press_timeout(),
            double_tap: preferences.double_tap_interval(),
            double_click: preferences.double_click_interval(),
        };
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
    ///
    /// Mouse and touch never fall below the OS double-click and double-tap
    /// intervals the host observed (ADR-0212 §3).
    #[must_use]
    pub fn double_tap_timeout_for(&self, kind: PointerKind) -> Duration {
        match kind {
            PointerKind::Mouse => floored(
                self.native
                    .mouse_interval
                    .unwrap_or(self.double_tap_timeout),
                self.floors.double_click,
            ),
            PointerKind::Touch => floored(
                self.native
                    .touch_interval
                    .unwrap_or(self.double_tap_timeout),
                self.floors.double_tap,
            ),
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
            spring_loading_delay: long_press_timeout,
            min_fling_velocity,
            max_fling_velocity,
            ..Self::default()
        })
    }

    // ========================================================================
    // Presets
    // ========================================================================

    /// The behavior values a platform uses for one pointer kind (ADR-0212 §4).
    ///
    /// Lookup falls back `(platform, kind)` → `(platform, PointerClass::of(kind))`
    /// → `platform`. A preset is plain values only; the host's observations
    /// ([`Self::resolve_preferences`]) replace it where the OS publishes a value,
    /// and an explicit subtree override replaces both.
    ///
    /// The platform is a value, never inferred from the build host: web, tests
    /// and embedders choose it at run time, and a headless test names it so its
    /// outcome is the same on every machine. Every number cites its source on
    /// the row that sets it; a row without a published or measured source keeps
    /// the framework's existing default and says so.
    #[must_use]
    pub fn preset(platform: TargetPlatform, kind: PointerKind) -> Self {
        Self::kind_row(platform, kind)
            .or_else(|| Self::class_row(platform, PointerClass::of(kind)))
            .unwrap_or_else(|| Self::platform_row(platform))
    }

    /// Rows for one pointer kind, where a platform treats it apart from its
    /// class.
    fn kind_row(platform: TargetPlatform, kind: PointerKind) -> Option<Self> {
        match (platform, kind) {
            // Android Views apply `ViewConfiguration.TOUCH_SLOP` to every
            // contact, stylus included: a pen keeps the touch numbers there.
            (TargetPlatform::Android, PointerKind::Pen { .. }) => Some(Self::android_touch()),
            (
                TargetPlatform::iOS
                | TargetPlatform::MacOS
                | TargetPlatform::Windows
                | TargetPlatform::Linux
                | TargetPlatform::Unknown,
                PointerKind::Pen { .. },
            ) => Self::class_row(platform, PointerClass::Fine).map(Self::pen_distances),
            _ => None,
        }
    }

    /// Rows for a precision class. Fuchsia has none.
    fn class_row(platform: TargetPlatform, class: PointerClass) -> Option<Self> {
        Some(match (platform, class) {
            (TargetPlatform::Android, PointerClass::Coarse) => Self::android_touch(),
            (TargetPlatform::Android, PointerClass::Fine) => Self::android_fine(),
            (TargetPlatform::iOS, PointerClass::Coarse) => Self::ios_touch(),
            (TargetPlatform::iOS, PointerClass::Fine) => Self::ios_fine(),
            (TargetPlatform::MacOS | TargetPlatform::Unknown, PointerClass::Fine) => {
                Self::fine_baseline()
            }
            (TargetPlatform::Windows, PointerClass::Fine) => Self::windows_fine(),
            (TargetPlatform::Linux, PointerClass::Fine) => Self::linux_fine(),
            (
                TargetPlatform::MacOS
                | TargetPlatform::Windows
                | TargetPlatform::Linux
                | TargetPlatform::Unknown,
                PointerClass::Coarse,
            ) => Self::coarse_baseline(),
            _ => return None,
        })
    }

    /// The last fallback: a platform's values for any kind it has no row for.
    fn platform_row(platform: TargetPlatform) -> Self {
        match platform {
            // Fuchsia keeps the Android-like feel it has always used.
            TargetPlatform::Android | TargetPlatform::Fuchsia => Self::android_touch(),
            TargetPlatform::iOS => Self::ios_touch(),
            TargetPlatform::MacOS => Self::fine_baseline(),
            _ => Self::coarse_baseline(),
        }
    }

    /// The framework's coarse-pointer values, used where a platform publishes
    /// none for touch (Windows, Linux and macOS publish no touch slop).
    ///
    /// - Slops: touch, pan and per-axis 18 px; scale ratio 0.05; double-tap
    ///   distance 100 px; double-tap 300 ms; long press 500 ms; fling
    ///   50–8000 px/s — the framework's existing defaults, pending an
    ///   on-device measurement.
    /// - `min_target` 44 px: WCAG 2.2 success criterion 2.5.5, Target Size
    ///   (Enhanced), 44 × 44 CSS px.
    /// - `swipe_edge_width` 20 px: the existing back-gesture strip width,
    ///   pending measurement.
    /// - `spring_loading_delay`: this row's long-press timeout. No platform
    ///   publishes a spring-loading dwell; holding over a target opens it after
    ///   the same dwell that opens a held menu.
    /// - `remainder_handoff`: [`RemainderHandoff::NextSequence`], ADR-0209 §6's
    ///   default; a mid-sequence hand-off is set per platform only after an
    ///   on-device confirmation.
    /// - `contactless_latch_idle` 1500 ms: Firefox's
    ///   `mousewheel.transaction.timeout` default. `phased_latch_idle` is
    ///   twice that, since a phased source may begin without ending (ADR-0209
    ///   §5); a framework value pending measurement.
    /// - `max_resolution_delay` 1500 ms: a framework ceiling (ADR-0208) that
    ///   admits a long press after a double-tap interval on every row, pending
    ///   measurement.
    /// - `haptic_frame_deadline_factor` 2: two refresh intervals of the
    ///   window's display, ADR-0210 §4.
    fn coarse_baseline() -> Self {
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
            position_smoothing: None,
            native: NativeSettings::default(),
            min_target: 44.0,
            spring_loading_delay: DEFAULT_LONG_PRESS_TIMEOUT,
            swipe_edge_width: 20.0,
            remainder_handoff: RemainderHandoff::NextSequence,
            contactless_latch_idle: Duration::from_millis(1500),
            phased_latch_idle: Duration::from_millis(3000),
            max_resolution_delay: Duration::from_millis(1500),
            haptic_frame_deadline_factor: NonZeroU32::new(2).expect("BUG: literal is nonzero"),
            floors: TimingFloors::default(),
        }
    }

    /// The framework's fine-pointer values: the coarse baseline with
    /// precise-pointer distances.
    ///
    /// - Hit and per-axis slop 1 px, free-pan slop 2 px: the existing
    ///   precise-pointer defaults ([`DEFAULT_MOUSE_SLOP`],
    ///   [`DEFAULT_MOUSE_PAN_SLOP`]), pending measurement.
    /// - `min_target` 24 px: WCAG 2.2 success criterion 2.5.8, Target Size
    ///   (Minimum), 24 × 24 CSS px.
    fn fine_baseline() -> Self {
        Self {
            touch_slop: DEFAULT_MOUSE_SLOP,
            pan_slop: DEFAULT_MOUSE_PAN_SLOP,
            pan_slop_vertical: DEFAULT_MOUSE_SLOP,
            pan_slop_horizontal: DEFAULT_MOUSE_SLOP,
            min_target: 24.0,
            ..Self::coarse_baseline()
        }
    }

    /// A pen on a platform that publishes no stylus slop: the fine row with
    /// the existing 8 px pen distances ([`DEFAULT_PEN_SLOP`]), pending
    /// measurement.
    fn pen_distances(self) -> Self {
        Self {
            touch_slop: DEFAULT_PEN_SLOP,
            pan_slop: DEFAULT_PEN_SLOP,
            pan_slop_vertical: DEFAULT_PEN_SLOP,
            pan_slop_horizontal: DEFAULT_PEN_SLOP,
            ..self
        }
    }

    /// Android touch, from AOSP `ViewConfiguration`
    /// (`frameworks/base/core/java/android/view/ViewConfiguration.java`), in
    /// dp ≡ logical px:
    ///
    /// - `TOUCH_SLOP` 8; pan slop `PAGING_TOUCH_SLOP` = `TOUCH_SLOP * 2` = 16.
    /// - `DOUBLE_TAP_SLOP` 100; `DOUBLE_TAP_TIMEOUT` 300 ms;
    ///   `DEFAULT_LONG_PRESS_TIMEOUT` 400 ms.
    /// - `MINIMUM_FLING_VELOCITY` 50 and `MAXIMUM_FLING_VELOCITY` 8000 dp/s.
    /// - `min_target` 48: Android's accessibility guidance, touch targets of at
    ///   least 48 × 48 dp.
    /// - `swipe_edge_width` 20: AndroidX `ViewDragHelper.EDGE_SIZE`, 20 dp.
    /// - Remaining values: [`Self::coarse_baseline`]'s, with the spring-loading
    ///   dwell following this row's long press.
    fn android_touch() -> Self {
        let long_press = Duration::from_millis(400);
        Self {
            touch_slop: 8.0,
            pan_slop: 16.0,
            pan_slop_vertical: 16.0,
            pan_slop_horizontal: 16.0,
            double_tap_slop: 100.0,
            double_tap_timeout: Duration::from_millis(300),
            long_press_timeout: long_press,
            spring_loading_delay: long_press,
            min_fling_velocity: 50.0,
            max_fling_velocity: 8000.0,
            min_target: 48.0,
            swipe_edge_width: 20.0,
            ..Self::coarse_baseline()
        }
    }

    /// Android mouse and trackpad: [`Self::fine_baseline`]'s distances with
    /// AOSP's timings (`DOUBLE_TAP_TIMEOUT` 300 ms,
    /// `DEFAULT_LONG_PRESS_TIMEOUT` 400 ms), which `ViewConfiguration`
    /// applies to every pointer.
    fn android_fine() -> Self {
        let touch = Self::android_touch();
        Self {
            double_tap_timeout: touch.double_tap_timeout,
            long_press_timeout: touch.long_press_timeout,
            spring_loading_delay: touch.spring_loading_delay,
            ..Self::fine_baseline()
        }
    }

    /// iOS touch. Apple publishes two of these numbers:
    ///
    /// - `touch_slop` 10 pt: `UILongPressGestureRecognizer.allowableMovement`'s
    ///   documented default.
    /// - `long_press_timeout` 500 ms:
    ///   `UILongPressGestureRecognizer.minimumPressDuration`'s documented
    ///   default, 0.5 s.
    /// - `min_target` 44 pt: the Human Interface Guidelines' minimum hit
    ///   target, 44 × 44 pt.
    /// - Pan slop 20 pt, double-tap distance 100 pt, double-tap 300 ms, fling
    ///   50–8000 pt/s and the 20 pt edge strip are the framework's existing
    ///   defaults pending an on-device measurement; the remaining values are
    ///   [`Self::coarse_baseline`]'s.
    fn ios_touch() -> Self {
        Self {
            touch_slop: 10.0,
            pan_slop: 20.0,
            pan_slop_vertical: 20.0,
            pan_slop_horizontal: 20.0,
            long_press_timeout: Duration::from_millis(500),
            spring_loading_delay: Duration::from_millis(500),
            min_target: 44.0,
            ..Self::coarse_baseline()
        }
    }

    /// iPadOS pointer: [`Self::fine_baseline`] with UIKit's documented 0.5 s
    /// `minimumPressDuration`, which applies to every touch type.
    fn ios_fine() -> Self {
        Self {
            long_press_timeout: Duration::from_millis(500),
            spring_loading_delay: Duration::from_millis(500),
            ..Self::fine_baseline()
        }
    }

    /// Windows mouse: [`Self::fine_baseline`] with `GetDoubleClickTime`'s
    /// documented 500 ms default double-click interval. The host's observed
    /// interval and double-click rectangle replace these when it reads them.
    fn windows_fine() -> Self {
        Self {
            double_tap_timeout: Duration::from_millis(500),
            ..Self::fine_baseline()
        }
    }

    /// Linux mouse: [`Self::fine_baseline`] with GTK's documented
    /// `gtk-double-click-time` default, 400 ms.
    fn linux_fine() -> Self {
        Self {
            double_tap_timeout: Duration::from_millis(400),
            ..Self::fine_baseline()
        }
    }

    // ========================================================================
    // Getters
    // ========================================================================

    /// Side of the smallest square target, in logical pixels, that this
    /// pointer class can hit reliably.
    #[must_use]
    pub const fn min_target(&self) -> f64 {
        self.min_target
    }

    /// How long a drag dwells over a drop target before it opens.
    #[must_use]
    pub const fn spring_loading_delay(&self) -> Duration {
        self.spring_loading_delay
    }

    /// Width, in logical pixels, of the edge strip that starts an edge swipe.
    #[must_use]
    pub const fn swipe_edge_width(&self) -> f64 {
        self.swipe_edge_width
    }

    /// Where a scroll remainder goes when the latched leaf reaches its edge.
    #[must_use]
    pub const fn remainder_handoff(&self) -> RemainderHandoff {
        self.remainder_handoff
    }

    /// Idle time after which a contactless scroll sequence (wheel, keyboard,
    /// assistive, autoscroll) ends.
    #[must_use]
    pub const fn contactless_latch_idle(&self) -> Duration {
        self.contactless_latch_idle
    }

    /// Idle time after which a phased contactless sequence ends without its
    /// end phase.
    #[must_use]
    pub const fn phased_latch_idle(&self) -> Duration {
        self.phased_latch_idle
    }

    /// Ceiling on the summed deadlines of any root-to-leaf composition path.
    #[must_use]
    pub const fn max_resolution_delay(&self) -> Duration {
        self.max_resolution_delay
    }

    /// Refresh intervals of the window's display that a frame-queued haptic
    /// request may wait for its frame.
    #[must_use]
    pub const fn haptic_frame_deadline_factor(&self) -> NonZeroU32 {
        self.haptic_frame_deadline_factor
    }

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

    /// One Euro smoothing a new gesture applies to the positions it reports
    /// after recognition, or `None` (every preset) for raw positions.
    #[must_use]
    pub const fn position_smoothing(&self) -> Option<OneEuroTuning> {
        self.position_smoothing
    }

    /// Choose whether a new gesture smooths the positions it reports after
    /// recognition.
    ///
    /// Smoothing is consumer policy, like the velocity estimator (ADR-0172):
    /// no platform publishes a tuning, so no preset selects one (ADR-0212 §4).
    /// Slop, arena and velocity decisions read the raw samples either way, so
    /// smoothing never delays or biases which gesture wins. Each accepted
    /// gesture starts from an unsmoothed position.
    ///
    /// Smoothed: drag and multi-drag update positions and deltas, tap-and-drag
    /// drag updates, long-press move updates, force-press update and peak
    /// positions, and the scale focal point (restarted when its contact set
    /// changes). Not smoothed: start positions, end positions and velocities,
    /// which stay the observed values, and native trackpad pan-zoom, which
    /// the platform reports already filtered. Existing contacts keep their
    /// captured settings.
    #[must_use]
    pub const fn with_position_smoothing(mut self, smoothing: Option<OneEuroTuning>) -> Self {
        self.position_smoothing = smoothing;
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
    /// Android touch is 8 against 16, iOS touch 10 against 20 — and
    /// coincide only in the coarse baseline (18 and 18). A recognizer reading
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

    /// Get the long-press timeout, never below the OS touch-and-hold delay the
    /// host observed (ADR-0212 §3).
    #[inline]
    pub fn long_press_timeout(&self) -> Duration {
        floored(self.long_press_timeout, self.floors.long_press)
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
