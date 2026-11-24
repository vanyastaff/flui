//! The action sequence each gesture method performs, built from where it lands (ADR-0400 §2).
//!
//! A gesture is one composition, whichever side performs it: the app's agent on the synthetic
//! path and the driver's OS lane both build the sequence here from the point and bounds the
//! target showed when it was found actionable, and both play it through [`plan`](super::plan).
//! Each builder is data to data: it reads no clock and no tree.
//!
//! A gesture that focuses its target before it types (`element.type_text`, and `element.press_keys`
//! with a target) is two sequences played in turn, the focusing [`tap`] and then [`text`] or
//! [`chords`]: committed text is one tick at a sequence's start, so it cannot follow a tap within
//! one sequence.

use std::fmt;
use std::time::Duration;

use super::{
    ActionSequence, Direction, InputSource, KeyAction, PointerAction, PointerButton, PointerDevice,
    ScrollUnit, WheelAction,
};
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::wire::ActionName;

/// How long a long press holds when the call names no duration, in milliseconds: the platforms'
/// long-press delay (500 ms) and a margin.
const LONG_PRESS_MS: u64 = 800;

/// The gap between the two taps of a double tap, in milliseconds: well inside every platform's
/// double-tap timeout.
const DOUBLE_TAP_GAP_MS: u64 = 60;

/// How long the move of a drag and of a pinch lasts, in milliseconds.
const STROKE_MS: u64 = 300;

/// A fling's speed when the call names none, in logical pixels a second: well above the
/// platforms' fling velocity.
const FLING_VELOCITY: f64 = 2_500.0;

/// How long a swipe holds still before it lifts, in milliseconds, so its release velocity is
/// under the fling velocity whatever speed it moved at.
pub const SWIPE_SETTLE_MS: u64 = 120;

/// The longest the move of a swipe or a fling lasts, in milliseconds; a slower stroke is this
/// long.
const MAX_STROKE_MS: u64 = 10_000;

/// How far a swipe goes when the call names no distance and its target shows no bounds, in
/// logical pixels.
const DEFAULT_STROKE_DISTANCE: f64 = 100.0;

/// The share of its target's extent along the stroke a swipe goes when the call names no
/// distance.
const STROKE_SHARE: f64 = 0.8;

/// A pinch's contacts start this far from the centre when the target shows no bounds, in logical
/// pixels; with bounds, a quarter of the shorter side.
const DEFAULT_PINCH_RADIUS: f64 = 50.0;

/// The nearest a pinch's contacts start to the centre, in logical pixels.
const MIN_PINCH_RADIUS: f64 = 4.0;

/// The source id of a gesture's mouse.
const MOUSE: &str = "gesture.mouse";

/// The source id of a gesture's touch contact; a pinch's two are [`PINCH_CONTACTS`].
const TOUCH: &str = "gesture.touch";

/// The source ids of a pinch's two touch contacts.
const PINCH_CONTACTS: [&str; 2] = ["gesture.pinch.a", "gesture.pinch.b"];

/// The source id of a gesture's keyboard.
const KEYS: &str = "gesture.keys";

/// The source id of a gesture's wheel.
const WHEEL: &str = "gesture.wheel";

/// Where a gesture lands: the point it aims at, and the bounds of what it aims at when it aims at
/// an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Aim {
    /// The point, in the window's logical surface.
    pub point: LogicalPoint,
    /// The bounds of the element aimed at; `None` for a point the call names.
    pub bounds: Option<LogicalRect>,
}

impl Aim {
    /// `point`, on an element with `bounds`.
    #[must_use]
    pub const fn new(point: LogicalPoint, bounds: Option<LogicalRect>) -> Self {
        Self { point, bounds }
    }
}

/// Why a gesture's parameters make no sequence.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GestureError {
    /// A swipe or fling whose distance or velocity is not finite and above zero.
    StrokeGoesNowhere {
        /// How far, in logical pixels.
        distance: f64,
        /// How fast, in logical pixels a second.
        velocity: f64,
    },
    /// A pinch whose scale is not finite and above zero.
    NoScale {
        /// The scale asked for.
        scale: f64,
    },
    /// A chord of `element.press_keys` that names no key.
    EmptyChord {
        /// The chord as asked.
        chord: String,
    },
    /// A chord of `element.press_keys` that does not read as `+`-joined keys: a `+` with no key
    /// after it, or a key name with space around it.
    UnreadableChord {
        /// The chord as asked.
        chord: String,
    },
}

