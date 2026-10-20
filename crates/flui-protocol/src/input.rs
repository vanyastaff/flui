//! Input (ADR-0238 §5): the path an action takes, the per-device action sequence in WebDriver
//! BiDi's `input.performActions` shape, the gestures composed from it, and the input journal
//! that observes what was delivered.
//!
//! Positions are logical pixels in the window's surface ([`LogicalPoint`]). Ticks align the
//! sources: every source's n-th action shares tick n, and a `pause` keeps a source in step
//! while another acts.

use crate::act::JournalSpan;
use crate::geometry::LogicalPoint;
use crate::locator::Target;
use crate::snapshot::Snapshot;
use crate::tree::WindowId;

vocabulary! {
    /// Which way input reaches the app.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum InputPath {
        /// A semantics action, with no pointer or key.
        Semantic => "semantic",
        /// Platform input injected into the app's own input path, which recognizers, input
        /// taps and recordings see as native input. The default on every target.
        #[default]
        Synthetic => "synthetic",
        /// Real OS input through the target's lane: opt-in, announced and guarded.
        Os => "os",
    }
}

/// Which input path to take, and whether another is acceptable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InputPolicy {
    /// The path tried first.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub prefer: InputPath,
    /// Refuse any other path instead of falling back to it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub require: bool,
}

impl InputPolicy {
    /// Prefer `path`, falling back when the target cannot take it.
    #[must_use]
    pub const fn prefer(path: InputPath) -> Self {
        Self {
            prefer: path,
            require: false,
        }
    }

    /// `path` only.
    #[must_use]
    pub const fn require(path: InputPath) -> Self {
        Self {
            prefer: path,
            require: true,
        }
    }
}

vocabulary! {
    /// What kind of pointer a source is.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum PointerDevice {
        /// A mouse.
        #[default]
        Mouse => "mouse",
        /// A touch contact.
        Touch => "touch",
        /// A pen or stylus.
        Pen => "pen",
    }
}

vocabulary! {
    /// A pointer button.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum PointerButton {
        /// The main button: left, the contact itself, the pen tip.
        #[default]
        Primary => "primary",
        /// The context button: right, the pen barrel.
        Secondary => "secondary",
        /// The middle button.
        Middle => "middle",
        /// The back button.
        Back => "back",
        /// The forward button.
        Forward => "forward",
    }
}

/// One tick of a pointer source.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "type", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum PointerAction {
    /// Press a button, or put the contact down, where the pointer is.
    Down {
        /// The button.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        button: PointerButton,
        /// Pen or touch pressure, `0.0..=1.0`.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        pressure: Option<f64>,
    },
    /// Release a button, or lift the contact.
    Up {
        /// The button.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        button: PointerButton,
    },
    /// Move to a point in one step. An OS lane never interpolates (ADR-0238 §11); a synthetic
    /// move over `duration_ms` is spread over the frames of that span.
    Move {
        /// Where to.
        to: LogicalPoint,
        /// Over how long.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        duration_ms: u64,
        /// Pen or touch pressure, `0.0..=1.0`.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        pressure: Option<f64>,
    },
    /// Cancel the contact: the platform took it away.
    Cancel,
    /// Do nothing for this tick, for `duration_ms`.
    Pause {
        /// How long.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        duration_ms: u64,
    },
}

vocabulary! {
    /// What a wheel delta counts.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ScrollUnit {
        /// Logical pixels.
        #[default]
        Pixel => "pixel",
        /// Lines, as a wheel notch counts.
        Line => "line",
    }
}

/// One tick of a wheel source.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "type", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum WheelAction {
    /// Scroll at a point.
    Scroll {
        /// Where the wheel is.
        at: LogicalPoint,
        /// Rightwards.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        delta_x: f64,
        /// Downwards.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        delta_y: f64,
        /// What the deltas count.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        unit: ScrollUnit,
    },
    /// Do nothing for this tick, for `duration_ms`.
    Pause {
        /// How long.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        duration_ms: u64,
    },
}

/// One tick of a key source. A key is named in the platform input vocabulary's spelling
/// (`"Enter"`, `"Control"`, `"a"`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "type", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum KeyAction {
    /// Press a key.
    Down {
        /// The key.
        key: String,
    },
    /// Release a key.
    Up {
        /// The key.
        key: String,
    },
    /// Do nothing for this tick, for `duration_ms`.
    Pause {
        /// How long.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        duration_ms: u64,
    },
}

