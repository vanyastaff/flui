//! Synthetic input an agent injects (`input.inject`, `input.release`, and the gestures it answers
//! on the synthetic path; ADR-0288 §9, ADR-0400 §2): the action sequence planned once by
//! [`flui_protocol::input::plan`], lowered into the events the window takes, played at frame
//! boundaries through the entry native input takes ([`UiRuntime::handle_input_on`]), and what
//! each input lease of the window holds.
//!
//! The plan's bounds and refusals are the protocol's. Lowering refuses only what this window
//! cannot deliver: a key that is neither a W3C key name nor the text of one, a composition caret
//! outside its text, a trackpad gesture.
//!
//! An injection plays the plan's ticks in order, on the plan's timeline: a tick starts at a frame
//! boundary, each of its inputs goes out at the first boundary at or past its offset in the tick
//! (stamped with the instant the plan gives it), and the tick lasts until its end. The next tick
//! starts at the first boundary at or past that end, unless the ending tick started or delivered an
//! input at that boundary: each tick has a boundary of its own, a boundary delivers the inputs of
//! one tick, and so a release follows a move by a frame at least. The answer waits for the frame
//! after the last tick, so `ran` means the window has presented what the input did. A window runs
//! one injection at a time, in the order they came.
//!
//! What an injection leaves held (a contact down, a button or a key pressed) is its lease's
//! ([`flui_view::dev_agent::InputLease`]) and stays held across injections, as the platform's
//! input would. `input.release`, or the lease going, lets it go: contacts are cancelled, never
//! lifted (a lift could click), then keys come up, the latest first.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use flui_foundation::PresentationId;
use flui_foundation::geometry::Point;
use flui_foundation::panic::FirstPanic;
use flui_interaction::ids::InputSurface;
use flui_platform::input::keyboard::{
    Code, Key, KeyEvent, KeyPress, KeyState, Modifiers, NamedKey,
};
use flui_platform::input::pointer::{
    ButtonChange, CancelReason, DeviceId, PenTool, PointerButton, PointerButtons, PointerCancel,
    PointerEvent, PointerId, PointerKind, PointerMove, PointerPosition, PointerPress,
    PointerRelease, PointerRole, PointerSample, PointerSource, Pressure, ScrollDelta, ScrollEvent,
    ScrollUnit,
};
use flui_platform::{EventTime, ImeEvent, PlatformInput};
use flui_protocol::method::{self, Method};
use flui_protocol::{
    act::ActOutcome, act::Effect, act::EffectKind, act::JournalSpan, geometry::LogicalPoint,
    input::ActionSequence, input::ImeAction, input::InputPath, input::PlannedInput,
    input::PointerDevice, input::Released, input::STEP, input::SourceKind, input::TimedInput,
    snapshot::Snapshot, tree::ElementRef,
};
use flui_semantics::TreeSnapshot;
use flui_view::__runtime::LeaseId;
use web_time::Instant;

use super::UiRuntime;
use super::agent::{AgentError, ReadingOwed, ReplySender, send_reply};
use super::input::InputOrigin;
use crate::presentation::PresentationState;

/// The most injections and releases a window keeps queued; past it, a new one answers `busy`.
const MAX_QUEUED: usize = 64;

/// The first pointer id an injection hands out. Platforms number their pointers far below it, so
/// an injected contact never shares an id with one of the platform's.
const INJECTED_POINTERS: u64 = 1 << 62;

/// Input lowered for the window from one or more plans played in turn: their ticks, each with its
/// inputs at their offsets, and the pointer and wheel sources the inputs name.
pub(super) struct Script {
    /// The pointer and wheel sources, by the index inputs name them with.
    sources: Vec<Source>,
    ticks: VecDeque<Tick>,
}

/// A pointer or wheel source of a script.
struct Source {
    name: String,
    kind: PointerKind,
}

/// One tick: its inputs, and how long it lasts from its start.
struct Tick {
    /// In the plan's order, each at its offset from the tick's start.
    steps: Vec<Step>,
    lasts: Duration,
}

/// One input of a tick, `after` its start.
struct Step {
    after: Duration,
    input: Input,
}

/// What one input of a script does.
enum Input {
    Down {
        source: usize,
        button: PointerButton,
        pressure: Option<Pressure>,
    },
    Up {
        source: usize,
        button: PointerButton,
    },
    Move {
        source: usize,
        to: Point<f64>,
        pressure: Option<Pressure>,
    },
    Cancel {
        source: usize,
    },
    Scroll {
        source: usize,
        at: Point<f64>,
        delta: ScrollDelta,
    },
    Key {
        state: KeyState,
        key: Key,
    },
    Ime(ImeEvent),
}

fn invalid(reason: &'static str) -> AgentError {
    AgentError::InvalidArgument { reason }
}

/// `sequence` planned ([`flui_protocol::input::plan`]) and lowered into the input this window
/// takes.
///
/// # Errors
///
/// `invalid_argument` for a sequence the plan refuses (its bounds and ranges, named in the
/// message) or whose keys or composition the window cannot deliver as asked; `not_supported` for
/// a trackpad source, which this build does not inject.
pub(super) fn lower(sequence: &ActionSequence) -> Result<Script, AgentError> {
    let plan = flui_protocol::input::plan(sequence)
        .map_err(|error| AgentError::UnplayableInput(error.to_string()))?;
    let mut sources = Vec::new();
    // The script's source each planned source's inputs name, for a pointer or a wheel.
    let mut pointer_of: Vec<Option<usize>> = Vec::with_capacity(plan.sources().len());
    for planned in plan.sources() {
        let kind = match planned.kind {
            SourceKind::Pointer(device) => Some(pointer_kind(device)?),
            SourceKind::Wheel => Some(PointerKind::Mouse),
            SourceKind::Keys | SourceKind::Text | SourceKind::Ime => None,
            SourceKind::Trackpad => {
                return Err(AgentError::NotSupported {
                    capability: method::InputInject::NAME,
                    reason: "this app's windows do not take injected trackpad gestures yet",
                });
            }
            _ => return Err(unknown_action()),
        };
        pointer_of.push(kind.map(|kind| {
            sources.push(Source {
                name: planned
                    .id
                    .clone()
                    .expect("BUG: the plan names every pointer and wheel source"),
                kind,
            });
            sources.len() - 1
        }));
    }
    let mut ticks = VecDeque::with_capacity(plan.tick_ends().len());
    let mut inputs = plan.inputs().iter().peekable();
    let mut start = Duration::ZERO;
    for (tick, &end) in plan.tick_ends().iter().enumerate() {
        let mut steps = Vec::new();
        while let Some(timed) = inputs.next_if(|timed| timed.tick == tick) {
            steps.push(Step {
                after: timed.at.saturating_sub(start),
                input: lower_input(timed, &pointer_of)?,
            });
        }
        ticks.push_back(Tick {
            steps,
            lasts: end.saturating_sub(start),
        });
        start = end;
    }
    Ok(Script { sources, ticks })
}

