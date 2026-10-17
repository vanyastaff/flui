//! The key rows (ADR-0216): the keys a context menu's default bindings name
//! arrive as those logical keys.

use std::time::Duration;

use flui_platform_api::PlatformInput;
use flui_platform_api::keyboard::{Key, KeyState, Modifiers, NamedKey};
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
