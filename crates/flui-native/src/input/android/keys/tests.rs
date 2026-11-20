//! The decidable half of the Android backend's key translation
//! (`input::android::keys`): the Menu key arrives named, a key's
//! character is looked up without Control and Meta, so Ctrl+X reaches a
//! shortcut as `x` with Control held, and a key during a composition is the
//! input method's. The JNI
//! side runs on a device.

use crate::input::android::keys::{
    DeadChars, DeadKeyComposer, KEYCODE_MENU, MappedChar, character_meta_state, key_composition,
    unnamed_key,
};
use flui_platform::input::keyboard::{ImeComposition, Key, NamedKey};

/// The Menu key opens the focused item's context menu on Android, so it
/// must arrive named; character keys stay to the key map.
#[test]
fn the_menu_key_is_named_context_menu() {
    const KEYCODE_A: i32 = 29;
    const KEYCODE_F10: i32 = 140;
    for (keycode, named) in [
        (KEYCODE_MENU, Some(NamedKey::ContextMenu)),
        (KEYCODE_A, None),
        (KEYCODE_F10, None),
    ] {
        assert_eq!(unnamed_key(keycode), named, "key code {keycode}");
    }
}

/// Android `KeyEvent` meta bits.
const SHIFT: u32 = 0x1 | 0x40;
const ALT: u32 = 0x2 | 0x10;
const CTRL: u32 = 0x1000 | 0x2000;
const META: u32 = 0x10000 | 0x20000;
const CAPS_LOCK: u32 = 0x0010_0000;

#[test]
fn a_character_is_looked_up_without_control_and_meta() {
    for (held, lookup) in [
        (CTRL, 0),
        (META, 0),
        (CTRL | SHIFT, SHIFT),
        (CTRL | ALT, ALT),
        (META | CAPS_LOCK, CAPS_LOCK),
        (SHIFT | ALT | CAPS_LOCK, SHIFT | ALT | CAPS_LOCK),
        (0, 0),
    ] {
        assert_eq!(
            character_meta_state(held),
            lookup,
            "meta state {held:#x} looks its character up with {lookup:#x}"
        );
    }
}

/// A key the native queue delivers while the input method composes is the
/// input method's: Escape cancels the composition, not a dialog.
#[test]
fn a_key_during_composition_is_the_input_methods() {
    assert_eq!(key_composition(true), ImeComposition::Active);
    assert_eq!(key_composition(false), ImeComposition::Inactive);
}

/// A stand-in for a device key map's dead-character table: acute and
/// circumflex over `e`, and the accent itself for a space or the same
/// accent again, as `KeyCharacterMap.getDeadChar` answers.
struct StandInMap;

impl DeadChars for StandInMap {
    fn dead_char(&self, accent: char, base: char) -> Option<char> {
        match (accent, base) {
            (accent, ' ') => Some(accent),
            (accent, base) if accent == base => Some(accent),
            ('´', 'e') => Some('é'),
            ('´', 'E') => Some('É'),
            ('^', 'e') => Some('ê'),
            _ => None,
        }
    }
}

/// What a hardware dead key and the keys after it type: the accent stays
/// pending, composes with the next character, and is never lost.
#[test]
fn a_hardware_dead_key_composes_with_the_next_key() {
    enum Step {
        Press(MappedChar),
        Named(NamedKey),
        /// A Control or Meta chord, or an input method starting to compose.
        Interrupt,
    }
    use MappedChar::{Accent, Text};
    use Step::{Interrupt, Named, Press};
    let rows: &[(&str, &[Step], &[Key])] = &[
        (
            "an accent then its letter",
            &[Press(Accent('´')), Press(Text('e'))],
            &[Key::Dead(Some('´')), Key::character("é")],
        ),
        (
            "shift between them keeps the accent",
            &[Press(Accent('´')), Named(NamedKey::Shift), Press(Text('E'))],
            &[Key::Dead(Some('´')), Key::character("É")],
        ),
        (
            "a pair that composes nothing types both",
            &[Press(Accent('´')), Press(Text('x'))],
            &[Key::Dead(Some('´')), Key::character("´x")],
        ),
        (
            "a space types the accent",
            &[Press(Accent('^')), Press(Text(' '))],
            &[Key::Dead(Some('^')), Key::character("^")],
        ),
        (
            "the accent twice types it",
            &[Press(Accent('´')), Press(Accent('´'))],
            &[Key::Dead(Some('´')), Key::character("´")],
        ),
        (
            "another accent replaces the pending one",
            &[Press(Accent('´')), Press(Accent('^')), Press(Text('e'))],
            &[
                Key::Dead(Some('´')),
                Key::Dead(Some('^')),
                Key::character("ê"),
            ],
        ),
        (
            "a key that types nothing drops the accent",
            &[
                Press(Accent('´')),
                Named(NamedKey::ArrowLeft),
                Press(Text('e')),
            ],
            &[Key::Dead(Some('´')), Key::character("e")],
        ),
        (
            "a key the map gives no character drops the accent",
            &[
                Press(Accent('´')),
                Press(MappedChar::Nothing),
                Press(Text('e')),
            ],
            &[
                Key::Dead(Some('´')),
                Key::Named(NamedKey::Unidentified),
                Key::character("e"),
            ],
        ),
        (
            "a Control chord drops the accent, so the shortcut fires and the next letter is bare",
            &[Press(Accent('´')), Interrupt, Press(Text('e'))],
            &[Key::Dead(Some('´')), Key::character("e")],
        ),
        (
            "an input method's composition drops the accent before its commit",
            &[Press(Accent('^')), Interrupt, Press(Text('e'))],
            &[Key::Dead(Some('^')), Key::character("e")],
        ),
        (
            "a letter with no accent pending is itself",
            &[Press(Text('e')), Press(Text('e'))],
            &[Key::character("e"), Key::character("e")],
        ),
    ];
    for (case, steps, want) in rows {
        let mut composer = DeadKeyComposer::default();
        let typed: Vec<Key> = steps
            .iter()
            .filter_map(|step| match step {
                Press(mapped) => Some(composer.press(*mapped, &StandInMap)),
                Interrupt => {
                    composer.interrupt();
                    None
                }
                Named(key) => {
                    composer.named(*key);
                    None
                }
            })
            .collect();
        assert_eq!(typed, *want, "{case}");
    }
    assert_eq!(
        DeadKeyComposer::unchanged(Accent('´')),
        Key::Dead(Some('´')),
        "a dead key's release names its accent"
    );
}