impl fmt::Display for GestureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StrokeGoesNowhere { distance, velocity } => {
                write!(
                    f,
                    "a stroke of {distance} px at {velocity} px/s goes nowhere"
                )
            }
            Self::NoScale { scale } => write!(f, "a pinch to scale {scale} is not one"),
            Self::EmptyChord { chord } => write!(f, "the chord {chord:?} names no key"),
            Self::UnreadableChord { chord } => {
                write!(f, "the chord {chord:?} does not read as `+`-joined keys")
            }
        }
    }
}

impl std::error::Error for GestureError {}

fn pointer(device: PointerDevice, id: &str, actions: Vec<PointerAction>) -> InputSource {
    InputSource::Pointer {
        id: id.to_owned(),
        device,
        actions,
    }
}

const fn move_to(to: LogicalPoint, duration_ms: u64) -> PointerAction {
    PointerAction::Move {
        to,
        duration_ms,
        pressure: None,
    }
}

const fn down(button: PointerButton) -> PointerAction {
    PointerAction::Down {
        button,
        pressure: None,
    }
}

const fn up(button: PointerButton) -> PointerAction {
    PointerAction::Up { button }
}

const fn pause(duration_ms: u64) -> PointerAction {
    PointerAction::Pause { duration_ms }
}

/// `element.tap`: the mouse moves to `at`, presses `button` and releases it.
#[must_use]
pub fn tap(at: LogicalPoint, button: PointerButton) -> ActionSequence {
    ActionSequence::new(vec![pointer(
        PointerDevice::Mouse,
        MOUSE,
        vec![move_to(at, 0), down(button), up(button)],
    )])
}

/// The semantics action that performs a tap of `button` on the semantic path: a primary tap
/// invokes its target, and no other tap has one.
#[must_use]
pub fn tap_action(button: PointerButton) -> Option<ActionName> {
    (button == PointerButton::Primary).then_some(ActionName::Invoke)
}

/// `element.double_tap`: two taps of `button` at `at`, 60 ms apart.
#[must_use]
pub fn double_tap(at: LogicalPoint, button: PointerButton) -> ActionSequence {
    ActionSequence::new(vec![pointer(
        PointerDevice::Mouse,
        MOUSE,
        vec![
            move_to(at, 0),
            down(button),
            up(button),
            pause(DOUBLE_TAP_GAP_MS),
            down(button),
            up(button),
        ],
    )])
}

/// `element.long_press`: a touch contact held at `at` for `duration_ms`, or 800 ms.
#[must_use]
pub fn long_press(at: LogicalPoint, duration_ms: Option<u64>) -> ActionSequence {
    ActionSequence::new(vec![pointer(
        PointerDevice::Touch,
        TOUCH,
        vec![
            move_to(at, 0),
            down(PointerButton::Primary),
            pause(duration_ms.unwrap_or(LONG_PRESS_MS)),
            up(PointerButton::Primary),
        ],
    )])
}

/// `element.drag_to`: the mouse presses at `from`, moves to `to` over 300 ms and releases.
#[must_use]
pub fn drag(from: LogicalPoint, to: LogicalPoint) -> ActionSequence {
    ActionSequence::new(vec![pointer(
        PointerDevice::Mouse,
        MOUSE,
        vec![
            move_to(from, 0),
            down(PointerButton::Primary),
            move_to(to, STROKE_MS),
            up(PointerButton::Primary),
        ],
    )])
}

/// The unit step on screen of `direction`.
const fn step_of(direction: Direction) -> (f64, f64) {
    match direction {
        Direction::Up => (0.0, -1.0),
        Direction::Down => (0.0, 1.0),
        Direction::Left => (-1.0, 0.0),
        Direction::Right => (1.0, 0.0),
    }
}

/// How long a stroke of `distance` at `velocity` moves, in whole milliseconds within
/// `1..=MAX_STROKE_MS`.
fn stroke_ms(distance: f64, velocity: f64) -> u64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a float `as` an integer saturates, and the stroke is clamped just below"
    )]
    let stroke_ms = ((distance / velocity) * 1_000.0) as u64;
    stroke_ms.clamp(1, MAX_STROKE_MS)
}

