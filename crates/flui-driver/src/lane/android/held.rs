//! What the Android lane holds, and the commands each event becomes: the one owner of its keys,
//! its contact and its pen.
//!
//! `input` cannot hold a key between two commands: `keyevent` presses and releases, and
//! `keycombination` holds its modifiers only while it presses the keys after them. So a key is
//! sent when it is released: alone as `keyevent`, or with the modifiers held as
//! `keycombination`; a modifier pressed and released with nothing between is sent alone. A key
//! held across other events is held here only, and nothing of it is down on the device. What
//! `input` holds between commands is one contact (`motionevent DOWN`, then `MOVE`s, then `UP`),
//! through the touch screen or the mouse, and the lane's pen holds its range and its touch through
//! its `uinput` device.
//!
//! - **Recorded once taken.** [`HeldInput::plan`] turns an event into commands and changes only
//!   what is held here alone (the keys, where each pointer is); what is held on the device changes
//!   with [`HeldInput::took`], once the device took a command (a `motionevent` returns once the
//!   app finished with it), or [`HeldInput::unsure`] when the command timed out and may have
//!   happened: a press that may have gone down is held, so it is released.
//! - **Released on every exit.** [`HeldInput::release_plan`] is what releases it: the contact
//!   cancelled where it is (`motionevent CANCEL`, which acts on nothing), the pen lifted and taken
//!   out of range. What stays held after it is named by [`HeldInput::held_names`]: a key a
//!   timed-out command may have left down cannot be released through `input` without pressing it,
//!   so it is named, never pressed again.
//! - **Refused before any event.** `Lane::check` plans the whole sequence on a copy, so a second
//!   contact, a press with a key held, a key or chord the system takes, a character the device
//!   cannot type or a command its `input` lacks is refused before the first event goes out.

use flui_protocol::input::{PointerButton, PointerDevice};

use super::device::InputCommands;
use super::keys::{self, KeyName, Keycode, TextStroke};
use super::safety::{self, Display, Refusal};
use crate::coordinates::DevicePoint;
use crate::steps::OsEvent;

/// How many device pixels of a scroll turn the wheel one detent; the app turns a detent into a
/// distance of its own, so a scroll's distance is not compared, only that it arrived.
pub const WHEEL_STEP: f64 = 40.0;

/// The most pointers one sequence names.
pub const MAX_POINTERS: usize = 16;

/// What `input motionevent` a contact goes through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MotionSource {
    /// `input touchscreen`: a finger.
    Touchscreen,
    /// `input mouse`: the mouse, its primary button.
    Mouse,
}

impl MotionSource {
    /// The source as `input` names it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Touchscreen => "touchscreen",
            Self::Mouse => "mouse",
        }
    }
}

/// A `motionevent` action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MotionAction {
    /// The contact goes down.
    Down,
    /// It moves.
    Move,
    /// It lifts.
    Up,
    /// It is cancelled: it lifts acting on nothing.
    Cancel,
}

impl MotionAction {
    fn name(self) -> &'static str {
        match self {
            Self::Down => "DOWN",
            Self::Move => "MOVE",
            Self::Up => "UP",
            Self::Cancel => "CANCEL",
        }
    }
}

/// What the pen does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PenChange {
    /// It comes into range at a point.
    Enter,
    /// It moves, in range or touching.
    Move,
    /// It touches down.
    Press,
    /// It lifts, staying in range.
    Lift,
    /// It leaves range.
    Leave,
}

/// One command to the device.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// `input <source> motionevent <action> <x> <y>`.
    Motion {
        /// The pointer, by its source's id in the sequence.
        pointer: String,
        /// Through which source.
        source: MotionSource,
        /// What it does.
        action: MotionAction,
        /// Where, in display pixels.
        at: DevicePoint,
    },
    /// `input keyevent <key>`: a press and a release.
    Key(Keycode),
    /// `input keycombination <keys>`: the modifiers held while the last key is pressed.
    Chord(Vec<Keycode>),
    /// `input text <run>`.
    Text(String),
    /// `input mouse scroll <x> <y>` with its axes, in detents.
    Scroll {
        /// Where, in display pixels.
        at: DevicePoint,
        /// Detents up; negative is down (`AXIS_VSCROLL`).
        vertical: f64,
        /// Detents right; negative is left (`AXIS_HSCROLL`).
        horizontal: f64,
    },
    /// A report of the lane's pen.
    Pen {
        /// What the pen does.
        change: PenChange,
        /// Where, in display pixels.
        at: DevicePoint,
    },
}

