//! macOS (AppKit) keyboard translation decisions.
//!
//! Everything the AppKit backend decides about a keystroke from its virtual
//! keycode — which W3C `code` names the physical key, and which named `Key`
//! intercepts the `NSEvent.characters` text path — lives here as pure
//! functions over the raw `NSEvent.keyCode` value.
//!
//! Like [`super::keys`] (the Win32 twin this module's shape follows), it is
//! deliberately free of `cfg` gates and platform types so the tables compile
//! — and their tests execute — on any host, even though the AppKit caller
//! (`platforms/macos/events.rs`) itself is only ever clippy-linted in CI,
//! never linked or run.
//!
//! # References
//!
//! macOS virtual keycodes are position-based (they name the physical key,
//! not the character it types) and are defined once, as the `kVK_*` constants
//! in Apple's Carbon `HIToolbox/Events.h` — still the canonical source; the
//! [`kvk`] module transcribes it. The `Code` each keycode maps to follows the
//! W3C uievents-code registry (<https://www.w3.org/TR/uievents-code/>) as
//! implemented by Firefox (`NativeKeyToDOMCodeName.h`, the `CODE_MAP_MAC`
//! rows) and the winit macOS backend (winit 0.30
//! `platform_impl/macos/event.rs`, `scancode_to_physicalkey`). Where those
//! two disagree the table follows Apple's header and Firefox, and the
//! divergence is commented at the arm — see
//! `follows_apples_header_where_the_winit_table_drifts` in the tests.
//!
//! # What never reaches these tables
//!
//! AppKit reports modifier keys through `flagsChanged:`, not
//! `keyDown:`/`keyUp:` — the modifier rows below (`kVK_Shift` …
//! `kVK_Function`) are complete for the table's own sake, but the backend
//! does not receive plain key events for them, and it does not translate
//! `flagsChanged:` today; modifier *state* rides every event's
//! `modifierFlags` instead.

use keyboard_types::{Code, Key, NamedKey};

