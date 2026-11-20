//! What the Windows lane holds down, and the one path every key goes through: a key event and
//! typed text alike become [`Keystroke`]s planned against [`HeldKeys`], which the safety check
//! reads before a sequence and the lane sends stroke by stroke. A held key or button is recorded
//! only once Windows took its press, and forgotten only once Windows took its release, so a
//! release Windows refused stays held for the next [`HeldKeys::release_all`] to try again.

use flui_protocol::error::ErrorCode;

use crate::error::{Error, Result};
use crate::lane::windows::keys::{self, Modifier, SHIFT, TextStroke, VirtualKey};
use crate::lane::windows::safety::{KeyEdge, Refusal, ShellShortcuts};

/// `VK_PACKET`, the key a Unicode packet arrives as.
pub const PACKET: VirtualKey = VirtualKey {
    code: 0xE7,
    extended: false,
};

/// The key pressed and released between held modifiers and their release, so that releasing them
/// completes nothing (the input-language switch, Sticky Keys, an app's menu bar on a lone Alt):
/// those fire on a release of modifiers with no other key pressed while they were held. `0xE8`
/// is unassigned in Microsoft's virtual-key code table, so no window gives it a meaning, and it is
/// the key AutoHotkey masks a modifier release with by default (`#MenuMaskKey`).
pub const MASK: VirtualKey = VirtualKey {
    code: 0xE8,
    extended: false,
};

/// A key event, its key resolved on the target's layout.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyInput {
    /// Press `key`, with Shift held around it when `shift` (a capital letter).
    Down {
        /// The key.
        key: VirtualKey,
        /// Whether it needs Shift.
        shift: bool,
    },
    /// Release `key`.
    Up {
        /// The key.
        key: VirtualKey,
    },
    /// Type `text`.
    Text(String),
}

/// One thing the lane sends to the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Keystroke {
    /// Press a key.
    Press(VirtualKey),
    /// Release a key.
    Release(VirtualKey),
    /// Press Shift for the key with this code, which needs it.
    ShiftFor(u16),
    /// Release the Shift pressed for the key with this code.
    UnshiftFor(u16),
    /// A UTF-16 unit, pressed and released as a Unicode packet.
    Unit(u16),
}

impl Keystroke {
    /// The key edges the keyboard sees for this stroke, as the shell-hotkey rule reads them.
    #[must_use]
    pub fn edges(self) -> Vec<KeyEdge> {
        match self {
            Self::Press(key) => vec![KeyEdge::Down(key)],
            Self::Release(key) => vec![KeyEdge::Up(key)],
            Self::ShiftFor(_) => vec![KeyEdge::Down(SHIFT)],
            Self::UnshiftFor(_) => vec![KeyEdge::Up(SHIFT)],
            Self::Unit(_) => vec![KeyEdge::Down(PACKET), KeyEdge::Up(PACKET)],
        }
    }
}

/// The keys the lane holds, and the counts the OS keeps across them for its shortcuts: see the
/// [module docs](self). The one owner of both: every key edge the OS took goes through
/// [`Self::apply`], a release of everything included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeldKeys {
    /// The keys down, by virtual-key code, in the order they went down.
    held: Vec<u16>,
    /// The keys pressed with a Shift the lane added for them.
    shifted: Vec<u16>,
    /// The shell's shortcuts as the OS has seen the lane's keys.
    shortcuts: ShellShortcuts,
}

impl HeldKeys {
    /// No key held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys down, by virtual-key code.
    #[must_use]
    pub fn held(&self) -> &[u16] {
        &self.held
    }

