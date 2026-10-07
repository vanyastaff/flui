//! The keyboard-only boundary for audited Winit and Android mappings.
//!
//! Native key tables stay in their maintained ecosystem adapters. Their results
//! are converted here into the owned FLUI contract; pointer input never passes
//! through upstream event values.

use flui_platform_api::{
    EventTime,
    keyboard::{
        Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
    },
};
use keyboard_types as upstream;

pub(crate) fn keyboard_input(
    event: upstream::KeyboardEvent,
    observed_ns: u64,
) -> flui_platform_api::PlatformInput {
    flui_platform_api::PlatformInput::Keyboard(key_event(
        &event,
        EventTime::from_nanos(observed_ns),
    ))
}

pub(crate) fn modifiers(value: upstream::Modifiers) -> Modifiers {
    use upstream::Modifiers as Up;
    #[expect(deprecated, reason = "legacy modifier spellings normalize to Meta")]
    let pairs = [
        (Up::ALT, Modifiers::ALT),
        (Up::ALT_GRAPH, Modifiers::ALT_GRAPH),
        (Up::CAPS_LOCK, Modifiers::CAPS_LOCK),
        (Up::CONTROL, Modifiers::CONTROL),
        (Up::FN, Modifiers::FN),
        (Up::FN_LOCK, Modifiers::FN_LOCK),
        (Up::META, Modifiers::META),
        (Up::NUM_LOCK, Modifiers::NUM_LOCK),
        (Up::SCROLL_LOCK, Modifiers::SCROLL_LOCK),
        (Up::SHIFT, Modifiers::SHIFT),
        (Up::SYMBOL, Modifiers::SYMBOL),
        (Up::SYMBOL_LOCK, Modifiers::SYMBOL_LOCK),
        (Up::HYPER, Modifiers::META),
        (Up::SUPER, Modifiers::META),
    ];
    pairs
        .into_iter()
        .filter(|(from, _)| value.contains(*from))
        .fold(Modifiers::NONE, |set, (_, to)| set | to)
}

fn named_key(value: upstream::NamedKey) -> NamedKey {
    match value.to_string().as_str() {
        "Hyper" | "Super" => NamedKey::Meta,
        spelling => NamedKey::from_w3c(spelling).unwrap_or(NamedKey::Unidentified),
    }
}

fn code(value: upstream::Code) -> Code {
    Code::from_w3c(&value.to_string()).unwrap_or(Code::Unidentified)
}

fn key_event(event: &upstream::KeyboardEvent, time: EventTime) -> KeyEvent {
    let state = match event.state {
        upstream::KeyState::Down => KeyState::Down,
        upstream::KeyState::Up => KeyState::Up,
    };
    let key = match &event.key {
        upstream::Key::Character(text) => Key::character(text.as_str()),
        upstream::Key::Named(value) => Key::Named(named_key(*value)),
    };
    let location = match event.location {
        upstream::Location::Standard => Location::Standard,
        upstream::Location::Left => Location::Left,
        upstream::Location::Right => Location::Right,
        upstream::Location::Numpad => Location::Numpad,
    };
    KeyEvent::new(state, key, code(event.code), time)
        .with_location(location)
        .with_modifiers(modifiers(event.modifiers))
        .with_repeat(if event.repeat {
            KeyRepeat::AutoRepeat
        } else {
            KeyRepeat::First
        })
        .with_composition(if event.is_composing {
            ImeComposition::Active
        } else {
            ImeComposition::Inactive
        })
}
