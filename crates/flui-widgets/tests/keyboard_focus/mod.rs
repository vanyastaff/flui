//! Keyboard focus through the default bindings: activation, the focus
//! indicator's input modality, and requests on retired nodes. Each file is one
//! family's rows; `contracts.rs` runs them as `keyboard_focus_contracts`.

pub(crate) mod activation;
pub(crate) mod modality;
pub(crate) mod requests;
pub(crate) mod routes;

use flui_interaction::FocusManager;
use flui_platform::EventTime;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey};

/// One key event, as a platform delivers it.
pub(crate) fn key(state: KeyState, key: Key, repeat: KeyRepeat) -> KeyEvent {
    KeyEvent::new(state, key, Code::Unidentified, EventTime::from_nanos(0)).with_repeat(repeat)
}

/// `key`'s key-down; whether the focus walk consumed it.
pub(crate) fn down(manager: &FocusManager, pressed: &Key) -> bool {
    manager
        .dispatch_key_event(&key(KeyState::Down, pressed.clone(), KeyRepeat::First))
        .is_handled()
}

/// `key`'s release.
pub(crate) fn up(manager: &FocusManager, released: &Key) {
    let _ = manager.dispatch_key_event(&key(KeyState::Up, released.clone(), KeyRepeat::First));
}

/// `key` pressed and released; whether its key-down was consumed.
pub(crate) fn press(manager: &FocusManager, pressed: &Key) -> bool {
    let consumed = down(manager, pressed);
    up(manager, pressed);
    consumed
}

/// `key` pressed with `modifiers` held, and released.
pub(crate) fn chord(manager: &FocusManager, pressed: &Key, modifiers: Modifiers) -> bool {
    let event = key(KeyState::Down, pressed.clone(), KeyRepeat::First).with_modifiers(modifiers);
    let consumed = manager.dispatch_key_event(&event).is_handled();
    let release = key(KeyState::Up, pressed.clone(), KeyRepeat::First).with_modifiers(modifiers);
    let _ = manager.dispatch_key_event(&release);
    consumed
}

/// Tab, as a key-down and its release.
pub(crate) fn tab(manager: &FocusManager) -> bool {
    press(manager, &Key::Named(NamedKey::Tab))
}
