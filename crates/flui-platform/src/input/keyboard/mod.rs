//! FLUI's keyboard event vocabulary (ADR-0089 §4, ADR-0143).
//!
//! A backend turns each native key message into one [`KeyEvent`]: what the key means under the
//! current layout ([`Key`]), which physical key it was ([`Code`], [`Location`]), the modifiers
//! reported active (including toggles), whether it is an auto-repeat and whether an input
//! method was composing. [`NamedKey`]
//! and [`Code`] are generated from the W3C value lists by `cargo xtask key-vocabulary`, so a key
//! the specification adds after the last regeneration arrives as `Unidentified`, a documented
//! loss rather than a silent one. Text input does not travel here: an input method edits the
//! focused field's [`TextStore`](crate::text::text_store::TextStore) (ADR-0090).

#[rustfmt::skip]
mod code;
mod modifiers;
#[rustfmt::skip]
mod named_key;

pub use code::Code;
pub use modifiers::Modifiers;
pub use named_key::NamedKey;

use crate::EventTime;

/// What a key means under the active layout: the W3C `key` value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// The key produces this text, never empty (usually one grapheme cluster; some layouts
    /// produce several characters for one key). Built through [`Key::character`], which reports
    /// a key that produces no text as [`NamedKey::Unidentified`].
    Character(KeyText),
    /// A key the W3C names, such as Enter or an arrow.
    Named(NamedKey),
    /// A dead key: it types nothing itself and changes what the next key types (the W3C
    /// `"Dead"` value). It carries the accent it leaves pending, as the layout prints it on the
    /// key (`´`, `^`, `¨`, not the combining mark), or `None` where the platform does not say.
    /// Its release reports the same value as its press.
    Dead(Option<char>),
}

impl Key {
    /// The key whose text is `text`, or [`NamedKey::Unidentified`] for an empty string.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_platform::input::keyboard::{Key, NamedKey};
    ///
    /// assert!(matches!(Key::character("é"), Key::Character(text) if text.as_str() == "é"));
    /// assert_eq!(Key::character(""), Key::Named(NamedKey::Unidentified));
    /// ```
    #[must_use]
    pub fn character(text: impl Into<String>) -> Self {
        let text = text.into();
        if text.is_empty() {
            Self::Named(NamedKey::Unidentified)
        } else {
            Self::Character(KeyText(text))
        }
    }
}

/// The text a [`Key::Character`] produces: never empty, so the variant cannot be built with
/// an invalid payload. Only [`Key::character`] makes one:
///
/// ```compile_fail
/// use flui_platform::input::keyboard::Key;
///
/// let _ = Key::Character(String::new());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyText(String);

impl KeyText {
    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for KeyText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<NamedKey> for Key {
    fn from(key: NamedKey) -> Self {
        Self::Named(key)
    }
}

/// Whether the key went down or came up, and what kind of key-down it was.
///
/// Only a key-down repeats or lacks a release, so a release carries neither: an auto-repeated
/// release cannot be built.
///
/// ```compile_fail
/// use flui_platform::input::keyboard::{KeyPress, KeyState};
///
/// let _ = KeyState::Up(KeyPress::AutoRepeat);
/// ```
///
/// A consumer that asks only whether the key is down matches `KeyState::Down(_)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(clippy::exhaustive_enums, reason = "a key goes down or comes up")]
pub enum KeyState {
    /// The key went down: pressed, auto-repeated while held, or typed with no key to release.
    Down(KeyPress),
    /// The key was released.
    Up,
}

/// What kind of key-down a [`KeyState::Down`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a key-down is the first press, a repeat of it, or a press with no key behind it"
)]
pub enum KeyPress {
    /// The key was just pressed; its repeats, if any, and its release follow.
    First,
    /// The platform repeated a key that is still held; its release follows.
    AutoRepeat,
    /// A key-down no repeat or release follows. A host that has a character but no key for it
    /// (Windows' Alt+numpad entry, which posts the character alone) reports one: nothing holds
    /// that key, so nothing waits for its release.
    Unpaired,
}

/// Which of several keys with the same meaning was used: the W3C `location` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the W3C `KeyboardEvent.location` values, which no platform extends"
)]
pub enum Location {
    /// The only key with this meaning, or a location the platform does not report.
    #[default]
    Standard,
    /// The left-hand key of a pair, such as the left Shift.
    Left,
    /// The right-hand key of a pair, such as the right Shift.
    Right,
    /// A key on the numeric keypad.
    Numpad,
}

/// Whether an input method was composing text when a key changed.
///
/// A shortcut must not fire on a key that arrives during a composition: the input method owns
/// it (Enter commits the composition, it does not submit a form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[expect(
    clippy::exhaustive_enums,
    reason = "an input method was composing or it was not"
)]
pub enum ImeComposition {
    /// No composition was in progress.
    #[default]
    Inactive,
    /// An input method was composing.
    Active,
}

/// One key press, repeat or release.
///
/// Built with [`KeyEvent::new`] and the `with_*` methods.
///
/// A key a host can't place physically has [`Code::Unidentified`] and is known by its
/// [`key`](Self::key) alone, so a host gives such a key's repeats and release the `key` it
/// reported at the key-down, even when a modifier changed the key's meaning while it was held.
/// A host that has no release to report marks the key-down [`KeyPress::Unpaired`].
///
/// # Examples
///
/// ```
/// use flui_platform::EventTime;
/// use flui_platform::input::keyboard::{
///     Code, ImeComposition, Key, KeyEvent, KeyPress, KeyState, Location, Modifiers, NamedKey,
/// };
///
/// let enter = KeyEvent::new(
///     KeyState::Down(KeyPress::AutoRepeat),
///     Key::Named(NamedKey::Enter),
///     Code::NumpadEnter,
///     EventTime::from_nanos(7),
/// )
/// .with_location(Location::Numpad)
/// .with_modifiers(Modifiers::CONTROL);
/// assert!(matches!(enter.state, KeyState::Down(_)));
/// assert_eq!(enter.composition, ImeComposition::Inactive);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct KeyEvent {
    /// Pressed, repeated or released.
    pub state: KeyState,
    /// What the key means under the active layout.
    pub key: Key,
    /// The physical key.
    pub code: Code,
    /// Which of several same-meaning keys was used.
    pub location: Location,
    /// Modifier/toggle states reported active when the key changed, including
    /// a modifier key's own change. Source facts are not command interpretation.
    pub modifiers: Modifiers,
    /// Whether an input method was composing when the key changed.
    pub composition: ImeComposition,
    /// When the platform observed the change.
    pub time: EventTime,
}

impl KeyEvent {
    /// A `state` change of `key` with no modifiers, at the standard location, outside any
    /// composition.
    #[must_use]
    pub const fn new(state: KeyState, key: Key, code: Code, time: EventTime) -> Self {
        Self {
            state,
            key,
            code,
            location: Location::Standard,
            modifiers: Modifiers::NONE,
            composition: ImeComposition::Inactive,
            time,
        }
    }

    /// This event at `location`.
    #[must_use]
    pub fn with_location(self, location: Location) -> Self {
        Self { location, ..self }
    }

    /// This event with `modifiers` held.
    #[must_use]
    pub fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    /// This event arriving during or outside an input method's `composition`.
    #[must_use]
    pub fn with_composition(self, composition: ImeComposition) -> Self {
        Self {
            composition,
            ..self
        }
    }
}
