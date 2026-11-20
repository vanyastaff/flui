//! What the X11 lane holds down and has mapped, and the one plan every key goes through.
//!
//! A key event and typed text become [`Keystroke`]s planned against the server's [`Keymap`] and
//! what the lane holds ([`HeldInput::plan`]): a key at the keymap's first level is pressed as it
//! is, one at the second level with Shift around it (or without the Shift the sequence holds, for
//! a key at the first level), and a character the keymap carries on no key through a spare key
//! mapped to it for the text. The shell-shortcut rule reads those strokes before a sequence
//! ([`HeldInput::shell_hotkey`]), and the lane sends them, reading the server again before each.
//!
//! [`HeldInput`] is the one owner of what the lane holds (keys, mapped keys) and of the count the
//! StickyKeys shortcut keeps across sends: a press is recorded once the server took it, before
//! the guard confirms its echo, and forgotten once the server took the release, so a press whose
//! echo never came and a refused release are both released again by [`HeldInput::release_all`],
//! which also gives every mapped key back its empty mapping.
//!
//! A mapped key is mapped before the text's first stroke and emptied after its last, each with a
//! pause of [`MAP_SETTLE`] between: an app reads its keymap again when the server tells it the
//! mapping changed, and a key pressed before it has, or emptied before it has read the first
//! change, would type nothing.

use std::time::Duration;

use crate::lane::x11::keys::{self, Keysym, Modifier, NO_SYMBOL, SHIFT_L};
use crate::lane::x11::safety::{Keycode, Refusal, ShellShortcuts};

/// How long the lane waits after it maps or empties a key before the next stroke.
pub const MAP_SETTLE: Duration = Duration::from_millis(50);

/// The server's core keymap: the keysyms on each keycode, as `GetKeyboardMapping` answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    min_keycode: u8,
    per_keycode: u8,
    keysyms: Vec<Keysym>,
}

impl Keymap {
    /// The keymap whose first keycode is `min_keycode`, `per_keycode` keysyms for each, in
    /// `keysyms`; a last keycode with fewer keysyms than that is left out.
    #[must_use]
    pub fn new(min_keycode: u8, per_keycode: u8, keysyms: Vec<Keysym>) -> Self {
        Self {
            min_keycode,
            per_keycode,
            keysyms,
        }
    }

    /// How many keysyms each keycode carries.
    #[must_use]
    pub fn per_keycode(&self) -> u8 {
        self.per_keycode
    }

    fn keys(&self) -> impl Iterator<Item = (Keycode, &[Keysym])> {
        let per = usize::from(self.per_keycode.max(1));
        self.keysyms
            .chunks_exact(per)
            .zip(self.min_keycode..=u8::MAX)
            .map(|(syms, code)| (Keycode(code), syms))
    }

    /// The key typing `keysym` at the first level, or at the second (with Shift) when `true`;
    /// `None` when no key carries it. A key carrying a letter's lower case alone types its upper
    /// case with Shift, as the core protocol reads a keymap.
    #[must_use]
    pub fn place(&self, keysym: Keysym) -> Option<(Keycode, bool)> {
        if keysym == NO_SYMBOL {
            return None;
        }
        let first = |level: usize| {
            self.keys()
                .find(|(_, syms)| syms.get(level) == Some(&keysym))
                .map(|(code, _)| code)
        };
        if let Some(code) = first(0) {
            return Some((code, false));
        }
        if let Some(code) = first(1) {
            return Some((code, true));
        }
        let lower = lower_case(keysym)?;
        self.keys()
            .find(|(_, syms)| {
                syms.first() == Some(&lower) && syms.get(1).is_none_or(|sym| *sym == NO_SYMBOL)
            })
            .map(|(code, _)| (code, true))
    }

    /// The keysym on `code`'s first level; `NoSymbol` for a key with nothing on it.
    #[must_use]
    pub fn keysym(&self, code: Keycode) -> Keysym {
        self.keys()
            .find(|(known, _)| *known == code)
            .and_then(|(_, syms)| syms.first().copied())
            .unwrap_or(NO_SYMBOL)
    }

    /// The keys with nothing on them, from the highest keycode down: the keys the lane maps a
    /// character to.
    #[must_use]
    pub fn spare(&self) -> Vec<Keycode> {
        let mut spare: Vec<Keycode> = self
            .keys()
            .filter(|(_, syms)| syms.iter().all(|sym| *sym == NO_SYMBOL))
            .map(|(code, _)| code)
            .collect();
        spare.reverse();
        spare
    }
}

