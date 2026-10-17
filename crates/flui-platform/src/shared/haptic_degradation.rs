//! Every backend's haptics degradation table, as pure functions (ADR-0210 §7).
//!
//! Each function answers what one backend plays for a request and the
//! [`HapticOutcome`] it reports, from the facts the backend reads off its
//! device (OS version, API level, which primitives or waveforms the actuator
//! has). The native backends call them; they are cfg-free so Linux CI runs
//! every backend's table. Three rules hold for every row: no backend
//! stretches a duration it was given, raises an intensity, or plays for a
//! hidden window (the frame queue never calls a backend for one).
//!
//! A pattern's continuous event holds its first control point's values from
//! its start until that point, then follows the curve; where a backend can
//! only step between values, a step takes the lower of its two ends.

use std::time::Duration;

use flui_platform_api::haptics::{
    HapticCapabilities, HapticEvent, HapticOutcome, HapticPattern, PatternPlayback,
};
use flui_platform_api::{DeviceId, HapticFeedback};

/// The pulse a backend that can only switch its actuator on and off plays
/// for a transient: one `EFFECT_TICK`-length tap.
pub const TRANSIENT_PULSE: Duration = Duration::from_millis(10);

/// The actuator a request plays on: `source` when the backend knows that
/// device as an actuator, else the window's (`None`), so a pen put away or
/// a device unplugged is not an error (ADR-0210 §5).
#[must_use]
pub fn actuator_for(
    source: Option<DeviceId>,
    known: impl Fn(DeviceId) -> bool,
) -> Option<DeviceId> {
    source.filter(|device| known(*device))
}

const fn exact_or_approximated(exact: bool) -> HapticOutcome {
    if exact {
        HapticOutcome::Submitted
    } else {
        HapticOutcome::Approximated
    }
}

// --- iOS ---------------------------------------------------------------------

/// A `UIImpactFeedbackGenerator.FeedbackStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IosImpact {
    /// `.light`.
    Light,
    /// `.medium`.
    Medium,
    /// `.heavy`.
    Heavy,
    /// `.rigid`.
    Rigid,
    /// `.soft`.
    Soft,
}

/// A `UINotificationFeedbackGenerator.FeedbackType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IosNotification {
    /// `.success`.
    Success,
    /// `.warning`.
    Warning,
    /// `.error`.
    Error,
}

/// The UIKit generator a semantic kind plays through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IosGenerator {
    /// `UIImpactFeedbackGenerator`.
    Impact(IosImpact),
    /// `UISelectionFeedbackGenerator`.
    Selection,
    /// `UINotificationFeedbackGenerator`.
    Notification(IosNotification),
}

/// iOS tier 1. `view_bound` is whether the generators take a view
/// (`init(style:view:)`, iOS 17.5+); before that the view-less initializers
/// play unaddressed and every outcome is approximated.
pub const fn ios_feedback(kind: HapticFeedback, view_bound: bool) -> (IosGenerator, HapticOutcome) {
    use IosGenerator::{Impact, Notification, Selection};
    let (generator, exact) = match kind {
        HapticFeedback::LightImpact => (Impact(IosImpact::Light), true),
        HapticFeedback::MediumImpact => (Impact(IosImpact::Medium), true),
        HapticFeedback::HeavyImpact => (Impact(IosImpact::Heavy), true),
        HapticFeedback::SelectionClick => (Selection, true),
        HapticFeedback::SuccessNotification => (Notification(IosNotification::Success), true),
        HapticFeedback::WarningNotification => (Notification(IosNotification::Warning), true),
        HapticFeedback::ErrorNotification => (Notification(IosNotification::Error), true),
        // SwiftUI's `.alignment` and `.levelChange` have no UIKit generator
        // on iOS; a selection tick is the nearest.
        HapticFeedback::Alignment | HapticFeedback::LevelChange => (Selection, false),
        HapticFeedback::DragLift => (Impact(IosImpact::Rigid), false),
        HapticFeedback::Drop => (Impact(IosImpact::Soft), false),
        HapticFeedback::Confirm => (Notification(IosNotification::Success), false),
        HapticFeedback::Reject => (Notification(IosNotification::Error), false),
        // `Vibrate` and a kind added later.
        _ => (Impact(IosImpact::Heavy), false),
    };
    (generator, exact_or_approximated(exact && view_bound))
}

/// The state of a Core Haptics engine (ADR-0210 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoreHapticsEngine {
    /// Running: patterns play exactly.
    Running,
    /// Stopped or reset and owed a restart before the next play; a pattern
    /// requested meanwhile falls back to tier 1.
    Restarting,
    /// The hardware has no Core Haptics (`supportsHaptics` is false), or the
    /// backend has no Core Haptics binding.
    Unsupported,
}

