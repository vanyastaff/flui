//! What the Windows lane holds down, and the one path every key goes through: a key event and
//! typed text alike become [`Keystroke`]s planned against [`HeldKeys`], which the safety check
//! reads before a sequence and the lane sends stroke by stroke. A held key or button is recorded
//! only once Windows took its press, and forgotten only once Windows took its release, so a
//! release Windows refused stays held for the next [`HeldKeys::release_all`] to try again.

use crate::error::{Error, Result};
use crate::lane::windows::keys::{self, SHIFT, TextStroke, VirtualKey};
use crate::lane::windows::safety::KeyEdge;

/// `VK_PACKET`, the key a Unicode packet arrives as.
pub const PACKET: VirtualKey = VirtualKey {
    code: 0xE7,
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

/// The keys the lane holds: see the [module docs](self).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeldKeys {
    /// The keys down, by virtual-key code, in the order they went down.
    held: Vec<u16>,
    /// The keys pressed with a Shift the lane added for them.
    shifted: Vec<u16>,
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

    /// The strokes `input` takes from this state.
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

    /// Record `stroke`, which Windows took.
    pub fn apply(&mut self, stroke: Keystroke) {
        match stroke {
            Keystroke::Press(key) => {
                if !self.held.contains(&key.code) {
                    self.held.push(key.code);
                }
            }
            Keystroke::Release(key) => self.held.retain(|code| *code != key.code),
            Keystroke::ShiftFor(code) => {
                if !self.held.contains(&SHIFT.code) {
                    self.held.push(SHIFT.code);
                }
                self.shifted.push(code);
            }
            Keystroke::UnshiftFor(code) => {
                self.shifted.retain(|shifted| *shifted != code);
                if self.shifted.is_empty() {
                    self.held.retain(|held| *held != SHIFT.code);
                }
            }
            Keystroke::Unit(_) => {}
        }
    }

    /// The key edges of `inputs` sent one after another from this state: what the shell-hotkey
    /// rule reads before the sequence's first event.
    #[must_use]
    pub fn sequence_edges(&self, inputs: &[KeyInput]) -> Vec<KeyEdge> {
        let mut state = self.clone();
        let mut edges = Vec::new();
        for input in inputs {
            for stroke in state.plan(input) {
                edges.extend(stroke.edges());
                state.apply(stroke);
            }
        }
        edges
    }

    /// Release every key held, last pressed first, through `release`; a key whose release fails
    /// stays held.
    ///
    /// # Errors
    ///
    /// The first release that failed, after every key was tried.
    pub fn release_all(&mut self, mut release: impl FnMut(VirtualKey) -> Result<()>) -> Result<()> {
        let mut first: Option<Error> = None;
        for code in self.held.clone().into_iter().rev() {
            let key = VirtualKey {
                code,
                extended: keys::is_extended(code),
            };
            match release(key) {
                Ok(()) => {
                    self.held.retain(|held| *held != code);
                    self.shifted.retain(|shifted| *shifted != code);
                }
                Err(error) => {
                    first.get_or_insert(error);
                }
            }
        }
        if !self.held.contains(&SHIFT.code) {
            self.shifted.clear();
        }
        first.map_or(Ok(()), Err)
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
