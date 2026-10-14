//! Which focus owned each held key, and the presentation's input modality.
//!
//! A key's repeats and its release belong to whoever received its key-down.
//! Enter that opens a page must not have its auto-repeats type into the field
//! the new page focuses, and a release whose press went to another window must
//! not activate anything here. The ledger records, per held key, the traversal
//! unit that had primary focus at key-down; a repeat or release reaches the
//! focus walk only while that unit still has it.

use std::collections::HashMap;

use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, NamedKey,
};

use crate::ids::FocusNodeId;

/// Whether focus should be shown: the last input that moved or used focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InputModality {
    /// A pointer press came last; focus moved by it is not indicated.
    Pointer,
    /// A key, traversal or assistive technology came last.
    Keyboard,
}

/// One held key. A physical key is its [`Code`]; a key the platform could not
/// place physically is named by its meaning instead. The pointer's back
/// button, delivered as a `BrowserBack` key, is held apart from any keyboard
/// key, so it neither ends nor is ended by a keyboard's own Back key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum KeyId {
    Physical(Code),
    Logical(Key),
    PointerBack,
}

impl KeyId {
    /// The keyboard key `event` names.
    pub(super) fn of(event: &KeyEvent) -> Self {
        if event.code == Code::Unidentified {
            Self::Logical(event.key.clone())
        } else {
            Self::Physical(event.code)
        }
    }
}

/// Who received a held key's key-down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyOrigin {
    /// An input method took it; none of the key's events reach the walk.
    Ime,
    /// The traversal unit holding primary focus at key-down, if any.
    Focus(Option<FocusNodeId>),
}

/// What the ledger lets through to the focus walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Admission {
    /// A first key-down: deliver, and it decides the input modality.
    Press,
    /// A repeat or release of a key whose key-down this unit received.
    Continuation,
    /// An input method's key, an orphan, or a key whose key-down went to
    /// another unit: global handlers saw it, the walk does not.
    Withheld,
}

#[derive(Debug, Default)]
pub(super) struct KeyLedger {
    pressed: HashMap<KeyId, KeyOrigin>,
}

impl KeyLedger {
    /// Record a key-down of the key `id`, or check a repeat or release
    /// against its key-down. `unit` is the traversal unit holding primary focus now. A release
    /// clears the key's record whatever it admits.
    pub(super) fn admit(
        &mut self,
        id: KeyId,
        event: &KeyEvent,
        unit: Option<FocusNodeId>,
    ) -> Admission {
        match (event.state(), event.repeat()) {
            (KeyState::Down, KeyRepeat::First) => {
                let ime = event.composition == ImeComposition::Active
                    || event.key == Key::Named(NamedKey::Process);
                let origin = if ime {
                    KeyOrigin::Ime
                } else {
                    KeyOrigin::Focus(unit)
                };
                self.pressed.insert(id, origin);
                if ime {
                    Admission::Withheld
                } else {
                    Admission::Press
                }
            }
            (KeyState::Down, KeyRepeat::AutoRepeat) => {
                Self::continuation(self.pressed.get(&id).copied(), unit)
            }
            (KeyState::Up, _) => Self::continuation(self.pressed.remove(&id), unit),
        }
    }

    fn continuation(origin: Option<KeyOrigin>, unit: Option<FocusNodeId>) -> Admission {
        if origin == Some(KeyOrigin::Focus(unit)) {
            Admission::Continuation
        } else {
            Admission::Withheld
        }
    }

    /// The key-down or repeat `event` moved focus by traversal: the key's
    /// further repeats belong to the unit that now has it (a held Tab keeps
    /// moving). A release has cleared its record already.
    pub(super) fn rebind(&mut self, id: &KeyId, event: &KeyEvent, unit: Option<FocusNodeId>) {
        if event.state() == KeyState::Down
            && let Some(origin) = self.pressed.get_mut(id)
            && *origin != KeyOrigin::Ime
        {
            *origin = KeyOrigin::Focus(unit);
        }
    }

    /// Forget every held key: the window lost activation, and the releases
    /// will not arrive here.
    pub(super) fn clear(&mut self) {
        self.pressed.clear();
    }
}

/// Whether `key` only modifies other keys. Pressing one does not show focus:
/// Alt on its way to Alt+Tab, or Shift before a click, says nothing about how
/// the user is moving focus.
pub(super) fn is_modifier(key: &Key) -> bool {
    matches!(
        key,
        Key::Named(
            NamedKey::Alt
                | NamedKey::AltGraph
                | NamedKey::CapsLock
                | NamedKey::Control
                | NamedKey::Fn
                | NamedKey::FnLock
                | NamedKey::Meta
                | NamedKey::NumLock
                | NamedKey::ScrollLock
                | NamedKey::Shift
                | NamedKey::Symbol
                | NamedKey::SymbolLock
        )
    )
}