impl Command {
    /// The device shell line of an `input` command; `None` for the pen, whose reports go to its
    /// `uinput` device.
    #[must_use]
    pub fn line(&self) -> Option<String> {
        Some(match self {
            Self::Motion {
                source, action, at, ..
            } => format!(
                "input {} motionevent {} {} {}",
                source.name(),
                action.name(),
                at.x,
                at.y
            ),
            Self::Key(code) => format!("input keyevent {}", code.name),
            Self::Chord(codes) => format!(
                "input keycombination {}",
                codes
                    .iter()
                    .map(|code| code.name)
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Self::Text(run) => format!("input text {}", super::adb::quote(run)),
            Self::Scroll {
                at,
                vertical,
                horizontal,
            } => format!(
                "input mouse scroll {} {} --axis VSCROLL,{vertical} --axis HSCROLL,{horizontal}",
                at.x, at.y
            ),
            Self::Pen { .. } => return None,
        })
    }

    /// Whether the command acts on the app, so the app's window must hold the focus before it:
    /// a contact or the pen going down, a key, text, a scroll.
    #[must_use]
    pub fn needs_focus(&self) -> bool {
        match self {
            Self::Motion { action, .. } => *action == MotionAction::Down,
            Self::Pen { change, .. } => *change == PenChange::Press,
            Self::Key(_) | Self::Chord(_) | Self::Text(_) | Self::Scroll { .. } => true,
        }
    }
}

/// A part of what the lane holds that is released on its own, so one part's failure never hides
/// what another left held (`crate::lane::release_outcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Part {
    /// The one contact `input motionevent` holds.
    Contact,
    /// The pen's range and touch, on its `uinput` device.
    Pen,
}

/// What the device takes, as the plan reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abilities {
    /// The commands its `input` has.
    pub input: InputCommands,
    /// Whether the lane's pen can be registered (`uinput_device`).
    pub uinput: bool,
    /// The display as it is turned now; `None` before the lane located the app's window.
    pub display: Option<Display>,
}

/// The contact `input` holds.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Contact {
    pointer: String,
    source: MotionSource,
    at: DevicePoint,
}

/// The pen, in range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pen {
    at: DevicePoint,
    touching: bool,
}

/// What the lane holds: see the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct HeldInput {
    contact: Option<Contact>,
    pen: Option<Pen>,
    /// Keys a timed-out command may have left down.
    unsure: Vec<String>,
    positions: Vec<(String, DevicePoint)>,
    modifiers: Vec<Keycode>,
    pending: Option<Keycode>,
    modifier_used: bool,
}