/// iOS tier 2: Core Haptics plays a pattern exactly while its engine runs.
pub const fn ios_pattern(engine: CoreHapticsEngine) -> HapticOutcome {
    match engine {
        CoreHapticsEngine::Running => HapticOutcome::Submitted,
        CoreHapticsEngine::Restarting | CoreHapticsEngine::Unsupported => HapticOutcome::FellBack,
    }
}

/// The most control points Core Haptics takes in one parameter curve.
pub const CORE_HAPTICS_CURVE_POINTS: usize = 16;

/// The Core Haptics event of one [`CoreHapticsPart`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CoreHapticsEvent {
    /// `CHHapticEventTypeHapticTransient` with these parameter values.
    Transient {
        /// `CHHapticEventParameterIDHapticIntensity`.
        intensity: f32,
        /// `CHHapticEventParameterIDHapticSharpness`.
        sharpness: f32,
    },
    /// `CHHapticEventTypeHapticContinuous` at intensity 1 and sharpness 0,
    /// shaped entirely by the part's curves, lasting `duration`.
    Continuous {
        /// How long it lasts.
        duration: Duration,
    },
}

/// Which dynamic parameter a curve controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoreHapticsControl {
    /// `CHHapticDynamicParameterIDHapticIntensityControl`: multiplies the
    /// event's intensity of 1.
    Intensity,
    /// `CHHapticDynamicParameterIDHapticSharpnessControl`: adds to the
    /// event's sharpness of 0.
    Sharpness,
}

/// One parameter curve of a part: `points` of `(time, value)` relative to
/// `at`, itself relative to the part's start.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreHapticsCurve {
    /// The parameter it controls.
    pub control: CoreHapticsControl,
    /// When it starts, after the part starts.
    pub at: Duration,
    /// Its control points, at most [`CORE_HAPTICS_CURVE_POINTS`].
    pub points: Vec<(Duration, f32)>,
}

/// One event of a pattern as its own Core Haptics pattern, started `at` its
/// time after the pattern starts. Each event is its own pattern because a
/// Core Haptics parameter curve scales every event of its pattern: kept
/// apart, an event that overlaps a continuous one keeps its own values.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreHapticsPart {
    /// When it starts, after the pattern starts.
    pub at: Duration,
    /// Its event.
    pub event: CoreHapticsEvent,
    /// The curves that shape a continuous event.
    pub curves: Vec<CoreHapticsCurve>,
}

/// `points` as consecutive curves of at most [`CORE_HAPTICS_CURVE_POINTS`],
/// each starting at the point the one before ended on.
fn split_curve(control: CoreHapticsControl, points: &[(Duration, f32)]) -> Vec<CoreHapticsCurve> {
    let mut curves = Vec::new();
    let mut start = 0;
    while start < points.len() {
        let end = points.len().min(start + CORE_HAPTICS_CURVE_POINTS);
        let base = points[start].0;
        curves.push(CoreHapticsCurve {
            control,
            at: base,
            points: points[start..end]
                .iter()
                .map(|&(time, value)| (time.saturating_sub(base), value))
                .collect(),
        });
        if end == points.len() {
            break;
        }
        start = end - 1;
    }
    curves
}

/// iOS tier 2: the pattern as Core Haptics parts, exact. A continuous
/// event holds its first point's values from its start, so its curves begin
/// with that value at time zero.
#[must_use]
pub fn ios_pattern_parts(pattern: &HapticPattern) -> Vec<CoreHapticsPart> {
    pattern
        .events()
        .iter()
        .map(|event| match event {
            HapticEvent::Transient {
                at,
                intensity,
                sharpness,
            } => CoreHapticsPart {
                at: *at,
                event: CoreHapticsEvent::Transient {
                    intensity: intensity.get(),
                    sharpness: sharpness.get(),
                },
                curves: Vec::new(),
            },
            HapticEvent::Continuous { at, curve } => {
                let points = curve.points();
                let held = |value: fn(&flui_platform_api::haptics::ControlPoint) -> f32| {
                    points
                        .first()
                        .map(|first| (Duration::ZERO, value(first)))
                        .into_iter()
                        .chain(points.iter().map(|point| (point.offset, value(point))))
                        .collect::<Vec<_>>()
                };
                let mut curves = split_curve(
                    CoreHapticsControl::Intensity,
                    &held(|point| point.intensity.get()),
                );
                curves.extend(split_curve(
                    CoreHapticsControl::Sharpness,
                    &held(|point| point.sharpness.get()),
                ));
                CoreHapticsPart {
                    at: *at,
                    event: CoreHapticsEvent::Continuous {
                        duration: curve.duration(),
                    },
                    curves,
                }
            }
        })
        .collect()
}