/// Apple's `kVK_*` virtual keycodes, from Carbon `HIToolbox/Events.h`.
///
/// `ANSI_*` names mean "the key in this position on a US (ANSI) keyboard" —
/// the keycode itself is layout-independent. Raw `u16` (the width of
/// `NSEvent.keyCode`) rather than a binding crate's type so this module
/// compiles on every host.
#[allow(missing_docs)]
pub mod kvk {
    pub const ANSI_A: u16 = 0x00;
    pub const ANSI_S: u16 = 0x01;
    pub const ANSI_D: u16 = 0x02;
    pub const ANSI_F: u16 = 0x03;
    pub const ANSI_H: u16 = 0x04;
    pub const ANSI_G: u16 = 0x05;
    pub const ANSI_Z: u16 = 0x06;
    pub const ANSI_X: u16 = 0x07;
    pub const ANSI_C: u16 = 0x08;
    pub const ANSI_V: u16 = 0x09;
    pub const ISO_SECTION: u16 = 0x0A;
    pub const ANSI_B: u16 = 0x0B;
    pub const ANSI_Q: u16 = 0x0C;
    pub const ANSI_W: u16 = 0x0D;
    pub const ANSI_E: u16 = 0x0E;
    pub const ANSI_R: u16 = 0x0F;
    pub const ANSI_Y: u16 = 0x10;
    pub const ANSI_T: u16 = 0x11;
    pub const ANSI_1: u16 = 0x12;
    pub const ANSI_2: u16 = 0x13;
    pub const ANSI_3: u16 = 0x14;
    pub const ANSI_4: u16 = 0x15;
    pub const ANSI_6: u16 = 0x16;
    pub const ANSI_5: u16 = 0x17;
    pub const ANSI_EQUAL: u16 = 0x18;
    pub const ANSI_9: u16 = 0x19;
    pub const ANSI_7: u16 = 0x1A;
    pub const ANSI_MINUS: u16 = 0x1B;
    pub const ANSI_8: u16 = 0x1C;
    pub const ANSI_0: u16 = 0x1D;
    pub const ANSI_RIGHT_BRACKET: u16 = 0x1E;
    pub const ANSI_O: u16 = 0x1F;
    pub const ANSI_U: u16 = 0x20;
    pub const ANSI_LEFT_BRACKET: u16 = 0x21;
    pub const ANSI_I: u16 = 0x22;
    pub const ANSI_P: u16 = 0x23;
    pub const RETURN: u16 = 0x24;
    pub const ANSI_L: u16 = 0x25;
    pub const ANSI_J: u16 = 0x26;
    pub const ANSI_QUOTE: u16 = 0x27;
    pub const ANSI_K: u16 = 0x28;
    pub const ANSI_SEMICOLON: u16 = 0x29;
    pub const ANSI_BACKSLASH: u16 = 0x2A;
    pub const ANSI_COMMA: u16 = 0x2B;
    pub const ANSI_SLASH: u16 = 0x2C;
    pub const ANSI_N: u16 = 0x2D;
    pub const ANSI_M: u16 = 0x2E;
    pub const ANSI_PERIOD: u16 = 0x2F;
    pub const TAB: u16 = 0x30;
    pub const SPACE: u16 = 0x31;
    pub const ANSI_GRAVE: u16 = 0x32;
    /// `kVK_Delete` is the key labeled "delete" on Apple keyboards — the
    /// BACKSPACE position; forward delete is [`FORWARD_DELETE`].
    pub const DELETE: u16 = 0x33;
    pub const ESCAPE: u16 = 0x35;
    pub const RIGHT_COMMAND: u16 = 0x36;
    pub const COMMAND: u16 = 0x37;
    pub const SHIFT: u16 = 0x38;
    pub const CAPS_LOCK: u16 = 0x39;
    pub const OPTION: u16 = 0x3A;
    pub const CONTROL: u16 = 0x3B;
    pub const RIGHT_SHIFT: u16 = 0x3C;
    pub const RIGHT_OPTION: u16 = 0x3D;
    pub const RIGHT_CONTROL: u16 = 0x3E;
    pub const FUNCTION: u16 = 0x3F;
    pub const F17: u16 = 0x40;
    pub const ANSI_KEYPAD_DECIMAL: u16 = 0x41;
    pub const ANSI_KEYPAD_MULTIPLY: u16 = 0x43;
    pub const ANSI_KEYPAD_PLUS: u16 = 0x45;
    /// The numpad "clear" key — the NumLock position of a PC numpad.
    pub const ANSI_KEYPAD_CLEAR: u16 = 0x47;
    pub const VOLUME_UP: u16 = 0x48;
    pub const VOLUME_DOWN: u16 = 0x49;
    pub const MUTE: u16 = 0x4A;
    pub const ANSI_KEYPAD_DIVIDE: u16 = 0x4B;
    pub const ANSI_KEYPAD_ENTER: u16 = 0x4C;
    pub const ANSI_KEYPAD_MINUS: u16 = 0x4E;
    pub const F18: u16 = 0x4F;
    pub const F19: u16 = 0x50;
    pub const ANSI_KEYPAD_EQUALS: u16 = 0x51;
    pub const ANSI_KEYPAD_0: u16 = 0x52;
    pub const ANSI_KEYPAD_1: u16 = 0x53;
    pub const ANSI_KEYPAD_2: u16 = 0x54;
    pub const ANSI_KEYPAD_3: u16 = 0x55;
    pub const ANSI_KEYPAD_4: u16 = 0x56;
    pub const ANSI_KEYPAD_5: u16 = 0x57;
    pub const ANSI_KEYPAD_6: u16 = 0x58;
    pub const ANSI_KEYPAD_7: u16 = 0x59;
    pub const F20: u16 = 0x5A;
    pub const ANSI_KEYPAD_8: u16 = 0x5B;
    pub const ANSI_KEYPAD_9: u16 = 0x5C;
    pub const JIS_YEN: u16 = 0x5D;
    pub const JIS_UNDERSCORE: u16 = 0x5E;
    pub const JIS_KEYPAD_COMMA: u16 = 0x5F;
    pub const F5: u16 = 0x60;
    pub const F6: u16 = 0x61;
    pub const F7: u16 = 0x62;
    pub const F3: u16 = 0x63;
    pub const F8: u16 = 0x64;
    pub const F9: u16 = 0x65;
    pub const JIS_EISU: u16 = 0x66;
    pub const F11: u16 = 0x67;
    pub const JIS_KANA: u16 = 0x68;
    pub const F13: u16 = 0x69;
    pub const F16: u16 = 0x6A;
    pub const F14: u16 = 0x6B;
    pub const F10: u16 = 0x6D;
    /// The PC-keyboard Menu key. `Events.h` defines no constant for `0x6E`;
    /// Firefox and Chromium both map it to `ContextMenu`.
    pub const PC_MENU: u16 = 0x6E;
    pub const F12: u16 = 0x6F;
    pub const F15: u16 = 0x71;
    /// The Help key on Apple pro keyboards — the Insert position of a PC
    /// keyboard, and `Insert` is what the winit backend and Chromium call
    /// it (Firefox says `Help`); this table sides with the winit wire so
    /// FLUI's two macOS backends agree.
    pub const HELP: u16 = 0x72;
    pub const HOME: u16 = 0x73;
    pub const PAGE_UP: u16 = 0x74;
    pub const FORWARD_DELETE: u16 = 0x75;
    pub const F4: u16 = 0x76;
    pub const END: u16 = 0x77;
    pub const F2: u16 = 0x78;
    pub const PAGE_DOWN: u16 = 0x79;
    pub const F1: u16 = 0x7A;
    pub const LEFT_ARROW: u16 = 0x7B;
    pub const RIGHT_ARROW: u16 = 0x7C;
    pub const DOWN_ARROW: u16 = 0x7D;
    pub const UP_ARROW: u16 = 0x7E;
}

