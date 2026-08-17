//! Win32 keyboard translation decisions.
//!
//! Everything the Win32 backend decides about a keystroke — which `Key` a
//! virtual-key code falls back to, which W3C `Code` a scancode names, whether
//! a `WM_CHAR` burst is typeable text, and how that text merges into the
//! key-down event — lives here as pure functions over raw integers.
//!
//! This module is deliberately free of `cfg` gates and platform types so the
//! per-decision tables compile — and their tests execute — on any host, even
//! though the Win32 caller (`platforms/windows`) itself is only ever
//! clippy-linted in CI, never linked or run.
//!
//! # The WM_CHAR pairing model
//!
//! Win32 splits one keystroke across two wire messages: `WM_KEYDOWN` carries
//! the key identity (virtual key + scancode), and `TranslateMessage` posts a
//! follow-up `WM_CHAR` carrying the fully-translated character — shift, caps
//! lock, AltGr, and dead-key composition applied, with characters outside the
//! BMP split into two `WM_CHAR`s (a UTF-16 surrogate pair). Because the
//! message loop runs `TranslateMessage` *before* `DispatchMessageW`, the
//! `WM_CHAR` burst for a keydown is already in the thread queue when the
//! window procedure sees the `WM_KEYDOWN` — so the backend drains it
//! synchronously and merges it into the keydown's `key` via
//! [`merge_wm_char`], producing ONE `KeyboardEvent` per physical press. That
//! is the contract the shared consumers are built on: text is typed from
//! `Key::Character` on a `KeyState::Down` event (`flui-widgets`'
//! `EditableText` key handler), so dispatching the character as a second,
//! separate event would double-type every key the fallback table already
//! names. The winit Windows backend performs the same merge with its own
//! queue peek (winit 0.30 `platform_impl/windows/keyboard.rs`).
//!
//! A `WM_CHAR` with no owning keydown still exists — Alt+numpad composition
//! is queued by `TranslateMessage` of the Alt *keyup* — and is assembled by
//! [`assemble_stray_wm_char`] into its own event instead of being dropped.

use keyboard_types::{Code, Key, Location, NamedKey};

// Virtual-key codes, from `winuser.h`. Raw `u16` rather than the `windows`
// crate's `VIRTUAL_KEY` newtype so this module compiles on every host.
mod vk {
    pub const BACK: u16 = 0x08;
    pub const TAB: u16 = 0x09;
    pub const RETURN: u16 = 0x0D;
    pub const SHIFT: u16 = 0x10;
    pub const CONTROL: u16 = 0x11;
    pub const MENU: u16 = 0x12;
    pub const PAUSE: u16 = 0x13;
    pub const CAPITAL: u16 = 0x14;
    pub const ESCAPE: u16 = 0x1B;
    pub const SPACE: u16 = 0x20;
    pub const PRIOR: u16 = 0x21;
    pub const NEXT: u16 = 0x22;
    pub const END: u16 = 0x23;
    pub const HOME: u16 = 0x24;
    pub const LEFT: u16 = 0x25;
    pub const UP: u16 = 0x26;
    pub const RIGHT: u16 = 0x27;
    pub const DOWN: u16 = 0x28;
    pub const SNAPSHOT: u16 = 0x2C;
    pub const INSERT: u16 = 0x2D;
    pub const DELETE: u16 = 0x2E;
    pub const KEY_0: u16 = 0x30;
    pub const KEY_9: u16 = 0x39;
    pub const KEY_A: u16 = 0x41;
    pub const KEY_Z: u16 = 0x5A;
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;
    pub const APPS: u16 = 0x5D;
    pub const NUMPAD0: u16 = 0x60;
    pub const NUMPAD9: u16 = 0x69;
    pub const MULTIPLY: u16 = 0x6A;
    pub const ADD: u16 = 0x6B;
    pub const SUBTRACT: u16 = 0x6D;
    pub const DECIMAL: u16 = 0x6E;
    pub const DIVIDE: u16 = 0x6F;
    pub const F1: u16 = 0x70;
    pub const F12: u16 = 0x7B;
    pub const NUMLOCK: u16 = 0x90;
    pub const SCROLL: u16 = 0x91;
    pub const LSHIFT: u16 = 0xA0;
    pub const RSHIFT: u16 = 0xA1;
    pub const LCONTROL: u16 = 0xA2;
    pub const RCONTROL: u16 = 0xA3;
    pub const LMENU: u16 = 0xA4;
    pub const RMENU: u16 = 0xA5;
    pub const OEM_1: u16 = 0xBA;
    pub const OEM_PLUS: u16 = 0xBB;
    pub const OEM_COMMA: u16 = 0xBC;
    pub const OEM_MINUS: u16 = 0xBD;
    pub const OEM_PERIOD: u16 = 0xBE;
    pub const OEM_2: u16 = 0xBF;
    pub const OEM_3: u16 = 0xC0;
    pub const OEM_4: u16 = 0xDB;
    pub const OEM_5: u16 = 0xDC;
    pub const OEM_6: u16 = 0xDD;
    pub const OEM_7: u16 = 0xDE;
    pub const OEM_102: u16 = 0xE2;
}

