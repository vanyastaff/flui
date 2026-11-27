//! The action sequence each gesture method performs, built from where it lands (ADR-0400 §2).
//!
//! A gesture is one composition, whichever side performs it: the app's agent on the synthetic
//! path and the driver's OS lane both build the sequence here from the point and bounds the
//! target showed when it was found actionable, and both play it through [`plan`](super::plan).
//! Each builder is data to data: it reads no clock and no tree. What a call leaves unnamed and a
//! window's settings decide (a long press's hold, a stroke's speed and least length) comes from
//! [`Defaults`]: the window's own gesture settings where the composer has them, the windowless
//! values otherwise.
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

/// The pause between the two taps of a double tap, in milliseconds: inside a double-tap timeout
/// of 100 ms or more with the frames the taps themselves take.
const DOUBLE_TAP_GAP_MS: u64 = 60;

/// How long the move of a drag and of a pinch lasts, in milliseconds.
const STROKE_MS: u64 = 300;

/// How long a swipe holds still before it lifts, in milliseconds: longer than the 100 ms a
/// velocity tracker looks back, so it lifts with no speed whatever speed it moved at.
pub const SWIPE_SETTLE_MS: u64 = 120;

/// The longest the move of a swipe or a fling lasts, in milliseconds; a slower stroke is this
/// long.
const MAX_STROKE_MS: u64 = 10_000;

/// How far a swipe goes when the call names no distance and its target shows no bounds, in
/// logical pixels.
const DEFAULT_STROKE_DISTANCE: f64 = 100.0;

/// The share of the room between its point and the edge of what it aims at a swipe goes when the
/// call names no distance, so the stroke ends inside the target's shown part.
const STROKE_SHARE: f64 = 0.8;

/// A pinch's contacts start this far from the centre when the target shows no bounds, in logical
/// pixels; with bounds, a quarter of the shorter side.
const DEFAULT_PINCH_RADIUS: f64 = 50.0;

/// The nearest a pinch's contacts start to the centre, in logical pixels.
const MIN_PINCH_RADIUS: f64 = 4.0;

/// What a gesture takes where its call names no duration, velocity or distance and the window it
/// plays in decides what is recognised as that gesture.
///
/// The app's agent fills it from the window's own gesture settings (the primary pointer of its
/// platform, the long-press timeout, the fling velocities and the slops, which follow its platform
/// and the system's preferences); a composer with no window to read, the driver's OS lane, takes
/// [`Defaults::WINDOWLESS`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Defaults {
    /// The pointer a tap, a double tap and a drag play with: the window's primary pointer, a
    /// touch contact where its platform's is touch and a mouse elsewhere. A long press, a swipe, a
    /// fling and a pinch are touch gestures on every window.
    pub pointer: PointerDevice,
    /// How long a long press holds, in milliseconds; at most
    /// [`Self::LONGEST_LONG_PRESS_MS`].
    pub long_press_ms: u64,
    /// A fling's speed, and a swipe's before it holds still, in logical pixels a second.
    pub fling_velocity: f64,
    /// The shortest stroke a swipe makes, in logical pixels, so it passes the slop a drag starts
    /// past.
    pub min_stroke: f64,
    /// The drag slop of [`Self::pointer`], in logical pixels: a drag no longer than it starts no
    /// drag, and [`drag`] refuses it.
    pub min_drag: f64,
}

impl Defaults {
    /// What a composer with no window to read takes: a mouse, a press held 800 ms, a stroke at
    /// 2500 px/s at least 36 px long, and a drag longer than a mouse's 2 px slop.
    pub const WINDOWLESS: Self = Self {
        pointer: PointerDevice::Mouse,
        long_press_ms: 800,
        fling_velocity: 2_500.0,
        min_stroke: 36.0,
        min_drag: 2.0,
    };

    /// The longest a long press naming no duration holds, in milliseconds, which the reply bound
    /// of its parameters counts ([`Addressed::holds_for`]): a window whose long-press timeout
    /// asks for more refuses the call with the reason, so it can name its duration.
    ///
    /// [`Addressed::holds_for`]: crate::method::Addressed::holds_for
    pub const LONGEST_LONG_PRESS_MS: u64 = 4_000;
}

/// The source id of a gesture's mouse.
const MOUSE: &str = "gesture.mouse";

/// The source id of a gesture's touch contact; a pinch's two are [`PINCH_CONTACTS`].
const TOUCH: &str = "gesture.touch";

/// The source id of a gesture's pen.
const PEN: &str = "gesture.pen";