/// Map an `NSEvent.keyCode` (a `kVK_*` virtual keycode) to the W3C `code`
/// value naming the physical key.
///
/// Two arms deliberately diverge from the winit macOS backend's table, both
/// toward Apple's `Events.h` and Firefox:
///
/// - **Volume keys.** `Events.h` says `kVK_VolumeUp`/`Down`/`Mute` are
///   `0x48`/`0x49`/`0x4A`; winit maps `0x49 → VolumeUp` and `0x4A →
///   VolumeDown` (dropping Mute), one code off, with a `TODO` in its own
///   source questioning the choice.
/// - **The ISO section key** (`0x0A`, between Left Shift and Z on ISO
///   boards). The registry and Firefox name that position `IntlBackslash`;
///   winit maps it to `Backquote` because German QWERTZ prints `^` there.
pub fn keycode_to_code(key_code: u16) -> Code {
    match key_code {
        kvk::ANSI_A => Code::KeyA,
        kvk::ANSI_S => Code::KeyS,
        kvk::ANSI_D => Code::KeyD,
        kvk::ANSI_F => Code::KeyF,
        kvk::ANSI_H => Code::KeyH,
        kvk::ANSI_G => Code::KeyG,
        kvk::ANSI_Z => Code::KeyZ,
        kvk::ANSI_X => Code::KeyX,
        kvk::ANSI_C => Code::KeyC,
        kvk::ANSI_V => Code::KeyV,
        kvk::ISO_SECTION => Code::IntlBackslash,
        kvk::ANSI_B => Code::KeyB,
        kvk::ANSI_Q => Code::KeyQ,
        kvk::ANSI_W => Code::KeyW,
        kvk::ANSI_E => Code::KeyE,
        kvk::ANSI_R => Code::KeyR,
        kvk::ANSI_Y => Code::KeyY,
        kvk::ANSI_T => Code::KeyT,
        kvk::ANSI_1 => Code::Digit1,
        kvk::ANSI_2 => Code::Digit2,
        kvk::ANSI_3 => Code::Digit3,
        kvk::ANSI_4 => Code::Digit4,
        kvk::ANSI_6 => Code::Digit6,
        kvk::ANSI_5 => Code::Digit5,
        kvk::ANSI_EQUAL => Code::Equal,
        kvk::ANSI_9 => Code::Digit9,
        kvk::ANSI_7 => Code::Digit7,
        kvk::ANSI_MINUS => Code::Minus,
        kvk::ANSI_8 => Code::Digit8,
        kvk::ANSI_0 => Code::Digit0,
        kvk::ANSI_RIGHT_BRACKET => Code::BracketRight,
        kvk::ANSI_O => Code::KeyO,
        kvk::ANSI_U => Code::KeyU,
        kvk::ANSI_LEFT_BRACKET => Code::BracketLeft,
        kvk::ANSI_I => Code::KeyI,
        kvk::ANSI_P => Code::KeyP,
        kvk::RETURN => Code::Enter,
        kvk::ANSI_L => Code::KeyL,
        kvk::ANSI_J => Code::KeyJ,
        kvk::ANSI_QUOTE => Code::Quote,
        kvk::ANSI_K => Code::KeyK,
        kvk::ANSI_SEMICOLON => Code::Semicolon,
        kvk::ANSI_BACKSLASH => Code::Backslash,
        kvk::ANSI_COMMA => Code::Comma,
        kvk::ANSI_SLASH => Code::Slash,
        kvk::ANSI_N => Code::KeyN,
        kvk::ANSI_M => Code::KeyM,
        kvk::ANSI_PERIOD => Code::Period,
        kvk::TAB => Code::Tab,
        kvk::SPACE => Code::Space,
        kvk::ANSI_GRAVE => Code::Backquote,
        kvk::DELETE => Code::Backspace,
        kvk::ESCAPE => Code::Escape,
        kvk::RIGHT_COMMAND => Code::MetaRight,
        kvk::COMMAND => Code::MetaLeft,
        kvk::SHIFT => Code::ShiftLeft,
        kvk::CAPS_LOCK => Code::CapsLock,
        kvk::OPTION => Code::AltLeft,
        kvk::CONTROL => Code::ControlLeft,
        kvk::RIGHT_SHIFT => Code::ShiftRight,
        kvk::RIGHT_OPTION => Code::AltRight,
        kvk::RIGHT_CONTROL => Code::ControlRight,
        kvk::FUNCTION => Code::Fn,
        kvk::F17 => Code::F17,
        kvk::ANSI_KEYPAD_DECIMAL => Code::NumpadDecimal,
        kvk::ANSI_KEYPAD_MULTIPLY => Code::NumpadMultiply,
        kvk::ANSI_KEYPAD_PLUS => Code::NumpadAdd,
        // The registry names the numpad-clear position NumLock (it IS the
        // NumLock key of a PC numpad).
        kvk::ANSI_KEYPAD_CLEAR => Code::NumLock,
        kvk::VOLUME_UP => Code::AudioVolumeUp,
        kvk::VOLUME_DOWN => Code::AudioVolumeDown,
        kvk::MUTE => Code::AudioVolumeMute,
        kvk::ANSI_KEYPAD_DIVIDE => Code::NumpadDivide,
        kvk::ANSI_KEYPAD_ENTER => Code::NumpadEnter,
        kvk::ANSI_KEYPAD_MINUS => Code::NumpadSubtract,
        kvk::F18 => Code::F18,
        kvk::F19 => Code::F19,
        kvk::ANSI_KEYPAD_EQUALS => Code::NumpadEqual,
        kvk::ANSI_KEYPAD_0 => Code::Numpad0,
        kvk::ANSI_KEYPAD_1 => Code::Numpad1,
        kvk::ANSI_KEYPAD_2 => Code::Numpad2,
        kvk::ANSI_KEYPAD_3 => Code::Numpad3,
        kvk::ANSI_KEYPAD_4 => Code::Numpad4,
        kvk::ANSI_KEYPAD_5 => Code::Numpad5,
        kvk::ANSI_KEYPAD_6 => Code::Numpad6,
        kvk::ANSI_KEYPAD_7 => Code::Numpad7,
        kvk::F20 => Code::F20,
        kvk::ANSI_KEYPAD_8 => Code::Numpad8,
        kvk::ANSI_KEYPAD_9 => Code::Numpad9,
        kvk::JIS_YEN => Code::IntlYen,
        kvk::JIS_UNDERSCORE => Code::IntlRo,
        kvk::JIS_KEYPAD_COMMA => Code::NumpadComma,
        kvk::F5 => Code::F5,
        kvk::F6 => Code::F6,
        kvk::F7 => Code::F7,
        kvk::F3 => Code::F3,
        kvk::F8 => Code::F8,
        kvk::F9 => Code::F9,
        kvk::JIS_EISU => Code::Lang2,
        kvk::F11 => Code::F11,
        kvk::JIS_KANA => Code::Lang1,
        kvk::F13 => Code::F13,
        kvk::F16 => Code::F16,
        kvk::F14 => Code::F14,
        kvk::F10 => Code::F10,
        kvk::PC_MENU => Code::ContextMenu,
        kvk::F12 => Code::F12,
        kvk::F15 => Code::F15,
        kvk::HELP => Code::Insert,
        kvk::HOME => Code::Home,
        kvk::PAGE_UP => Code::PageUp,
        kvk::FORWARD_DELETE => Code::Delete,
        kvk::F4 => Code::F4,
        kvk::END => Code::End,
        kvk::F2 => Code::F2,
        kvk::PAGE_DOWN => Code::PageDown,
        kvk::F1 => Code::F1,
        kvk::LEFT_ARROW => Code::ArrowLeft,
        kvk::RIGHT_ARROW => Code::ArrowRight,
        kvk::DOWN_ARROW => Code::ArrowDown,
        kvk::UP_ARROW => Code::ArrowUp,
        _ => Code::Unidentified,
    }
}

