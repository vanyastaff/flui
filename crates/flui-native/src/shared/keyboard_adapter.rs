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

// Winit's KeyEvent has private platform state, so integration tests cannot
// construct the complete native input. This private boundary is tested here;
// native public producer coverage remains in the backend probes.
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn every_named_key_and_code_round_trips_through_keyboard_types() {
        for key in NamedKey::ALL {
            let upstream = upstream::NamedKey::from_str(key.as_str())
                .unwrap_or_else(|_| panic!("keyboard-types lacks {key:?}"));
            assert_eq!(named_key(upstream), *key);
        }
        for physical in Code::ALL {
            let upstream = upstream::Code::from_str(physical.as_str())
                .unwrap_or_else(|_| panic!("keyboard-types lacks {physical:?}"));
            assert_eq!(code(upstream), *physical);
        }
    }

    #[expect(deprecated, reason = "the legacy names are what is under test")]
    fn legacy_meta_names_are_meta() {
        assert_eq!(named_key(upstream::NamedKey::Hyper), NamedKey::Meta);
        assert_eq!(named_key(upstream::NamedKey::Super), NamedKey::Meta);
        assert_eq!(
            modifiers(upstream::Modifiers::SUPER | upstream::Modifiers::CONTROL),
            Modifiers::META | Modifiers::CONTROL
        );
    }

    fn a_key_event_keeps_every_field() {
        let upstream = upstream::KeyboardEvent {
            state: upstream::KeyState::Down,
            key: upstream::Key::Character("q".to_owned()),
            code: upstream::Code::KeyQ,
            location: upstream::Location::Left,
            modifiers: upstream::Modifiers::ALT,
            repeat: true,
            is_composing: true,
        };
        let flui_platform_api::PlatformInput::Keyboard(event) = keyboard_input(upstream, 19) else {
            panic!("keyboard adapter produces keyboard input")
        };
        assert_eq!(event.state(), KeyState::Down);
        assert_eq!(event.key, Key::character("q"));
        assert_eq!(event.code, Code::KeyQ);
        assert_eq!(event.location, Location::Left);
        assert_eq!(event.modifiers, Modifiers::ALT);
        assert_eq!(event.repeat(), KeyRepeat::AutoRepeat);
        assert_eq!(event.composition, ImeComposition::Active);
        assert_eq!(event.time, EventTime::from_nanos(19));
    }

    #[test]
    fn keyboard_adapter_contract() {
        crate::table_test::run_table(
            "keyboard_adapter_contract",
            &[
                (
                    "every_named_key_and_code_round_trips_through_keyboard_types",
                    every_named_key_and_code_round_trips_through_keyboard_types,
                ),
                ("legacy_meta_names_are_meta", legacy_meta_names_are_meta),
                (
                    "a_key_event_keeps_every_field",
                    a_key_event_keeps_every_field,
                ),
            ],
        );
    }
}