/// One planned input as the input the window takes.
fn lower_input(timed: &TimedInput, pointer_of: &[Option<usize>]) -> Result<Input, AgentError> {
    let source = || {
        pointer_of
            .get(timed.source)
            .copied()
            .flatten()
            .expect("BUG: a pointer input names a pointer or wheel source of its plan")
    };
    Ok(match &timed.input {
        PlannedInput::PointerDown {
            button, pressure, ..
        } => Input::Down {
            source: source(),
            button: button_of(*button),
            pressure: pressure.map(pressure_of).transpose()?,
        },
        PlannedInput::PointerUp { button, .. } => Input::Up {
            source: source(),
            button: button_of(*button),
        },
        PlannedInput::PointerMove { to, pressure } => Input::Move {
            source: source(),
            to: Point::new(to.x, to.y),
            pressure: pressure.map(pressure_of).transpose()?,
        },
        PlannedInput::PointerCancel { .. } => Input::Cancel { source: source() },
        PlannedInput::Scroll {
            at,
            delta_x,
            delta_y,
            unit,
        } => {
            let unit = match unit {
                flui_protocol::input::ScrollUnit::Line => ScrollUnit::Lines,
                _ => ScrollUnit::Pixels,
            };
            Input::Scroll {
                source: source(),
                at: point(*at),
                delta: ScrollDelta::try_new(unit, *delta_x, *delta_y)
                    .map_err(|_| invalid("a scroll delta is finite"))?,
            }
        }
        PlannedInput::KeyDown(key) => Input::Key {
            state: KeyState::Down(KeyPress::First),
            key: key_named(key)?,
        },
        PlannedInput::KeyUp(key) => Input::Key {
            state: KeyState::Up,
            key: key_named(key)?,
        },
        PlannedInput::Text(text) => Input::Ime(ImeEvent::Commit(text.clone())),
        PlannedInput::Ime(action) => Input::Ime(ime_event(action)?),
        _ => return Err(unknown_action()),
    })
}

impl Script {
    /// How many ticks the script has left.
    fn len(&self) -> usize {
        self.ticks.len()
    }

    /// How long a window at 60 Hz takes to play the script: the time its ticks last, and a frame
    /// ([`STEP`]) for each, as [`ActionSequence::plays_within`] counts it. Saturates.
    fn plays_within(&self) -> Duration {
        let ticks = u32::try_from(self.ticks.len()).unwrap_or(u32::MAX);
        self.ticks
            .iter()
            .map(|tick| tick.lasts)
            .fold(Duration::ZERO, Duration::saturating_add)
            .saturating_add(STEP.saturating_mul(ticks))
    }

    /// This script, then `next`: `next`'s ticks after this one's, its sources as it names them.
    pub(super) fn then(mut self, next: Self) -> Self {
        let offset = self.sources.len();
        self.sources.extend(next.sources);
        self.ticks.extend(next.ticks.into_iter().map(|mut tick| {
            for step in &mut tick.steps {
                step.input.shift_source(offset);
            }
            tick
        }));
        self
    }
}

impl Input {
    /// Name the source `offset` places later.
    fn shift_source(&mut self, offset: usize) {
        match self {
            Self::Down { source, .. }
            | Self::Up { source, .. }
            | Self::Move { source, .. }
            | Self::Cancel { source }
            | Self::Scroll { source, .. } => *source += offset,
            Self::Key { .. } | Self::Ime(_) => {}
        }
    }
}

fn unknown_action() -> AgentError {
    AgentError::NotSupported {
        capability: method::InputInject::NAME,
        reason: "this app's windows do not know that kind of input",
    }
}

/// The pointer devices `input.inject` delivers, as the agent's hello lists them: each one
/// [`pointer_kind`] maps.
pub(crate) fn injected_pointers() -> Vec<PointerDevice> {
    PointerDevice::ALL
        .iter()
        .copied()
        .filter(|device| pointer_kind(*device).is_ok())
        .collect()
}

fn pointer_kind(device: PointerDevice) -> Result<PointerKind, AgentError> {
    Ok(match device {
        PointerDevice::Mouse => PointerKind::Mouse,
        PointerDevice::Touch => PointerKind::Touch,
        PointerDevice::Pen => PointerKind::Pen { tool: PenTool::Tip },
        PointerDevice::Virtual => PointerKind::Virtual,
        _ => return Err(unknown_action()),
    })
}

fn button_of(button: flui_protocol::input::PointerButton) -> PointerButton {
    match button {
        flui_protocol::input::PointerButton::Secondary => PointerButton::SECONDARY,
        flui_protocol::input::PointerButton::Middle => PointerButton::AUXILIARY,
        flui_protocol::input::PointerButton::Back => PointerButton::BACK,
        flui_protocol::input::PointerButton::Forward => PointerButton::FORWARD,
        _ => PointerButton::PRIMARY,
    }
}