/// Split a `WM_KEYDOWN`/`WM_KEYUP` `lParam` into the fields the translation
/// needs: the 8-bit scancode (bits 16–23), the extended-key flag (bit 24,
/// the `E0` prefix that distinguishes e.g. arrow keys from the numpad), and
/// the previous-key-state repeat flag (bit 30, meaningful on keydown only).
///
/// Layout per the `WM_KEYDOWN` docs:
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-keydown>
pub fn parse_key_lparam(lparam: isize) -> (u16, bool, bool) {
    let scancode = ((lparam >> 16) & 0xFF) as u16;
    let extended = (lparam >> 24) & 1 == 1;
    let repeat = (lparam >> 30) & 1 == 1;
    (scancode, extended, repeat)
}

/// The layout-independent `Key` a virtual-key code falls back to when no
/// `WM_CHAR` text is available for the event — key-ups, and chords whose
/// translation is a control character (Ctrl+S delivers `WM_CHAR` `0x13`,
/// which [`wm_char_text`] rejects, so shortcut matching still sees `"s"`).
///
/// `shift` upcases the letter fallbacks so a Ctrl+Shift+S chord reports
/// `"S"`, matching the W3C `key` value and the winit backend's
/// `logical_key`. Punctuation (`OEM_*`) uses the US-layout positions — the
/// only layout-independent choice available without the live keyboard
/// layout; a real typed character never takes this path because the
/// `WM_CHAR` merge wins.
pub fn vk_to_key(vk_code: u16, shift: bool) -> Key {
    use Key as K;

    match vk_code {
        vk::RETURN => K::Named(NamedKey::Enter),
        vk::TAB => K::Named(NamedKey::Tab),
        vk::SPACE => K::Character(" ".into()),
        vk::BACK => K::Named(NamedKey::Backspace),
        vk::DELETE => K::Named(NamedKey::Delete),
        vk::ESCAPE => K::Named(NamedKey::Escape),

        vk::LEFT => K::Named(NamedKey::ArrowLeft),
        vk::RIGHT => K::Named(NamedKey::ArrowRight),
        vk::UP => K::Named(NamedKey::ArrowUp),
        vk::DOWN => K::Named(NamedKey::ArrowDown),

        vk::HOME => K::Named(NamedKey::Home),
        vk::END => K::Named(NamedKey::End),
        vk::PRIOR => K::Named(NamedKey::PageUp),
        vk::NEXT => K::Named(NamedKey::PageDown),
        vk::INSERT => K::Named(NamedKey::Insert),

        vk::CAPITAL => K::Named(NamedKey::CapsLock),
        vk::NUMLOCK => K::Named(NamedKey::NumLock),
        vk::SCROLL => K::Named(NamedKey::ScrollLock),
        vk::SNAPSHOT => K::Named(NamedKey::PrintScreen),
        vk::PAUSE => K::Named(NamedKey::Pause),
        vk::APPS => K::Named(NamedKey::ContextMenu),

        // WM_KEYDOWN delivers the GENERIC modifier VKs (VK_SHIFT etc.);
        // the left/right VKs only appear via GetKeyState-style queries,
        // but both spellings map to the same W3C `key` value anyway —
        // sidedness is the `code`'s job.
        vk::SHIFT | vk::LSHIFT | vk::RSHIFT => K::Named(NamedKey::Shift),
        vk::CONTROL | vk::LCONTROL | vk::RCONTROL => K::Named(NamedKey::Control),
        vk::MENU | vk::LMENU | vk::RMENU => K::Named(NamedKey::Alt),
        vk::LWIN | vk::RWIN => K::Named(NamedKey::Meta),

        // VK_A..=VK_Z are the ASCII uppercase letters.
        c @ vk::KEY_A..=vk::KEY_Z => {
            let ch = if shift {
                c as u8 as char
            } else {
                (c as u8 as char).to_ascii_lowercase()
            };
            K::Character(ch.to_string())
        }
        // VK_0..=VK_9 are the ASCII digits. Their shifted values are
        // layout-dependent, so no `shift` handling: the WM_CHAR merge is
        // what delivers `!`/`@`/… on a real press.
        c @ vk::KEY_0..=vk::KEY_9 => K::Character((c as u8 as char).to_string()),

        // Numpad digits and operators (delivered with NumLock on).
        c @ vk::NUMPAD0..=vk::NUMPAD9 => {
            K::Character(((b'0' + (c - vk::NUMPAD0) as u8) as char).to_string())
        }
        vk::MULTIPLY => K::Character("*".into()),
        vk::ADD => K::Character("+".into()),
        vk::SUBTRACT => K::Character("-".into()),
        vk::DECIMAL => K::Character(".".into()),
        vk::DIVIDE => K::Character("/".into()),

        c @ vk::F1..=vk::F12 => {
            let named = [
                NamedKey::F1,
                NamedKey::F2,
                NamedKey::F3,
                NamedKey::F4,
                NamedKey::F5,
                NamedKey::F6,
                NamedKey::F7,
                NamedKey::F8,
                NamedKey::F9,
                NamedKey::F10,
                NamedKey::F11,
                NamedKey::F12,
            ][(c - vk::F1) as usize];
            K::Named(named)
        }

        // OEM punctuation, US-layout positions (unshifted).
        vk::OEM_1 => K::Character(";".into()),
        vk::OEM_PLUS => K::Character("=".into()),
        vk::OEM_COMMA => K::Character(",".into()),
        vk::OEM_MINUS => K::Character("-".into()),
        vk::OEM_PERIOD => K::Character(".".into()),
        vk::OEM_2 => K::Character("/".into()),
        vk::OEM_3 => K::Character("`".into()),
        vk::OEM_4 => K::Character("[".into()),
        vk::OEM_5 | vk::OEM_102 => K::Character("\\".into()),
        vk::OEM_6 => K::Character("]".into()),
        vk::OEM_7 => K::Character("'".into()),

        _ => K::Named(NamedKey::Unidentified),
    }
}