// --- Android -----------------------------------------------------------------

/// A `HapticFeedbackConstants` value and the API level that added it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AndroidHapticConstant {
    /// The constant's value.
    pub value: i32,
    /// The API level that added it.
    pub since: u32,
}

impl AndroidHapticConstant {
    /// `LONG_PRESS`.
    pub const LONG_PRESS: Self = Self { value: 0, since: 3 };
    /// `VIRTUAL_KEY`.
    pub const VIRTUAL_KEY: Self = Self { value: 1, since: 5 };
    /// `KEYBOARD_TAP`.
    pub const KEYBOARD_TAP: Self = Self { value: 3, since: 8 };
    /// `CLOCK_TICK`.
    pub const CLOCK_TICK: Self = Self {
        value: 4,
        since: 21,
    };
    /// `CONTEXT_CLICK`.
    pub const CONTEXT_CLICK: Self = Self {
        value: 6,
        since: 23,
    };
    /// `GESTURE_END`.
    pub const GESTURE_END: Self = Self {
        value: 13,
        since: 30,
    };
    /// `CONFIRM`.
    pub const CONFIRM: Self = Self {
        value: 16,
        since: 30,
    };
    /// `REJECT`.
    pub const REJECT: Self = Self {
        value: 17,
        since: 30,
    };
    /// `GESTURE_THRESHOLD_ACTIVATE`.
    pub const GESTURE_THRESHOLD_ACTIVATE: Self = Self {
        value: 23,
        since: 34,
    };
    /// `DRAG_START`.
    pub const DRAG_START: Self = Self {
        value: 25,
        since: 34,
    };
    /// `SEGMENT_TICK`.
    pub const SEGMENT_TICK: Self = Self {
        value: 26,
        since: 34,
    };
    /// `SEGMENT_FREQUENT_TICK`.
    pub const SEGMENT_FREQUENT_TICK: Self = Self {
        value: 27,
        since: 34,
    };
}

/// Android tier 1 at `api_level`: `View.performHapticFeedback` with the
/// constant for the kind, or the nearest older constant before the API level
/// that added it.
pub const fn android_feedback(
    kind: HapticFeedback,
    api_level: u32,
) -> (AndroidHapticConstant, HapticOutcome) {
    use AndroidHapticConstant as C;
    let (wanted, exact, older) = match kind {
        HapticFeedback::LightImpact => (C::VIRTUAL_KEY, true, C::VIRTUAL_KEY),
        HapticFeedback::MediumImpact => (C::KEYBOARD_TAP, true, C::KEYBOARD_TAP),
        HapticFeedback::HeavyImpact => (C::CONTEXT_CLICK, true, C::LONG_PRESS),
        HapticFeedback::SelectionClick => (C::CLOCK_TICK, true, C::VIRTUAL_KEY),
        HapticFeedback::SuccessNotification | HapticFeedback::Confirm => {
            (C::CONFIRM, true, C::VIRTUAL_KEY)
        }
        HapticFeedback::ErrorNotification | HapticFeedback::Reject => {
            (C::REJECT, true, C::LONG_PRESS)
        }
        HapticFeedback::WarningNotification => {
            (C::GESTURE_THRESHOLD_ACTIVATE, false, C::LONG_PRESS)
        }
        HapticFeedback::Alignment => (C::SEGMENT_TICK, true, C::CLOCK_TICK),
        HapticFeedback::LevelChange => (C::SEGMENT_FREQUENT_TICK, true, C::CLOCK_TICK),
        HapticFeedback::DragLift => (C::DRAG_START, true, C::LONG_PRESS),
        HapticFeedback::Drop => (C::GESTURE_END, true, C::VIRTUAL_KEY),
        // `Vibrate` and a kind added later.
        _ => (C::LONG_PRESS, false, C::LONG_PRESS),
    };
    let available = api_level >= wanted.since;
    let constant = if available { wanted } else { older };
    (constant, exact_or_approximated(exact && available))
}

/// A `VibrationEffect.Composition` primitive and the API level that added it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AndroidPrimitive {
    /// The `PRIMITIVE_*` value.
    pub value: i32,
    /// The API level that added it.
    pub since: u32,
}