/// One step of an input-method composition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "type", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum ImeAction {
    /// Show `text` as the composition, the caret at `cursor` characters (its end when absent).
    SetComposition {
        /// The text being composed.
        text: String,
        /// Where the caret is, in characters.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        cursor: Option<u32>,
    },
    /// End the composition by committing `text`.
    Commit {
        /// The committed text.
        text: String,
    },
    /// End the composition without committing.
    Cancel,
}

/// One tick of a trackpad gesture source.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "type", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum TrackpadAction {
    /// Start a gesture at a point.
    Begin {
        /// Where the pointer is.
        at: LogicalPoint,
    },
    /// Scale by this factor since the last step.
    Pinch {
        /// Above 1 zooms in.
        scale: f64,
    },
    /// Rotate by this many degrees since the last step, clockwise.
    Rotate {
        /// Degrees.
        degrees: f64,
    },
    /// Pan by this much since the last step.
    Pan {
        /// Rightwards.
        dx: f64,
        /// Downwards.
        dy: f64,
    },
    /// End the gesture.
    End,
    /// Do nothing for this tick, for `duration_ms`.
    Pause {
        /// How long.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        duration_ms: u64,
    },
}

/// One device's actions, tick by tick.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "type", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum InputSource {
    /// A mouse, touch contact or pen.
    Pointer {
        /// The source's name, unique in the sequence; one id is one contact.
        id: String,
        /// What kind of pointer.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "crate::skip::is_default")
        )]
        device: PointerDevice,
        /// Its ticks.
        actions: Vec<PointerAction>,
    },
    /// A scroll wheel.
    Wheel {
        /// The source's name.
        id: String,
        /// Its ticks.
        actions: Vec<WheelAction>,
    },
    /// A keyboard.
    Keys {
        /// The source's name.
        id: String,
        /// Its ticks.
        actions: Vec<KeyAction>,
    },
    /// Committed text, with no keys: one tick.
    Text {
        /// The text.
        text: String,
    },
    /// An input-method composition, one step per tick.
    Ime {
        /// Its steps.
        actions: Vec<ImeAction>,
    },
    /// A trackpad's pinch, rotate and pan phases.
    Trackpad {
        /// The source's name.
        id: String,
        /// Its ticks.
        actions: Vec<TrackpadAction>,
    },
}

/// Input from several devices, aligned by tick.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ActionSequence {
    /// The devices, each with its ticks.
    pub sources: Vec<InputSource>,
}

impl ActionSequence {
    /// `sources`, aligned by tick.
    #[must_use]
    pub fn new(sources: Vec<InputSource>) -> Self {
        Self { sources }
    }
}

/// `input.inject` (synthetic, in the app) and `input.perform` (the driver chooses the path).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InputParams {
    /// The window that takes the input.
    pub window: WindowId,
    /// The input.
    pub actions: ActionSequence,
    /// Which path, for `input.perform`; `input.inject` is always synthetic.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub via: InputPolicy,
    /// What to read back once the input settled.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub snapshot: Snapshot,
}

impl InputParams {
    /// `actions` into `window`, by the session's preferred path.
    #[must_use]
    pub fn new(window: WindowId, actions: ActionSequence) -> Self {
        Self {
            window,
            actions,
            via: InputPolicy::default(),
            snapshot: Snapshot::None,
        }
    }
}

/// How a gesture is performed; every field has the session's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct GestureOptions {
    /// Which input path; the session's when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub via: Option<InputPolicy>,
    /// Skip every actionability check except `attached`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub force: bool,
    /// How long locating and actionability may take, in milliseconds; 5000 when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub timeout_ms: Option<u64>,
    /// What to read back once the gesture settled.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub snapshot: Snapshot,
}

/// `element.tap` and `element.double_tap`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct TapParams {
    /// What to tap.
    pub target: Target,
    /// Which button.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub button: PointerButton,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

impl TapParams {
    /// A primary tap on `target`.
    #[must_use]
    pub fn new(target: impl Into<Target>) -> Self {
        Self {
            target: target.into(),
            button: PointerButton::Primary,
            options: GestureOptions::default(),
        }
    }
}