/// The fewest moves a fling's stroke makes: with the press they are the three samples a velocity
/// tracker measures a release from, so a stroke over in fewer lifts with no speed.
const FLING_MOVES: u64 = 2;

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
    /// A fling whose distance or velocity the window chose and whose stroke is over in fewer
    /// moves than a release velocity is measured from ([`FLING_MOVES`]), so it would lift with no
    /// speed.
    FlingTooShort {
        /// How far, in logical pixels.
        distance: f64,
        /// How fast, in logical pixels a second.
        velocity: f64,
    },
    /// A drag no longer than the slop a drag starts past.
    DragTooShort {
        /// How long, in logical pixels.
        length: f64,
        /// The slop, in logical pixels.
        slop: f64,
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
    /// A chord of `element.press_keys` with a key that is neither a key name nor the text one key
    /// types ([`is_one_key`]): a mistyped name (`Ctrl-a`, `Page_Down`), never typed as text.
    NotAKey {
        /// The chord as asked.
        chord: String,
        /// The key it names.
        key: String,
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
            Self::FlingTooShort { distance, velocity } => write!(
                f,
                "a fling of {distance:.1} px at {velocity:.0} px/s is over before its release \
                 speed can be measured: name a longer distance or a slower velocity"
            ),
            Self::DragTooShort { length, slop } => write!(
                f,
                "a drag of {length:.1} px does not pass this window's drag slop of {slop} px"
            ),
            Self::NoScale { scale } => write!(f, "a pinch to scale {scale} is not one"),
            Self::EmptyChord { chord } => write!(f, "the chord {chord:?} names no key"),
            Self::UnreadableChord { chord } => {
                write!(f, "the chord {chord:?} does not read as `+`-joined keys")
            }
            Self::NotAKey { chord, key } => write!(
                f,
                "the chord {chord:?} names {key:?}, which is neither a key name (`Enter`, \
                 `ArrowDown`) nor the text one key types (`a`)"
            ),
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

/// The source id a gesture's `device` plays through.
const fn source_of(device: PointerDevice) -> &'static str {
    match device {
        PointerDevice::Touch => TOUCH,
        PointerDevice::Pen => PEN,
        _ => MOUSE,
    }
}

/// `element.tap`: the window's primary pointer ([`Defaults::pointer`]) moves to `at`, presses
/// `button` and releases it.
#[must_use]
pub fn tap(at: LogicalPoint, button: PointerButton, defaults: &Defaults) -> ActionSequence {
    ActionSequence::new(vec![pointer(
        defaults.pointer,
        source_of(defaults.pointer),
        vec![move_to(at, 0), down(button), up(button)],
    )])
}

/// The semantics action that performs a tap of `button` on the semantic path: a primary tap
/// invokes its target, and no other tap has one.
#[must_use]
pub fn tap_action(button: PointerButton) -> Option<ActionName> {
    (button == PointerButton::Primary).then_some(ActionName::Invoke)
}

/// `element.double_tap`: two taps of `button` at `at`, 60 ms apart, with the window's primary
/// pointer.
#[must_use]
pub fn double_tap(at: LogicalPoint, button: PointerButton, defaults: &Defaults) -> ActionSequence {
    ActionSequence::new(vec![pointer(
        defaults.pointer,
        source_of(defaults.pointer),
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

/// `element.long_press`: a touch contact held at `at` for `duration_ms`, or
/// [`Defaults::long_press_ms`].
#[must_use]
pub fn long_press(
    at: LogicalPoint,
    duration_ms: Option<u64>,
    defaults: &Defaults,
) -> ActionSequence {
    ActionSequence::new(vec![pointer(
        PointerDevice::Touch,
        TOUCH,
        vec![
            move_to(at, 0),
            down(PointerButton::Primary),
            pause(duration_ms.unwrap_or(defaults.long_press_ms)),
            up(PointerButton::Primary),
        ],
    )])
}

/// `element.drag_to`: the window's primary pointer presses at `from`, moves to `to` over 300 ms,
/// holds still for [`SWIPE_SETTLE_MS`] and releases, so it is dropped where it ends rather than
/// flung on.
///
/// # Errors
///
/// [`GestureError::DragTooShort`] for a drag no longer than [`Defaults::min_drag`], which starts
/// no drag.
pub fn drag(
    from: LogicalPoint,
    to: LogicalPoint,
    defaults: &Defaults,
) -> Result<ActionSequence, GestureError> {
    let length = (to.x - from.x).hypot(to.y - from.y);
    if length.is_nan() || length <= defaults.min_drag {
        return Err(GestureError::DragTooShort {
            length,
            slop: defaults.min_drag,
        });
    }
    Ok(ActionSequence::new(vec![pointer(
        defaults.pointer,
        source_of(defaults.pointer),
        vec![
            move_to(from, 0),
            down(PointerButton::Primary),
            move_to(to, STROKE_MS),
            pause(SWIPE_SETTLE_MS),
            up(PointerButton::Primary),
        ],
    )]))
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

/// The room from `aim`'s point to the edge of what it aims at, along `(dx, dy)`, in logical
/// pixels; [`DEFAULT_STROKE_DISTANCE`] for a point the call names, with no bounds.
fn room_of(aim: Aim, dx: f64, dy: f64) -> f64 {
    let Some(bounds) = aim.bounds else {
        return DEFAULT_STROKE_DISTANCE;
    };
    let point = aim.point;
    let room = if dx > 0.0 {
        bounds.x + bounds.width - point.x
    } else if dx < 0.0 {
        point.x - bounds.x
    } else if dy > 0.0 {
        bounds.y + bounds.height - point.y
    } else {
        point.y - bounds.y
    };
    room.max(0.0)
}

/// `element.swipe` (`fling` false) and `element.fling`: a touch contact pressed at the aim, moved
/// `distance` towards `direction` at `velocity`, and lifted; a swipe holds still for
/// [`SWIPE_SETTLE_MS`] first, so it ends below the fling velocity.
///
/// With no distance named, the stroke goes most of the way from the point to the edge of what it
/// aims at in that direction, so it ends inside the part the aim's bounds hold (the part of the
/// element shown, where the agent aims), and at least [`Defaults::min_stroke`], so it passes the
/// slop; an aim with no room that way makes no stroke. The velocity is
/// [`Defaults::fling_velocity`] when absent.
///
/// # Errors
///
/// [`GestureError::StrokeGoesNowhere`] for a distance or velocity that is not finite and above
/// zero, and [`GestureError::FlingTooShort`] for a fling whose distance or velocity the window
/// chose and whose stroke is over before its release speed can be measured.
pub fn swipe(
    aim: Aim,
    direction: Direction,
    distance: Option<f64>,
    velocity: Option<f64>,
    fling: bool,
    defaults: &Defaults,
) -> Result<ActionSequence, GestureError> {
    let (named_distance, named_velocity) = (distance, velocity);
    let (dx, dy) = step_of(direction);
    let distance = named_distance.unwrap_or_else(|| {
        let room = room_of(aim, dx, dy);
        if room > 0.0 {
            (room * STROKE_SHARE).max(defaults.min_stroke)
        } else {
            0.0
        }
    });
    let chosen = named_distance.is_none() || named_velocity.is_none();
    let velocity = named_velocity.unwrap_or(defaults.fling_velocity);
    if !(distance.is_finite() && distance > 0.0 && velocity.is_finite() && velocity > 0.0) {
        return Err(GestureError::StrokeGoesNowhere { distance, velocity });
    }
    let stroke = stroke_ms(distance, velocity);
    let step_ms = u64::try_from(super::STEP.as_millis()).unwrap_or(u64::MAX);
    if fling && chosen && stroke.div_ceil(step_ms) < FLING_MOVES {
        return Err(GestureError::FlingTooShort { distance, velocity });
    }
    let start = aim.point;
    let end = LogicalPoint::new(start.x + dx * distance, start.y + dy * distance);
    let mut actions = vec![
        move_to(start, 0),
        down(PointerButton::Primary),
        move_to(end, stroke),
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

/// Whether `key` names one key, the one rule for a key a chord or a key action names: a key name,
/// an upper-case ASCII letter then ASCII letters and digits (the shape of every W3C key name:
/// `Enter`, `ArrowDown`, `F12`), or the text one key types, one Unicode scalar value and only what
/// joins it into one character (a combining accent, a variation selector, an emoji's
/// skin tone, tags or keycap, a flag's second regional indicator, or what a zero-width joiner
/// joins). A name of that shape the window does not know is refused where it is lowered; anything
/// else (`Ctrl-a`, `Page_Down`, `Arrow Down`, `ctrl`, two symbols such as `→→`) is not a key, and
/// never typed as text.
#[must_use]
pub fn is_one_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let rest = chars.as_str();
    if rest.is_empty() {
        return true;
    }
    if first.is_ascii_uppercase() && rest.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return true;
    }
    let mut previous = first;
    let mut flag_open = is_regional_indicator(first);
    for next in rest.chars() {
        let joined = if previous == ZERO_WIDTH_JOINER {
            !next.is_ascii() && !next.is_whitespace()
        } else if flag_open && is_regional_indicator(next) {
            flag_open = false;
            true
        } else {
            joins(next)
        };
        if !joined {
            return false;
        }
        previous = next;
    }
    previous != ZERO_WIDTH_JOINER
}

/// U+200D, which joins the characters on either side of it into one (an emoji sequence).
const ZERO_WIDTH_JOINER: char = '\u{200D}';

/// Whether `c` joins the character before it into one character a key types: a combining mark of
/// the combining blocks, the zero-width joiner, a variation selector, an emoji skin-tone modifier
/// or a tag character.
fn joins(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
            | ZERO_WIDTH_JOINER
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{E0100}'..='\u{E01EF}'
            | '\u{1F3FB}'..='\u{1F3FF}'
            | '\u{E0020}'..='\u{E007F}'
    )
}

/// Whether `c` is a regional indicator, two of which make a flag.
fn is_regional_indicator(c: char) -> bool {
    matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}

/// The keys of one chord, in the order they go down: its keys joined by `+`, where a `+` that
/// stands where a key does is the plus key (`"+"`, `"Control++"`), and a key name has no space
/// around it (`" "` alone is the space key). Each key is one key ([`is_one_key`]). The one reading
/// of a chord, for every composer.
///
/// # Errors
///
/// [`GestureError::EmptyChord`] for an empty chord, [`GestureError::UnreadableChord`] for a `+`
/// with no key after it, a key after a `+` key with no `+` between, or a key name with space
/// around it, and [`GestureError::NotAKey`] for a key that is not one key.
pub fn chord_keys(chord: &str) -> Result<Vec<&str>, GestureError> {
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
        if !is_one_key(key) {
            return Err(GestureError::NotAKey {
                chord: chord.to_owned(),
                key: key.to_owned(),
            });
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
/// [`GestureError::EmptyChord`] for a chord that names no key,
/// [`GestureError::UnreadableChord`] for one that does not read as `+`-joined keys, and
/// [`GestureError::NotAKey`] for one naming a key that is not one key.
pub fn chords(chords: &[String]) -> Result<ActionSequence, GestureError> {
    let mut actions = Vec::new();
    for chord in chords {
        let keys = chord_keys(chord)?;
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
/// target's bounds or on the window's gesture settings, the longest it can be. A window that
/// serves the gesture may hold the request this long past the act's timeout (ADR-0288 §4).
pub(crate) trait PlayBound {
    /// The bound.
    fn plays_within(&self) -> Duration;
}

const ORIGIN: LogicalPoint = LogicalPoint::new(0.0, 0.0);

impl PlayBound for super::TapParams {
    /// A double tap, the longer of the two gestures these parameters take.
    fn plays_within(&self) -> Duration {
        double_tap(ORIGIN, self.button, &Defaults::WINDOWLESS).plays_within()
    }
}

impl PlayBound for super::LongPressParams {
    /// The press named, or the longest a window holds one naming no duration.
    fn plays_within(&self) -> Duration {
        let duration_ms = self.duration_ms.unwrap_or(Defaults::LONGEST_LONG_PRESS_MS);
        long_press(ORIGIN, Some(duration_ms), &Defaults::WINDOWLESS).plays_within()
    }
}

impl PlayBound for super::DragParams {
    /// A drag far enough to pass any slop: its length does not change how long it plays.
    fn plays_within(&self) -> Duration {
        drag(
            ORIGIN,
            LogicalPoint::new(1_000.0, 0.0),
            &Defaults::WINDOWLESS,
        )
        .map_or(Duration::ZERO, |sequence| sequence.plays_within())
    }
}

impl PlayBound for super::SwipeParams {
    /// A swipe, which holds still before it lifts, of the longest stroke
    /// its parameters allow: the distance and velocity named, or the longest stroke (10 s) when
    /// either is the window's to choose.
    fn plays_within(&self) -> Duration {
        let stroke = match (self.distance, self.velocity) {
            (Some(distance), Some(velocity)) => {
                if distance.is_finite() && distance > 0.0 && velocity.is_finite() && velocity > 0.0
                {
                    stroke_ms(distance, velocity)
                } else {
                    0
                }
            }
            _ => MAX_STROKE_MS,
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
        tap(ORIGIN, PointerButton::Primary, &Defaults::WINDOWLESS)
            .plays_within()
            .saturating_add(text(&self.text).plays_within())
    }
}

impl PlayBound for super::PressKeysParams {
    /// The focusing tap when there is a target, then the chords.
    fn plays_within(&self) -> Duration {
        let focus = if self.target.is_some() {
            tap(ORIGIN, PointerButton::Primary, &Defaults::WINDOWLESS).plays_within()
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