    /// The strokes `input` takes from this state. A release that would complete a shortcut of the
    /// shell's (letting go of Control+Shift or Alt+Shift, the fifth lone Shift) is preceded by a
    /// press and release of [`MASK`], so the app still gets its chord and the shell nothing.
    #[must_use]
    pub fn plan(&self, input: &KeyInput) -> Vec<Keystroke> {
        match input {
            KeyInput::Down { key, shift } => {
                if *shift && !self.held.contains(&SHIFT.code) {
                    vec![Keystroke::ShiftFor(key.code), Keystroke::Press(*key)]
                } else {
                    vec![Keystroke::Press(*key)]
                }
            }
            KeyInput::Up { key } => {
                let mut strokes = vec![Keystroke::Release(*key)];
                if self.shifted.contains(&key.code) {
                    strokes.push(Keystroke::UnshiftFor(key.code));
                }
                if self.completes_shortcut(&strokes) {
                    strokes.splice(0..0, [Keystroke::Press(MASK), Keystroke::Release(MASK)]);
                }
                strokes
            }
            KeyInput::Text(text) => keys::text_strokes(text)
                .into_iter()
                .flat_map(|stroke| match stroke {
                    TextStroke::Key(key) => vec![Keystroke::Press(key), Keystroke::Release(key)],
                    TextStroke::Unit(unit) => vec![Keystroke::Unit(unit)],
                })
                .collect(),
        }
    }

    /// Whether `strokes`, sent from this state, complete a shortcut of the shell's.
    fn completes_shortcut(&self, strokes: &[Keystroke]) -> bool {
        let mut state = self.clone();
        strokes.iter().any(|stroke| state.apply(*stroke).is_some())
    }

    /// Record `stroke`, which Windows took; what of the shell's it completed, if anything.
    pub fn apply(&mut self, stroke: Keystroke) -> Option<&'static str> {
        let mut completed = None;
        for edge in stroke.edges() {
            let taken = match edge {
                KeyEdge::Down(key) => {
                    let taken = self.shortcuts.press(key, &self.held);
                    if key != PACKET && !self.held.contains(&key.code) {
                        self.held.push(key.code);
                    }
                    taken
                }
                KeyEdge::Up(key) => {
                    self.held.retain(|code| *code != key.code);
                    self.shortcuts.release(key, &self.held)
                }
            };
            completed = completed.or(taken);
        }
        match stroke {
            Keystroke::ShiftFor(code) => self.shifted.push(code),
            Keystroke::UnshiftFor(code) | Keystroke::Release(VirtualKey { code, .. }) => {
                self.shifted.retain(|shifted| *shifted != code);
            }
            _ => {}
        }
        if !self.held.contains(&SHIFT.code) {
            self.shifted.clear();
        }
        completed
    }

    /// Send `stroke` through `send` and record it as soon as `send` says Windows took it; only
    /// then `confirm` (that the guard saw it come back), whose failure is reported with the
    /// stroke already recorded, so a release of everything releases it.
    ///
    /// # Errors
    ///
    /// `send`'s failure, with nothing recorded; `confirm`'s, with the stroke recorded.
    pub fn deliver(
        &mut self,
        stroke: Keystroke,
        send: impl FnOnce() -> Result<()>,
        confirm: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        send()?;
        let _ = self.apply(stroke);
        confirm()
    }

    /// What of the shell's `inputs`, sent from this state, would take: a press the shell takes
    /// ([`safety::press_takes`](crate::lane::windows::safety::press_takes)). A release that would
    /// complete a shortcut is masked by [`Self::plan`], and modifiers a sequence leaves down by
    /// [`Self::release_all`], so neither is refused.
    #[must_use]
    pub fn shell_hotkey(&self, inputs: &[KeyInput]) -> Option<&'static str> {
        let mut state = self.clone();
        for input in inputs {
            for stroke in state.plan(input) {
                if let Some(taken) = state.apply(stroke) {
                    return Some(taken);
                }
            }
        }
        None
    }

    /// Release every key held, last pressed first, through `send`, completing no shortcut of the
    /// shell's: while a modifier is held, [`MASK`] is pressed and released first, which breaks a
    /// chord of modifiers and a run of Shift taps as any other key does. A release that would
    /// still complete one (the mask was not sent) is not sent, and that key stays held; so does a
    /// key whose release fails. Each stroke Windows took is recorded as the OS saw it.
    ///
    /// # Errors
    ///
    /// After every key was tried: [`Error::InputHeld`] naming the keys still held and the first
    /// stroke that failed or was refused, or that stroke's error when nothing stayed held.
    pub fn release_all(&mut self, mut send: impl FnMut(Keystroke) -> Result<()>) -> Result<()> {
        let mut first: Option<Error> = None;
        let modifier_held = self.held.iter().any(|&code| {
            Modifier::of(VirtualKey {
                code,
                extended: false,
            })
            .is_some()
        });
        if modifier_held {
            let masked = self
                .send_unless_shortcut(Keystroke::Press(MASK), &mut send)
                .and_then(|()| self.send_unless_shortcut(Keystroke::Release(MASK), &mut send));
            if let Err(error) = masked {
                first.get_or_insert(error);
            }
        }
        for code in self.held.clone().into_iter().rev() {
            let key = VirtualKey {
                code,
                extended: keys::is_extended(code),
            };
            if let Err(error) = self.send_unless_shortcut(Keystroke::Release(key), &mut send) {
                first.get_or_insert(error);
            }
        }
        match first {
            None => Ok(()),
            Some(first) if self.held.is_empty() => Err(first),
            Some(first) => Err(Error::InputHeld {
                message: format!(
                    "{} still held after the release of everything: {first}",
                    self.held
                        .iter()
                        .map(|&code| keys::name_of(code))
                        .collect::<Vec<String>>()
                        .join(", ")
                ),
                retry: ErrorCode::InputHeld.default_retry(),
            }),
        }
    }

    /// Send `stroke` through `send` unless it completes a shortcut of the shell's from this state,
    /// and record it once Windows took it.
    fn send_unless_shortcut(
        &mut self,
        stroke: Keystroke,
        send: &mut impl FnMut(Keystroke) -> Result<()>,
    ) -> Result<()> {
        let mut next = self.clone();
        if let Some(handler) = next.apply(stroke) {
            return Err(Refusal::ShellHotkey { handler }.into_error());
        }
        send(stroke)?;
        *self = next;
        Ok(())
    }
}