/// The named `Key` a keycode resolves to *before* the backend consults
/// `NSEvent.characters` — `None` means "let the characters text decide".
///
/// This intercept exists because for these keys `characters` is not typeable
/// text: editing/navigation/function keys deliver control characters or
/// Unicode private-use-area codepoints (`NSUpArrowFunctionKey` is `U+F700`,
/// `NSF13FunctionKey` is `U+F70A`, …), which would otherwise ship as
/// garbage `Key::Character` values. Letters, digits, and punctuation return
/// `None` on purpose: their `characters` string is the layout- and
/// modifier-aware translation (the macOS analogue of the Win32 `WM_CHAR`
/// merge in [`super::keys`]) and must keep winning.
pub fn keycode_to_key(key_code: u16) -> Option<Key> {
    let named = match key_code {
        kvk::LEFT_ARROW => NamedKey::ArrowLeft,
        kvk::RIGHT_ARROW => NamedKey::ArrowRight,
        kvk::DOWN_ARROW => NamedKey::ArrowDown,
        kvk::UP_ARROW => NamedKey::ArrowUp,

        kvk::F1 => NamedKey::F1,
        kvk::F2 => NamedKey::F2,
        kvk::F3 => NamedKey::F3,
        kvk::F4 => NamedKey::F4,
        kvk::F5 => NamedKey::F5,
        kvk::F6 => NamedKey::F6,
        kvk::F7 => NamedKey::F7,
        kvk::F8 => NamedKey::F8,
        kvk::F9 => NamedKey::F9,
        kvk::F10 => NamedKey::F10,
        kvk::F11 => NamedKey::F11,
        kvk::F12 => NamedKey::F12,
        kvk::F13 => NamedKey::F13,
        kvk::F14 => NamedKey::F14,
        kvk::F15 => NamedKey::F15,
        kvk::F16 => NamedKey::F16,
        kvk::F17 => NamedKey::F17,
        kvk::F18 => NamedKey::F18,
        kvk::F19 => NamedKey::F19,
        kvk::F20 => NamedKey::F20,

        // Both Enter keys report the same logical key; the numpad one is
        // distinguished by its `code`/`location`.
        kvk::RETURN | kvk::ANSI_KEYPAD_ENTER => NamedKey::Enter,
        kvk::TAB => NamedKey::Tab,
        kvk::DELETE => NamedKey::Backspace,
        kvk::ESCAPE => NamedKey::Escape,
        kvk::FORWARD_DELETE => NamedKey::Delete,
        kvk::HOME => NamedKey::Home,
        kvk::END => NamedKey::End,
        kvk::PAGE_UP => NamedKey::PageUp,
        kvk::PAGE_DOWN => NamedKey::PageDown,
        // Insert, matching this key's `Code` (see [`kvk::HELP`]).
        kvk::HELP => NamedKey::Insert,
        kvk::PC_MENU => NamedKey::ContextMenu,

        // Modifiers arrive via `flagsChanged:`, not `keyDown:` — mapped for
        // table completeness (see the module doc).
        kvk::SHIFT | kvk::RIGHT_SHIFT => NamedKey::Shift,
        kvk::CONTROL | kvk::RIGHT_CONTROL => NamedKey::Control,
        kvk::OPTION | kvk::RIGHT_OPTION => NamedKey::Alt,
        kvk::COMMAND | kvk::RIGHT_COMMAND => NamedKey::Meta,
        kvk::CAPS_LOCK => NamedKey::CapsLock,
        kvk::FUNCTION => NamedKey::Fn,

        kvk::VOLUME_UP => NamedKey::AudioVolumeUp,
        kvk::VOLUME_DOWN => NamedKey::AudioVolumeDown,
        kvk::MUTE => NamedKey::AudioVolumeMute,

        kvk::JIS_EISU => NamedKey::Eisu,
        kvk::JIS_KANA => NamedKey::KanaMode,

        // Space types text but its `characters` IS the space — still, keep
        // the historical intercept so the key never depends on the
        // characters path.
        kvk::SPACE => return Some(Key::Character(" ".to_string())),

        _ => return None,
    };
    Some(Key::Named(named))
}