impl AndroidPrimitive {
    /// `PRIMITIVE_CLICK`: sharp and strong.
    pub const CLICK: Self = Self {
        value: 1,
        since: 30,
    };
    /// `PRIMITIVE_THUD`: round and strong.
    pub const THUD: Self = Self {
        value: 2,
        since: 31,
    };
    /// `PRIMITIVE_TICK`: sharp and light.
    pub const TICK: Self = Self {
        value: 7,
        since: 30,
    };
    /// `PRIMITIVE_LOW_TICK`: round and light.
    pub const LOW_TICK: Self = Self {
        value: 8,
        since: 31,
    };

    /// The primitive nearest a transient's sharpness and intensity.
    #[must_use]
    pub fn nearest(intensity: f32, sharpness: f32) -> Self {
        match (sharpness >= 0.5, intensity >= 0.5) {
            (true, true) => Self::CLICK,
            (true, false) => Self::TICK,
            (false, true) => Self::THUD,
            (false, false) => Self::LOW_TICK,
        }
    }
}

/// What an Android vibrator offers, as the backend reads it once per device.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct AndroidVibrator {
    /// `Build.VERSION.SDK_INT`.
    pub api_level: u32,
    /// `Vibrator.areEnvelopeEffectsSupported()` (API 36).
    pub envelopes: bool,
    /// The primitives `Vibrator.areAllPrimitivesSupported` admits.
    pub primitives: Vec<i32>,
    /// `Vibrator.hasAmplitudeControl()`.
    pub amplitude_control: bool,
}

impl AndroidVibrator {
    /// A vibrator at `api_level` with the given envelope, primitive and
    /// amplitude support.
    #[must_use]
    pub fn new(
        api_level: u32,
        envelopes: bool,
        primitives: Vec<i32>,
        amplitude_control: bool,
    ) -> Self {
        Self {
            api_level,
            envelopes,
            primitives,
            amplitude_control,
        }
    }
}

/// One step of an envelope: reach `intensity` and `sharpness` over
/// `duration`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvelopeStep {
    /// The intensity reached.
    pub intensity: f32,
    /// The sharpness reached.
    pub sharpness: f32,
    /// How long the ramp takes.
    pub duration: Duration,
}

/// One primitive of a composition, `at` its start time in the pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposedPrimitive {
    /// The primitive.
    pub primitive: AndroidPrimitive,
    /// Its scale: the transient's intensity.
    pub scale: f32,
    /// When it starts, after the pattern starts.
    pub at: Duration,
}

/// How an Android backend plays a pattern.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AndroidPatternPlan {
    /// One `VibrationEffect.BasicEnvelopeBuilder` envelope: starting at
    /// `initial_sharpness`, the steps in order.
    Envelope {
        /// The sharpness the envelope starts at.
        initial_sharpness: f32,
        /// The ramps, in order.
        steps: Vec<EnvelopeStep>,
    },
    /// A `VibrationEffect.Composition` of primitives.
    Composition(Vec<ComposedPrimitive>),
    /// A `VibrationEffect.createWaveform` of `(duration, amplitude)` steps,
    /// amplitude in `0..=255`.
    Waveform(Vec<(Duration, u8)>),
    /// The pattern cannot play; its fallback does.
    FallBack,
}

/// The ramp an envelope takes to reach a held value: the shortest step it
/// can express in whole milliseconds.
pub const ENVELOPE_RAMP: Duration = Duration::from_millis(1);

