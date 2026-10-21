//! The key rows: the keys a context menu's default bindings name arrive as
//! those logical keys (ADR-0216), and a key during a composition is the input
//! method's (ADR-0090).

use std::time::Duration;

use flui_platform::PlatformInput;
use flui_platform::keyboard::{ImeComposition, Key, KeyState, Modifiers, NamedKey};
use flui_runtime::HostInput;

use super::rows::Outcome;
use super::{BackendUnderTest, KeyStep, RowOutcome, Stamp, Step, StepKey};

const NO_KEYBOARD: RowOutcome =
    RowOutcome::NotApplicable("the backend has no shared keyboard decoder");

/// The menu key arrives as `ContextMenu` and Shift+F10 as `F10` with Shift
/// held, each pressed and released: what `DefaultFocusTraversal` binds to
/// `ShowContextMenuIntent`.
pub(super) fn menu_keys_arrive_as_the_bindings_name_them<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let presses = [
        (StepKey::Menu, false, NamedKey::ContextMenu, 10),
        (StepKey::F10, true, NamedKey::F10, 20),
    ];
    for (key, shift, expected, millis) in presses {
        let at = Stamp::coarse(Duration::from_millis(millis));
        let Some(reading) = backend.reading(Step::Key(KeyStep { key, shift, at })) else {
            return Ok(NO_KEYBOARD);
        };
        let events: Vec<_> = backend
            .decode(reading)
            .filter_map(|input| match input {
                HostInput::Platform(PlatformInput::Keyboard(event)) => Some(event),
                _ => None,
            })
            .collect();
        for state in [KeyState::Down, KeyState::Up] {
            let Some(event) = events.iter().find(|event| event.state() == state) else {
                return Err(format!("{key:?}: no {state:?} event among {events:?}"));
            };
            if event.key != Key::Named(expected) {
                return Err(format!(
                    "{key:?}: {state:?} arrived as {:?}, not {expected:?}",
                    event.key
                ));
            }
            if event.modifiers.contains(Modifiers::SHIFT) != shift {
                return Err(format!(
                    "{key:?}: {state:?} carried {:?} with Shift {}",
                    event.modifiers,
                    if shift { "held" } else { "up" }
                ));
            }
        }
    }
    Ok(RowOutcome::Held)
}

/// Escape pressed while the platform's input method composes arrives, pressed
/// and released, marked [`ImeComposition::Active`]: the input method's key,
/// which no shortcut, focus traversal or drag cancellation takes, so it
/// cancels the composition first.
pub(super) fn a_key_during_composition_is_the_input_methods<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let at = Stamp::coarse(Duration::from_millis(30));
    let step = KeyStep {
        key: StepKey::Escape,
        shift: false,
        at,
    };
    let Some(reading) = backend.reading(Step::ComposingKey(step)) else {
        return Ok(RowOutcome::NotApplicable(
            "the backend has no input-method key path the table drives",
        ));
    };
    let events: Vec<_> = backend
        .decode(reading)
        .filter_map(|input| match input {
            HostInput::Platform(PlatformInput::Keyboard(event)) => Some(event),
            _ => None,
        })
        .collect();
    for state in [KeyState::Down, KeyState::Up] {
        let Some(event) = events.iter().find(|event| event.state() == state) else {
            return Err(format!("no {state:?} event among {events:?}"));
        };
        if event.composition != ImeComposition::Active {
            return Err(format!(
                "Escape {state:?} during a composition arrived as {:?}, not the input method's",
                event.composition
            ));
        }
    }
    Ok(RowOutcome::Held)
}