#[cfg(test)]
mod tests {
    use super::super::keys::location_for_code;
    use super::*;
    use keyboard_types::Location;

    /// macOS keycodes are NOT alphabetical or sequential: the letter block
    /// follows the ancient Apple layout (A S D F H G Z X C V …) and the
    /// digit row is scrambled (6 sits between 4 and 5). A table generated
    /// by pattern instead of transcription gets exactly these wrong.
    #[test]
    fn letter_and_digit_rows_follow_the_apple_scramble() {
        for (code, expected) in [
            (kvk::ANSI_A, Code::KeyA),
            (kvk::ANSI_S, Code::KeyS),
            (kvk::ANSI_D, Code::KeyD),
            (kvk::ANSI_H, Code::KeyH),
            (kvk::ANSI_G, Code::KeyG),
            (kvk::ANSI_Z, Code::KeyZ),
            (kvk::ANSI_Q, Code::KeyQ),
            (kvk::ANSI_B, Code::KeyB),
            (kvk::ANSI_4, Code::Digit4),
            (kvk::ANSI_6, Code::Digit6),
            (kvk::ANSI_5, Code::Digit5),
            (kvk::ANSI_9, Code::Digit9),
            (kvk::ANSI_7, Code::Digit7),
            (kvk::ANSI_8, Code::Digit8),
            (kvk::ANSI_0, Code::Digit0),
        ] {
            assert_eq!(keycode_to_code(code), expected, "keycode {code:#04x}");
        }
        // Letters and digits have no named-key intercept: their
        // `characters` translation must win.
        assert_eq!(keycode_to_key(kvk::ANSI_A), None);
        assert_eq!(keycode_to_key(kvk::ANSI_6), None);
    }

    /// Punctuation and editing keys, including the bracket pair whose
    /// keycodes are ordered Right (0x1E) before Left (0x21).
    #[test]
    fn punctuation_and_editing_keys_resolve() {
        for (code, expected) in [
            (kvk::ANSI_EQUAL, Code::Equal),
            (kvk::ANSI_MINUS, Code::Minus),
            (kvk::ANSI_RIGHT_BRACKET, Code::BracketRight),
            (kvk::ANSI_LEFT_BRACKET, Code::BracketLeft),
            (kvk::ANSI_QUOTE, Code::Quote),
            (kvk::ANSI_SEMICOLON, Code::Semicolon),
            (kvk::ANSI_BACKSLASH, Code::Backslash),
            (kvk::ANSI_COMMA, Code::Comma),
            (kvk::ANSI_SLASH, Code::Slash),
            (kvk::ANSI_PERIOD, Code::Period),
            (kvk::ANSI_GRAVE, Code::Backquote),
            (kvk::TAB, Code::Tab),
            (kvk::SPACE, Code::Space),
            (kvk::DELETE, Code::Backspace),
            (kvk::ESCAPE, Code::Escape),
            (kvk::RETURN, Code::Enter),
        ] {
            assert_eq!(keycode_to_code(code), expected, "keycode {code:#04x}");
        }
        // Punctuation is characters-typed; editing keys are intercepted.
        assert_eq!(keycode_to_key(kvk::ANSI_SEMICOLON), None);
        assert_eq!(
            keycode_to_key(kvk::DELETE),
            Some(Key::Named(NamedKey::Backspace))
        );
        assert_eq!(
            keycode_to_key(kvk::FORWARD_DELETE),
            Some(Key::Named(NamedKey::Delete))
        );
    }