/// A point the plan admitted: finite, and within its bound of the window.
fn point(at: LogicalPoint) -> Point<f64> {
    Point::new(at.x, at.y)
}

fn pressure_of(pressure: f64) -> Result<Pressure, AgentError> {
    let range = invalid("a pressure is between 0 and 1");
    if !(0.0..=1.0).contains(&pressure) {
        return Err(range);
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a pressure in 0..=1 is a f32's to carry"
    )]
    Pressure::try_new(pressure as f32).map_err(|_| range)
}

/// A key the protocol names: a W3C key name (`Enter`, `Shift`), or the text the key types
/// (`a`, `é`); `Dead` is a dead key whose accent the protocol does not say. A longer ASCII word
/// is a name, and an unknown one is refused rather than typed.
fn key_named(name: &str) -> Result<Key, AgentError> {
    if name == "Dead" {
        return Ok(Key::Dead(None));
    }
    if let Some(named) = NamedKey::from_w3c(name) {
        return Ok(Key::Named(named));
    }
    let word = name.len() > 1 && name.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if name.is_empty() || word {
        return Err(invalid(
            "a key is a W3C key name (`Enter`) or the text it types (`a`)",
        ));
    }
    Ok(Key::character(name))
}

fn ime_event(action: &ImeAction) -> Result<ImeEvent, AgentError> {
    match action {
        ImeAction::SetComposition {
            text,
            caret_hidden: true,
            cursor,
        } => {
            if cursor.is_some() {
                return Err(invalid("a hidden caret has no position"));
            }
            Ok(ImeEvent::Preedit {
                text: text.clone(),
                cursor: None,
            })
        }
        ImeAction::SetComposition { text, cursor, .. } => {
            let caret = match cursor {
                None => text.len(),
                Some(cursor) => {
                    let cursor = usize::try_from(*cursor).unwrap_or(usize::MAX);
                    let chars = text.chars().count();
                    if cursor > chars {
                        return Err(invalid("a composition's caret is within its text"));
                    }
                    text.char_indices()
                        .nth(cursor)
                        .map_or(text.len(), |(at, _)| at)
                }
            };
            Ok(ImeEvent::Preedit {
                text: text.clone(),
                cursor: Some((caret, caret)),
            })
        }
        ImeAction::Commit { text } => Ok(ImeEvent::Commit(text.clone())),
        ImeAction::Cancel => Ok(ImeEvent::Preedit {
            text: String::new(),
            cursor: None,
        }),
        _ => Err(unknown_action()),
    }
}

/// What a window's input leases hold, and the injections and releases it is running.
#[derive(Default)]
pub(crate) struct AgentInput {
    leases: BTreeMap<LeaseId, Held>,
    /// Run in order, the front one at each frame boundary.
    jobs: VecDeque<Job>,
    /// How many injected pointer ids the window has handed out.
    pointers: u64,
    /// How many injections the window has queued: the next one's id.
    injections: u64,
    /// How many frame boundaries the window's work has seen, the current one included.
    boundaries: u64,
}

/// What one lease holds: its sources' pointers, as the platform's own would be, and the keys it
/// pressed.
#[derive(Default)]
struct Held {
    pointers: Vec<Contact>,
    /// Pressed, the earliest first.
    keys: Vec<Key>,
}

/// One injected pointer.
struct Contact {
    source: String,
    kind: PointerKind,
    id: PointerId,
    role: PointerRole,
    /// Where it is; `None` until it first moves.
    at: Option<Point<f64>>,
    buttons: PointerButtons,
}

impl Contact {
    fn info(&self) -> PointerSource {
        PointerSource::new(self.id, self.kind)
            .with_device(DeviceId::AGENT)
            .with_role(self.role)
    }
}

/// The modifiers `keys` hold.
fn modifiers_of(keys: &[Key]) -> Modifiers {
    keys.iter()
        .fold(Modifiers::NONE, |modifiers, key| match key {
            Key::Named(NamedKey::Shift) => modifiers.union(Modifiers::SHIFT),
            Key::Named(NamedKey::Control) => modifiers.union(Modifiers::CONTROL),
            Key::Named(NamedKey::Alt) => modifiers.union(Modifiers::ALT),
            Key::Named(NamedKey::AltGraph) => modifiers.union(Modifiers::ALT_GRAPH),
            Key::Named(NamedKey::Meta) => modifiers.union(Modifiers::META),
            _ => modifiers,
        })
}

fn sample(
    time: EventTime,
    at: Point<f64>,
    pressure: Option<Pressure>,
) -> Result<PointerSample, AgentError> {
    let position = PointerPosition::try_new(at).map_err(|_| invalid("a point is finite"))?;
    let sample = PointerSample::new(time, position);
    Ok(match pressure {
        Some(pressure) => sample.with_pressure(pressure),
        None => sample,
    })
}

impl Held {
    /// The contact of `source`, made when the lease first uses it.
    fn contact(&mut self, pointers: &mut u64, source: &Source) -> Result<&mut Contact, AgentError> {
        if let Some(index) = self
            .pointers
            .iter()
            .position(|contact| contact.source == source.name)
        {
            let contact = &mut self.pointers[index];
            if contact.kind != source.kind {
                return Err(invalid(
                    "a source keeps the kind of pointer it first injected through its lease",
                ));
            }
            return Ok(contact);
        }
        let exhausted = || invalid("the window has handed out every injected pointer id");
        let next = pointers.checked_add(1).ok_or_else(exhausted)?;
        let id = INJECTED_POINTERS
            .checked_add(next)
            .and_then(|id| PointerId::try_from(id).ok())
            .ok_or_else(exhausted)?;
        *pointers = next;
        let role = if self
            .pointers
            .iter()
            .any(|contact| contact.kind == source.kind)
        {
            PointerRole::Additional
        } else {
            PointerRole::Primary
        };
        self.pointers.push(Contact {
            source: source.name.clone(),
            kind: source.kind,
            id,
            role,
            at: None,
            buttons: PointerButtons::NONE,
        });
        Ok(self
            .pointers
            .last_mut()
            .expect("BUG: a contact was just pushed"))
    }