/// Map a Win32 Set-1 make scancode (+ the lParam extended flag) to the W3C
/// `code` value naming the physical key.
///
/// The table follows the W3C registry
/// (<https://www.w3.org/TR/uievents-code/>) as implemented by the winit
/// Windows backend and Firefox's `NativeKeyToDOMCodeName.h`, including the
/// legacy quirks those sources agree on: `Pause` reports `0x45`
/// (`0xE046` when Ctrl is held), `NumLock` reports the *extended* `0xE045`,
/// and `PrintScreen` reports `0xE037` (`0x54` when Alt is held).
pub fn scancode_to_code(scancode: u16, extended: bool) -> Code {
    let ext_scancode = if extended {
        0xE000 | u32::from(scancode)
    } else {
        u32::from(scancode)
    };

    match ext_scancode {
        0x0001 => Code::Escape,
        0x0002 => Code::Digit1,
        0x0003 => Code::Digit2,
        0x0004 => Code::Digit3,
        0x0005 => Code::Digit4,
        0x0006 => Code::Digit5,
        0x0007 => Code::Digit6,
        0x0008 => Code::Digit7,
        0x0009 => Code::Digit8,
        0x000A => Code::Digit9,
        0x000B => Code::Digit0,
        0x000C => Code::Minus,
        0x000D => Code::Equal,
        0x000E => Code::Backspace,
        0x000F => Code::Tab,
        0x0010 => Code::KeyQ,
        0x0011 => Code::KeyW,
        0x0012 => Code::KeyE,
        0x0013 => Code::KeyR,
        0x0014 => Code::KeyT,
        0x0015 => Code::KeyY,
        0x0016 => Code::KeyU,
        0x0017 => Code::KeyI,
        0x0018 => Code::KeyO,
        0x0019 => Code::KeyP,
        0x001A => Code::BracketLeft,
        0x001B => Code::BracketRight,
        0x001C => Code::Enter,
        0x001D => Code::ControlLeft,
        0x001E => Code::KeyA,
        0x001F => Code::KeyS,
        0x0020 => Code::KeyD,
        0x0021 => Code::KeyF,
        0x0022 => Code::KeyG,
        0x0023 => Code::KeyH,
        0x0024 => Code::KeyJ,
        0x0025 => Code::KeyK,
        0x0026 => Code::KeyL,
        0x0027 => Code::Semicolon,
        0x0028 => Code::Quote,
        0x0029 => Code::Backquote,
        0x002A => Code::ShiftLeft,
        0x002B => Code::Backslash,
        0x002C => Code::KeyZ,
        0x002D => Code::KeyX,
        0x002E => Code::KeyC,
        0x002F => Code::KeyV,
        0x0030 => Code::KeyB,
        0x0031 => Code::KeyN,
        0x0032 => Code::KeyM,
        0x0033 => Code::Comma,
        0x0034 => Code::Period,
        0x0035 => Code::Slash,
        0x0036 => Code::ShiftRight,
        0x0037 => Code::NumpadMultiply,
        0x0038 => Code::AltLeft,
        0x0039 => Code::Space,
        0x003A => Code::CapsLock,
        0x003B => Code::F1,
        0x003C => Code::F2,
        0x003D => Code::F3,
        0x003E => Code::F4,
        0x003F => Code::F5,
        0x0040 => Code::F6,
        0x0041 => Code::F7,
        0x0042 => Code::F8,
        0x0043 => Code::F9,
        0x0044 => Code::F10,
        // 0xE046 is the make code the same key reports while Ctrl is held
        // (Ctrl+Pause = Break).
        0x0045 | 0xE046 => Code::Pause,
        0x0046 => Code::ScrollLock,
        0x0047 => Code::Numpad7,
        0x0048 => Code::Numpad8,
        0x0049 => Code::Numpad9,
        0x004A => Code::NumpadSubtract,
        0x004B => Code::Numpad4,
        0x004C => Code::Numpad5,
        0x004D => Code::Numpad6,
        0x004E => Code::NumpadAdd,
        0x004F => Code::Numpad1,
        0x0050 => Code::Numpad2,
        0x0051 => Code::Numpad3,
        0x0052 => Code::Numpad0,
        0x0053 => Code::NumpadDecimal,
        // 0x54 is the make code the same key reports while Alt is held
        // (Alt+PrintScreen = SysRq); 0xE037 is the ordinary press.
        0x0054 | 0xE037 => Code::PrintScreen,
        0x0056 => Code::IntlBackslash,
        0x0057 => Code::F11,
        0x0058 => Code::F12,
        0x0059 => Code::NumpadEqual,
        0x0064 => Code::F13,
        0x0065 => Code::F14,
        0x0066 => Code::F15,
        0x0067 => Code::F16,
        0x0068 => Code::F17,
        0x0069 => Code::F18,
        0x006A => Code::F19,
        0x006B => Code::F20,
        0x006C => Code::F21,
        0x006D => Code::F22,
        0x006E => Code::F23,
        0x0070 => Code::KanaMode,
        0x0071 => Code::Lang2,
        0x0072 => Code::Lang1,
        0x0073 => Code::IntlRo,
        0x0076 => Code::F24,
        0x0079 => Code::Convert,
        0x007B => Code::NonConvert,
        0x007D => Code::IntlYen,
        0x007E => Code::NumpadComma,

        0xE010 => Code::MediaTrackPrevious,
        0xE019 => Code::MediaTrackNext,
        0xE01C => Code::NumpadEnter,
        0xE01D => Code::ControlRight,
        0xE020 => Code::AudioVolumeMute,
        0xE021 => Code::LaunchApp2,
        0xE022 => Code::MediaPlayPause,
        0xE024 => Code::MediaStop,
        0xE02E => Code::AudioVolumeDown,
        0xE030 => Code::AudioVolumeUp,
        0xE032 => Code::BrowserHome,
        0xE035 => Code::NumpadDivide,
        0xE038 => Code::AltRight,
        0xE045 => Code::NumLock,
        0xE047 => Code::Home,
        0xE048 => Code::ArrowUp,
        0xE049 => Code::PageUp,
        0xE04B => Code::ArrowLeft,
        0xE04D => Code::ArrowRight,
        0xE04F => Code::End,
        0xE050 => Code::ArrowDown,
        0xE051 => Code::PageDown,
        0xE052 => Code::Insert,
        0xE053 => Code::Delete,
        0xE05B => Code::MetaLeft,
        0xE05C => Code::MetaRight,
        0xE05D => Code::ContextMenu,
        0xE05E => Code::Power,
        0xE065 => Code::BrowserSearch,
        0xE066 => Code::BrowserFavorites,
        0xE067 => Code::BrowserRefresh,
        0xE068 => Code::BrowserStop,
        0xE069 => Code::BrowserForward,
        0xE06A => Code::BrowserBack,
        0xE06B => Code::LaunchApp1,
        0xE06C => Code::LaunchMail,
        0xE06D => Code::MediaSelect,

        _ => Code::Unidentified,
    }
}