    /// Modifier keycodes carry sidedness in the `code` (the shared location
    /// derivation turns it into `Location::Left`/`Right`), and Apple's
    /// left-hand Command is `0x37` with RIGHT Command at the LOWER `0x36`.
    #[test]
    fn modifier_keycodes_report_sidedness_and_location() {
        for (code, expected, location) in [
            (kvk::COMMAND, Code::MetaLeft, Location::Left),
            (kvk::RIGHT_COMMAND, Code::MetaRight, Location::Right),
            (kvk::SHIFT, Code::ShiftLeft, Location::Left),
            (kvk::RIGHT_SHIFT, Code::ShiftRight, Location::Right),
            (kvk::OPTION, Code::AltLeft, Location::Left),
            (kvk::RIGHT_OPTION, Code::AltRight, Location::Right),
            (kvk::CONTROL, Code::ControlLeft, Location::Left),
            (kvk::RIGHT_CONTROL, Code::ControlRight, Location::Right),
        ] {
            assert_eq!(keycode_to_code(code), expected, "keycode {code:#04x}");
            assert_eq!(location_for_code(expected), location);
        }
        assert_eq!(keycode_to_code(kvk::CAPS_LOCK), Code::CapsLock);
        assert_eq!(keycode_to_code(kvk::FUNCTION), Code::Fn);
        assert_eq!(
            keycode_to_key(kvk::FUNCTION),
            Some(Key::Named(NamedKey::Fn))
        );
    }

    /// The numpad cluster: digits are near-sequential but 8 and 9 sit
    /// after F20, and the clear key is the registry's NumLock. Every
    /// numpad code must also derive `Location::Numpad`.
    #[test]
    fn numpad_cluster_reports_numpad_codes_and_location() {
        let cluster = [
            (kvk::ANSI_KEYPAD_0, Code::Numpad0),
            (kvk::ANSI_KEYPAD_7, Code::Numpad7),
            (kvk::ANSI_KEYPAD_8, Code::Numpad8),
            (kvk::ANSI_KEYPAD_9, Code::Numpad9),
            (kvk::ANSI_KEYPAD_DECIMAL, Code::NumpadDecimal),
            (kvk::ANSI_KEYPAD_MULTIPLY, Code::NumpadMultiply),
            (kvk::ANSI_KEYPAD_PLUS, Code::NumpadAdd),
            (kvk::ANSI_KEYPAD_MINUS, Code::NumpadSubtract),
            (kvk::ANSI_KEYPAD_DIVIDE, Code::NumpadDivide),
            (kvk::ANSI_KEYPAD_ENTER, Code::NumpadEnter),
            (kvk::ANSI_KEYPAD_EQUALS, Code::NumpadEqual),
            (kvk::ANSI_KEYPAD_CLEAR, Code::NumLock),
            (kvk::JIS_KEYPAD_COMMA, Code::NumpadComma),
        ];
        for (code, expected) in cluster {
            assert_eq!(keycode_to_code(code), expected, "keycode {code:#04x}");
            assert_eq!(
                location_for_code(expected),
                Location::Numpad,
                "{expected:?}"
            );
        }
        // Numpad Enter reports the shared Enter logical key; the digits
        // type through `characters`.
        assert_eq!(
            keycode_to_key(kvk::ANSI_KEYPAD_ENTER),
            Some(Key::Named(NamedKey::Enter))
        );
        assert_eq!(keycode_to_key(kvk::ANSI_KEYPAD_5), None);
    }