/// The lower case of an upper-case Latin-1 letter's keysym.
fn lower_case(keysym: Keysym) -> Option<Keysym> {
    match keysym.0 {
        0x41..=0x5A | 0xC0..=0xD6 | 0xD8..=0xDE => Some(Keysym(keysym.0 + 0x20)),
        _ => None,
    }
}

/// One key event or text, as the lane plans it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyInput {
    /// Press the key typing `keysym`, named `name` for a refusal.
    Down {
        /// The keysym.
        keysym: Keysym,
        /// The key's name, as the sequence gave it.
        name: String,
    },
    /// Release the key typing `keysym`.
    Up {
        /// The keysym.
        keysym: Keysym,
        /// The key's name, as the sequence gave it.
        name: String,
    },
    /// Type `text`.
    Text(String),
}

/// One request the lane sends for a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Keystroke {
    /// Press a key.
    Press(Keycode),
    /// Release a key.
    Release(Keycode),
    /// Map a spare key to `keysym`, on every level.
    Map {
        /// The key.
        key: Keycode,
        /// What it types.
        keysym: Keysym,
    },
    /// Give a mapped key back its empty mapping.
    Unmap(Keycode),
    /// Wait [`MAP_SETTLE`].
    Settle,
}

/// What the lane holds: see the [module docs](self).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeldInput {
    /// The keys pressed, oldest first, with the keysym each was pressed for.
    keys: Vec<(Keycode, Keysym)>,
    /// The spare keys mapped, with what each types.
    mapped: Vec<(Keycode, Keysym)>,
    shortcuts: ShellShortcuts,
}

impl HeldInput {
    /// Nothing held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys held, oldest first.
    #[must_use]
    pub fn keys(&self) -> Vec<Keycode> {
        self.keys.iter().map(|(code, _)| *code).collect()
    }

    /// The keys mapped to a character.
    #[must_use]
    pub fn mapped(&self) -> Vec<Keycode> {
        self.mapped.iter().map(|(code, _)| *code).collect()
    }

    fn holds(&self, code: Keycode) -> bool {
        self.keys.iter().any(|(held, _)| *held == code)
    }

    fn held_shift(&self) -> Option<Keycode> {
        self.keys
            .iter()
            .find(|(_, keysym)| Modifier::of(*keysym) == Some(Modifier::Shift))
            .map(|(code, _)| *code)
    }

    /// The strokes `input` takes on `keymap`, against what the lane holds now: see the
    /// [module docs](self).
    ///
    /// # Errors
    ///
    /// [`Refusal::UnknownKey`] for a named key no key carries; [`Refusal::UnsentInput`] for text
    /// with a control character no key types; [`Refusal::NoSpareKey`] when the keymap has too few
    /// keys free for the characters it carries on none.
    pub fn plan(&self, input: &KeyInput, keymap: &Keymap) -> Result<Vec<Keystroke>, Refusal> {
        let shift = keymap.place(SHIFT_L).map(|(code, _)| code);
        match input {
            KeyInput::Down { keysym, name } => {
                let (code, level) = keymap
                    .place(*keysym)
                    .ok_or_else(|| Refusal::UnknownKey { name: name.clone() })?;
                Ok(match (level, self.held_shift(), shift) {
                    (true, None, Some(shift)) => vec![
                        Keystroke::Press(shift),
                        Keystroke::Press(code),
                        Keystroke::Release(shift),
                    ],
                    _ => vec![Keystroke::Press(code)],
                })
            }
            KeyInput::Up { keysym, name } => {
                let held = self
                    .keys
                    .iter()
                    .find(|(_, held)| held == keysym)
                    .map(|(code, _)| *code);
                let code = match held {
                    Some(code) => code,
                    None => {
                        keymap
                            .place(*keysym)
                            .ok_or_else(|| Refusal::UnknownKey { name: name.clone() })?
                            .0
                    }
                };
                Ok(vec![Keystroke::Release(code)])
            }
            KeyInput::Text(text) => self.plan_text(text, keymap, shift),
        }
    }