/// The W3C `location` a `code` implies: numpad keys report `Numpad`, sided
/// modifiers report their side, everything else is `Standard`. Mirrors the
/// winit backend's location derivation so the two Windows wires agree.
pub fn location_for_code(code: Code) -> Location {
    match code {
        Code::ShiftLeft | Code::ControlLeft | Code::AltLeft | Code::MetaLeft => Location::Left,
        Code::ShiftRight | Code::ControlRight | Code::AltRight | Code::MetaRight => Location::Right,
        Code::Numpad0
        | Code::Numpad1
        | Code::Numpad2
        | Code::Numpad3
        | Code::Numpad4
        | Code::Numpad5
        | Code::Numpad6
        | Code::Numpad7
        | Code::Numpad8
        | Code::Numpad9
        | Code::NumpadEnter
        | Code::NumpadAdd
        | Code::NumpadSubtract
        | Code::NumpadMultiply
        | Code::NumpadDivide
        | Code::NumpadDecimal
        | Code::NumpadComma
        | Code::NumpadEqual
        | Code::NumLock => Location::Numpad,
        _ => Location::Standard,
    }
}

/// Decode a keydown's `WM_CHAR` burst (the UTF-16 code units of every
/// `WM_CHAR` `TranslateMessage` queued for it, in order) into typeable text.
///
/// Returns `None` — meaning "keep the virtual-key fallback" — when the burst
/// is empty (no translation: arrows, F-keys, dead-key presses), is not valid
/// UTF-16 (a stranded surrogate half), or contains any control character:
/// Ctrl+letter chords translate to C0 codes (Ctrl+S → `0x13`), and Enter /
/// Tab / Backspace / Escape translate to `\r` / `\t` / `0x08` / `0x1B`,
/// none of which are text — the named-key or letter fallback is the right
/// event for all of them.
pub fn wm_char_text(units: &[u16]) -> Option<String> {
    if units.is_empty() {
        return None;
    }
    let text = String::from_utf16(units).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text)
}