    /// The function row's keycodes are thoroughly shuffled (F3 is 0x63,
    /// F13 is 0x69 next to F16 at 0x6A…); all twenty resolve, in both
    /// tables — their `characters` are PUA codepoints, so the named-key
    /// intercept is what keeps them out of `Key::Character`.
    #[test]
    fn function_row_covers_f1_through_f20() {
        let rows = [
            (kvk::F1, Code::F1, NamedKey::F1),
            (kvk::F2, Code::F2, NamedKey::F2),
            (kvk::F3, Code::F3, NamedKey::F3),
            (kvk::F4, Code::F4, NamedKey::F4),
            (kvk::F5, Code::F5, NamedKey::F5),
            (kvk::F6, Code::F6, NamedKey::F6),
            (kvk::F7, Code::F7, NamedKey::F7),
            (kvk::F8, Code::F8, NamedKey::F8),
            (kvk::F9, Code::F9, NamedKey::F9),
            (kvk::F10, Code::F10, NamedKey::F10),
            (kvk::F11, Code::F11, NamedKey::F11),
            (kvk::F12, Code::F12, NamedKey::F12),
            (kvk::F13, Code::F13, NamedKey::F13),
            (kvk::F14, Code::F14, NamedKey::F14),
            (kvk::F15, Code::F15, NamedKey::F15),
            (kvk::F16, Code::F16, NamedKey::F16),
            (kvk::F17, Code::F17, NamedKey::F17),
            (kvk::F18, Code::F18, NamedKey::F18),
            (kvk::F19, Code::F19, NamedKey::F19),
            (kvk::F20, Code::F20, NamedKey::F20),
        ];
        for (keycode, code, key) in rows {
            assert_eq!(keycode_to_code(keycode), code, "keycode {keycode:#04x}");
            assert_eq!(
                keycode_to_key(keycode),
                Some(Key::Named(key)),
                "keycode {keycode:#04x}"
            );
        }
    }

    /// The navigation cluster, plus the two delete keys: Apple's "delete"
    /// (0x33) is Backspace, "forward delete" (0x75) is Delete, and Help
    /// (0x72) is the Insert position.
    #[test]
    fn navigation_cluster_resolves() {
        for (keycode, code, key) in [
            (kvk::LEFT_ARROW, Code::ArrowLeft, NamedKey::ArrowLeft),
            (kvk::RIGHT_ARROW, Code::ArrowRight, NamedKey::ArrowRight),
            (kvk::DOWN_ARROW, Code::ArrowDown, NamedKey::ArrowDown),
            (kvk::UP_ARROW, Code::ArrowUp, NamedKey::ArrowUp),
            (kvk::HOME, Code::Home, NamedKey::Home),
            (kvk::END, Code::End, NamedKey::End),
            (kvk::PAGE_UP, Code::PageUp, NamedKey::PageUp),
            (kvk::PAGE_DOWN, Code::PageDown, NamedKey::PageDown),
            (kvk::FORWARD_DELETE, Code::Delete, NamedKey::Delete),
            (kvk::HELP, Code::Insert, NamedKey::Insert),
            (kvk::PC_MENU, Code::ContextMenu, NamedKey::ContextMenu),
        ] {
            assert_eq!(keycode_to_code(keycode), code, "keycode {keycode:#04x}");
            assert_eq!(
                keycode_to_key(keycode),
                Some(Key::Named(key)),
                "keycode {keycode:#04x}"
            );
        }
    }

    /// JIS and ISO layout keys per the registry's Intl/Lang rows.
    #[test]
    fn jis_and_iso_keys_resolve() {
        assert_eq!(keycode_to_code(kvk::JIS_YEN), Code::IntlYen);
        assert_eq!(keycode_to_code(kvk::JIS_UNDERSCORE), Code::IntlRo);
        assert_eq!(keycode_to_code(kvk::JIS_EISU), Code::Lang2);
        assert_eq!(keycode_to_code(kvk::JIS_KANA), Code::Lang1);
        assert_eq!(keycode_to_code(kvk::ISO_SECTION), Code::IntlBackslash);
        assert_eq!(
            keycode_to_key(kvk::JIS_EISU),
            Some(Key::Named(NamedKey::Eisu))
        );
        assert_eq!(
            keycode_to_key(kvk::JIS_KANA),
            Some(Key::Named(NamedKey::KanaMode))
        );
    }

    /// The two documented divergences from the winit macOS table, both
    /// following Apple's `Events.h` and Firefox instead: the volume trio
    /// at its header-assigned codes (winit shifts them down one and drops
    /// Mute), and the ISO section key as `IntlBackslash` (winit says
    /// `Backquote`).
    #[test]
    fn follows_apples_header_where_the_winit_table_drifts() {
        assert_eq!(keycode_to_code(kvk::VOLUME_UP), Code::AudioVolumeUp);
        assert_eq!(keycode_to_code(kvk::VOLUME_DOWN), Code::AudioVolumeDown);
        assert_eq!(keycode_to_code(kvk::MUTE), Code::AudioVolumeMute);
        assert_eq!(keycode_to_code(kvk::ISO_SECTION), Code::IntlBackslash);
    }

