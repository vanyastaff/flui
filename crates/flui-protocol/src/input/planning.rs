//! [`plan`]: an action sequence planned once into timed input (ADR-0400 §2), which the driver's
//! OS lane turns into the events it sends. The runtime's agent still plans `input.inject` with a
//! planner of its own, whose bounds and refusals are its own, until it plays this plan as
//! ADR-0400 §2 decides (`docs/gaps.md` keeps that gap).
//!
//! [`plan`] checks a sequence against its bounds (sources, ticks, span, ranges) and lays its
//! ticks on one timeline. Tick `n` holds the `n`th action of every source and starts
//! where tick `n - 1` ended. Within a tick the presses, releases, cancels, scrolls, keys and text
//! come first, at the tick's start, in the order the sources are listed; then the moves share the
//! tick's timeline, each split into steps of [`STEP`] (at most [`MAX_STEPS`]) at its own speed,
//! every step at the whole millisecond it falls in. A tick lasts as long as its longest pause or
//! move, and one [`STEP`] after a move that names no duration, so the window's frames see it.
//! A path never interpolates further and never coalesces: a step is one event.

use std::fmt;
use std::time::Duration;

use super::{
    ActionSequence, ImeAction, InputSource, KeyAction, PointerAction, PointerButton, PointerDevice,
    ScrollUnit, TrackpadAction, WheelAction,
};
use crate::geometry::LogicalPoint;

/// How long one step of a split move lasts, and how long a tick lasts after a move that names no
/// duration: one frame at 60 Hz, so the window's frames see each step.
pub const STEP: Duration = Duration::from_millis(16);

/// The most steps one move is split into; a longer move takes longer steps.
pub const MAX_STEPS: u32 = 1024;

/// The most sources one sequence names.
pub const MAX_SOURCES: usize = 32;

/// The most ticks one sequence plays.
pub const MAX_TICKS: usize = 4096;

/// The longest one sequence lasts: a longer one is refused rather than holding a window's input
/// for it.
pub const MAX_SPAN: Duration = Duration::from_secs(600);

/// How far from the window's origin a point may be, in logical pixels: past any window, and small
/// enough that a step between two points never overflows.
pub const MAX_COORDINATE: f64 = 1.0e9;

/// What kind of device a planned source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceKind {
    /// A mouse, touch contact, pen or virtual pointer.
    Pointer(PointerDevice),
    /// A scroll wheel.
    Wheel,
    /// A keyboard.
    Keys,
    /// Committed text.
    Text,
    /// An input-method composition.
    Ime,
    /// A trackpad gesture.
    Trackpad,
}

/// One source of a plan, in the order the sequence lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlannedSource {
    /// Its name in the sequence; a text or input-method source has none.
    pub id: Option<String>,
    /// What kind of device it is.
    pub kind: SourceKind,
}

/// One input of a plan, of one source.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlannedInput {
    /// Press `button`, or put the contact down, where the pointer is.
    PointerDown {
        /// The button.
        button: PointerButton,
        /// Pen or touch pressure, `0.0..=1.0`.
        pressure: Option<f64>,
        /// Where the pointer is: where its last step put it, unknown before its first.
        at: Option<LogicalPoint>,
    },
    /// Release `button`, or lift the contact.
    PointerUp {
        /// The button.
        button: PointerButton,
        /// Where the pointer is, unknown before its first step.
        at: Option<LogicalPoint>,
    },
    /// One step of a move: the pointer jumps to `to`.
    PointerMove {
        /// Where to.
        to: LogicalPoint,
        /// Pen or touch pressure, `0.0..=1.0`.
        pressure: Option<f64>,
    },
    /// The platform took the contact away.
    PointerCancel {
        /// Where the pointer is, unknown before its first step.
        at: Option<LogicalPoint>,
    },
    /// A wheel turned at a point.
    Scroll {
        /// Where the wheel is.
        at: LogicalPoint,
        /// Rightwards.
        delta_x: f64,
        /// Downwards.
        delta_y: f64,
        /// What the deltas count.
        unit: ScrollUnit,
    },
    /// A key went down, in the platform input vocabulary's spelling.
    KeyDown(String),
    /// A key came up.
    KeyUp(String),
    /// Text was committed.
    Text(String),
    /// One step of an input-method composition.
    Ime(ImeAction),
    /// One phase of a trackpad gesture.
    Trackpad(TrackpadAction),
}