/// The merge decision itself: translated `WM_CHAR` text wins over the
/// case-folded virtual-key fallback; absent (or rejected) text keeps the
/// fallback. This one line is why Shift+A types `A` while Ctrl+S still
/// matches a `"s"` shortcut.
pub fn merge_wm_char(fallback: Key, text: Option<String>) -> Key {
    match text {
        Some(text) => Key::Character(text),
        None => fallback,
    }
}

/// The complete `key` decision for a Win32 keydown: the [`merge_wm_char`]
/// merge, plus the Alt+numpad composition guard.
///
/// While Alt is held without Ctrl, a numpad-digit keydown that produced no
/// `WM_CHAR` is a step of Windows' Alt+numpad character entry, not a
/// keystroke of its own: the digits type nothing (their character arrives
/// composed, as an out-of-band `WM_CHAR` on Alt release — see
/// [`assemble_stray_wm_char`]), so surfacing the digit fallback would type
/// `65` before Alt+65's `A`, and — because NumLock-off composition reports
/// the navigation VKs on the same physical keys — surfacing a `Home`/arrow
/// fallback would fire caret movement mid-composition. Such a keydown
/// reports `Unidentified` instead. The guard is keyed on the physical
/// numpad cluster (`Code::Numpad0..=Numpad9`), which is NumLock-invariant;
/// Ctrl+Alt is excluded because that chord is AltGr, whose translated text
/// arrives on the keydown itself and must keep winning.
pub fn key_for_keydown(
    fallback: Key,
    text: Option<String>,
    alt: bool,
    ctrl: bool,
    code: Code,
) -> Key {
    let numpad_digit = matches!(
        code,
        Code::Numpad0
            | Code::Numpad1
            | Code::Numpad2
            | Code::Numpad3
            | Code::Numpad4
            | Code::Numpad5
            | Code::Numpad6
            | Code::Numpad7
            | Code::Numpad8
            | Code::Numpad9
    );
    if alt && !ctrl && text.is_none() && numpad_digit {
        return Key::Named(NamedKey::Unidentified);
    }
    merge_wm_char(fallback, text)
}

