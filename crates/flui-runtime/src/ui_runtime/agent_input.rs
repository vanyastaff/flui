//! Synthetic input an agent injects (`input.inject`, `input.release`; ADR-0288 §9): the action
//! sequence planned as ticks, each tick dispatched at a frame boundary through the entry native
//! input takes ([`UiRuntime::handle_input_on`]), and what each input lease of the window holds.
//!
//! An injection's ticks run in order, one per frame boundary: a tick's presses, releases, keys
//! and text go out at once, and its moves are spread over the frames of their duration. The
//! answer waits for the frame after the last tick, so `ran` means the window has presented what
//! the input did. A window runs one injection at a time, in the order they came.
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
use flui_interaction::InputSurface;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey};
use flui_platform::pointer::{
    ButtonChange, CancelReason, DeviceId, PenTool, PointerButton, PointerButtons, PointerCancel,
    PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition, PointerPress,
    PointerRelease, PointerRole, PointerSample, Pressure, ScrollDelta, ScrollEvent, ScrollUnit,
};
use flui_platform::{EventTime, ImeEvent, PlatformInput};
use flui_protocol::method::{self, Method};
use flui_protocol::{
    ActOutcome, ActionSequence, Effect, EffectKind, ImeAction, InputPath, InputSource, JournalSpan,
    KeyAction, LogicalPoint, PointerAction, PointerDevice, Released, Snapshot, WheelAction,
};
use flui_semantics::TreeSnapshot;
use flui_view::__runtime::LeaseId;
use web_time::Instant;

use super::UiRuntime;
use super::agent::{AgentError, ReadingOwed, ReplySender, send_reply};
use super::input::InputOrigin;
use crate::presentation::PresentationState;

/// The most ticks one injection plans.
const MAX_TICKS: usize = 4096;

/// The most sources one injection names.
const MAX_SOURCES: usize = 32;

/// The longest one injection lasts: a longer sequence is refused rather than holding the
/// window's input for it.
const MAX_SPAN: Duration = Duration::from_secs(600);

/// The most injections and releases a window keeps queued; past it, a new one answers `busy`.
const MAX_QUEUED: usize = 64;

/// The first pointer id an injection hands out. Platforms number their pointers far below it, so
/// an injected contact never shares an id with one of the platform's.
const INJECTED_POINTERS: u64 = 1 << 62;

/// An action sequence as ticks.
pub(super) struct Plan {
    /// The pointer and wheel sources, by the index steps name them with.
    sources: Vec<Source>,
    ticks: VecDeque<Tick>,
}

/// A pointer or wheel source of a plan.
struct Source {
    name: String,
    kind: PointerKind,
}

/// One tick: what goes out at its frame boundary, and how long it lasts.
struct Tick {
    steps: Vec<Step>,
    lasts: Duration,
}