/// The whole pattern as one envelope timeline from its start, with silent
/// steps between events, or `None` when two events overlap. A continuous
/// event ramps to its first point's values in [`ENVELOPE_RAMP`] and holds
/// them until that point; a transient is a [`TRANSIENT_PULSE`] at its
/// intensity and sharpness.
fn envelope(pattern: &HapticPattern) -> Option<(f32, Vec<EnvelopeStep>)> {
    let initial_sharpness = match pattern.events().first()? {
        HapticEvent::Transient { sharpness, .. } => sharpness.get(),
        HapticEvent::Continuous { curve, .. } => curve.points().first()?.sharpness.get(),
    };
    let mut steps = Vec::new();
    let mut cursor = Duration::ZERO;
    let mut sharpness = initial_sharpness;
    let mut step = |intensity: f32, sharpness: f32, duration: Duration| {
        if !duration.is_zero() {
            steps.push(EnvelopeStep {
                intensity,
                sharpness,
                duration,
            });
        }
    };
    for event in pattern.events() {
        let at = event.at();
        if at < cursor {
            return None;
        }
        step(0.0, sharpness, at.saturating_sub(cursor));
        match event {
            HapticEvent::Transient {
                intensity,
                sharpness: crisp,
                ..
            } => {
                let (level, crisp) = (intensity.get(), crisp.get());
                step(level, crisp, ENVELOPE_RAMP);
                step(
                    level,
                    crisp,
                    TRANSIENT_PULSE.saturating_sub(ENVELOPE_RAMP * 2),
                );
                step(0.0, crisp, ENVELOPE_RAMP);
                sharpness = crisp;
                cursor = at.saturating_add(TRANSIENT_PULSE);
            }
            HapticEvent::Continuous { curve, .. } => {
                let points = curve.points();
                let first = points.first()?;
                let ramp = ENVELOPE_RAMP.min(first.offset);
                step(first.intensity.get(), first.sharpness.get(), ramp);
                step(
                    first.intensity.get(),
                    first.sharpness.get(),
                    first.offset.saturating_sub(ramp),
                );
                for pair in points.windows(2) {
                    step(
                        pair[1].intensity.get(),
                        pair[1].sharpness.get(),
                        pair[1].offset.saturating_sub(pair[0].offset),
                    );
                }
                sharpness = points.last().map_or(sharpness, |last| last.sharpness.get());
                cursor = at.saturating_add(curve.duration());
            }
        }
    }
    Some((initial_sharpness, steps))
}

/// Android tier 2. Where the device has envelopes (API 36,
/// `areEnvelopeEffectsSupported`), the whole pattern plays as one envelope:
/// exactly when it has only continuous events, and approximated when it has
/// transients, which an envelope plays as short pulses since a composition
/// cannot mix primitives with an envelope. A transient-only pattern becomes
/// primitives chosen by sharpness and intensity (sharpness quantized); a
/// pattern whose events overlap, or a device without envelopes, takes an
/// amplitude waveform (sharpness dropped). A primitive or amplitude control
/// the device lacks falls back, and so does every pattern below API 30.
pub fn android_pattern(
    pattern: &HapticPattern,
    vibrator: &AndroidVibrator,
) -> (AndroidPatternPlan, HapticOutcome) {
    let fall_back = (AndroidPatternPlan::FallBack, HapticOutcome::FellBack);
    if vibrator.api_level < 30 {
        return fall_back;
    }
    let has_transients = pattern
        .events()
        .iter()
        .any(|event| matches!(event, HapticEvent::Transient { .. }));
    let has_continuous = pattern
        .events()
        .iter()
        .any(|event| matches!(event, HapticEvent::Continuous { .. }));
    if vibrator.envelopes
        && vibrator.api_level >= 36
        && has_continuous
        && let Some((initial_sharpness, steps)) = envelope(pattern)
    {
        return (
            AndroidPatternPlan::Envelope {
                initial_sharpness,
                steps,
            },
            if has_transients {
                HapticOutcome::Approximated
            } else {
                HapticOutcome::Submitted
            },
        );
    }
    let transients_only = pattern
        .events()
        .iter()
        .all(|event| matches!(event, HapticEvent::Transient { .. }));
    if transients_only {
        let composed: Vec<_> = pattern
            .events()
            .iter()
            .filter_map(|event| match event {
                HapticEvent::Transient {
                    at,
                    intensity,
                    sharpness,
                } => Some(ComposedPrimitive {
                    primitive: AndroidPrimitive::nearest(intensity.get(), sharpness.get()),
                    scale: intensity.get(),
                    at: *at,
                }),
                HapticEvent::Continuous { .. } => None,
            })
            .collect();
        let supported = composed.iter().all(|step| {
            vibrator.api_level >= step.primitive.since
                && vibrator.primitives.contains(&step.primitive.value)
        });
        return if supported {
            (
                AndroidPatternPlan::Composition(composed),
                HapticOutcome::Approximated,
            )
        } else {
            fall_back
        };
    }
    if !vibrator.amplitude_control {
        return fall_back;
    }
    match amplitude_steps(pattern) {
        Some(steps) => (
            AndroidPatternPlan::Waveform(
                steps
                    .into_iter()
                    .map(|(duration, level)| (duration, amplitude(level)))
                    .collect(),
            ),
            HapticOutcome::Approximated,
        ),
        None => fall_back,
    }
}

/// `level` in `0.0..=1.0` as an amplitude in `0..=255`, rounded down so it
/// never raises the intensity, and at least 1 for a nonzero level so a
/// faint step still plays.
fn amplitude(level: f32) -> u8 {
    if level <= 0.0 {
        return 0;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "level is in 0.0..=1.0, so the product is in 0.0..=255.0"
    )]
    let scaled = (level * 255.0).floor() as u8;
    scaled.max(1)
}