/// Fold one out-of-band `WM_CHAR` code unit (no owning keydown — Alt+numpad
/// composition, or a directly-sent message) into at most one completed text.
///
/// Returns `(pending_high_surrogate, completed_text)`: a high surrogate is
/// held for the low half that follows in the next message; everything else
/// completes immediately, through the same typeability filter as
/// [`wm_char_text`]. A malformed sequence (lone or mismatched half) yields
/// neither state nor text.
pub fn assemble_stray_wm_char(
    pending_high: Option<u16>,
    unit: u16,
) -> (Option<u16>, Option<String>) {
    if (0xD800..=0xDBFF).contains(&unit) {
        // Start of a surrogate pair: hold it (a preceding unpaired high
        // surrogate is malformed and dropped).
        (Some(unit), None)
    } else if (0xDC00..=0xDFFF).contains(&unit) {
        match pending_high {
            // Completion of a held pair.
            Some(high) => (None, wm_char_text(&[high, unit])),
            // A low half with nothing held is malformed.
            None => (None, None),
        }
    } else {
        // A plain BMP unit; any held high surrogate was left unpaired.
        (None, wm_char_text(&[unit]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> Key {
        Key::Character(s.into())
    }

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// The bug this module exists for: `vk_to_key` case-folds, so without
    /// the WM_CHAR merge a shifted press is untypeable. The translated
    /// character — whatever produced it (Shift, CapsLock, AltGr, a dead
    /// key) — must win over the fallback.
    #[test]
    fn translated_wm_char_text_wins_over_the_case_folded_fallback() {
        // Shift+A: keydown VK_A falls back to "a", WM_CHAR delivers "A".
        assert_eq!(
            merge_wm_char(vk_to_key(0x41, false), wm_char_text(&utf16("A"))),
            ch("A")
        );
        // AltGr on a German layout: VK_Q's fallback is "q", WM_CHAR "@".
        assert_eq!(
            merge_wm_char(vk_to_key(0x51, false), wm_char_text(&utf16("@"))),
            ch("@")
        );
        // Dead-key composition: the composed char rides the FOLLOWING
        // keydown (VK_E), whose fallback is "e".
        assert_eq!(
            merge_wm_char(vk_to_key(0x45, false), wm_char_text(&utf16("é"))),
            ch("é")
        );
    }

    /// Ctrl chords translate to C0 control codes; treating those as text
    /// would both type garbage and hide the letter from shortcut matching.
    /// The burst is rejected wholesale and the fallback survives.
    #[test]
    fn control_character_bursts_keep_the_vk_fallback() {
        // Ctrl+S delivers WM_CHAR 0x13.
        assert_eq!(wm_char_text(&[0x13]), None);
        assert_eq!(merge_wm_char(vk_to_key(0x53, false), None), ch("s"));
        // Ctrl+Shift+S: same rejection, but the fallback respects shift.
        assert_eq!(merge_wm_char(vk_to_key(0x53, true), None), ch("S"));
        // Enter, Tab, Backspace, Escape all translate to control chars;
        // their named keys must survive the merge.
        for (unit, vk_code, key) in [
            (0x0D_u16, 0x0D_u16, NamedKey::Enter),
            (0x09, 0x09, NamedKey::Tab),
            (0x08, 0x08, NamedKey::Backspace),
            (0x1B, 0x1B, NamedKey::Escape),
        ] {
            assert_eq!(wm_char_text(&[unit]), None, "unit {unit:#x} is control");
            assert_eq!(
                merge_wm_char(vk_to_key(vk_code, false), wm_char_text(&[unit])),
                Key::Named(key)
            );
        }
        // Space is NOT a control character: it is real text.
        assert_eq!(wm_char_text(&[0x20]).as_deref(), Some(" "));
    }

    /// Characters outside the BMP arrive as two WM_CHARs (a surrogate
    /// pair); the drained burst must reassemble them, and a stranded half
    /// must reject rather than panic or replace.
    #[test]
    fn surrogate_pairs_reassemble_and_stranded_halves_reject() {
        assert_eq!(wm_char_text(&[0xD83D, 0xDE00]).as_deref(), Some("😀"));
        assert_eq!(wm_char_text(&[0xD83D]), None);
        assert_eq!(wm_char_text(&[]), None);
    }

    /// Alt+numpad character entry: while Alt (without Ctrl) is held, the
    /// intermediate numpad keydowns are composition input, not keystrokes —
    /// they produce no WM_CHAR of their own, and the composed character
    /// arrives as an out-of-band WM_CHAR on Alt release. Letting the digit
    /// fallbacks through would make Alt+6 5 type "65" into a text field
    /// (and, with NumLock off, fire Home/End/arrow editing actions mid-
    /// composition) before the composed character lands, so a numpad-
    /// cluster keydown under plain Alt with no translated text must not
    /// surface a typeable or editing key at all.
    #[test]
    fn alt_numpad_composition_steps_do_not_surface_keys() {
        // NumLock on: VK_NUMPAD6, scancode 0x4D non-extended → Numpad6.
        let fallback = vk_to_key(0x66, false); // VK_NUMPAD6 → "6"
        assert_eq!(
            key_for_keydown(fallback, None, true, false, Code::Numpad6),
            Key::Named(NamedKey::Unidentified),
            "an Alt+numpad digit with no translation is a composition step"
        );
        // NumLock off: the same physical key reports VK_HOME → Named(Home);
        // Code is still Numpad7, and a Home fallback would move the caret.
        assert_eq!(
            key_for_keydown(Key::Named(NamedKey::Home), None, true, false, Code::Numpad7),
            Key::Named(NamedKey::Unidentified),
            "NumLock-off composition digits must not surface editing keys"
        );
        // AltGr (Ctrl+Alt) is NOT composition: translated text still wins.
        assert_eq!(
            key_for_keydown(ch("q"), Some("@".into()), true, true, Code::KeyQ),
            ch("@")
        );
        // A numpad digit without Alt types normally.
        assert_eq!(
            key_for_keydown(ch("6"), Some("6".into()), false, false, Code::Numpad6),
            ch("6")
        );
        // Alt over a NON-numpad key keeps its fallback (mnemonic/shortcut
        // identity, same as the winit wire's logical_key).
        assert_eq!(
            key_for_keydown(ch("s"), None, true, false, Code::KeyS),
            ch("s")
        );
    }

    /// The out-of-band WM_CHAR path (Alt+numpad composition) assembles one
    /// unit at a time across messages.
    #[test]
    fn stray_wm_chars_assemble_across_messages() {
        // BMP character completes immediately.
        assert_eq!(
            assemble_stray_wm_char(None, 0x00FC),
            (None, Some("ü".into()))
        );
        // Surrogate pair across two messages.
        let (pending, none) = assemble_stray_wm_char(None, 0xD83D);
        assert_eq!((pending, none), (Some(0xD83D), None));
        assert_eq!(
            assemble_stray_wm_char(pending, 0xDE00),
            (None, Some("😀".into()))
        );
        // Control unit: rejected, no state kept.
        assert_eq!(assemble_stray_wm_char(None, 0x0008), (None, None));
        // Lone low half is malformed.
        assert_eq!(assemble_stray_wm_char(None, 0xDE00), (None, None));
        // A second high surrogate replaces an unpaired one.
        assert_eq!(
            assemble_stray_wm_char(Some(0xD83D), 0xD83E),
            (Some(0xD83E), None)
        );
    }

    /// The named-key half of the VK fallback table, including the keys the
    /// winit backend names that this table used to drop to `Unidentified`.
    #[test]
    fn vk_fallback_names_the_standard_named_keys() {
        for (vk_code, key) in [
            (0x0D_u16, NamedKey::Enter),
            (0x25, NamedKey::ArrowLeft),
            (0x70, NamedKey::F1),
            (0x7B, NamedKey::F12),
            (0x14, NamedKey::CapsLock),
            (0x90, NamedKey::NumLock),
            (0x91, NamedKey::ScrollLock),
            (0x2C, NamedKey::PrintScreen),
            (0x13, NamedKey::Pause),
            (0x5D, NamedKey::ContextMenu),
            // The generic modifier VKs WM_KEYDOWN actually delivers.
            (0x10, NamedKey::Shift),
            (0x11, NamedKey::Control),
            (0x12, NamedKey::Alt),
            (0x5B, NamedKey::Meta),
        ] {
            assert_eq!(
                vk_to_key(vk_code, false),
                Key::Named(key),
                "vk {vk_code:#x}"
            );
        }
    }

    /// The character half of the VK fallback table: numpad and US-layout
    /// OEM punctuation no longer fall to `Unidentified`.
    #[test]
    fn vk_fallback_covers_numpad_and_oem_punctuation() {
        assert_eq!(vk_to_key(0x60, false), ch("0")); // VK_NUMPAD0
        assert_eq!(vk_to_key(0x69, false), ch("9")); // VK_NUMPAD9
        assert_eq!(vk_to_key(0x6A, false), ch("*"));
        assert_eq!(vk_to_key(0x6B, false), ch("+"));
        assert_eq!(vk_to_key(0x6D, false), ch("-"));
        assert_eq!(vk_to_key(0x6E, false), ch("."));
        assert_eq!(vk_to_key(0x6F, false), ch("/"));
        assert_eq!(vk_to_key(0xBA, false), ch(";")); // VK_OEM_1
        assert_eq!(vk_to_key(0xBB, false), ch("="));
        assert_eq!(vk_to_key(0xBC, false), ch(","));
        assert_eq!(vk_to_key(0xBD, false), ch("-"));
        assert_eq!(vk_to_key(0xBE, false), ch("."));
        assert_eq!(vk_to_key(0xBF, false), ch("/"));
        assert_eq!(vk_to_key(0xC0, false), ch("`"));
        assert_eq!(vk_to_key(0xDB, false), ch("["));
        assert_eq!(vk_to_key(0xDC, false), ch("\\"));
        assert_eq!(vk_to_key(0xDD, false), ch("]"));
        assert_eq!(vk_to_key(0xDE, false), ch("'"));
        assert_eq!(vk_to_key(0xE2, false), ch("\\")); // VK_OEM_102
        // Digits ignore shift (shifted values are layout-dependent).
        assert_eq!(vk_to_key(0x31, true), ch("1"));
    }

    /// The scancode quirk keys, exactly as the W3C registry / winit /
    /// Firefox agree on them: Pause is the NON-extended 0x45, NumLock the
    /// EXTENDED 0x45, and PrintScreen has an Alt-variant make code.
    #[test]
    fn scancode_quirk_keys_resolve_correctly() {
        assert_eq!(scancode_to_code(0x45, false), Code::Pause);
        assert_eq!(scancode_to_code(0x45, true), Code::NumLock);
        assert_eq!(scancode_to_code(0x46, true), Code::Pause); // Ctrl+Pause
        assert_eq!(scancode_to_code(0x37, true), Code::PrintScreen);
        assert_eq!(scancode_to_code(0x54, false), Code::PrintScreen); // Alt+PrtSc
    }

    /// The extended bit is what separates the editing/arrow cluster from
    /// the numpad, and the right-hand modifiers from the left.
    #[test]
    fn extended_bit_separates_numpad_from_navigation() {
        assert_eq!(scancode_to_code(0x1C, false), Code::Enter);
        assert_eq!(scancode_to_code(0x1C, true), Code::NumpadEnter);
        assert_eq!(scancode_to_code(0x35, false), Code::Slash);
        assert_eq!(scancode_to_code(0x35, true), Code::NumpadDivide);
        assert_eq!(scancode_to_code(0x48, false), Code::Numpad8);
        assert_eq!(scancode_to_code(0x48, true), Code::ArrowUp);
        assert_eq!(scancode_to_code(0x53, false), Code::NumpadDecimal);
        assert_eq!(scancode_to_code(0x53, true), Code::Delete);
        assert_eq!(scancode_to_code(0x1D, false), Code::ControlLeft);
        assert_eq!(scancode_to_code(0x1D, true), Code::ControlRight);
        assert_eq!(scancode_to_code(0x38, false), Code::AltLeft);
        assert_eq!(scancode_to_code(0x38, true), Code::AltRight);
    }

    /// Everything the winit backend's `Code` table can produce, this table
    /// produces from the corresponding Win32 scancode — the two Windows
    /// wires must agree on physical-key identity.
    #[test]
    fn covers_every_code_the_winit_backend_produces() {
        let winit_backend_codes: &[(u16, bool, Code)] = &[
            (0x1E, false, Code::KeyA),
            (0x30, false, Code::KeyB),
            (0x2E, false, Code::KeyC),
            (0x20, false, Code::KeyD),
            (0x12, false, Code::KeyE),
            (0x21, false, Code::KeyF),
            (0x22, false, Code::KeyG),
            (0x23, false, Code::KeyH),
            (0x17, false, Code::KeyI),
            (0x24, false, Code::KeyJ),
            (0x25, false, Code::KeyK),
            (0x26, false, Code::KeyL),
            (0x32, false, Code::KeyM),
            (0x31, false, Code::KeyN),
            (0x18, false, Code::KeyO),
            (0x19, false, Code::KeyP),
            (0x10, false, Code::KeyQ),
            (0x13, false, Code::KeyR),
            (0x1F, false, Code::KeyS),
            (0x14, false, Code::KeyT),
            (0x16, false, Code::KeyU),
            (0x2F, false, Code::KeyV),
            (0x11, false, Code::KeyW),
            (0x2D, false, Code::KeyX),
            (0x15, false, Code::KeyY),
            (0x2C, false, Code::KeyZ),
            (0x0B, false, Code::Digit0),
            (0x02, false, Code::Digit1),
            (0x03, false, Code::Digit2),
            (0x04, false, Code::Digit3),
            (0x05, false, Code::Digit4),
            (0x06, false, Code::Digit5),
            (0x07, false, Code::Digit6),
            (0x08, false, Code::Digit7),
            (0x09, false, Code::Digit8),
            (0x0A, false, Code::Digit9),
            (0x1C, false, Code::Enter),
            (0x01, false, Code::Escape),
            (0x0E, false, Code::Backspace),
            (0x0F, false, Code::Tab),
            (0x39, false, Code::Space),
            (0x2A, false, Code::ShiftLeft),
            (0x36, false, Code::ShiftRight),
            (0x1D, false, Code::ControlLeft),
            (0x1D, true, Code::ControlRight),
            (0x38, false, Code::AltLeft),
            (0x38, true, Code::AltRight),
            (0x5B, true, Code::MetaLeft),
            (0x5C, true, Code::MetaRight),
            (0x4B, true, Code::ArrowLeft),
            (0x4D, true, Code::ArrowRight),
            (0x48, true, Code::ArrowUp),
            (0x50, true, Code::ArrowDown),
            (0x47, true, Code::Home),
            (0x4F, true, Code::End),
            (0x49, true, Code::PageUp),
            (0x51, true, Code::PageDown),
            (0x52, true, Code::Insert),
            (0x53, true, Code::Delete),
            (0x3B, false, Code::F1),
            (0x3C, false, Code::F2),
            (0x3D, false, Code::F3),
            (0x3E, false, Code::F4),
            (0x3F, false, Code::F5),
            (0x40, false, Code::F6),
            (0x41, false, Code::F7),
            (0x42, false, Code::F8),
            (0x43, false, Code::F9),
            (0x44, false, Code::F10),
            (0x57, false, Code::F11),
            (0x58, false, Code::F12),
        ];
        for &(scancode, extended, code) in winit_backend_codes {
            assert_eq!(
                scancode_to_code(scancode, extended),
                code,
                "scancode {scancode:#04x} extended={extended}"
            );
        }
    }

    #[test]
    fn location_reports_numpad_and_sidedness() {
        assert_eq!(location_for_code(Code::Numpad5), Location::Numpad);
        assert_eq!(location_for_code(Code::NumpadEnter), Location::Numpad);
        assert_eq!(location_for_code(Code::ShiftLeft), Location::Left);
        assert_eq!(location_for_code(Code::ControlRight), Location::Right);
        assert_eq!(location_for_code(Code::KeyA), Location::Standard);
        assert_eq!(location_for_code(Code::Enter), Location::Standard);
    }

    /// The lParam bit layout: scancode in bits 16–23, extended flag bit 24,
    /// repeat flag bit 30.
    #[test]
    fn key_lparam_fields_unpack() {
        // VK_UP: scancode 0x48, extended, first press.
        let lparam = (0x48_isize << 16) | (1 << 24);
        assert_eq!(parse_key_lparam(lparam), (0x48, true, false));
        // Held 'a': scancode 0x1E, not extended, repeat.
        let lparam = (0x1E_isize << 16) | (1 << 30);
        assert_eq!(parse_key_lparam(lparam), (0x1E, false, true));
    }
}