/// `element.swipe` (`fling` false) and `element.fling`: a touch contact pressed at the aim, moved
/// `distance` towards `direction` at `velocity`, and lifted; a swipe holds still for
/// [`SWIPE_SETTLE_MS`] first, so it ends below the fling velocity.
///
/// The distance is most of the aimed element's extent along the stroke when absent, and the
/// velocity 2500 logical pixels a second.
///
/// # Errors
///
/// [`GestureError::StrokeGoesNowhere`] for a distance or velocity that is not finite and above
/// zero.
pub fn swipe(
    aim: Aim,
    direction: Direction,
    distance: Option<f64>,
    velocity: Option<f64>,
    fling: bool,
) -> Result<ActionSequence, GestureError> {
    let (dx, dy) = step_of(direction);
    let extent = aim.bounds.map_or(DEFAULT_STROKE_DISTANCE, |bounds| {
        if dx == 0.0 {
            bounds.height
        } else {
            bounds.width
        }
    });
    let distance = distance.unwrap_or(extent * STROKE_SHARE);
    let velocity = velocity.unwrap_or(FLING_VELOCITY);
    if !(distance.is_finite() && distance > 0.0 && velocity.is_finite() && velocity > 0.0) {
        return Err(GestureError::StrokeGoesNowhere { distance, velocity });
    }
    let start = aim.point;
    let end = LogicalPoint::new(start.x + dx * distance, start.y + dy * distance);
    let mut actions = vec![
        move_to(start, 0),
        down(PointerButton::Primary),
        move_to(end, stroke_ms(distance, velocity)),
    ];
    if !fling {
        actions.push(pause(SWIPE_SETTLE_MS));
    }
    actions.push(up(PointerButton::Primary));
    Ok(ActionSequence::new(vec![pointer(
        PointerDevice::Touch,
        TOUCH,
        actions,
    )]))
}

/// `element.pinch`: two touch contacts on either side of the aim, moved over 300 ms until
/// their distance is `scale` times what it was.
///
/// # Errors
///
/// [`GestureError::NoScale`] for a scale that is not finite and above zero.
pub fn pinch(aim: Aim, scale: f64) -> Result<ActionSequence, GestureError> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(GestureError::NoScale { scale });
    }
    let radius = aim
        .bounds
        .map_or(DEFAULT_PINCH_RADIUS, |bounds| {
            bounds.width.min(bounds.height) / 4.0
        })
        .max(MIN_PINCH_RADIUS);
    let contact = |id: &str, side: f64| {
        let at = |r: f64| LogicalPoint::new(aim.point.x + side * r, aim.point.y);
        pointer(
            PointerDevice::Touch,
            id,
            vec![
                move_to(at(radius), 0),
                down(PointerButton::Primary),
                move_to(at(radius * scale), STROKE_MS),
                up(PointerButton::Primary),
            ],
        )
    };
    let [first, second] = PINCH_CONTACTS;
    Ok(ActionSequence::new(vec![
        contact(first, -1.0),
        contact(second, 1.0),
    ]))
}

/// The text `element.type_text` commits, once its target has focus.
#[must_use]
pub fn text(text: &str) -> ActionSequence {
    ActionSequence::new(vec![InputSource::Text {
        text: text.to_owned(),
    }])
}

/// The keys of one chord, `+`-joined: a `+` where a key stands is the plus key (`"+"`,
/// `"Control++"`).
fn keys_of(chord: &str) -> Result<Vec<&str>, GestureError> {
    let unreadable = || GestureError::UnreadableChord {
        chord: chord.to_owned(),
    };
    if chord.is_empty() {
        return Err(GestureError::EmptyChord {
            chord: chord.to_owned(),
        });
    }
    let mut keys = Vec::new();
    let mut rest = chord;
    loop {
        let (key, after) = match rest.strip_prefix('+') {
            Some(after) => ("+", after),
            None => rest.split_at(rest.find('+').unwrap_or(rest.len())),
        };
        // A key's name has no space around it; a space is a key of its own (`" "`).
        if key.chars().count() > 1 && key.trim() != key {
            return Err(unreadable());
        }
        keys.push(key);
        if after.is_empty() {
            return Ok(keys);
        }
        rest = match after.strip_prefix('+') {
            Some(next) if !next.is_empty() => next,
            _ => return Err(unreadable()),
        };
    }
}

/// The chords `element.press_keys` presses in turn, each `+`-joined (`"Control+a"`, and
/// `"Control++"` for the plus key): each chord's keys go down in order and come up in reverse.
///
/// # Errors
///
/// [`GestureError::EmptyChord`] for a chord that names no key, and
/// [`GestureError::UnreadableChord`] for one that does not read as `+`-joined keys.
pub fn chords(chords: &[String]) -> Result<ActionSequence, GestureError> {
    let mut actions = Vec::new();
    for chord in chords {
        let keys = keys_of(chord)?;
        actions.extend(keys.iter().map(|key| KeyAction::Down {
            key: (*key).to_owned(),
        }));
        actions.extend(keys.iter().rev().map(|key| KeyAction::Up {
            key: (*key).to_owned(),
        }));
    }
    Ok(ActionSequence::new(vec![InputSource::Keys {
        id: KEYS.to_owned(),
        actions,
    }]))
}