    /// One input at `time`: the lease's state moves on only when it makes its event; `None` for
    /// one that changes nothing the window sees.
    fn step(
        &mut self,
        pointers: &mut u64,
        sources: &[Source],
        input: Input,
        time: EventTime,
    ) -> Result<Option<PlatformInput>, AgentError> {
        let modifiers = modifiers_of(&self.keys);
        let made = match input {
            Input::Down {
                source,
                button,
                pressure,
            } => {
                let contact = self.contact(pointers, &sources[source])?;
                let at = contact
                    .at
                    .ok_or_else(|| invalid("a pointer moves before it goes down"))?;
                if contact.buttons.contains(button) {
                    return Ok(None);
                }
                let press = PointerPress::new(
                    contact.info(),
                    button,
                    contact.buttons,
                    sample(time, at, pressure)?,
                )
                .with_modifiers(modifiers);
                let event = if contact.buttons == PointerButtons::NONE {
                    PointerEvent::Down(press)
                } else {
                    PointerEvent::ButtonChange(ButtonChange::Pressed(press))
                };
                contact.buttons = contact.buttons.with(button);
                PlatformInput::Pointer(event)
            }
            Input::Up { source, button } => {
                let contact = self.contact(pointers, &sources[source])?;
                let (Some(at), true) = (contact.at, contact.buttons.contains(button)) else {
                    // Lifting a button that is up sends nothing.
                    return Ok(None);
                };
                let release = PointerRelease::new(
                    contact.info(),
                    button,
                    contact.buttons,
                    sample(time, at, None)?,
                )
                .with_modifiers(modifiers);
                contact.buttons = contact.buttons.without(button);
                let event = if contact.buttons == PointerButtons::NONE {
                    PointerEvent::Up(release)
                } else {
                    PointerEvent::ButtonChange(ButtonChange::Released(release))
                };
                PlatformInput::Pointer(event)
            }
            Input::Move {
                source,
                to,
                pressure,
            } => {
                let contact = self.contact(pointers, &sources[source])?;
                if contact.at == Some(to) {
                    return Ok(None);
                }
                let sample = sample(time, to, pressure)?;
                contact.at = Some(to);
                // A touch contact that is up has no position the window sees.
                if contact.kind == PointerKind::Touch && contact.buttons == PointerButtons::NONE {
                    return Ok(None);
                }
                PlatformInput::Pointer(PointerEvent::Move(
                    PointerMove::new(contact.info(), contact.buttons, sample)
                        .with_modifiers(modifiers),
                ))
            }
            Input::Cancel { source } => {
                let contact = self.contact(pointers, &sources[source])?;
                if contact.buttons == PointerButtons::NONE {
                    return Ok(None);
                }
                contact.buttons = PointerButtons::NONE;
                PlatformInput::Pointer(PointerEvent::Cancel(PointerCancel::new(
                    contact.info(),
                    time,
                    CancelReason::Platform,
                )))
            }
            Input::Scroll { source, at, delta } => {
                let contact = self.contact(pointers, &sources[source])?;
                let position =
                    PointerPosition::try_new(at).map_err(|_| invalid("a point is finite"))?;
                contact.at = Some(at);
                PlatformInput::Pointer(PointerEvent::Scroll(
                    ScrollEvent::new(contact.info(), time, position, delta)
                        .with_modifiers(modifiers),
                ))
            }
            Input::Key { state, key } => {
                let pressed = self.keys.iter().position(|held| *held == key);
                let state = match (state, pressed) {
                    (KeyState::Down(_), None) => {
                        self.keys.push(key.clone());
                        KeyState::Down(KeyPress::First)
                    }
                    // Pressing a pressed key repeats it, as WebDriver dispatches a second
                    // `keyDown` and as a platform repeats a held key.
                    (KeyState::Down(_), Some(_)) => KeyState::Down(KeyPress::AutoRepeat),
                    (KeyState::Up, Some(index)) => {
                        self.keys.remove(index);
                        KeyState::Up
                    }
                    // Lifting a key that is up sends nothing.
                    (KeyState::Up, None) => return Ok(None),
                };
                // A modifier's own event carries the modifiers as they are after it.
                PlatformInput::Keyboard(
                    KeyEvent::new(state, key, Code::Unidentified, time)
                        .with_modifiers(modifiers_of(&self.keys)),
                )
            }
            Input::Ime(event) => PlatformInput::Ime(event),
        };
        Ok(Some(made))
    }

    /// Let go of everything held, stamped `time`: each contact cancelled, the latest first, then
    /// each key up, the latest first. The input, and how many contacts and keys it let go.
    fn let_go(self, time: EventTime) -> (Vec<PlatformInput>, Released) {
        let mut batch = Vec::new();
        let mut released = Released::default();
        for contact in self.pointers.iter().rev() {
            if contact.buttons != PointerButtons::NONE {
                batch.push(PlatformInput::Pointer(PointerEvent::Cancel(
                    PointerCancel::new(contact.info(), time, CancelReason::Platform),
                )));
                released.pointers = released.pointers.saturating_add(1);
            }
        }
        let mut keys = self.keys;
        while let Some(key) = keys.pop() {
            batch.push(PlatformInput::Keyboard(
                KeyEvent::new(KeyState::Up, key, Code::Unidentified, time)
                    .with_modifiers(modifiers_of(&keys)),
            ));
            released.keys = released.keys.saturating_add(1);
        }
        (batch, released)
    }
}

enum Job {
    Inject(Box<Injection>),
    Release {
        lease: LeaseId,
        reply: ReplySender<Released>,
    },
}