/// One input of a tick.
enum Step {
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
        over: Duration,
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

/// `sequence` as ticks: tick `i` holds the `i`th action of every source, and lasts as long as
/// its longest pause or move.
///
/// # Errors
///
/// `invalid_argument` for a sequence the window cannot play as asked: two sources of one name,
/// a key that is neither a W3C key name nor the text of one, a point or pressure out of range,
/// more than [`MAX_TICKS`] ticks or [`MAX_SOURCES`] sources, or longer than [`MAX_SPAN`];
/// `not_supported` for a trackpad source, which this build does not inject.
pub(super) fn plan(sequence: &ActionSequence) -> Result<Plan, AgentError> {
    if sequence.sources.len() > MAX_SOURCES {
        return Err(invalid("an injection names at most 32 sources"));
    }
    let count = sequence.ticks();
    if count > MAX_TICKS {
        return Err(invalid("an injection plays at most 4096 ticks"));
    }
    if sequence.span() > MAX_SPAN {
        return Err(invalid("an injection lasts at most ten minutes"));
    }
    let mut ticks: Vec<Tick> = (0..count)
        .map(|_| Tick {
            steps: Vec::new(),
            lasts: Duration::ZERO,
        })
        .collect();
    let mut sources: Vec<Source> = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for source in &sequence.sources {
        if let Some(name) = source_name(source) {
            if names.contains(&name) {
                return Err(invalid("two sources of an injection share a name"));
            }
            names.push(name);
        }
        match source {
            InputSource::Pointer {
                id,
                device,
                actions,
            } => {
                let index = sources.len();
                sources.push(Source {
                    name: id.clone(),
                    kind: pointer_kind(*device),
                });
                for (tick, action) in ticks.iter_mut().zip(actions) {
                    plan_pointer(tick, index, action)?;
                }
            }
            InputSource::Wheel { id, actions } => {
                let index = sources.len();
                sources.push(Source {
                    name: id.clone(),
                    kind: PointerKind::Mouse,
                });
                for (tick, action) in ticks.iter_mut().zip(actions) {
                    match action {
                        WheelAction::Scroll {
                            at,
                            delta_x,
                            delta_y,
                            unit,
                        } => {
                            let unit = match unit {
                                flui_protocol::ScrollUnit::Line => ScrollUnit::Lines,
                                _ => ScrollUnit::Pixels,
                            };
                            let delta = ScrollDelta::try_new(unit, *delta_x, *delta_y)
                                .map_err(|_| invalid("a scroll delta is finite"))?;
                            tick.steps.push(Step::Scroll {
                                source: index,
                                at: point(*at)?,
                                delta,
                            });
                        }
                        WheelAction::Pause { duration_ms } => {
                            last_at_least(tick, Duration::from_millis(*duration_ms));
                        }
                        _ => return Err(unknown_action()),
                    }
                }
            }
            InputSource::Keys { actions, .. } => {
                for (tick, action) in ticks.iter_mut().zip(actions) {
                    match action {
                        KeyAction::Down { key } => tick.steps.push(Step::Key {
                            state: KeyState::Down,
                            key: key_named(key)?,
                        }),
                        KeyAction::Up { key } => tick.steps.push(Step::Key {
                            state: KeyState::Up,
                            key: key_named(key)?,
                        }),
                        KeyAction::Pause { duration_ms } => {
                            last_at_least(tick, Duration::from_millis(*duration_ms));
                        }
                        _ => return Err(unknown_action()),
                    }
                }
            }
            InputSource::Text { text } => {
                if let Some(tick) = ticks.first_mut() {
                    tick.steps.push(Step::Ime(ImeEvent::Commit(text.clone())));
                }
            }
            InputSource::Ime { actions } => {
                for (tick, action) in ticks.iter_mut().zip(actions) {
                    tick.steps.push(Step::Ime(ime_event(action)?));
                }
            }
            InputSource::Trackpad { .. } => {
                return Err(AgentError::NotSupported {
                    capability: method::InputInject::NAME,
                    reason: "this app's windows do not take injected trackpad gestures yet",
                });
            }
            _ => return Err(unknown_action()),
        }
    }
    Ok(Plan {
        sources,
        ticks: ticks.into(),
    })
}

impl Plan {
    /// How many ticks the plan has left.
    fn len(&self) -> usize {
        self.ticks.len()
    }
}

fn unknown_action() -> AgentError {
    AgentError::NotSupported {
        capability: method::InputInject::NAME,
        reason: "this app's windows do not know that kind of input",
    }
}

fn source_name(source: &InputSource) -> Option<&str> {
    match source {
        InputSource::Pointer { id, .. }
        | InputSource::Wheel { id, .. }
        | InputSource::Keys { id, .. }
        | InputSource::Trackpad { id, .. } => Some(id),
        _ => None,
    }
}

fn pointer_kind(device: PointerDevice) -> PointerKind {
    match device {
        PointerDevice::Touch => PointerKind::Touch,
        PointerDevice::Pen => PointerKind::Pen { tool: PenTool::Tip },
        _ => PointerKind::Mouse,
    }
}

fn last_at_least(tick: &mut Tick, duration: Duration) {
    tick.lasts = tick.lasts.max(duration);
}

fn plan_pointer(tick: &mut Tick, source: usize, action: &PointerAction) -> Result<(), AgentError> {
    let step = match action {
        PointerAction::Down { button, pressure } => Step::Down {
            source,
            button: button_of(*button),
            pressure: pressure.map(pressure_of).transpose()?,
        },
        PointerAction::Up { button } => Step::Up {
            source,
            button: button_of(*button),
        },
        PointerAction::Move {
            to,
            duration_ms,
            pressure,
        } => {
            let over = Duration::from_millis(*duration_ms);
            last_at_least(tick, over);
            Step::Move {
                source,
                to: point(*to)?,
                pressure: pressure.map(pressure_of).transpose()?,
                over,
            }
        }
        PointerAction::Cancel => Step::Cancel { source },
        PointerAction::Pause { duration_ms } => {
            last_at_least(tick, Duration::from_millis(*duration_ms));
            return Ok(());
        }
        _ => return Err(unknown_action()),
    };
    tick.steps.push(step);
    Ok(())
}

fn button_of(button: flui_protocol::PointerButton) -> PointerButton {
    match button {
        flui_protocol::PointerButton::Secondary => PointerButton::SECONDARY,
        flui_protocol::PointerButton::Middle => PointerButton::AUXILIARY,
        flui_protocol::PointerButton::Back => PointerButton::BACK,
        flui_protocol::PointerButton::Forward => PointerButton::FORWARD,
        _ => PointerButton::PRIMARY,
    }
}

/// How far from the window's origin a point may be, in logical pixels: past any window, and small
/// enough that a move between two points never overflows.
const MAX_COORDINATE: f64 = 1.0e9;

fn point(at: LogicalPoint) -> Result<Point<f64>, AgentError> {
    let near = |value: f64| value.is_finite() && value.abs() <= MAX_COORDINATE;
    if near(at.x) && near(at.y) {
        Ok(Point::new(at.x, at.y))
    } else {
        Err(invalid(
            "a point is finite and within a billion pixels of the window",
        ))
    }
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
/// (`a`, `é`). A longer ASCII word is a name, and an unknown one is refused rather than typed.
fn key_named(name: &str) -> Result<Key, AgentError> {
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
        ImeAction::SetComposition { text, cursor } => {
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
    fn info(&self) -> PointerInfo {
        PointerInfo::new(self.id, self.kind)
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

/// What a lease's steps came to: the input they make, in order, the moves they start, and the
/// failure that stopped them. The lease's state reflects exactly the input made.
#[derive(Default)]
struct Applied {
    batch: Vec<PlatformInput>,
    glides: Vec<Glide>,
    error: Option<AgentError>,
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

    /// Apply `steps` at `time`, stopping at the first that cannot go out.
    fn apply(
        &mut self,
        pointers: &mut u64,
        sources: &[Source],
        steps: Vec<Step>,
        time: EventTime,
    ) -> Applied {
        let mut applied = Applied::default();
        for step in steps {
            match self.step(pointers, sources, step, time) {
                Ok(Some(Made::Input(input))) => applied.batch.push(input),
                Ok(Some(Made::Glide(glide))) => applied.glides.push(glide),
                Ok(None) => {}
                Err(error) => {
                    applied.error = Some(error);
                    break;
                }
            }
        }
        applied
    }

    /// One step: the lease's state moves on only when it makes its input.
    fn step(
        &mut self,
        pointers: &mut u64,
        sources: &[Source],
        step: Step,
        time: EventTime,
    ) -> Result<Option<Made>, AgentError> {
        let modifiers = modifiers_of(&self.keys);
        let made = match step {
            Step::Down {
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
                Made::Input(PlatformInput::Pointer(event))
            }
            Step::Up { source, button } => {
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
                Made::Input(PlatformInput::Pointer(event))
            }
            Step::Move {
                source,
                to,
                pressure,
                over,
            } => {
                let contact = self.contact(pointers, &sources[source])?;
                Made::Glide(Glide {
                    source,
                    from: contact.at.unwrap_or(to),
                    to,
                    over,
                    pressure,
                })
            }
            Step::Cancel { source } => {
                let contact = self.contact(pointers, &sources[source])?;
                if contact.buttons == PointerButtons::NONE {
                    return Ok(None);
                }
                contact.buttons = PointerButtons::NONE;
                Made::Input(PlatformInput::Pointer(PointerEvent::Cancel(
                    PointerCancel::new(contact.info(), time, CancelReason::Platform),
                )))
            }
            Step::Scroll { source, at, delta } => {
                let contact = self.contact(pointers, &sources[source])?;
                let position =
                    PointerPosition::try_new(at).map_err(|_| invalid("a point is finite"))?;
                contact.at = Some(at);
                Made::Input(PlatformInput::Pointer(PointerEvent::Scroll(
                    ScrollEvent::new(contact.info(), time, position, delta)
                        .with_modifiers(modifiers),
                )))
            }
            Step::Key { state, key } => {
                let pressed = self.keys.iter().position(|held| *held == key);
                let repeat = match (state, pressed) {
                    (KeyState::Down, None) => {
                        self.keys.push(key.clone());
                        KeyRepeat::First
                    }
                    // Pressing a pressed key repeats it, as WebDriver dispatches a second
                    // `keyDown` and as a platform repeats a held key.
                    (KeyState::Down, Some(_)) => KeyRepeat::AutoRepeat,
                    (KeyState::Up, Some(index)) => {
                        self.keys.remove(index);
                        KeyRepeat::First
                    }
                    // Lifting a key that is up sends nothing.
                    _ => return Ok(None),
                };
                // A modifier's own event carries the modifiers as they are after it.
                Made::Input(PlatformInput::Keyboard(
                    KeyEvent::new(state, key, Code::Unidentified, time)
                        .with_modifiers(modifiers_of(&self.keys))
                        .with_repeat(repeat),
                ))
            }
            Step::Ime(event) => Made::Input(PlatformInput::Ime(event)),
        };
        Ok(Some(made))
    }

    /// Move each glide's pointer to where it is `elapsed` into its move, at `time`.
    fn glide(
        &mut self,
        pointers: &mut u64,
        sources: &[Source],
        glides: &[Glide],
        elapsed: Duration,
        time: EventTime,
    ) -> Applied {
        let mut applied = Applied::default();
        for glide in glides {
            let modifiers = modifiers_of(&self.keys);
            let moved = self
                .contact(pointers, &sources[glide.source])
                .and_then(|contact| {
                    let at = glide.at(elapsed);
                    if contact.at == Some(at) {
                        return Ok(None);
                    }
                    let sample = sample(time, at, glide.pressure)?;
                    contact.at = Some(at);
                    // A touch contact that is up has no position the window sees.
                    if contact.kind == PointerKind::Touch && contact.buttons == PointerButtons::NONE
                    {
                        return Ok(None);
                    }
                    Ok(Some(PointerEvent::Move(
                        PointerMove::new(contact.info(), contact.buttons, sample)
                            .with_modifiers(modifiers),
                    )))
                });
            match moved {
                Ok(Some(event)) => applied.batch.push(PlatformInput::Pointer(event)),
                Ok(None) => {}
                Err(error) => {
                    applied.error = Some(error);
                    break;
                }
            }
        }
        applied
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

/// What one step made.
enum Made {
    Input(PlatformInput),
    Glide(Glide),
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

/// One `input.inject` the window is playing.
struct Injection {
    /// Which of the window's injections it is.
    id: u64,
    lease: LeaseId,
    plan: Plan,
    /// The tick under way, while its moves and pauses last.
    current: Option<Running>,
    /// How many ticks it had.
    total: usize,
    snapshot: Snapshot,
    before: Option<TreeSnapshot>,
    reply: ReplySender<ActOutcome>,
    /// The window's journal entries its ticks so far made.
    observed: Option<JournalSpan>,
}

/// A tick whose moves or pause are still running.
struct Running {
    started: Instant,
    lasts: Duration,
    moves: Vec<Glide>,
}

/// A move spread over its duration.
struct Glide {
    source: usize,
    from: Point<f64>,
    to: Point<f64>,
    over: Duration,
    pressure: Option<Pressure>,
}

impl Glide {
    /// Where the pointer is `elapsed` into the move.
    fn at(&self, elapsed: Duration) -> Point<f64> {
        if elapsed >= self.over {
            return self.to;
        }
        let t = elapsed.as_secs_f64() / self.over.as_secs_f64();
        Point::new(
            self.from.x + (self.to.x - self.from.x) * t,
            self.from.y + (self.to.y - self.from.y) * t,
        )
    }
}

impl AgentInput {
    /// Queue an injection of `plan` through `lease`.
    ///
    /// # Errors
    ///
    /// `busy`, with the reply back, when the window already has [`MAX_QUEUED`] jobs.
    pub(super) fn inject(
        &mut self,
        lease: LeaseId,
        plan: Plan,
        snapshot: Snapshot,
        before: Option<TreeSnapshot>,
        reply: ReplySender<ActOutcome>,
    ) -> Result<(), ReplySender<ActOutcome>> {
        if self.jobs.len() >= MAX_QUEUED {
            return Err(reply);
        }
        let total = plan.len();
        self.injections = self
            .injections
            .checked_add(1)
            .expect("BUG: a window queues fewer than 2^64 injections");
        self.jobs.push_back(Job::Inject(Box::new(Injection {
            id: self.injections,
            lease,
            plan,
            current: None,
            total,
            snapshot,
            before,
            reply,
            observed: None,
        })));
        Ok(())
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

    /// The next step of the window's work at a frame boundary at `now`, stamped `time`.
    fn turn(&mut self, now: Instant, time: EventTime) -> Turn {
        let Self {
            leases,
            jobs,
            pointers,
            injections: _,
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
                    .map_or_else(Default::default, |held| held.let_go(time));
                return Turn::Released(batch, released, reply);
            }
        };
        let held = leases.entry(injection.lease).or_default();
        let mut batch = Vec::new();
        if let Some(running) = injection.current.take() {
            // The running tick's moves go on; it ends once it has lasted its time.
            let elapsed = now.saturating_duration_since(running.started);
            let moved = held.glide(
                pointers,
                &injection.plan.sources,
                &running.moves,
                elapsed,
                time,
            );
            batch.extend(moved.batch);
            if let Some(error) = moved.error {
                return Self::failed(jobs, batch, error);
            }
            if elapsed < running.lasts {
                injection.current = Some(running);
                return Turn::Sent(batch, injection.id);
            }
        } else if let Some(Tick { steps, lasts }) = injection.plan.ticks.pop_front() {
            let applied = held.apply(pointers, &injection.plan.sources, steps, time);
            batch.extend(applied.batch);
            if let Some(error) = applied.error {
                return Self::failed(jobs, batch, error);
            }
            let moved = held.glide(
                pointers,
                &injection.plan.sources,
                &applied.glides,
                Duration::ZERO,
                time,
            );
            batch.extend(moved.batch);
            if let Some(error) = moved.error {
                return Self::failed(jobs, batch, error);
            }
            if !lasts.is_zero() {
                injection.current = Some(Running {
                    started: now,
                    lasts,
                    moves: applied.glides,
                });
                return Turn::Sent(batch, injection.id);
            }
        }
        if !injection.plan.ticks.is_empty() {
            // The next tick starts at the next boundary.
            return Turn::Sent(batch, injection.id);
        }
        let Some(Job::Inject(injection)) = jobs.pop_front() else {
            unreachable!("BUG: the front job is an injection");
        };
        Turn::Played(batch, injection)
    }

    /// The front injection stopped at `error` in the tick under way, after making `batch`: the
    /// ticks before it went out in full, and it is neither in the plan nor running any more.
    fn failed(jobs: &mut VecDeque<Job>, batch: Vec<PlatformInput>, error: AgentError) -> Turn {
        let Some(Job::Inject(injection)) = jobs.pop_front() else {
            unreachable!("BUG: the front job is an injection");
        };
        let played = injection
            .total
            .saturating_sub(injection.plan.len())
            .saturating_sub(1);
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
    /// Queue an injection of `plan` through `lease` in `state`'s window, and ask for the frame
    /// whose boundary starts it.
    pub(super) fn queue_injection(
        &self,
        state: &PresentationState,
        lease: LeaseId,
        plan: Plan,
        snapshot: Snapshot,
        before: Option<TreeSnapshot>,
        reply: ReplySender<ActOutcome>,
    ) {
        let queued = state
            .agent_input
            .borrow_mut()
            .inject(lease, plan, snapshot, before, reply);
        match queued {
            Ok(()) => self.request_redraw_for(state),
            Err(reply) => send_reply(&reply, None, Err(AgentError::InputQueueFull)),
        }
    }

    /// Queue `input.release` through `lease` in `state`'s window.
    pub(super) fn queue_release(
        &self,
        state: &PresentationState,
        lease: LeaseId,
        reply: ReplySender<Released>,
    ) {
        let queued = state.agent_input.borrow_mut().release(lease, reply);
        match queued {
            Ok(()) => self.request_redraw_for(state),
            Err(reply) => send_reply(&reply, None, Err(AgentError::InputQueueFull)),
        }
    }

    /// Advance every window's injections at a frame boundary at `now`: a running tick's moves go
    /// on, a tick that has lasted its time ends, and the next one starts, one per window per
    /// boundary. A panic in the app's handler of injected input continues once every window has
    /// had its turn; the first is kept.
    pub(super) fn advance_agent_input(&self, now: Instant) {
        let time = self.event_time_at(now);
        let mut first = FirstPanic::new();
        for state in self.presentations.iter() {
            first.contain(|| self.advance_window_input(state, now, time));
        }
        first.resume();
    }

    fn advance_window_input(&self, state: &PresentationState, now: Instant, time: EventTime) {
        loop {
            // Taken with the window's input borrowed; no app code runs until it is let go.
            let turn = state.agent_input.borrow_mut().turn(now, time);
            match turn {
                Turn::Idle => return,
                Turn::Sent(batch, id) => {
                    self.request_redraw_for(state);
                    let sent = self.send_agent_input(state, batch);
                    state.agent_input.borrow_mut().observe(id, sent.observed);
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
                        snapshot,
                        before,
                        reply,
                        observed,
                        ..
                    } = *injection;
                    let sent = self.send_agent_input(state, batch);
                    let mut outcome =
                        ActOutcome::new(InputPath::Synthetic, Effect::new(EffectKind::Ran));
                    outcome.observed = widen(observed, sent.observed);
                    // Owed once the input has gone out, a handler's panic included: the answer
                    // waits for the frame after it, which shows what the input did. The window
                    // is still open, since closing one takes the runtime mutably.
                    self.park_act_read(state, ReadingOwed::new(outcome, snapshot, before), reply);
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

    /// Release what each of `leases` holds in `state`'s window, and drop the work queued through
    /// them: their holders have gone.
    fn release_agent_leases(&self, state: &PresentationState, leases: Vec<LeaseId>) {
        let time = self.event_time_at(flui_foundation::MonotonicClock::now(&self.clock));
        let mut batch = Vec::new();
        let mut dropped = Vec::new();
        {
            let mut input = state.agent_input.borrow_mut();
            for lease in leases {
                let (released, jobs) = input.forget(lease, time);
                batch.extend(released);
                dropped.extend(jobs);
            }
        }
        let mut sent = self.send_agent_input(state, batch);
        // An answer wakes its asker, user code: they go after the input is let go, and before a
        // panic continues.
        sent.panic
            .absorb(super::deliver_each(dropped, Abandoned::lease_released));
        sent.resume();
    }
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
fn widen(a: Option<JournalSpan>, b: Option<JournalSpan>) -> Option<JournalSpan> {
    match (a, b) {
        (Some(a), Some(b)) => Some(JournalSpan {
            from: a.from.min(b.from),
            to: a.to.max(b.to),
        }),
        (a, b) => a.or(b),
    }
}