/// The pattern as consecutive `(duration, level)` steps from its start, a
/// transient as a [`TRANSIENT_PULSE`] at its intensity, or `None` when two
/// events overlap, which an on/off or amplitude timeline cannot express.
fn amplitude_steps(pattern: &HapticPattern) -> Option<Vec<(Duration, f32)>> {
    let mut steps = Vec::new();
    let mut cursor = Duration::ZERO;
    for event in pattern.events() {
        let at = event.at();
        if at < cursor {
            return None;
        }
        if at > cursor {
            steps.push((at.saturating_sub(cursor), 0.0));
        }
        match event {
            HapticEvent::Transient { intensity, .. } => {
                steps.push((TRANSIENT_PULSE, intensity.get()));
                cursor = at.saturating_add(TRANSIENT_PULSE);
            }
            HapticEvent::Continuous { curve, .. } => {
                let points = curve.points();
                let mut previous = (Duration::ZERO, points.first()?.intensity.get());
                for point in points {
                    let level = previous.1.min(point.intensity.get());
                    steps.push((point.offset.saturating_sub(previous.0), level));
                    previous = (point.offset, point.intensity.get());
                }
                cursor = at.saturating_add(curve.duration());
            }
        }
    }
    Some(steps)
}

// --- Windows -----------------------------------------------------------------

/// A `KnownSimpleHapticsControllerWaveforms` waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PenWaveform {
    /// `Click`.
    Click,
    /// `Press`.
    Press,
    /// `Release`.
    Release,
    /// `BuzzContinuous`.
    BuzzContinuous,
    /// `Success`.
    Success,
    /// `Error`.
    Error,
}

/// A pen's `SimpleHapticsController`, as the backend reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct PenController {
    /// The waveforms `SupportedFeedback` lists.
    pub waveforms: Vec<PenWaveform>,
    /// `IsIntensitySupported`.
    pub intensity: bool,
    /// `IsPlayDurationSupported`: a continuous waveform can be handed its
    /// length instead of being stopped when it is over.
    pub play_duration: bool,
}

impl PenController {
    /// A controller with `waveforms`, and intensity control when `intensity`,
    /// that takes no play duration.
    #[must_use]
    pub fn new(waveforms: Vec<PenWaveform>, intensity: bool) -> Self {
        Self {
            waveforms,
            intensity,
            play_duration: false,
        }
    }

    /// This controller, taking a play duration when `play_duration`.
    #[must_use]
    pub fn with_play_duration(self, play_duration: bool) -> Self {
        Self {
            play_duration,
            ..self
        }
    }
}

/// How long a pen buzzes for semantic feedback that plays a continuous
/// waveform (`Vibrate`): the web table's vibration length, so no kind
/// buzzes until the pen leaves.
pub const PEN_BUZZ: Duration = Duration::from_millis(200);

/// What a window's pen actuator can play (ADR-0210 §2): nothing without a
/// pen controller, or without `Click`, which every kind falls back to;
/// otherwise semantic feedback and approximated patterns (a transient is a
/// `Click`; a continuous event needs `BuzzContinuous`, else the pattern
/// falls back), with intensity where the controller takes it and never
/// sharpness.
#[must_use]
pub fn windows_capabilities(controller: Option<&PenController>) -> HapticCapabilities {
    match controller {
        Some(controller) if controller.waveforms.contains(&PenWaveform::Click) => {
            HapticCapabilities::new(
                true,
                PatternPlayback::Approximated,
                controller.intensity,
                false,
            )
        }
        _ => HapticCapabilities::NONE,
    }
}

/// Windows tier 1: the pen's nearest waveform. `None` without a pen
/// controller, since the window's own actuator (`InputHapticsManager`) is
/// in no published `windows` crate (ADR-0210 §8): the request is
/// [`HapticOutcome::Unavailable`]. A controller lacking the waveform plays
/// `Click` approximated, or nothing without one. The backend plays the
/// waveform as [`pen_feedback_step`].
#[must_use]
pub fn windows_feedback(
    kind: HapticFeedback,
    controller: Option<&PenController>,
) -> Option<(PenWaveform, HapticOutcome)> {
    let controller = controller?;
    let (wanted, exact) = match kind {
        HapticFeedback::SelectionClick => (PenWaveform::Click, true),
        HapticFeedback::DragLift => (PenWaveform::Press, true),
        HapticFeedback::Drop => (PenWaveform::Release, true),
        HapticFeedback::SuccessNotification => (PenWaveform::Success, true),
        HapticFeedback::ErrorNotification => (PenWaveform::Error, true),
        HapticFeedback::Confirm => (PenWaveform::Success, false),
        HapticFeedback::Reject | HapticFeedback::WarningNotification => (PenWaveform::Error, false),
        HapticFeedback::Vibrate => (PenWaveform::BuzzContinuous, false),
        _ => (PenWaveform::Click, false),
    };
    if controller.waveforms.contains(&wanted) {
        return Some((wanted, exact_or_approximated(exact)));
    }
    controller
        .waveforms
        .contains(&PenWaveform::Click)
        .then_some((PenWaveform::Click, HapticOutcome::Approximated))
}