impl Job {
    fn lease(&self) -> LeaseId {
        match self {
            Self::Inject(injection) => injection.lease,
            Self::Release { lease, .. } => *lease,
        }
    }
}

/// One injection the window is playing: an `input.inject`, or a gesture's input.
struct Injection {
    /// Which of the window's injections it is.
    id: u64,
    lease: LeaseId,
    script: Script,
    /// The tick under way, while its inputs and its time last.
    current: Option<Running>,
    /// How many ticks it had.
    total: usize,
    /// The element a gesture aimed at, which its answer names.
    element: Option<ElementRef>,
    snapshot: Snapshot,
    before: Option<TreeSnapshot>,
    reply: ReplySender<ActOutcome>,
    /// The window's journal entries its ticks so far made.
    observed: Option<JournalSpan>,
    /// The deadline of the act that queued it, and how long its script takes to play
    /// ([`Script::plays_within`]); `None` for `input.inject`. See [`Injection::expiry`].
    deadline: Option<(Instant, Duration)>,
}

impl Injection {
    /// Whether one of its ticks has started.
    fn started(&self) -> bool {
        self.current.is_some() || self.script.len() != self.total
    }

    /// The ticks that went out in full.
    fn played(&self) -> usize {
        self.total
            .saturating_sub(self.script.len())
            .saturating_sub(usize::from(self.current.is_some()))
    }

    /// The answer an act's input gets at `now` if its act's promise has run out (ADR-0288 §4):
    /// none of its ticks started by the act's deadline, or, under way by then, it has not ended
    /// within its play time past it. `None` while it may still play, and always for input whose
    /// asker bounds its own reply (`input.inject`).
    fn expiry(&self, now: Instant) -> Option<AgentError> {
        let (deadline, plays) = self.deadline?;
        if !self.started() {
            return (now >= deadline).then_some(AgentError::InputTimedOut);
        }
        let ends_by = deadline.checked_add(plays).unwrap_or(deadline);
        if now < ends_by {
            return None;
        }
        let played = self.played();
        Some(if played == 0 {
            AgentError::InputTimedOut
        } else {
            AgentError::InputPartial {
                failure: Box::new(AgentError::InputTimedOut),
                played: u32::try_from(played).unwrap_or(u32::MAX),
                total: u32::try_from(self.total).unwrap_or(u32::MAX),
            }
        })
    }

    /// The instant [`Self::expiry`] next answers it at.
    fn expires_at(&self) -> Option<Instant> {
        let (deadline, plays) = self.deadline?;
        Some(if self.started() {
            deadline.checked_add(plays).unwrap_or(deadline)
        } else {
            deadline
        })
    }
}

/// A tick whose inputs or time are still running.
struct Running {
    started: Instant,
    /// The frame boundary it started at.
    boundary: u64,
    lasts: Duration,
    /// Its inputs not yet out.
    steps: VecDeque<Step>,
}

/// Which of a window's injections one is, as [`UiRuntime::queue_injection`] queued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct InjectionId(u64);

/// What an injection of a script is asked with.
pub(super) struct Injecting {
    /// The lease it goes through.
    pub(super) lease: LeaseId,
    /// Its input.
    pub(super) script: Script,
    /// The element a gesture aimed at, which the answer names.
    pub(super) element: Option<ElementRef>,
    /// The reading the answer carries.
    pub(super) snapshot: Snapshot,
    /// The tree the input starts on, for a delta.
    pub(super) before: Option<TreeSnapshot>,
    /// The deadline of the act that queues it, on the presentation's clock; `None` for
    /// `input.inject`, whose asker bounds its own reply. See [`UiRuntime::expire_act_input`].
    pub(super) deadline: Option<Instant>,
}

impl AgentInput {
    /// Queue `injecting`.
    ///
    /// # Errors
    ///
    /// `busy`, with the reply back, when the window already has [`MAX_QUEUED`] jobs.
    fn inject(
        &mut self,
        injecting: Injecting,
        reply: ReplySender<ActOutcome>,
    ) -> Result<InjectionId, ReplySender<ActOutcome>> {
        if self.jobs.len() >= MAX_QUEUED {
            return Err(reply);
        }
        let Injecting {
            lease,
            script,
            element,
            snapshot,
            before,
            deadline,
        } = injecting;
        let total = script.len();
        let deadline = deadline.map(|deadline| (deadline, script.plays_within()));
        self.injections = self
            .injections
            .checked_add(1)
            .expect("BUG: a window queues fewer than 2^64 injections");
        self.jobs.push_back(Job::Inject(Box::new(Injection {
            id: self.injections,
            lease,
            script,
            current: None,
            total,
            element,
            snapshot,
            before,
            reply,
            observed: None,
            deadline,
        })));
        Ok(InjectionId(self.injections))
    }

    /// Take off the queue every act's input whose promise ran out by `now`
    /// ([`Injection::expiry`]), each with the answer it gets. What one under way left held stays
    /// with its lease, as a failed injection's does; nothing of any of them goes out after this.
    fn expire(&mut self, now: Instant) -> Vec<(ReplySender<ActOutcome>, AgentError)> {
        let mut expired = Vec::new();
        let mut at = 0;
        while at < self.jobs.len() {
            let answer = match &self.jobs[at] {
                Job::Inject(injection) => injection.expiry(now),
                Job::Release { .. } => None,
            };
            let Some(answer) = answer else {
                at += 1;
                continue;
            };
            let Some(Job::Inject(injection)) = self.jobs.remove(at) else {
                unreachable!("BUG: the job just read is an injection");
            };
            expired.push((injection.reply, answer));
        }
        expired
    }

    /// The earliest instant an act's queued input is answered at if it has not played by then.
    fn next_deadline(&self) -> Option<Instant> {
        self.jobs
            .iter()
            .filter_map(|job| match job {
                Job::Inject(injection) => injection.expires_at(),
                Job::Release { .. } => None,
            })
            .min()
    }