impl HeldInput {
    /// Nothing held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The commands `event` becomes with what is held now, what is held here alone changed to
    /// after it.
    ///
    /// # Errors
    ///
    /// The [`Refusal`] of an event the lane does not send: see the [module docs](self).
    pub fn plan(
        &mut self,
        event: &OsEvent,
        abilities: &Abilities,
    ) -> Result<Vec<Command>, Refusal> {
        match event {
            OsEvent::PointerMove {
                pointer,
                device,
                to,
            } => self.pointer_move(pointer, *device, *to, abilities),
            OsEvent::PointerDown {
                pointer,
                device,
                button,
            } => self.pointer_down(pointer, *device, *button, abilities),
            OsEvent::PointerUp {
                pointer, device, ..
            } => Ok(self.pointer_up(pointer, *device)),
            OsEvent::Scroll {
                at,
                delta_x,
                delta_y,
            } => {
                self.refuse_held_keys("a scroll")?;
                if !abilities.input.pointer_scroll {
                    return Err(Refusal::NeedsCommand {
                        command: "scroll",
                        what: "a turn of the wheel".to_owned(),
                    });
                }
                Ok(vec![Command::Scroll {
                    at: *at,
                    vertical: -f64::from(*delta_y) / WHEEL_STEP,
                    horizontal: f64::from(*delta_x) / WHEEL_STEP,
                }])
            }
            OsEvent::KeyDown(name) => self.key_down(name),
            OsEvent::KeyUp(name) => self.key_up(name, abilities),
            OsEvent::Text(text) => {
                self.refuse_held_keys("typed text")?;
                let strokes = keys::text_strokes(text)
                    .map_err(|character| Refusal::UnsentText { character })?;
                Ok(strokes
                    .into_iter()
                    .map(|stroke| match stroke {
                        TextStroke::Run(run) => Command::Text(run),
                        TextStroke::Key(code) => Command::Key(code),
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    fn pointer_move(
        &mut self,
        pointer: &str,
        device: PointerDevice,
        to: DevicePoint,
        abilities: &Abilities,
    ) -> Result<Vec<Command>, Refusal> {
        let commands = match device {
            PointerDevice::Pen => {
                if !abilities.uinput {
                    return Err(Refusal::NeedsUinput {
                        what: "a pen".to_owned(),
                    });
                }
                vec![Command::Pen {
                    change: if self.pen.is_some() {
                        PenChange::Move
                    } else {
                        PenChange::Enter
                    },
                    at: to,
                }]
            }
            PointerDevice::Touch | PointerDevice::Mouse => match &self.contact {
                // A move while nothing is down only places the pointer: `input` has no hover.
                Some(contact) if contact.pointer == pointer => vec![Command::Motion {
                    pointer: pointer.to_owned(),
                    source: contact.source,
                    action: MotionAction::Move,
                    at: to,
                }],
                _ => Vec::new(),
            },
            other => return Err(unsent_device(other)),
        };
        self.place(pointer, to)?;
        Ok(commands)
    }

    fn pointer_down(
        &mut self,
        pointer: &str,
        device: PointerDevice,
        button: PointerButton,
        abilities: &Abilities,
    ) -> Result<Vec<Command>, Refusal> {
        self.refuse_held_keys("a press")?;
        if button != PointerButton::Primary {
            return Err(Refusal::UnsentInput {
                what: format!("the {} button of a {}", button_name(button), device.name()),
            });
        }
        let pen_down = self.pen.is_some_and(|pen| pen.touching);
        if self.contact.is_some() || pen_down {
            return Err(Refusal::UnsentInput {
                what: "a second contact: `input` drives one pointer at a time".to_owned(),
            });
        }
        let at = self
            .positions
            .iter()
            .find(|(known, _)| known == pointer)
            .map(|(_, at)| *at)
            .ok_or_else(|| Refusal::UnsentInput {
                what: format!("a press of {pointer} before it moved anywhere"),
            })?;
        let display = abilities.display.ok_or(Refusal::WindowGone)?;
        if device != PointerDevice::Mouse
            && let Some(gesture) = safety::gesture_edge(at, &display)
        {
            return Err(Refusal::SystemGesture { point: at, gesture });
        }
        match device {
            PointerDevice::Pen => {
                if !abilities.uinput {
                    return Err(Refusal::NeedsUinput {
                        what: "a pen".to_owned(),
                    });
                }
                Ok(vec![Command::Pen {
                    change: PenChange::Press,
                    at,
                }])
            }
            PointerDevice::Touch | PointerDevice::Mouse => {
                if !abilities.input.motion_event {
                    return Err(Refusal::NeedsCommand {
                        command: "motionevent",
                        what: format!("a {} press held between events", device.name()),
                    });
                }
                Ok(vec![Command::Motion {
                    pointer: pointer.to_owned(),
                    source: if device == PointerDevice::Mouse {
                        MotionSource::Mouse
                    } else {
                        MotionSource::Touchscreen
                    },
                    action: MotionAction::Down,
                    at,
                }])
            }
            other => Err(unsent_device(other)),
        }
    }

    fn pointer_up(&self, pointer: &str, device: PointerDevice) -> Vec<Command> {
        match (device, &self.contact, self.pen) {
            (PointerDevice::Pen, _, Some(pen)) if pen.touching => vec![Command::Pen {
                change: PenChange::Lift,
                at: pen.at,
            }],
            (PointerDevice::Touch | PointerDevice::Mouse, Some(contact), _)
                if contact.pointer == pointer =>
            {
                vec![Command::Motion {
                    pointer: pointer.to_owned(),
                    source: contact.source,
                    action: MotionAction::Up,
                    at: contact.at,
                }]
            }
            // A release of nothing held sends nothing.
            _ => Vec::new(),
        }
    }

    fn key_down(&mut self, name: &str) -> Result<Vec<Command>, Refusal> {
        let code = resolve(name)?;
        if keys::is_modifier(code) {
            if !self.modifiers.contains(&code) {
                self.modifiers.push(code);
            }
            return Ok(Vec::new());
        }
        if self.pending.is_some_and(|pending| pending != code) {
            return Err(Refusal::UnsentInput {
                what: "two keys held at once: `input` presses one key at a time".to_owned(),
            });
        }
        self.pending = Some(code);
        Ok(Vec::new())
    }

    fn key_up(&mut self, name: &str, abilities: &Abilities) -> Result<Vec<Command>, Refusal> {
        let code = resolve(name)?;
        if self.pending == Some(code) {
            self.pending = None;
            if self.modifiers.is_empty() {
                return Ok(vec![Command::Key(code)]);
            }
            if let Some(effect) = safety::system_chord(&self.modifiers, code) {
                return Err(Refusal::SystemChord {
                    chord: safety::chord_name(&self.modifiers, code),
                    effect,
                });
            }
            if !abilities.input.key_combination {
                return Err(Refusal::NeedsCommand {
                    command: "keycombination",
                    what: format!("the chord {}", safety::chord_name(&self.modifiers, code)),
                });
            }
            self.modifier_used = true;
            let mut chord = self.modifiers.clone();
            chord.push(code);
            return Ok(vec![Command::Chord(chord)]);
        }
        let Some(index) = self.modifiers.iter().position(|held| *held == code) else {
            // A release of a key not held sends nothing.
            return Ok(Vec::new());
        };
        self.modifiers.remove(index);
        let lone = self.modifiers.is_empty() && !self.modifier_used;
        if self.modifiers.is_empty() {
            self.modifier_used = false;
        }
        if !lone {
            return Ok(Vec::new());
        }
        if code == keys::META {
            return Err(Refusal::SystemKey {
                name: "Meta".to_owned(),
                effect: "opens the app drawer when tapped alone",
            });
        }
        Ok(vec![Command::Key(code)])
    }

    /// Refuse `what` while a key is held here: `input` sends no key state with it.
    fn refuse_held_keys(&self, what: &str) -> Result<(), Refusal> {
        if self.modifiers.is_empty() && self.pending.is_none() {
            return Ok(());
        }
        Err(Refusal::UnsentInput {
            what: format!("{what} while a key is held: `input` sends no key state with it"),
        })
    }

    /// Remember that `pointer` is at `at`.
    fn place(&mut self, pointer: &str, at: DevicePoint) -> Result<(), Refusal> {
        if let Some((_, known)) = self
            .positions
            .iter_mut()
            .find(|(known, _)| known == pointer)
        {
            *known = at;
            return Ok(());
        }
        if self.positions.len() >= MAX_POINTERS {
            return Err(Refusal::UnsentInput {
                what: format!("more than {MAX_POINTERS} pointers in one sequence"),
            });
        }
        self.positions.push((pointer.to_owned(), at));
        Ok(())
    }

    /// The device took `command`.
    pub fn took(&mut self, command: &Command) {
        match command {
            Command::Motion {
                pointer,
                source,
                action,
                at,
            } => match action {
                MotionAction::Down => {
                    self.contact = Some(Contact {
                        pointer: pointer.clone(),
                        source: *source,
                        at: *at,
                    });
                }
                MotionAction::Move => {
                    if let Some(contact) = &mut self.contact {
                        contact.at = *at;
                    }
                }
                MotionAction::Up | MotionAction::Cancel => self.contact = None,
            },
            Command::Pen { change, at } => {
                let touching = match change {
                    PenChange::Enter | PenChange::Move => self.pen.is_some_and(|pen| pen.touching),
                    PenChange::Press => true,
                    PenChange::Lift => false,
                    PenChange::Leave => {
                        self.pen = None;
                        return;
                    }
                };
                self.pen = Some(Pen { at: *at, touching });
            }
            Command::Key(_) | Command::Chord(_) | Command::Text(_) | Command::Scroll { .. } => {}
        }
    }

    /// `command` timed out: it may have happened. What it may have pressed is held; what it may
    /// have released stays held.
    pub fn unsure(&mut self, command: &Command) {
        match command {
            Command::Motion {
                action: MotionAction::Down,
                ..
            }
            | Command::Pen {
                change: PenChange::Enter | PenChange::Press | PenChange::Move,
                ..
            } => self.took(command),
            Command::Key(code) => self.unsure.push(code.name.to_owned()),
            Command::Chord(codes) => self
                .unsure
                .extend(codes.iter().map(|code| code.name.to_owned())),
            Command::Text(_) => self.unsure.push("a key of typed text".to_owned()),
            Command::Motion { .. } | Command::Pen { .. } | Command::Scroll { .. } => {}
        }
    }

    /// The commands that release what the device holds of the lane's, in order: the contact's,
    /// then the pen's.
    #[must_use]
    pub fn release_plan(&self) -> Vec<Command> {
        let mut commands = self.release_plan_of(Part::Contact);
        commands.extend(self.release_plan_of(Part::Pen));
        commands
    }

    /// The commands that release what `part` holds, in order.
    #[must_use]
    pub fn release_plan_of(&self, part: Part) -> Vec<Command> {
        match part {
            Part::Contact => self
                .contact
                .iter()
                .map(|contact| Command::Motion {
                    pointer: contact.pointer.clone(),
                    source: contact.source,
                    action: MotionAction::Cancel,
                    at: contact.at,
                })
                .collect(),
            Part::Pen => {
                let Some(pen) = self.pen else {
                    return Vec::new();
                };
                let lift = pen.touching.then_some(Command::Pen {
                    change: PenChange::Lift,
                    at: pen.at,
                });
                lift.into_iter()
                    .chain(std::iter::once(Command::Pen {
                        change: PenChange::Leave,
                        at: pen.at,
                    }))
                    .collect()
            }
        }
    }

    /// Forget what is held here alone, the keys and where each pointer is, as the end of a
    /// sequence does.
    pub fn forget_plans(&mut self) {
        self.positions.clear();
        self.modifiers.clear();
        self.pending = None;
        self.modifier_used = false;
    }

    /// The pen's device is gone, and with it everything it held: the kernel removes a device's
    /// contacts with it.
    pub fn pen_device_gone(&mut self) {
        self.pen = None;
    }

    /// What the device still holds of the lane's, by name.
    #[must_use]
    pub fn held_names(&self) -> Vec<String> {
        let mut names = self.held_by(Part::Contact);
        names.extend(self.held_by(Part::Pen));
        names.extend(self.unsure.iter().cloned());
        names
    }

    /// What `part` still holds on the device, by name.
    #[must_use]
    pub fn held_by(&self, part: Part) -> Vec<String> {
        match part {
            Part::Contact => self
                .contact
                .iter()
                .map(|contact| match contact.source {
                    MotionSource::Touchscreen => format!("the touch contact {}", contact.pointer),
                    MotionSource::Mouse => format!("the mouse button of {}", contact.pointer),
                })
                .collect(),
            Part::Pen => self
                .pen
                .iter()
                .map(|pen| {
                    if pen.touching {
                        "the pen, touching".to_owned()
                    } else {
                        "the pen, in range".to_owned()
                    }
                })
                .collect(),
        }
    }

    /// The keys a timed-out command may have left down, by name, forgotten once taken: `input`
    /// has no release for them, so they are named once.
    pub fn take_unsure(&mut self) -> Vec<String> {
        std::mem::take(&mut self.unsure)
    }

    /// Whether the device holds anything of the lane's.
    #[must_use]
    pub fn holds_anything(&self) -> bool {
        self.contact.is_some() || self.pen.is_some() || !self.unsure.is_empty()
    }

    /// Whether the pen is in range.
    #[must_use]
    pub fn pen_in_range(&self) -> bool {
        self.pen.is_some()
    }
}

fn resolve(name: &str) -> Result<Keycode, Refusal> {
    match keys::resolve(name) {
        KeyName::Key(code) => Ok(code),
        KeyName::System(effect) => Err(Refusal::SystemKey {
            name: name.to_owned(),
            effect,
        }),
        _ => Err(Refusal::UnknownKey {
            name: name.to_owned(),
        }),
    }
}

fn unsent_device(device: PointerDevice) -> Refusal {
    Refusal::UnsentInput {
        what: format!("{} input", device.name()),
    }
}

fn button_name(button: PointerButton) -> &'static str {
    match button {
        PointerButton::Primary => "primary",
        PointerButton::Secondary => "secondary",
        PointerButton::Middle => "middle",
        PointerButton::Back => "back",
        PointerButton::Forward => "forward",
        _ => "other",
    }
}