/// One waveform a pen plays for a pattern event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenStep {
    /// The waveform.
    pub waveform: PenWaveform,
    /// When it starts, after the pattern starts.
    pub at: Duration,
    /// How long it plays; zero for a one-shot `Click`.
    pub length: Duration,
    /// Its intensity, when the controller takes one.
    pub intensity: Option<f32>,
}

/// Windows tier 2: transients as `Click`, continuous events as
/// `BuzzContinuous` for their length at their lowest intensity, intensity
/// only where the controller supports it, sharpness dropped: approximated.
/// `None` without a pen controller ([`HapticOutcome::Unavailable`]); a
/// controller lacking a waveform falls back.
#[must_use]
pub fn windows_pattern(
    pattern: &HapticPattern,
    controller: Option<&PenController>,
) -> Option<(Vec<PenStep>, HapticOutcome)> {
    let controller = controller?;
    let steps: Vec<PenStep> = pattern
        .events()
        .iter()
        .map(|event| match event {
            HapticEvent::Transient { at, intensity, .. } => PenStep {
                waveform: PenWaveform::Click,
                at: *at,
                length: Duration::ZERO,
                intensity: controller.intensity.then_some(intensity.get()),
            },
            HapticEvent::Continuous { at, curve } => PenStep {
                waveform: PenWaveform::BuzzContinuous,
                at: *at,
                length: curve.duration(),
                intensity: controller.intensity.then(|| {
                    curve
                        .points()
                        .iter()
                        .map(|point| point.intensity.get())
                        .fold(1.0, f32::min)
                }),
            },
        })
        .collect();
    let playable = steps
        .iter()
        .all(|step| controller.waveforms.contains(&step.waveform));
    Some(if playable {
        (steps, HapticOutcome::Approximated)
    } else {
        (Vec::new(), HapticOutcome::FellBack)
    })
}

/// The step a pen plays for semantic feedback's `waveform`: now, at the
/// controller's own intensity, and a continuous waveform for [`PEN_BUZZ`].
#[must_use]
pub fn pen_feedback_step(waveform: PenWaveform) -> PenStep {
    PenStep {
        waveform,
        at: Duration::ZERO,
        length: if waveform == PenWaveform::BuzzContinuous {
            PEN_BUZZ
        } else {
            Duration::ZERO
        },
        intensity: None,
    }
}

/// One call a pen backend makes on its `SimpleHapticsController`, `at` its
/// time after the request.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PenCommand {
    /// `SendHapticFeedback`, with the intensity when there is one, and for
    /// `length` when the controller takes a play duration.
    Play {
        /// When.
        at: Duration,
        /// The waveform.
        waveform: PenWaveform,
        /// Its intensity, when the controller takes one.
        intensity: Option<f32>,
        /// How long it plays, handed to the controller.
        length: Option<Duration>,
    },
    /// `StopFeedback`: a continuous waveform the controller could not be
    /// given a length for is over.
    Stop {
        /// When.
        at: Duration,
    },
}

impl PenCommand {
    /// When the backend makes the call.
    #[must_use]
    pub const fn at(&self) -> Duration {
        match self {
            Self::Play { at, .. } | Self::Stop { at } => *at,
        }
    }
}

/// The calls that play `steps` on `controller`, in time order. A one-shot
/// waveform is played and left; a continuous one is handed its length where
/// the controller takes a play duration, and otherwise stopped at its end,
/// so no waveform plays longer than its step. At one instant a stop comes
/// before a play, so a step that starts as another ends is not cut; a stop
/// may end an overlapping step early, never late.
#[must_use]
pub fn pen_timeline(steps: &[PenStep], controller: &PenController) -> Vec<PenCommand> {
    let mut commands = Vec::with_capacity(steps.len() * 2);
    for step in steps {
        let continuous = !step.length.is_zero();
        commands.push(PenCommand::Play {
            at: step.at,
            waveform: step.waveform,
            intensity: step.intensity,
            length: (continuous && controller.play_duration).then_some(step.length),
        });
        if continuous && !controller.play_duration {
            commands.push(PenCommand::Stop {
                at: step.at.saturating_add(step.length),
            });
        }
    }
    commands.sort_by_key(|command| (command.at(), matches!(command, PenCommand::Play { .. })));
    commands
}