    /// Take injection `id` off the queue if none of its ticks has started, to be dropped unsent
    /// once the queue is let go: nothing of it goes out. An injection already under way, or
    /// gone, stays as it is.
    fn withdraw(&mut self, id: InjectionId) -> Option<Job> {
        let at = self.jobs.iter().position(|job| match job {
            Job::Inject(injection) => {
                injection.id == id.0
                    && injection.current.is_none()
                    && injection.script.len() == injection.total
            }
            Job::Release { .. } => false,
        })?;
        self.jobs.remove(at)
    }

    /// Queue `input.release` through `lease`, behind the work already queued.
    ///
    /// # Errors
    ///
    /// The reply back when the window already has [`MAX_QUEUED`] jobs.
    pub(super) fn release(
        &mut self,
        lease: LeaseId,
        reply: ReplySender<Released>,
    ) -> Result<(), ReplySender<Released>> {
        if self.jobs.len() >= MAX_QUEUED {
            return Err(reply);
        }
        self.jobs.push_back(Job::Release { lease, reply });
        Ok(())
    }

    /// The window closed: every queued job, to be answered that it is gone.
    pub(crate) fn abandon(&mut self) -> Vec<Abandoned> {
        self.leases.clear();
        self.jobs.drain(..).map(Abandoned::from).collect()
    }

    /// The journal entries in `span` came of injection `id`'s tick, if it is still under way.
    fn observe(&mut self, id: u64, span: Option<JournalSpan>) {
        if let Some(Job::Inject(injection)) = self.jobs.front_mut()
            && injection.id == id
        {
            injection.observed = widen(injection.observed, span);
        }
    }

    /// A frame boundary begins: the number it has among the boundaries the window's work saw.
    fn begin_boundary(&mut self) -> u64 {
        self.boundaries = self.boundaries.saturating_add(1);
        self.boundaries
    }

    /// The next step of the window's work at frame boundary `boundary`, at `now`; an input is
    /// stamped with the instant `stamp` gives the moment the plan puts it at.
    fn turn(&mut self, boundary: u64, now: Instant, stamp: &dyn Fn(Instant) -> EventTime) -> Turn {
        let Self {
            leases,
            jobs,
            pointers,
            ..
        } = self;
        let Some(front) = jobs.front_mut() else {
            return Turn::Idle;
        };
        let injection = match front {
            Job::Inject(injection) => injection,
            Job::Release { lease, .. } => {
                let lease = *lease;
                let Some(Job::Release { reply, .. }) = jobs.pop_front() else {
                    unreachable!("BUG: the front job is a release");
                };
                let (batch, released) = leases
                    .remove(&lease)
                    .map_or_else(Default::default, |held| held.let_go(stamp(now)));
                return Turn::Released(batch, released, reply);
            }
        };
        let held = leases.entry(injection.lease).or_default();
        let mut batch = Vec::new();
        // Whether this boundary delivered an input of the running tick: a boundary delivers the
        // inputs of one tick at most, so the next tick waits for the next boundary.
        let mut delivered = false;
        loop {
            if let Some(running) = &mut injection.current {
                let elapsed = now.saturating_duration_since(running.started);
                while let Some(step) = running.steps.pop_front_if(|step| step.after <= elapsed) {
                    delivered = true;
                    let time = stamp(
                        running
                            .started
                            .checked_add(step.after)
                            .unwrap_or(running.started),
                    );
                    match held.step(pointers, &injection.script.sources, step.input, time) {
                        Ok(Some(input)) => batch.push(input),
                        Ok(None) => {}
                        Err(error) => return Self::failed(jobs, batch, error),
                    }
                }
                if !running.steps.is_empty() || elapsed < running.lasts {
                    return Turn::Sent(batch, injection.id);
                }
                let started_here = running.boundary == boundary;
                injection.current = None;
                if started_here || delivered {
                    // Each tick has a boundary of its own, and a boundary delivers the inputs
                    // of one tick: the next starts at the next one.
                    break;
                }
            }
            let Some(Tick { steps, lasts }) = injection.script.ticks.pop_front() else {
                break;
            };
            injection.current = Some(Running {
                started: now,
                boundary,
                lasts,
                steps: VecDeque::from(steps),
            });
        }
        if !injection.script.ticks.is_empty() {
            return Turn::Sent(batch, injection.id);
        }
        let Some(Job::Inject(injection)) = jobs.pop_front() else {
            unreachable!("BUG: the front job is an injection");
        };
        Turn::Played(batch, injection)
    }

    /// The front injection stopped at `error` in the tick under way, after making `batch`: the
    /// ticks before it went out in full, and it is neither in the script nor running any more.
    fn failed(jobs: &mut VecDeque<Job>, batch: Vec<PlatformInput>, error: AgentError) -> Turn {
        let Some(Job::Inject(injection)) = jobs.pop_front() else {
            unreachable!("BUG: the front job is an injection");
        };
        let played = injection.played();
        let total = injection.total;
        Turn::Failed(batch, error, played, total, injection.reply)
    }

    /// The lease went: what it holds is let go, stamped `time`, and its queued work, a
    /// half-played injection included, is taken off the queue to be answered that the lease was
    /// released.
    fn forget(&mut self, lease: LeaseId, time: EventTime) -> (Vec<PlatformInput>, Vec<Abandoned>) {
        let (dropped, kept): (VecDeque<Job>, VecDeque<Job>) = std::mem::take(&mut self.jobs)
            .into_iter()
            .partition(|job| job.lease() == lease);
        self.jobs = kept;
        let batch = self
            .leases
            .remove(&lease)
            .map_or_default(|held| held.let_go(time).0);
        (batch, dropped.into_iter().map(Abandoned::from).collect())
    }
}

/// A queued job that will not run, to be answered why.
pub(crate) enum Abandoned {
    Inject(ReplySender<ActOutcome>),
    Release(ReplySender<Released>),
}

