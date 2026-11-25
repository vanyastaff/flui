//! What the macOS lane holds down, and the one path every key goes through: a key event and typed
//! text alike become [`Keystroke`]s planned against [`HeldKeys`], which the shortcut check reads
//! before a sequence and the lane posts stroke by stroke.
//!
//! A held key is recorded only once its event was posted, and forgotten only once its release
//! was. Quartz says nothing back about a posted event, so "posted" is the last thing the lane
//! knows: an event that could not be made is not posted, and its key keeps its state.
//!
//! Every event the lane posts carries the modifiers it holds as its flags ([`HeldKeys::flags`]),
//! and a modifier goes out as a flags-changed event with the flags it leaves: the app reads a
//! chord from the flags, and the lane's private event source keeps no modifier state the lane did
//! not set itself.

use crate::error::{Error, Result};
use crate::lane::macos::keys::{self, KeyCode, Modifier, SHIFT, TextChunk, TextStroke};
use crate::lane::macos::safety::{KeyEdge, LANE, ModifierTaps, Refusal};
use crate::lane::{Delivered, OsInput, SendFailure};

/// A key event, its key resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyInput {
    /// Press `key`, with Shift held around it when `shift`, typing `character` if it types one.
    Down {
        /// The key.
        key: KeyCode,
        /// Whether it needs Shift.
        shift: bool,
        /// What it types, which the event carries as its text.
        character: Option<char>,
    },
    /// Release `key`, which typed `character` when it went down.
    Up {
        /// The key.
        key: KeyCode,
        /// What it typed.
        character: Option<char>,
    },
    /// Type `text`.
    Text(String),
}

/// One thing the lane posts to the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Keystroke {
    /// Press a key, carrying the character it types.
    Press(KeyCode, Option<char>),
    /// Release a key.
    Release(KeyCode, Option<char>),
    /// Press Shift for the key with this code, which needs it.
    ShiftFor(u16),
    /// Release the Shift pressed for the key with this code.
    UnshiftFor(u16),
    /// A run of text, pressed and released as one key event carrying it.
    Text(TextChunk),
}

impl OsInput for Keystroke {
    /// Every stroke is a key event the app may see.
    fn acts(&self) -> bool {
        true
    }
}

/// The key code a text event carries: `0`. An app takes typed text from the event's text, not
/// from its key, and no shortcut of the system's takes key `0` with any modifiers; not the space
/// bar, which activates a focused button whatever text the event carries.
pub const TEXT_KEY: KeyCode = KeyCode(0x00);

impl Keystroke {
    /// The key edges the keyboard sees for this stroke, as the shortcut rule reads them.
    #[must_use]
    pub fn edges(self) -> Vec<KeyEdge> {
        match self {
            Self::Press(key, _) => vec![KeyEdge::Down(key)],
            Self::Release(key, _) => vec![KeyEdge::Up(key)],
            Self::ShiftFor(_) => vec![KeyEdge::Down(SHIFT)],
            Self::UnshiftFor(_) => vec![KeyEdge::Up(SHIFT)],
            Self::Text(_) => vec![KeyEdge::Down(TEXT_KEY), KeyEdge::Up(TEXT_KEY)],
        }
    }
}

/// The keys the lane holds, and the counts the system keeps across them for its shortcuts: see
/// the [module docs](self). The one owner of both: every key edge the lane posted goes through
/// [`Self::apply`], a release of everything included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeldKeys {
    /// The keys down, by key code, in the order they went down.
    held: Vec<u16>,
    /// The keys pressed with a Shift the lane added for them.
    shifted: Vec<u16>,
    /// The system's tap shortcuts as the lane's keys have fed them.
    taps: ModifierTaps,
}

impl HeldKeys {
    /// No key held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys down, by key code.
    #[must_use]
    pub fn held(&self) -> &[u16] {
        &self.held
    }

    /// The `CGEventFlags` of the modifiers held now.
    #[must_use]
    pub fn flags(&self) -> u64 {
        flags_of(&self.held)
    }

    /// The flags an event posting `stroke` carries: those the stroke leaves, so a modifier's own
    /// press carries its flag and its release does not.
    #[must_use]
    pub fn flags_after(&self, stroke: Keystroke) -> u64 {
        let mut next = self.clone();
        let _ = next.apply(stroke);
        next.flags()
    }

    /// The strokes `input` takes from this state.
    #[must_use]
    pub fn plan(&self, input: &KeyInput) -> Vec<Keystroke> {
        match input {
            KeyInput::Down {
                key,
                shift,
                character,
            } => {
                let held_shift = self
                    .held
                    .iter()
                    .any(|&code| Modifier::of(KeyCode(code)) == Some(Modifier::Shift));
                if *shift && !held_shift {
                    vec![
                        Keystroke::ShiftFor(key.0),
                        Keystroke::Press(*key, *character),
                    ]
                } else {
                    vec![Keystroke::Press(*key, *character)]
                }
            }
            KeyInput::Up { key, character } => {
                let mut strokes = vec![Keystroke::Release(*key, *character)];
                if self.shifted.contains(&key.0) {
                    strokes.push(Keystroke::UnshiftFor(key.0));
                }
                strokes
            }
            KeyInput::Text(text) => keys::text_strokes(text)
                .into_iter()
                .flat_map(|stroke| match stroke {
                    TextStroke::Key(key) => {
                        vec![Keystroke::Press(key, None), Keystroke::Release(key, None)]
                    }
                    TextStroke::Text(chunk) => vec![Keystroke::Text(chunk)],
                })
                .collect(),
        }
    }