    fn plan_text(
        &self,
        text: &str,
        keymap: &Keymap,
        shift: Option<Keycode>,
    ) -> Result<Vec<Keystroke>, Refusal> {
        let strokes = keys::text_strokes(text).map_err(|c| Refusal::UnsentInput {
            what: format!("the control character {c:?} in typed text"),
        })?;
        // The characters no key carries, each on a spare key of its own.
        let mut absent: Vec<Keysym> = Vec::new();
        for stroke in &strokes {
            let keysym = stroke.keysym();
            let reachable = keymap
                .place(keysym)
                .is_some_and(|(_, level)| !level || shift.is_some());
            if !reachable && !absent.contains(&keysym) {
                absent.push(keysym);
            }
        }
        let spare: Vec<Keycode> = keymap
            .spare()
            .into_iter()
            .filter(|code| !self.holds(*code))
            .collect();
        if absent.len() > spare.len() {
            let text: String = absent
                .iter()
                .filter_map(|keysym| {
                    let code = keysym.0 & !0x0100_0000;
                    char::from_u32(code)
                })
                .collect();
            return Err(Refusal::NoSpareKey { text });
        }
        let mapping: Vec<(Keycode, Keysym)> = spare.into_iter().zip(absent).collect();
        let mut plan = Vec::new();
        for (key, keysym) in &mapping {
            plan.push(Keystroke::Map {
                key: *key,
                keysym: *keysym,
            });
        }
        if !mapping.is_empty() {
            plan.push(Keystroke::Settle);
        }
        let held_shift = self.held_shift();
        for stroke in strokes {
            let keysym = stroke.keysym();
            if let Some((key, _)) = mapping.iter().find(|(_, mapped)| *mapped == keysym) {
                plan.extend([Keystroke::Press(*key), Keystroke::Release(*key)]);
                continue;
            }
            let (code, level) = keymap
                .place(keysym)
                .expect("BUG: a stroke not mapped to a spare key is on the keymap");
            match (level, held_shift, shift) {
                (true, None, Some(shift)) => plan.extend([
                    Keystroke::Press(shift),
                    Keystroke::Press(code),
                    Keystroke::Release(code),
                    Keystroke::Release(shift),
                ]),
                (false, Some(held), _) => plan.extend([
                    Keystroke::Release(held),
                    Keystroke::Press(code),
                    Keystroke::Release(code),
                    Keystroke::Press(held),
                ]),
                _ => plan.extend([Keystroke::Press(code), Keystroke::Release(code)]),
            }
        }
        if !mapping.is_empty() {
            plan.push(Keystroke::Settle);
            plan.extend(mapping.iter().map(|(key, _)| Keystroke::Unmap(*key)));
        }
        Ok(plan)
    }

    /// The keysym `stroke` presses or releases, as `keymap` and the lane's mapped keys say.
    fn keysym_of(&self, code: Keycode, keymap: &Keymap) -> Keysym {
        self.mapped
            .iter()
            .chain(&self.keys)
            .find(|(held, _)| *held == code)
            .map_or_else(|| keymap.keysym(code), |(_, keysym)| *keysym)
    }