impl From<Job> for Abandoned {
    fn from(job: Job) -> Self {
        match job {
            Job::Inject(injection) => Self::Inject(injection.reply),
            Job::Release { reply, .. } => Self::Release(reply),
        }
    }
}

impl Abandoned {
    /// Answer that the window has gone.
    pub(crate) fn answer(self) {
        self.refuse(|| AgentError::PresentationGone);
    }

    /// Answer that the lease it was asked through was released first.
    fn lease_released(self) {
        self.refuse(|| AgentError::LeaseReleased);
    }

    fn refuse(self, error: impl FnOnce() -> AgentError) {
        match self {
            Self::Inject(reply) => send_reply(&reply, None, Err(error())),
            Self::Release(reply) => send_reply(&reply, None, Err(error())),
        }
    }
}

/// What one frame boundary's turn of a window's work came to.
enum Turn {
    /// Nothing is queued.
    Idle,
    /// Input injection `id` sends; the window waits for its next frame boundary.
    Sent(Vec<PlatformInput>, u64),
    /// A release let its lease's input go: send it, answer, and take the next job.
    Released(Vec<PlatformInput>, Released, ReplySender<Released>),
    /// The injection played its last tick: send its input, and answer once a frame has passed.
    Played(Vec<PlatformInput>, Box<Injection>),
    /// The injection cannot go on: send what it made, answer the failure with how many of how
    /// many ticks went out, and take the next job.
    Failed(
        Vec<PlatformInput>,
        AgentError,
        usize,
        usize,
        ReplySender<ActOutcome>,
    ),
}

impl UiRuntime {
    /// Queue `injecting` in `state`'s window, after asking for the frame whose boundary starts
    /// it: the injection, or `None` when the queue was full and `reply` was answered `busy`.
    ///
    /// The frame is asked for first. Its wake reaches the platform, user code, and one that
    /// panics has queued nothing, so the containment that answers the panic answers what
    /// happened; the wake reads none of the agent's state, and the redraw marks it sets are
    /// durable whichever way it returns.
    pub(super) fn queue_injection(
        &self,
        state: &PresentationState,
        injecting: Injecting,
        reply: ReplySender<ActOutcome>,
    ) -> Option<InjectionId> {
        self.request_redraw_for(state);
        let queued = state.agent_input.borrow_mut().inject(injecting, reply);
        match queued {
            Ok(id) => Some(id),
            Err(reply) => {
                send_reply(&reply, None, Err(AgentError::InputQueueFull));
                None
            }
        }
    }

    /// Queue `input.release` through `lease` in `state`'s window, after asking for the frame
    /// that plays it, as [`Self::queue_injection`] does.
    pub(super) fn queue_release(
        &self,
        state: &PresentationState,
        lease: LeaseId,
        reply: ReplySender<Released>,
    ) {
        self.request_redraw_for(state);
        let queued = state.agent_input.borrow_mut().release(lease, reply);
        if let Err(reply) = queued {
            send_reply(&reply, None, Err(AgentError::InputQueueFull));
        }
    }

    /// Advance every window's injections at a frame boundary at `now`: a running tick's inputs
    /// due by now go out, a tick that has lasted its time ends, and the next one starts, at most
    /// one per window per boundary. A panic in the app's handler of injected input continues once
    /// every window has had its turn; the first is kept.
    pub(super) fn advance_agent_input(&self, now: Instant) {
        let mut first = FirstPanic::new();
        for state in self.presentations.iter() {
            first.contain(|| self.advance_window_input(state, now));
        }
        // After the boundary's input went on: what could start at it has.
        first.contain(|| self.expire_act_input());
        first.resume();
    }

    /// Answer every act's queued input whose act's promise has run out, on each presentation's
    /// clock (ADR-0400 §2): none of its ticks started by the act's deadline (`timeout`, nothing
    /// played), or under way then and not ended within its play time past it (`timeout`, with
    /// the ticks played). Each answer runs contained, after the queue is let go; the first panic
    /// continues once every window's are sent.
    ///
    /// Run at a frame boundary after its input went on, and at an owner turn without a frame
    /// after the acts whose deadline passed were attempted, so the input a last attempt queued on
    /// a frame starts on that frame, and a window that draws no frame answers its acts at their
    /// deadline whatever they queued ([`Self::next_act_input_deadline`] wakes the owner then).
    pub(super) fn expire_act_input(&self) {
        let mut first = FirstPanic::new();
        for state in self.presentations.iter() {
            let now = state.clock().now();
            let expired = state.agent_input.borrow_mut().expire(now);
            for (reply, answer) in expired {
                first.contain(|| send_reply(&reply, None, Err(answer)));
            }
        }
        first.resume();
    }

    /// The earliest instant [`Self::expire_act_input`] answers an act's queued input at, in
    /// `state`'s window, on its clock.
    pub(super) fn next_act_input_deadline(state: &PresentationState) -> Option<Instant> {
        state.agent_input.borrow().next_deadline()
    }