/// One input at its instant.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TimedInput {
    /// When, from the sequence's start.
    pub at: Duration,
    /// The tick it belongs to, from 0.
    pub tick: usize,
    /// The source, by its index in [`Plan::sources`].
    pub source: usize,
    /// The input.
    pub input: PlannedInput,
}

/// An action sequence as timed input: see the [module docs](self).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    sources: Vec<PlannedSource>,
    inputs: Vec<TimedInput>,
    span: Duration,
}

impl Plan {
    /// The sources, in the order the sequence lists them.
    #[must_use]
    pub fn sources(&self) -> &[PlannedSource] {
        &self.sources
    }

    /// Every input, in the order it is delivered: by instant, and within one instant as the
    /// [module docs](self) order them.
    #[must_use]
    pub fn inputs(&self) -> &[TimedInput] {
        &self.inputs
    }

    /// How long playing the plan takes: the end of its last tick.
    #[must_use]
    pub const fn span(&self) -> Duration {
        self.span
    }
}

/// Why a sequence cannot be planned.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PlanError {
    /// It names more than [`MAX_SOURCES`] sources.
    TooManySources {
        /// How many it names.
        sources: usize,
    },
    /// It plays more than [`MAX_TICKS`] ticks.
    TooManyTicks {
        /// How many it plays.
        ticks: usize,
    },
    /// It lasts longer than [`MAX_SPAN`].
    TooLong {
        /// How long it would last, saturated.
        span: Duration,
    },
    /// Two sources share a name.
    SharedName {
        /// The name.
        id: String,
    },
    /// A value is outside the range its field takes: a point past [`MAX_COORDINATE`] or not
    /// finite, a pressure outside `0.0..=1.0`, a delta, scale or angle not finite.
    OutOfRange {
        /// The source, by its index in the sequence.
        source: usize,
        /// The tick, from 0.
        tick: usize,
        /// The field.
        field: &'static str,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManySources { sources } => write!(
                f,
                "an action sequence names at most {MAX_SOURCES} sources, not {sources}"
            ),
            Self::TooManyTicks { ticks } => write!(
                f,
                "an action sequence plays at most {MAX_TICKS} ticks, not {ticks}"
            ),
            Self::TooLong { span } => write!(
                f,
                "an action sequence lasts at most {} s, not {} ms",
                MAX_SPAN.as_secs(),
                span.as_millis()
            ),
            Self::SharedName { id } => {
                write!(f, "two sources of an action sequence are named `{id}`")
            }
            Self::OutOfRange {
                source,
                tick,
                field,
            } => write!(
                f,
                "source {source}, tick {tick}: `{field}` is out of range (a point within \
                 {MAX_COORDINATE} logical pixels, a pressure in 0..=1, finite deltas)"
            ),
        }
    }
}

impl std::error::Error for PlanError {}