    /// What the desktop takes of `inputs` planned on `keymap`, read after what the server has
    /// already seen of the lane's keys; `None` when nothing.
    ///
    /// # Errors
    ///
    /// As [`Self::plan`], for the first input that cannot be planned.
    pub fn shell_hotkey(
        &self,
        inputs: &[KeyInput],
        keymap: &Keymap,
    ) -> Result<Option<&'static str>, Refusal> {
        let mut trial = self.clone();
        for input in inputs {
            for stroke in trial.plan(input, keymap)? {
                let completed = trial.record(stroke, keymap);
                if completed.is_some() {
                    return Ok(completed);
                }
            }
        }
        Ok(None)
    }

    /// Record `stroke` as the server took it, feeding the shortcut count; what it completes.
    fn record(&mut self, stroke: Keystroke, keymap: &Keymap) -> Option<&'static str> {
        match stroke {
            Keystroke::Press(code) => {
                let keysym = self.keysym_of(code, keymap);
                let modifiers: Vec<Modifier> = self
                    .keys
                    .iter()
                    .filter_map(|(_, held)| Modifier::of(*held))
                    .collect();
                let alone = self.keys.is_empty();
                let completed = self.shortcuts.press(keysym, &modifiers, alone);
                if !self.holds(code) {
                    self.keys.push((code, keysym));
                }
                completed
            }
            Keystroke::Release(code) => {
                let keysym = self.keysym_of(code, keymap);
                self.keys.retain(|(held, _)| *held != code);
                self.shortcuts.release(keysym)
            }
            Keystroke::Map { key, keysym } => {
                self.mapped.retain(|(held, _)| *held != key);
                self.mapped.push((key, keysym));
                None
            }
            Keystroke::Unmap(key) => {
                self.mapped.retain(|(held, _)| *held != key);
                None
            }
            Keystroke::Settle => None,
        }
    }

    /// Send `stroke` through `send`, recording it once the server took it; then wait for its echo
    /// through `echo`. A press is held, and a mapping kept to be emptied, even when its echo never
    /// comes.
    ///
    /// # Errors
    ///
    /// What `send` or `echo` failed with.
    pub fn deliver(
        &mut self,
        stroke: Keystroke,
        keymap: &Keymap,
        send: impl FnOnce() -> crate::Result<()>,
        echo: impl FnOnce() -> crate::Result<()>,
    ) -> crate::Result<()> {
        send()?;
        self.record(stroke, keymap);
        if matches!(stroke, Keystroke::Press(_) | Keystroke::Release(_)) {
            echo()?;
        }
        Ok(())
    }

    /// Release every key held, the newest first, then empty every mapped key, each through
    /// `send`; a release or mapping the server refused stays held, to be sent again.
    ///
    /// # Errors
    ///
    /// The first refusal.
    pub fn release_all(
        &mut self,
        keymap: &Keymap,
        mut send: impl FnMut(Keystroke) -> crate::Result<()>,
    ) -> crate::Result<()> {
        let mut first = Ok(());
        // Taken apart from what they release, which each release changes.
        let strokes: Vec<Keystroke> = self
            .keys
            .iter()
            .rev()
            .map(|(code, _)| Keystroke::Release(*code))
            .chain(self.mapped.iter().map(|(code, _)| Keystroke::Unmap(*code)))
            .collect();
        for stroke in strokes {
            match send(stroke) {
                Ok(()) => {
                    self.record(stroke, keymap);
                }
                Err(error) => {
                    if first.is_ok() {
                        first = Err(error);
                    }
                }
            }
        }
        first
    }
}

/// The mouse buttons the lane holds, as bits: button `n` at bit `n - 1`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeldButtons(u16);

impl HeldButtons {
    /// The buttons held, as bits.
    #[must_use]
    pub fn bits(self) -> u16 {
        self.0
    }

    fn bit(button: u8) -> u16 {
        1_u16
            .checked_shl(u32::from(button.saturating_sub(1)))
            .unwrap_or(0)
    }

    /// Press (`pressed`) or release `button` through `send`, recording it once the server took
    /// it; then wait for its echo through `echo`.
    ///
    /// # Errors
    ///
    /// What `send` or `echo` failed with.
    pub fn deliver(
        &mut self,
        button: u8,
        pressed: bool,
        send: impl FnOnce() -> crate::Result<()>,
        echo: impl FnOnce() -> crate::Result<()>,
    ) -> crate::Result<()> {
        send()?;
        if pressed {
            self.0 |= Self::bit(button);
        } else {
            self.0 &= !Self::bit(button);
        }
        echo()
    }

    /// Release every button held through `send`; one the server refused stays held.
    ///
    /// # Errors
    ///
    /// The first refusal.
    pub fn release_all(
        &mut self,
        mut send: impl FnMut(u8) -> crate::Result<()>,
    ) -> crate::Result<()> {
        let mut first = Ok(());
        for button in 1..=16_u8 {
            if self.0 & Self::bit(button) == 0 {
                continue;
            }
            match send(button) {
                Ok(()) => self.0 &= !Self::bit(button),
                Err(error) => {
                    if first.is_ok() {
                        first = Err(error);
                    }
                }
            }
        }
        first
    }
}

/// How far one click of an X wheel button scrolls, in device pixels: the clicks a scroll of `n`
/// device pixels turns are `n` over this, at least one.
pub const WHEEL_STEP: i32 = 40;

/// The wheel button and the clicks a scroll of `delta` device pixels on one axis turns: buttons 4
/// (up) and 5 (down) vertically, 6 (left) and 7 (right) horizontally; `None` for no scroll.
#[must_use]
pub fn wheel_clicks(delta: i32, vertical: bool) -> Option<(u8, u32)> {
    if delta == 0 {
        return None;
    }
    let button = match (vertical, delta > 0) {
        (true, false) => 4,
        (true, true) => 5,
        (false, false) => 6,
        (false, true) => 7,
    };
    let clicks = delta
        .unsigned_abs()
        .div_ceil(WHEEL_STEP.unsigned_abs())
        .max(1);
    Some((button, clicks))
}