    fn advance_window_input(&self, state: &PresentationState, now: Instant) {
        let stamp = |instant: Instant| self.event_time_at(instant);
        let boundary = state.agent_input.borrow_mut().begin_boundary();
        loop {
            // Taken with the window's input borrowed; no app code runs until it is let go.
            let turn = state.agent_input.borrow_mut().turn(boundary, now, &stamp);
            match turn {
                Turn::Idle => return,
                Turn::Sent(batch, id) => {
                    // The tick's input left the queue with this turn: it goes out before the
                    // wake that asks for the next boundary, and a wake that panics loses none.
                    let mut sent = self.send_agent_input(state, batch);
                    state.agent_input.borrow_mut().observe(id, sent.observed);
                    sent.panic.contain(|| self.request_redraw_for(state));
                    sent.resume();
                    return;
                }
                Turn::Released(batch, released, reply) => {
                    // Answered after its input went out, a panic in a handler included: the
                    // input was let go.
                    let mut sent = self.send_agent_input(state, batch);
                    sent.panic
                        .contain(|| send_reply(&reply, None, Ok(released)));
                    sent.resume();
                }
                Turn::Played(batch, injection) => {
                    let Injection {
                        element,
                        snapshot,
                        before,
                        reply,
                        observed,
                        ..
                    } = *injection;
                    let mut sent = self.send_agent_input(state, batch);
                    let mut outcome =
                        ActOutcome::new(InputPath::Synthetic, Effect::new(EffectKind::Ran));
                    outcome.element = element;
                    outcome.observed = widen(observed, sent.observed);
                    // Owed once the input has gone out, a handler's panic included: the answer
                    // waits for the frame after it, which shows what the input did. The window
                    // is still open, since closing one takes the runtime mutably. Parking wakes
                    // the platform, so it runs behind a handler's panic, which stays first.
                    let owed = ReadingOwed::new(outcome, snapshot, before);
                    self.park_act_read(state, owed, reply, &mut sent.panic);
                    sent.resume();
                    return;
                }
                Turn::Failed(batch, error, played, total, reply) => {
                    let mut sent = self.send_agent_input(state, batch);
                    let error = if played == 0 {
                        error
                    } else {
                        AgentError::InputPartial {
                            failure: Box::new(error),
                            played: u32::try_from(played).unwrap_or(u32::MAX),
                            total: u32::try_from(total).unwrap_or(u32::MAX),
                        }
                    };
                    sent.panic.contain(|| send_reply(&reply, None, Err(error)));
                    sent.resume();
                }
            }
        }
    }

    /// Send injected `batch` to `state`'s window, each input contained: a handler that panics
    /// does not keep the rest from going out. What went out made the journal entries it says, and
    /// its first panic continues once the caller has answered for it.
    #[must_use = "the first panic is resumed with `SentInput::resume`"]
    fn send_agent_input(&self, state: &PresentationState, batch: Vec<PlatformInput>) -> SentInput {
        let before = super::agent_journal::last_seq(state);
        let mut first = FirstPanic::new();
        for input in batch {
            first.contain(|| {
                self.handle_input_from(state.id(), InputSurface::Window, input, InputOrigin::Agent);
            });
        }
        let after = super::agent_journal::last_seq(state);
        SentInput {
            observed: (after > before).then(|| JournalSpan {
                from: before.saturating_add(1),
                to: after,
            }),
            panic: first,
        }
    }

    /// How many leases each window's handles have let go so far, counted before a drain reads its
    /// commands: every call asked through one of them was enqueued before it was let go, so the
    /// drain takes the call first and releases the lease after ([`Self::release_counted_leases`]).
    pub(super) fn count_released_leases(&self) -> Vec<(PresentationId, usize)> {
        self.presentations
            .iter()
            .filter_map(|state| {
                let count = state.dev_agent.borrow().as_ref()?.released_count();
                (count > 0).then(|| (state.id(), count))
            })
            .collect()
    }

    /// Release the leases [`Self::count_released_leases`] counted, every window's even if one's
    /// handler panics; the first panic is returned. A lease counted but not yet taken stays with
    /// its port for the next drain.
    pub(super) fn release_counted_leases(
        &self,
        counted: Vec<(PresentationId, usize)>,
    ) -> FirstPanic {
        let mut first = FirstPanic::new();
        for (presentation, count) in counted {
            let Some(state) = self.presentations.get(presentation) else {
                // It closed, and released everything as it did.
                continue;
            };
            let leases = state
                .dev_agent
                .borrow()
                .as_ref()
                .map_or_default(|slot| slot.take_released(count));
            first.contain(|| self.release_agent_leases(state, leases));
        }
        first
    }

    /// Release what each of `leases` holds in `state`'s window, and drop the work queued and the
    /// acts held through them: their holders have gone.
    fn release_agent_leases(&self, state: &PresentationState, leases: Vec<LeaseId>) {
        let time = self.event_time_at(flui_foundation::MonotonicClock::now(&self.clock));
        let mut batch = Vec::new();
        let mut dropped = Vec::new();
        {
            let mut input = state.agent_input.borrow_mut();
            for lease in &leases {
                let (released, jobs) = input.forget(*lease, time);
                batch.extend(released);
                dropped.extend(jobs);
            }
        }
        let held = state.agent_acts.borrow_mut().forget(&leases);
        let mut sent = self.send_agent_input(state, batch);
        // An answer wakes its asker, user code: they go after the input is let go, and before a
        // panic continues.
        sent.panic
            .absorb(super::deliver_each(dropped, Abandoned::lease_released));
        sent.panic.absorb(super::deliver_each(
            held,
            super::agent_held::HeldAct::lease_released,
        ));
        sent.resume();
    }
}

/// Take injection `id` off `state`'s queue if none of its ticks has started: whether it was. Its
/// answer is dropped unsent, after the queue is let go.
pub(super) fn withdraw(state: &PresentationState, id: InjectionId) -> bool {
    let withdrawn = state.agent_input.borrow_mut().withdraw(id);
    let was = withdrawn.is_some();
    drop(withdrawn);
    was
}

/// What sending one batch of injected input came to.
struct SentInput {
    /// The window's journal entries it made.
    observed: Option<JournalSpan>,
    /// The first panic of a handler it reached, and of what its caller ran after it.
    panic: FirstPanic,
}

impl SentInput {
    /// Continue the first panic, if a handler panicked.
    fn resume(self) {
        self.panic.resume();
    }
}

/// The span covering both `a` and `b`.
pub(super) fn widen(a: Option<JournalSpan>, b: Option<JournalSpan>) -> Option<JournalSpan> {
    match (a, b) {
        (Some(a), Some(b)) => Some(JournalSpan {
            from: a.from.min(b.from),
            to: a.to.max(b.to),
        }),
        (a, b) => a.or(b),
    }
}
