//! FLUI's keyboard event vocabulary (ADR-0089 §4, ADR-0143).
//!
//! A backend turns each native key message into one [`KeyEvent`]: what the key means under the
//! current layout ([`Key`]), which physical key it was ([`Code`], [`Location`]), the modifiers
//! held, whether it is an auto-repeat and whether an input method was composing. [`NamedKey`]
//! and [`Code`] are generated from the W3C value lists by `cargo xtask key-vocabulary`, so a key
//! the specification adds after the last regeneration arrives as `Unidentified`, a documented
//! loss rather than a silent one. Text input does not travel here: an input method edits the
//! focused field's [`TextStore`](crate::TextStore) (ADR-0090).

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
    /// A key the W3C names, such as Enter, an arrow or a dead key.
    Named(NamedKey),
}

impl Key {
    /// The key whose text is `text`, or [`NamedKey::Unidentified`] for an empty string.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_platform_api::keyboard::{Key, NamedKey};
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
/// an invalid payload.
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

/// Whether the key went down or came up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyState {
    /// The key was pressed, or auto-repeated while held ([`KeyEvent::repeat`]).
    Down,
    /// The key was released.
    Up,
}

/// Which of several keys with the same meaning was used: the W3C `location` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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

/// Whether a key-down is the first press or an auto-repeat of a held key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyRepeat {
    /// The key was just pressed, or released.
    #[default]
    First,
    /// The platform repeated a key that is still held.
    AutoRepeat,
}

/// Whether an input method was composing text when a key changed.
///
/// A shortcut must not fire on a key that arrives during a composition: the input method owns
/// it (Enter commits the composition, it does not submit a form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
/// # Examples
///
/// ```
/// use flui_platform_api::EventTime;
/// use flui_platform_api::keyboard::{
///     Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
/// };
///
/// let enter = KeyEvent::new(KeyState::Down, Key::Named(NamedKey::Enter), Code::NumpadEnter, EventTime::from_nanos(7))
///     .with_location(Location::Numpad)
///     .with_modifiers(Modifiers::CONTROL)
///     .with_repeat(KeyRepeat::AutoRepeat);
/// assert_eq!(enter.repeat, KeyRepeat::AutoRepeat);
/// assert_eq!(enter.composition, ImeComposition::Inactive);
///
/// // A release is never a repeat.
/// let release = KeyEvent::new(KeyState::Up, Key::character("a"), Code::KeyA, EventTime::from_nanos(8))
///     .with_repeat(KeyRepeat::AutoRepeat);
/// assert_eq!(release.repeat, KeyRepeat::First);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct KeyEvent {
    /// Pressed or released.
    pub state: KeyState,
    /// What the key means under the active layout.
    pub key: Key,
    /// The physical key.
    pub code: Code,
    /// Which of several same-meaning keys was used.
    pub location: Location,
    /// The modifiers held when the key changed, including a modifier key's own change.
    pub modifiers: Modifiers,
    /// Whether this is an auto-repeat of a held key; always [`KeyRepeat::First`] on a release.
    pub repeat: KeyRepeat,
    /// Whether an input method was composing when the key changed.
    pub composition: ImeComposition,
    /// When the platform observed the change.
    pub time: EventTime,
}

impl KeyEvent {
    /// A first press or a release with no modifiers, at the standard location, outside any
    /// composition.
    #[must_use]
    pub const fn new(state: KeyState, key: Key, code: Code, time: EventTime) -> Self {
        Self {
            state,
            key,
            code,
            location: Location::Standard,
            modifiers: Modifiers::NONE,
            repeat: KeyRepeat::First,
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

    /// This event as `repeat`. A release is never a repeat, so a [`KeyState::Up`] event stays
    /// [`KeyRepeat::First`].
    #[must_use]
    pub fn with_repeat(self, repeat: KeyRepeat) -> Self {
        let repeat = match self.state {
            KeyState::Down => repeat,
            KeyState::Up => KeyRepeat::First,
        };
        Self { repeat, ..self }
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