// --- macOS -------------------------------------------------------------------

/// An `NSHapticFeedbackManager.FeedbackPattern`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MacFeedback {
    /// `.generic`.
    Generic,
    /// `.alignment`.
    Alignment,
    /// `.levelChange`.
    LevelChange,
}

/// macOS tier 1: `alignment` and `levelChange` exactly, every other kind as
/// `generic`, approximated.
pub const fn macos_feedback(kind: HapticFeedback) -> (MacFeedback, HapticOutcome) {
    match kind {
        HapticFeedback::Alignment => (MacFeedback::Alignment, HapticOutcome::Submitted),
        HapticFeedback::LevelChange => (MacFeedback::LevelChange, HapticOutcome::Submitted),
        _ => (MacFeedback::Generic, HapticOutcome::Approximated),
    }
}

/// macOS tier 2: `NSHapticFeedbackPerformer` plays no patterns, so every
/// pattern falls back.
pub const fn macos_pattern() -> HapticOutcome {
    HapticOutcome::FellBack
}

// --- Web ---------------------------------------------------------------------

/// What the page's Vibration API offers now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WebVibration {
    /// `navigator.vibrate` exists and the page has user activation.
    Available,
    /// No `navigator.vibrate` (Safari, Firefox), or no user activation yet.
    Unavailable,
}

/// Web tier 1: `navigator.vibrate` pulses, alternating on and off
/// milliseconds; `Vibrate` exactly, every other kind approximated.
/// `None` when vibration is unavailable.
#[must_use]
pub fn web_feedback(
    kind: HapticFeedback,
    vibration: WebVibration,
) -> Option<(Vec<u32>, HapticOutcome)> {
    if vibration == WebVibration::Unavailable {
        return None;
    }
    let pulses: &[u32] = match kind {
        HapticFeedback::Vibrate => return Some((vec![200], HapticOutcome::Submitted)),
        HapticFeedback::SelectionClick | HapticFeedback::Alignment => &[5],
        HapticFeedback::Drop => &[15],
        HapticFeedback::MediumImpact | HapticFeedback::DragLift => &[20],
        HapticFeedback::HeavyImpact => &[30],
        HapticFeedback::SuccessNotification | HapticFeedback::Confirm => &[10, 50, 10],
        HapticFeedback::WarningNotification => &[20, 50, 20],
        HapticFeedback::ErrorNotification | HapticFeedback::Reject => &[30, 50, 30, 50, 30],
        // `LightImpact`, `LevelChange` and a kind added later.
        _ => &[10],
    };
    Some((pulses.to_vec(), HapticOutcome::Approximated))
}

/// Web tier 2: pulses only. Each step with a nonzero level is a pulse for
/// its length, intensity and sharpness dropped: approximated. Overlapping
/// events fall back. `None` when vibration is unavailable.
#[must_use]
pub fn web_pattern(
    pattern: &HapticPattern,
    vibration: WebVibration,
) -> Option<(Vec<u32>, HapticOutcome)> {
    if vibration == WebVibration::Unavailable {
        return None;
    }
    let Some(steps) = amplitude_steps(pattern) else {
        return Some((Vec::new(), HapticOutcome::FellBack));
    };
    // Even entries vibrate and odd ones pause; consecutive steps of one state
    // merge. Milliseconds round down, so a pulse never lengthens.
    let mut pulses: Vec<u32> = Vec::new();
    for (length, level) in steps {
        let millis = u32::try_from(length.as_millis()).unwrap_or(u32::MAX);
        let on = level > 0.0;
        let last_matches = pulses
            .len()
            .checked_sub(1)
            .is_some_and(|last| (last % 2 == 0) == on);
        if last_matches {
            if let Some(last) = pulses.last_mut() {
                *last = last.saturating_add(millis);
            }
        } else {
            if pulses.is_empty() && !on {
                pulses.push(0);
            }
            pulses.push(millis);
        }
    }
    if pulses.len().is_multiple_of(2) {
        // A trailing pause plays nothing.
        pulses.pop();
    }
    Some((pulses, HapticOutcome::Approximated))
}

// --- Linux -------------------------------------------------------------------

/// Linux, both tiers: no backend has a haptic actuator.
pub const fn linux() -> HapticOutcome {
    HapticOutcome::Unavailable
}