    /// Outside the two divergences above, everything the winit macOS
    /// backend's table produces, this table produces from the same keycode
    /// — FLUI's native and fallback macOS wires must agree on physical-key
    /// identity. Transcribed from winit 0.30 `platform_impl/macos/event.rs`
    /// (`scancode_to_physicalkey`), with winit's `SuperLeft`/`SuperRight`
    /// spelled as the W3C `MetaLeft`/`MetaRight`.
    #[test]
    fn agrees_with_the_winit_backend_elsewhere() {
        let winit_backend_codes: &[(u16, Code)] = &[
            (0x00, Code::KeyA),
            (0x01, Code::KeyS),
            (0x02, Code::KeyD),
            (0x03, Code::KeyF),
            (0x04, Code::KeyH),
            (0x05, Code::KeyG),
            (0x06, Code::KeyZ),
            (0x07, Code::KeyX),
            (0x08, Code::KeyC),
            (0x09, Code::KeyV),
            (0x0B, Code::KeyB),
            (0x0C, Code::KeyQ),
            (0x0D, Code::KeyW),
            (0x0E, Code::KeyE),
            (0x0F, Code::KeyR),
            (0x10, Code::KeyY),
            (0x11, Code::KeyT),
            (0x12, Code::Digit1),
            (0x13, Code::Digit2),
            (0x14, Code::Digit3),
            (0x15, Code::Digit4),
            (0x16, Code::Digit6),
            (0x17, Code::Digit5),
            (0x18, Code::Equal),
            (0x19, Code::Digit9),
            (0x1A, Code::Digit7),
            (0x1B, Code::Minus),
            (0x1C, Code::Digit8),
            (0x1D, Code::Digit0),
            (0x1E, Code::BracketRight),
            (0x1F, Code::KeyO),
            (0x20, Code::KeyU),
            (0x21, Code::BracketLeft),
            (0x22, Code::KeyI),
            (0x23, Code::KeyP),
            (0x24, Code::Enter),
            (0x25, Code::KeyL),
            (0x26, Code::KeyJ),
            (0x27, Code::Quote),
            (0x28, Code::KeyK),
            (0x29, Code::Semicolon),
            (0x2A, Code::Backslash),
            (0x2B, Code::Comma),
            (0x2C, Code::Slash),
            (0x2D, Code::KeyN),
            (0x2E, Code::KeyM),
            (0x2F, Code::Period),
            (0x30, Code::Tab),
            (0x31, Code::Space),
            (0x32, Code::Backquote),
            (0x33, Code::Backspace),
            (0x35, Code::Escape),
            (0x36, Code::MetaRight),
            (0x37, Code::MetaLeft),
            (0x38, Code::ShiftLeft),
            (0x39, Code::CapsLock),
            (0x3A, Code::AltLeft),
            (0x3B, Code::ControlLeft),
            (0x3C, Code::ShiftRight),
            (0x3D, Code::AltRight),
            (0x3E, Code::ControlRight),
            (0x3F, Code::Fn),
            (0x40, Code::F17),
            (0x41, Code::NumpadDecimal),
            (0x43, Code::NumpadMultiply),
            (0x45, Code::NumpadAdd),
            (0x47, Code::NumLock),
            (0x4B, Code::NumpadDivide),
            (0x4C, Code::NumpadEnter),
            (0x4E, Code::NumpadSubtract),
            (0x4F, Code::F18),
            (0x50, Code::F19),
            (0x51, Code::NumpadEqual),
            (0x52, Code::Numpad0),
            (0x53, Code::Numpad1),
            (0x54, Code::Numpad2),
            (0x55, Code::Numpad3),
            (0x56, Code::Numpad4),
            (0x57, Code::Numpad5),
            (0x58, Code::Numpad6),
            (0x59, Code::Numpad7),
            (0x5A, Code::F20),
            (0x5B, Code::Numpad8),
            (0x5C, Code::Numpad9),
            (0x5D, Code::IntlYen),
            (0x60, Code::F5),
            (0x61, Code::F6),
            (0x62, Code::F7),
            (0x63, Code::F3),
            (0x64, Code::F8),
            (0x65, Code::F9),
            (0x67, Code::F11),
            (0x69, Code::F13),
            (0x6A, Code::F16),
            (0x6B, Code::F14),
            (0x6D, Code::F10),
            (0x6F, Code::F12),
            (0x71, Code::F15),
            (0x72, Code::Insert),
            (0x73, Code::Home),
            (0x74, Code::PageUp),
            (0x75, Code::Delete),
            (0x76, Code::F4),
            (0x77, Code::End),
            (0x78, Code::F2),
            (0x79, Code::PageDown),
            (0x7A, Code::F1),
            (0x7B, Code::ArrowLeft),
            (0x7C, Code::ArrowRight),
            (0x7D, Code::ArrowDown),
            (0x7E, Code::ArrowUp),
        ];
        for &(keycode, code) in winit_backend_codes {
            assert_eq!(keycode_to_code(keycode), code, "keycode {keycode:#04x}");
        }
    }

    /// An unknown keycode degrades to `Unidentified`/characters, never a
    /// wrong key.
    #[test]
    fn unknown_keycodes_degrade_gracefully() {
        assert_eq!(keycode_to_code(0x7F), Code::Unidentified);
        assert_eq!(keycode_to_code(0xFFFF), Code::Unidentified);
        assert_eq!(keycode_to_key(0x7F), None);
    }
}