/// `element.long_press`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct LongPressParams {
    /// What to press.
    pub target: Target,
    /// How long to hold, in milliseconds; the platform's long-press delay and a margin when
    /// absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub duration_ms: Option<u64>,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

/// `element.drag_to`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct DragParams {
    /// What to pick up.
    pub target: Target,
    /// Where to drop it.
    pub to: Target,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

vocabulary! {
    /// A direction on screen.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Direction {
        /// Upwards.
        Up => "up",
        /// Downwards.
        Down => "down",
        /// Leftwards.
        Left => "left",
        /// Rightwards.
        Right => "right",
    }
}

/// `element.swipe` and `element.fling`: a stroke from the target in a direction.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct SwipeParams {
    /// Where the stroke starts.
    pub target: Target,
    /// Which way it goes.
    pub direction: Direction,
    /// How far, in logical pixels; most of the target's extent along `direction` when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub distance: Option<f64>,
    /// How fast it ends, in logical pixels a second; a fling ends above the platform's fling
    /// velocity, a swipe below it, when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub velocity: Option<f64>,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

/// `element.pinch`: two contacts around the target's centre.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PinchParams {
    /// What to pinch.
    pub target: Target,
    /// The final scale: above 1 spreads the contacts, below 1 closes them.
    pub scale: f64,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

/// `element.scroll_into_view`: scroll each scrollable ancestor until the target is visible.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ScrollIntoViewParams {
    /// What to bring into view; a point is refused.
    pub target: Target,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

/// `element.type_text`: focus the target, then commit text into it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct TypeTextParams {
    /// The text field.
    pub target: Target,
    /// The text.
    pub text: String,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

/// `element.press_keys`: press chords in turn, each `+`-joined (`"Control+a"`, `"Enter"`).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct PressKeysParams {
    /// What to focus first; the focused element of the window when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub target: Option<Target>,
    /// The window, when there is no target.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub window: Option<WindowId>,
    /// The chords, in order.
    pub keys: Vec<String>,
    /// How.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub options: GestureOptions,
}

vocabulary! {
    /// What kind of device delivered a journal entry.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum DeviceClass {
        /// A mouse.
        Mouse => "mouse",
        /// A touch contact.
        Touch => "touch",
        /// A pen.
        Pen => "pen",
        /// A trackpad gesture.
        Trackpad => "trackpad",
        /// A keyboard.
        Keyboard => "keyboard",
        /// An input method.
        Ime => "ime",
    }
}

vocabulary! {
    /// What a journal entry delivered.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum InputKind {
        /// A button press or contact down.
        PointerDown => "pointer_down",
        /// A pointer move.
        PointerMove => "pointer_move",
        /// A button release or contact up.
        PointerUp => "pointer_up",
        /// A contact the platform cancelled.
        PointerCancel => "pointer_cancel",
        /// A wheel or trackpad scroll.
        Scroll => "scroll",
        /// A trackpad pinch, rotate or pan step.
        Gesture => "gesture",
        /// A key press.
        KeyDown => "key_down",
        /// A key release.
        KeyUp => "key_up",
        /// Committed text.
        Text => "text",
        /// An input-method composition step.
        Composition => "composition",
    }
}

/// One delivered input in a window's journal.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct JournalEntry {
    /// Its place in the window's journal, from 1.
    pub seq: u64,
    /// What delivered it.
    pub device: DeviceClass,
    /// What it was.
    pub kind: InputKind,
    /// Where, for pointer input.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub at: Option<LogicalPoint>,
    /// When, in microseconds of the runtime's clock.
    pub time_us: u64,
    /// Whether the driver injected it, synthetic or through a lane.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub injected: bool,
}

/// `input.journal`: the entries after `since`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct JournalParams {
    /// The window.
    pub window: WindowId,
    /// Entries after this sequence number; every entry still held when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub since: Option<u64>,
    /// Entries to answer at most.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max_entries: Option<u32>,
}

/// What `input.journal` answers.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Journal {
    /// The entries, oldest first.
    pub entries: Vec<JournalEntry>,
    /// The span they cover, when there are any.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub span: Option<JournalSpan>,
    /// Entries after `since` the bounded journal no longer holds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub dropped: u64,
}

/// What `input.release` answers: the contacts and keys the agent had held and let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Released {
    /// Pointer contacts and buttons released or cancelled.
    pub pointers: u32,
    /// Keys released.
    pub keys: u32,
}
