//! The decidable half of the Android backend's key translation
//! (`input::android::keys`): the Menu key arrives named, a key's
//! character is looked up without Control and Meta, so Ctrl+X reaches a
//! shortcut as `x` with Control held, and a key during a composition is the
//! input method's. The JNI
//! side runs on a device.

use crate::input::android::keys::{
    KEYCODE_MENU, character_meta_state, key_composition, unnamed_key,
};
use flui_platform::keyboard::{ImeComposition, NamedKey};

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
