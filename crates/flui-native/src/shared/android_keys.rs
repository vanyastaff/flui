//! The decidable half of the Android backend's key translation: the named
//! keys the key-code table leaves unnamed, and which meta state a key's
//! character is looked up with.
//!
//! `KEYCODE_MENU` is the Menu key of a phone's keyboard and of every PC
//! keyboard attached to it, the key that opens the focused item's context
//! menu on Android (`ShowContextMenuIntent`'s Android binding); the table
//! gives it a physical code only, so it is named here.
//!
//! Android's `KeyCharacterMap.get` answers no character while Control or
//! Meta is held, so Ctrl+X would reach a shortcut as an unidentified key. A
//! key's logical value is the character it types without those modifiers,
//! as on every other backend: Shift, Alt (the alternate characters the
//! device's map assigns it), Caps Lock and Num Lock still choose it, and the
//! event carries Control and Meta as modifiers beside it.
//!
//! Cfg-free plain values, so Linux CI pins them; `platforms::android` reads
//! the map through JNI.

use flui_platform_api::keyboard::NamedKey;

/// `KeyEvent.KEYCODE_MENU`.
pub const KEYCODE_MENU: i32 = 82;

/// The named key `keycode` is where the key-code table names none, or
/// `None` when it is a character key or unknown.
#[must_use]
pub const fn unnamed_key(keycode: i32) -> Option<NamedKey> {
    match keycode {
        KEYCODE_MENU => Some(NamedKey::ContextMenu),
        _ => None,
    }
}

/// `KeyEvent.META_CTRL_ON`, `META_CTRL_LEFT_ON` and `META_CTRL_RIGHT_ON`.
const META_CTRL_MASK: u32 = 0x0000_7000;
/// `KeyEvent.META_META_ON`, `META_META_LEFT_ON` and `META_META_RIGHT_ON`.
const META_META_MASK: u32 = 0x0007_0000;

/// The meta state to look a key's character up with, for an event whose
/// meta state is `meta_state`: Control and Meta left out.
#[must_use]
pub const fn character_meta_state(meta_state: u32) -> u32 {
    meta_state & !(META_CTRL_MASK | META_META_MASK)
}