/// The mouse buttons the lane holds, as `safety::ButtonBits`: physical buttons, 0 left to 4 the
/// second extra button.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeldButtons(u8);

impl HeldButtons {
    /// The buttons down, as bits.
    #[must_use]
    pub fn bits(self) -> u8 {
        self.0
    }

    /// Record a press (`pressed`) or release of physical button `button`, which Windows took.
    pub fn apply(&mut self, button: u8, pressed: bool) {
        if pressed {
            self.0 |= 1 << button;
        } else {
            self.0 &= !(1 << button);
        }
    }

    /// Send a press (`pressed`) or release of `button` through `send`, record it as soon as
    /// `send` says Windows took it, and only then `confirm`, as [`HeldKeys::deliver`] does.
    ///
    /// # Errors
    ///
    /// `send`'s failure, with nothing recorded; `confirm`'s, with the change recorded.
    pub fn deliver(
        &mut self,
        button: u8,
        pressed: bool,
        send: impl FnOnce() -> Result<()>,
        confirm: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        send()?;
        self.apply(button, pressed);
        confirm()
    }

    /// Release every button held through `release`; a button whose release fails stays held.
    ///
    /// # Errors
    ///
    /// The first release that failed, after every button was tried.
    pub fn release_all(&mut self, mut release: impl FnMut(u8) -> Result<()>) -> Result<()> {
        let mut first: Option<Error> = None;
        for button in 0..5_u8 {
            if self.0 & (1 << button) == 0 {
                continue;
            }
            match release(button) {
                Ok(()) => self.apply(button, false),
                Err(error) => {
                    first.get_or_insert(error);
                }
            }
        }
        first.map_or(Ok(()), Err)
    }
}