/// One step of `element.scroll_until`: the wheel turned at `at` by `distance` logical pixels
/// towards `direction`, which scrolls a scrollable's content that way.
#[must_use]
pub fn scroll(at: LogicalPoint, direction: Direction, distance: f64) -> ActionSequence {
    let (dx, dy) = step_of(direction);
    ActionSequence::new(vec![InputSource::Wheel {
        id: WHEEL.to_owned(),
        actions: vec![WheelAction::Scroll {
            at,
            delta_x: dx * distance,
            delta_y: dy * distance,
            unit: ScrollUnit::Pixel,
        }],
    }])
}

/// How long the sequences a gesture plays take a window to play once its target is found
/// ([`ActionSequence::plays_within`]), from its parameters alone: where a duration depends on the
/// target's bounds, the longest it can be. A window that serves the gesture may hold the request
/// this long past the act's timeout (ADR-0288 §4).
pub(crate) trait PlayBound {
    /// The bound.
    fn plays_within(&self) -> Duration;
}

const ORIGIN: LogicalPoint = LogicalPoint::new(0.0, 0.0);

impl PlayBound for super::TapParams {
    /// A double tap, the longer of the two gestures these parameters take.
    fn plays_within(&self) -> Duration {
        double_tap(ORIGIN, self.button).plays_within()
    }
}

impl PlayBound for super::LongPressParams {
    fn plays_within(&self) -> Duration {
        long_press(ORIGIN, self.duration_ms).plays_within()
    }
}

impl PlayBound for super::DragParams {
    fn plays_within(&self) -> Duration {
        drag(ORIGIN, ORIGIN).plays_within()
    }
}

impl PlayBound for super::SwipeParams {
    /// A swipe, which holds still before it lifts, of the longest stroke the distance allows: the
    /// one named, or the longest stroke (10 s) when the target's extent sets it.
    fn plays_within(&self) -> Duration {
        let stroke = match self.distance {
            Some(distance) => {
                let velocity = self.velocity.unwrap_or(FLING_VELOCITY);
                if distance.is_finite() && distance > 0.0 && velocity.is_finite() && velocity > 0.0
                {
                    stroke_ms(distance, velocity)
                } else {
                    0
                }
            }
            None => MAX_STROKE_MS,
        };
        ActionSequence::new(vec![pointer(
            PointerDevice::Touch,
            TOUCH,
            vec![
                move_to(ORIGIN, 0),
                down(PointerButton::Primary),
                move_to(ORIGIN, stroke),
                pause(SWIPE_SETTLE_MS),
                up(PointerButton::Primary),
            ],
        )])
        .plays_within()
    }
}

impl PlayBound for super::PinchParams {
    fn plays_within(&self) -> Duration {
        pinch(Aim::new(ORIGIN, None), 1.0)
            .map_or(Duration::ZERO, |sequence| sequence.plays_within())
    }
}

impl PlayBound for super::TypeTextParams {
    /// The focusing tap, then the text.
    fn plays_within(&self) -> Duration {
        tap(ORIGIN, PointerButton::Primary)
            .plays_within()
            .saturating_add(text(&self.text).plays_within())
    }
}

impl PlayBound for super::PressKeysParams {
    /// The focusing tap when there is a target, then the chords.
    fn plays_within(&self) -> Duration {
        let focus = if self.target.is_some() {
            tap(ORIGIN, PointerButton::Primary).plays_within()
        } else {
            Duration::ZERO
        };
        let keys = chords(&self.keys).map_or(Duration::ZERO, |sequence| sequence.plays_within());
        focus.saturating_add(keys)
    }
}

impl PlayBound for super::ScrollIntoViewParams {
    /// A semantics action, which plays no input.
    fn plays_within(&self) -> Duration {
        Duration::ZERO
    }
}

impl PlayBound for super::ScrollUntilParams {
    /// One scroll past the timeout: the window answers once the scroll under way when the
    /// timeout passes has played, and scrolls no more.
    fn plays_within(&self) -> Duration {
        scroll(ORIGIN, Direction::Down, 1.0).plays_within()
    }
}