/// `sequence` as timed input: see the [module docs](self).
///
/// # Errors
///
/// [`PlanError`] for a sequence no path plays as asked; nothing of it is planned.
pub fn plan(sequence: &ActionSequence) -> Result<Plan, PlanError> {
    if sequence.sources.len() > MAX_SOURCES {
        return Err(PlanError::TooManySources {
            sources: sequence.sources.len(),
        });
    }
    let ticks = sequence.ticks();
    if ticks > MAX_TICKS {
        return Err(PlanError::TooManyTicks { ticks });
    }
    // The sequence's own span counts the durations it names and never exceeds the plan's, which
    // adds a step for each move that names none: a sequence already past the bound is refused
    // before its steps are laid, which bounds how many there are. The bound holds the span that
    // is played, checked once the ticks are laid.
    let span = sequence.span();
    if span > MAX_SPAN {
        return Err(PlanError::TooLong { span });
    }
    let mut sources = Vec::with_capacity(sequence.sources.len());
    for source in &sequence.sources {
        let planned = planned_source(source);
        if let Some(id) = &planned.id
            && sources
                .iter()
                .any(|known: &PlannedSource| known.id.as_ref() == Some(id))
        {
            return Err(PlanError::SharedName { id: id.clone() });
        }
        sources.push(planned);
    }
    let mut positions: Vec<Option<LogicalPoint>> = vec![None; sequence.sources.len()];
    let mut inputs = Vec::new();
    let mut start = Duration::ZERO;
    for tick in 0..ticks {
        let mut lasts = Duration::ZERO;
        let mut strokes: Vec<Stroke> = Vec::new();
        for (index, source) in sequence.sources.iter().enumerate() {
            let range = |field| PlanError::OutOfRange {
                source: index,
                tick,
                field,
            };
            let mut instant = |input| {
                inputs.push(TimedInput {
                    at: start,
                    tick,
                    source: index,
                    input,
                });
            };
            let mut wait = |duration_ms: u64| {
                lasts = lasts.max(Duration::from_millis(duration_ms));
            };
            match source {
                InputSource::Pointer { actions, .. } => match actions.get(tick) {
                    Some(PointerAction::Down { button, pressure }) => {
                        instant(PlannedInput::PointerDown {
                            button: *button,
                            pressure: checked_pressure(*pressure)
                                .ok_or_else(|| range("pressure"))?,
                            at: positions[index],
                        });
                    }
                    Some(PointerAction::Up { button }) => instant(PlannedInput::PointerUp {
                        button: *button,
                        at: positions[index],
                    }),
                    Some(PointerAction::Move {
                        to,
                        duration_ms,
                        pressure,
                    }) => {
                        if !in_range(*to) {
                            return Err(range("to"));
                        }
                        strokes.push(Stroke {
                            source: index,
                            from: positions[index].unwrap_or(*to),
                            to: *to,
                            duration: Duration::from_millis(*duration_ms),
                            pressure: checked_pressure(*pressure)
                                .ok_or_else(|| range("pressure"))?,
                        });
                        positions[index] = Some(*to);
                    }
                    Some(PointerAction::Cancel) => instant(PlannedInput::PointerCancel {
                        at: positions[index],
                    }),
                    Some(PointerAction::Pause { duration_ms }) => wait(*duration_ms),
                    None => {}
                },
                InputSource::Wheel { actions, .. } => match actions.get(tick) {
                    Some(WheelAction::Scroll {
                        at,
                        delta_x,
                        delta_y,
                        unit,
                    }) => {
                        if !in_range(*at) {
                            return Err(range("at"));
                        }
                        if !delta_x.is_finite() || !delta_y.is_finite() {
                            return Err(range("delta"));
                        }
                        instant(PlannedInput::Scroll {
                            at: *at,
                            delta_x: *delta_x,
                            delta_y: *delta_y,
                            unit: *unit,
                        });
                    }
                    Some(WheelAction::Pause { duration_ms }) => wait(*duration_ms),
                    None => {}
                },
                InputSource::Keys { actions, .. } => match actions.get(tick) {
                    Some(KeyAction::Down { key }) => instant(PlannedInput::KeyDown(key.clone())),
                    Some(KeyAction::Up { key }) => instant(PlannedInput::KeyUp(key.clone())),
                    Some(KeyAction::Pause { duration_ms }) => wait(*duration_ms),
                    None => {}
                },
                InputSource::Text { text } => {
                    if tick == 0 {
                        instant(PlannedInput::Text(text.clone()));
                    }
                }
                InputSource::Ime { actions } => {
                    if let Some(action) = actions.get(tick) {
                        instant(PlannedInput::Ime(action.clone()));
                    }
                }
                InputSource::Trackpad { actions, .. } => match actions.get(tick) {
                    Some(TrackpadAction::Pause { duration_ms }) => wait(*duration_ms),
                    Some(action) => {
                        if !trackpad_in_range(action) {
                            return Err(range("trackpad"));
                        }
                        instant(PlannedInput::Trackpad(action.clone()));
                    }
                    None => {}
                },
            }
        }
        let end = lay_strokes(&strokes, start, tick, lasts, &mut inputs);
        start = end;
    }
    if start > MAX_SPAN {
        return Err(PlanError::TooLong { span: start });
    }
    Ok(Plan {
        sources,
        inputs,
        span: start,
    })
}

/// A move one source makes in a tick, before it is split into steps.
struct Stroke {
    source: usize,
    from: LogicalPoint,
    to: LogicalPoint,
    duration: Duration,
    pressure: Option<f64>,
}

