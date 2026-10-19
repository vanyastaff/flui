//! Which focus owned each held key, and the presentation's input modality.
//!
//! A key's repeats and its release belong to whoever received its key-down.
//! Enter that opens a page must not have its auto-repeats type into the field
//! the new page focuses, and a release whose press went to another window must
//! not activate anything here. The ledger records, per held key, the traversal
//! unit that had primary focus at key-down; a repeat or release reaches the
//! focus walk only while that unit still has it. A release that arrives after
//! focus moved to another unit is no key event anywhere: the path that heard
//! the press is told its key was cancelled instead, so what the press began
//! (a keyboard scroll) ends without anything activating.

use std::collections::HashMap;
use std::rc::{Rc, Weak};

use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, NamedKey,
};

use super::FocusNode;
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
#[derive(Debug, Clone)]
enum KeyOrigin {
    /// An input method took it; none of the key's events reach the walk.
    Ime,
    /// The focus walk took it.
    Focus {
        /// The traversal unit holding primary focus at key-down, if any.
        unit: Option<FocusNodeId>,
        /// The press, as its path is told of it when the key is cancelled.
        press: HeldPress,
    },
}

/// A key-down the focus walk took, and where that walk started.
#[derive(Debug, Clone)]
pub(super) struct HeldPress {
    /// The node whose path heard the press.
    pub(super) leaf: Option<Weak<FocusNode>>,
    /// The key-down itself.
    pub(super) event: KeyEvent,
}

/// What the ledger lets through to the focus walk.
#[derive(Debug)]
pub(super) enum Admission {
    /// A first key-down: deliver, and it decides the input modality.
    Press,
    /// A repeat or release of a key whose key-down this unit received.
    Continuation,
    /// The release of a key whose key-down another unit's walk took, after
    /// focus moved: the release reaches no key handler, and the press's path
    /// is told the key was cancelled.
    Cancelled(HeldPress),
    /// An input method's key, an orphan, or a repeat of a key whose key-down
    /// went to another unit: global handlers saw it, the walk does not.
    Withheld,
}

/// A first key-down of a key whose earlier press, another unit's walk took,
/// never had its release. The earlier press is forgotten and nothing is
/// recorded for the new one: the earlier press's path is told the key was
/// cancelled, since no release reaches it now, and the key-down is then
/// admitted again against the focus that leaves.
#[derive(Debug)]
pub(super) struct Superseded(pub(super) HeldPress);

#[derive(Debug, Default)]
pub(super) struct KeyLedger {
    pressed: HashMap<KeyId, KeyOrigin>,
}

impl KeyLedger {
    /// Record a key-down of the key `id`, or check a repeat or release
    /// against its key-down. `unit` is the traversal unit holding primary
    /// focus now, and `leaf` the node a walk would start at. A release clears
    /// the key's record whatever it admits.
    ///
    /// # Errors
    ///
    /// [`Superseded`] for a first key-down that replaces an unreleased press
    /// of another unit: nothing is admitted until that press is cancelled.
    pub(super) fn admit(
        &mut self,
        id: KeyId,
        event: &KeyEvent,
        unit: Option<FocusNodeId>,
        leaf: Option<&Rc<FocusNode>>,
    ) -> Result<Admission, Superseded> {
        let admission = match (event.state(), event.repeat()) {
            (KeyState::Down, KeyRepeat::First) => {
                if let Some(KeyOrigin::Focus { unit: pressed, .. }) = self.pressed.get(&id)
                    && *pressed != unit
                    && let Some(KeyOrigin::Focus { press, .. }) = self.pressed.remove(&id)
                {
                    return Err(Superseded(press));
                }
                let ime = event.composition == ImeComposition::Active
                    || event.key == Key::Named(NamedKey::Process);
                let origin = if ime {
                    KeyOrigin::Ime
                } else {
                    KeyOrigin::Focus {
                        unit,
                        press: HeldPress {
                            leaf: leaf.map(Rc::downgrade),
                            event: event.clone(),
                        },
                    }
                };
                self.pressed.insert(id, origin);
                if ime {
                    Admission::Withheld
                } else {
                    Admission::Press
                }
            }
            (KeyState::Down, KeyRepeat::AutoRepeat) => match self.pressed.get(&id) {
                Some(KeyOrigin::Focus { unit: pressed, .. }) if *pressed == unit => {
                    Admission::Continuation
                }
                _ => Admission::Withheld,
            },
            (KeyState::Up, _) => match self.pressed.remove(&id) {
                Some(KeyOrigin::Focus { unit: pressed, .. }) if pressed == unit => {
                    Admission::Continuation
                }
                Some(KeyOrigin::Focus { press, .. }) => Admission::Cancelled(press),
                _ => Admission::Withheld,
            },
        };
        Ok(admission)
    }

    /// The key-down or repeat `event` moved focus by traversal: the key's
    /// further repeats belong to the unit that now has it (a held Tab keeps
    /// moving), walking from `leaf`. A release has cleared its record already.
    pub(super) fn rebind(
        &mut self,
        id: &KeyId,
        event: &KeyEvent,
        unit: Option<FocusNodeId>,
        leaf: Option<&Rc<FocusNode>>,
    ) {
        if event.state() == KeyState::Down
            && let Some(KeyOrigin::Focus {
                unit: pressed,
                press,
            }) = self.pressed.get_mut(id)
        {
            *pressed = unit;
            press.leaf = leaf.map(Rc::downgrade);
        }
    }

    /// Forget every held key: the window lost activation, or its focus owner
    /// closed, and the releases will not arrive here. Returns the presses the
    /// focus walk took, whose paths are owed a cancellation.
    pub(super) fn forget(&mut self) -> Vec<HeldPress> {
        self.pressed
            .drain()
            .filter_map(|(_, origin)| match origin {
                KeyOrigin::Focus { press, .. } => Some(press),
                KeyOrigin::Ime => None,
            })
            .collect()
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