    /// Record `stroke`, which the lane posted; what of the system's it completed, if anything.
    pub fn apply(&mut self, stroke: Keystroke) -> Option<&'static str> {
        let mut completed = None;
        for edge in stroke.edges() {
            let taken = match edge {
                KeyEdge::Down(key) => {
                    let taken = self.taps.press(key, &self.held);
                    if !matches!(stroke, Keystroke::Text(_)) && !self.held.contains(&key.0) {
                        self.held.push(key.0);
                    }
                    taken
                }
                KeyEdge::Up(key) => {
                    if !matches!(stroke, Keystroke::Text(_)) {
                        self.held.retain(|code| *code != key.0);
                    }
                    self.taps.release(key)
                }
            };
            completed = completed.or(taken);
        }
        match stroke {
            Keystroke::ShiftFor(code) => self.shifted.push(code),
            Keystroke::UnshiftFor(code) | Keystroke::Release(KeyCode(code), _) => {
                self.shifted.retain(|shifted| *shifted != code);
            }
            _ => {}
        }
        if !self.held.contains(&SHIFT.0) {
            self.shifted.clear();
        }
        completed
    }

    /// Post `stroke` through `post`, which takes the flags the event carries, and record it and
    /// say it in `delivered` once `post` says it went out. A stroke posted in part (a text run's
    /// press without its release) is said and not recorded: it holds nothing.
    ///
    /// # Errors
    ///
    /// `post`'s failure, with nothing recorded.
    pub fn deliver(
        &mut self,
        stroke: Keystroke,
        delivered: &mut Delivered,
        post: impl FnOnce(Keystroke, u64) -> std::result::Result<(), SendFailure>,
    ) -> Result<()> {
        let flags = self.flags_after(stroke);
        delivered.send(&stroke, || post(stroke, flags))?;
        let _ = self.apply(stroke);
        Ok(())
    }

    /// What of the system's `inputs`, sent from this state, would take: see
    /// [`safety::press_takes`](crate::lane::macos::safety::press_takes) and [`ModifierTaps`]. A
    /// sequence that leaves a modifier down alone whose release, by a later sequence or by an
    /// interrupted run's release of everything, would complete a tap shortcut is refused too.
    #[must_use]
    pub fn system_shortcut(&self, inputs: &[KeyInput]) -> Option<&'static str> {
        let mut state = self.clone();
        for input in inputs {
            for stroke in state.plan(input) {
                if let Some(taken) = state.apply(stroke) {
                    return Some(taken);
                }
            }
        }
        state.taps.pending()
    }

    /// Release every key held, last pressed first, through `post`. A release that would complete
    /// a tap shortcut of the system's is not posted, and that key stays held; so does a key whose
    /// release could not be posted. The Windows lane masks such a release with an unassigned
    /// virtual key instead; no key code is unassigned on every Mac keyboard, so this lane refuses
    /// it.
    ///
    /// # Errors
    ///
    /// After every key was tried: [`Error::ReleaseFailed`] naming the keys still held, every one
    /// whose release failed or was refused, and the first such stroke's error.
    pub fn release_all(
        &mut self,
        mut post: impl FnMut(Keystroke, u64) -> Result<()>,
    ) -> Result<()> {
        let mut first: Option<Error> = None;
        for code in self.held.clone().into_iter().rev() {
            let stroke = Keystroke::Release(KeyCode(code), None);
            let mut next = self.clone();
            if let Some(handler) = next.apply(stroke) {
                first.get_or_insert(Refusal::SystemShortcut { handler }.into_error());
                continue;
            }
            match post(stroke, next.flags()) {
                Ok(()) => *self = next,
                Err(error) => {
                    first.get_or_insert(error);
                }
            }
        }
        match first {
            None => Ok(()),
            Some(first) => Err(Error::ReleaseFailed {
                lane: LANE.to_owned(),
                held: self
                    .held
                    .iter()
                    .map(|&code| keys::name_of(code))
                    .collect::<Vec<String>>(),
                message: first.to_string(),
                first: None,
            }),
        }
    }
}

/// The `CGEventFlags` of the modifiers among `held`.
fn flags_of(held: &[u16]) -> u64 {
    held.iter()
        .filter_map(|&code| Modifier::of(KeyCode(code)))
        .fold(0, |flags, modifier| flags | modifier.flag())
}