/// Lays the steps of a tick's `strokes` after its instants, which start at `start`, and answers
/// when the tick ends: at its longest stroke, its longest pause (`lasts`), or one [`STEP`] after a
/// stroke that names no duration.
fn lay_strokes(
    strokes: &[Stroke],
    start: Duration,
    tick: usize,
    lasts: Duration,
    inputs: &mut Vec<TimedInput>,
) -> Duration {
    let mut end = lasts;
    // Every step of every stroke at the whole millisecond it falls in, counted from the tick's
    // start: one stroke's steps are at least a millisecond apart, and the steps of several
    // strokes due in one millisecond go out together, in the order their sources are listed.
    let mut steps: Vec<(Duration, usize, LogicalPoint, Option<f64>)> = Vec::new();
    for (order, stroke) in strokes.iter().enumerate() {
        let count = split(stroke.duration);
        let share = stroke.duration.checked_div(count).unwrap_or(Duration::ZERO);
        for step in 1..=count {
            let fraction = f64::from(step) / f64::from(count);
            let at = LogicalPoint::new(
                stroke.from.x + (stroke.to.x - stroke.from.x) * fraction,
                stroke.from.y + (stroke.to.y - stroke.from.y) * fraction,
            );
            steps.push((
                whole_millis(share.saturating_mul(step - 1)),
                order,
                at,
                stroke.pressure,
            ));
        }
        end = end.max(if stroke.duration.is_zero() {
            STEP
        } else {
            stroke.duration
        });
    }
    steps.sort_by_key(|&(due, order, ..)| (due, order));
    for (due, order, to, pressure) in steps {
        inputs.push(TimedInput {
            at: start.saturating_add(due),
            tick,
            source: strokes[order].source,
            input: PlannedInput::PointerMove { to, pressure },
        });
    }
    start.saturating_add(end)
}

fn planned_source(source: &InputSource) -> PlannedSource {
    let (id, kind) = match source {
        InputSource::Pointer { id, device, .. } => (Some(id), SourceKind::Pointer(*device)),
        InputSource::Wheel { id, .. } => (Some(id), SourceKind::Wheel),
        InputSource::Keys { id, .. } => (Some(id), SourceKind::Keys),
        InputSource::Text { .. } => (None, SourceKind::Text),
        InputSource::Ime { .. } => (None, SourceKind::Ime),
        InputSource::Trackpad { id, .. } => (Some(id), SourceKind::Trackpad),
    };
    PlannedSource {
        id: id.cloned(),
        kind,
    }
}

/// `pressure` when it is absent or in `0.0..=1.0`; `None` when it is outside.
#[expect(
    clippy::option_option,
    reason = "the outer `None` is a refusal, the inner one a pressure the action leaves out"
)]
fn checked_pressure(pressure: Option<f64>) -> Option<Option<f64>> {
    match pressure {
        Some(pressure) if !(0.0..=1.0).contains(&pressure) => None,
        pressure => Some(pressure),
    }
}

fn in_range(point: LogicalPoint) -> bool {
    [point.x, point.y]
        .iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_COORDINATE)
}

fn trackpad_in_range(action: &TrackpadAction) -> bool {
    match action {
        TrackpadAction::Begin { at } => in_range(*at),
        TrackpadAction::Pinch { scale } => scale.is_finite() && *scale > 0.0,
        TrackpadAction::Rotate { degrees } => degrees.is_finite(),
        TrackpadAction::Pan { dx, dy } => dx.is_finite() && dy.is_finite(),
        TrackpadAction::End | TrackpadAction::Pause { .. } => true,
    }
}

/// `instant` cut to the whole millisecond it falls in.
fn whole_millis(instant: Duration) -> Duration {
    Duration::new(instant.as_secs(), instant.subsec_millis() * 1_000_000)
}

/// How many steps a move of `duration` takes: one per [`STEP`], at least one, at most
/// [`MAX_STEPS`].
fn split(duration: Duration) -> u32 {
    let steps = duration.as_millis().div_ceil(STEP.as_millis()).max(1);
    u32::try_from(steps).map_or(MAX_STEPS, |steps| steps.min(MAX_STEPS))
}
