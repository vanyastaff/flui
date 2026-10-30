//! Win32 keyboard translation decisions.
//!
//! Everything the Win32 backend decides about a keystroke — which `Key` a
//! virtual-key code falls back to, which W3C `Code` a scancode names, whether
//! a `WM_CHAR` burst is typeable text, and how that text merges into the
//! key-down event — lives here as pure functions over raw integers, with the
//! virtual keys and `MK_*` bits of the mouse buttons ([`MOUSE_BUTTONS`]),
//! which OLE drag and drop reads too. Beside them is what every backend
//! shares: the one table of key locations (`location_of_number`,
//! [`location_for_code`]) and the adapter from `keyboard-types` the winit and
//! Android hosts read their keys through.
//!
//! This module is deliberately free of `cfg` gates and platform types so the
//! per-decision tables compile — and their tests execute — on any host, even
//! though the Win32 caller (`input::windows`) itself is only ever
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

use flui_platform::keyboard::{Code, Key, Location, NamedKey};
use flui_platform::pointer::PointerButton;

#[cfg(any(
    winit_host,
    wayland_host,
    x11_host,
    target_os = "windows",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
mod unplaced;
#[cfg(any(
    winit_host,
    wayland_host,
    x11_host,
    target_os = "windows",
    target_os = "macos",
    target_arch = "wasm32"
))]
pub(crate) use unplaced::UnplacedKeys;

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
    pub const PROCESSKEY: u16 = 0xE5;
}

/// One mouse button as Win32 names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseButtonCodes {
    /// Its virtual-key code (`VK_*`).
    pub virtual_key: u16,
    /// The `MK_*` bit a mouse message's `WPARAM` and OLE's key state carry
    /// for it.
    pub key_state_bit: u32,
    /// The button it is.
    pub button: PointerButton,
}

/// Each mouse button's Win32 codes. The Win32 input reads the buttons a
/// message holds from it, and OLE drag and drop the buttons whose release
/// drops a session.
pub const MOUSE_BUTTONS: [MouseButtonCodes; 5] = [
    // VK_LBUTTON, MK_LBUTTON
    MouseButtonCodes {
        virtual_key: 0x01,
        key_state_bit: 0x0001,
        button: PointerButton::PRIMARY,
    },
    // VK_RBUTTON, MK_RBUTTON
    MouseButtonCodes {
        virtual_key: 0x02,
        key_state_bit: 0x0002,
        button: PointerButton::SECONDARY,
    },
    // VK_MBUTTON, MK_MBUTTON
    MouseButtonCodes {
        virtual_key: 0x04,
        key_state_bit: 0x0010,
        button: PointerButton::AUXILIARY,
    },
    // VK_XBUTTON1, MK_XBUTTON1
    MouseButtonCodes {
        virtual_key: 0x05,
        key_state_bit: 0x0020,
        button: PointerButton::BACK,
    },
    // VK_XBUTTON2, MK_XBUTTON2
    MouseButtonCodes {
        virtual_key: 0x06,
        key_state_bit: 0x0040,
        button: PointerButton::FORWARD,
    },
];

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
        vk::SPACE => K::character(" "),
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
            K::character(ch.to_string())
        }
        // VK_0..=VK_9 are the ASCII digits. Their shifted values are
        // layout-dependent, so no `shift` handling: the WM_CHAR merge is
        // what delivers `!`/`@`/… on a real press.
        c @ vk::KEY_0..=vk::KEY_9 => K::character((c as u8 as char).to_string()),

        // Numpad digits and operators (delivered with NumLock on).
        c @ vk::NUMPAD0..=vk::NUMPAD9 => {
            K::character(((b'0' + (c - vk::NUMPAD0) as u8) as char).to_string())
        }
        vk::MULTIPLY => K::character("*"),
        vk::ADD => K::character("+"),
        vk::SUBTRACT => K::character("-"),
        vk::DECIMAL => K::character("."),
        vk::DIVIDE => K::character("/"),

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
        vk::OEM_1 => K::character(";"),
        vk::OEM_PLUS => K::character("="),
        vk::OEM_COMMA => K::character(","),
        vk::OEM_MINUS => K::character("-"),
        vk::OEM_PERIOD => K::character("."),
        vk::OEM_2 => K::character("/"),
        vk::OEM_3 => K::character("`"),
        vk::OEM_4 => K::character("["),
        vk::OEM_5 | vk::OEM_102 => K::character("\\"),
        vk::OEM_6 => K::character("]"),
        vk::OEM_7 => K::character("'"),

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

/// One key location: the number W3C UI Events gives it and the codes of the
/// keys that sit there.
struct LocationRow {
    location: Location,
    /// `KeyboardEvent.location`'s `DOM_KEY_LOCATION_*`.
    #[cfg_attr(
        not(any(target_arch = "wasm32", test)),
        expect(dead_code, reason = "only the DOM reports a location by number")
    )]
    dom_number: u32,
    codes: &'static [Code],
}

/// Every key location. A number or a code no row names is `Standard`.
///
/// The one table of key locations: the DOM reports the number
/// (`location_of_number`); Win32 and AppKit report only the key, whose code
/// implies its place ([`location_for_code`]). The winit and Android adapters
/// read keyboard-types' own `Location`, whose variants the adapter matches
/// one by one, and `input::keys::mapping` holds them to the same rows.
const LOCATIONS: [LocationRow; 4] = [
    LocationRow {
        location: Location::Standard,
        dom_number: 0,
        codes: &[],
    },
    LocationRow {
        location: Location::Left,
        dom_number: 1,
        codes: &[
            Code::ShiftLeft,
            Code::ControlLeft,
            Code::AltLeft,
            Code::MetaLeft,
        ],
    },
    LocationRow {
        location: Location::Right,
        dom_number: 2,
        codes: &[
            Code::ShiftRight,
            Code::ControlRight,
            Code::AltRight,
            Code::MetaRight,
        ],
    },
    LocationRow {
        location: Location::Numpad,
        dom_number: 3,
        codes: &[
            Code::Numpad0,
            Code::Numpad1,
            Code::Numpad2,
            Code::Numpad3,
            Code::Numpad4,
            Code::Numpad5,
            Code::Numpad6,
            Code::Numpad7,
            Code::Numpad8,
            Code::Numpad9,
            Code::NumpadEnter,
            Code::NumpadAdd,
            Code::NumpadSubtract,
            Code::NumpadMultiply,
            Code::NumpadDivide,
            Code::NumpadDecimal,
            Code::NumpadComma,
            Code::NumpadEqual,
            Code::NumLock,
        ],
    },
];

/// The location of a character that no key typed (Win32's Alt+numpad text,
/// a directly sent `WM_CHAR`): the standard one, the place of a code no row
/// names.
#[cfg(any(windows, test))]
pub(crate) const CHARACTER_WITHOUT_A_KEY: Location = Location::Standard;

/// The location the DOM numbers `number` (`KeyboardEvent.location`,
/// `DOM_KEY_LOCATION_*`). A number it does not define is `Standard`, not
/// dropped.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn location_of_number(number: u32) -> Location {
    LOCATIONS
        .iter()
        .find(|row| row.dom_number == number)
        .map_or(Location::Standard, |row| row.location)
}

/// The W3C `location` a `code` implies: numpad keys report `Numpad`, sided
/// modifiers report their side, everything else is `Standard`. Win32 and
/// AppKit (via [`super::apple::keys`]) report no location, only the key, so
/// their location comes from its code by the same table that names the
/// location the DOM reports.
pub fn location_for_code(code: Code) -> Location {
    LOCATIONS
        .iter()
        .find(|row| row.codes.contains(&code))
        .map_or(Location::Standard, |row| row.location)
}

/// Whether a key message whose virtual key is `vk_code` reports a key an
/// input method already took (`VK_PROCESSKEY`).
///
/// Such a key is not the application's: the input method turned it into
/// composition, whose result reaches the field through the input method's
/// own route (TSF's text store, or the `WM_CHAR`s a legacy input method
/// posts for its result). Reported as a key as well, it would be a second
/// producer for one press (ADR-0069), so it produces no key at all, neither
/// a character nor a named key. The window keeps which presses it took by
/// scancode: their release is no key either, while a press it delivered is
/// released even when the release reports `VK_PROCESSKEY`.
pub fn taken_by_input_method(vk_code: u16) -> bool {
    vk_code == vk::PROCESSKEY
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
        Some(text) => Key::character(text),
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

#[cfg(any(
    target_os = "android",
    all(
        feature = "winit-backend",
        any(target_os = "windows", target_os = "macos", target_os = "linux")
    )
))]
pub(crate) mod adapter {
    //! The keyboard-only boundary for audited Winit and Android mappings.
    //!
    //! Native key tables stay in their maintained ecosystem adapters. Their results
    //! are converted here into the owned FLUI contract; pointer input never passes
    //! through upstream event values.

    use flui_platform::{
        EventTime,
        keyboard::{
            Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
        },
    };
    use keyboard_types as upstream;

    pub(crate) fn keyboard_input(
        event: upstream::KeyboardEvent,
        observed_ns: u64,
    ) -> flui_platform::PlatformInput {
        flui_platform::PlatformInput::Keyboard(key_event(
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

    /// keyboard-types' location, variant by variant: a variant keyboard-types
    /// adds does not compile until it is named here.
    fn location(value: upstream::Location) -> Location {
        match value {
            upstream::Location::Standard => Location::Standard,
            upstream::Location::Left => Location::Left,
            upstream::Location::Right => Location::Right,
            upstream::Location::Numpad => Location::Numpad,
        }
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
        KeyEvent::new(state, key, code(event.code), time)
            .with_location(location(event.location))
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
            let flui_platform::PlatformInput::Keyboard(event) = keyboard_input(upstream, 19) else {
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
}

/// The table of the one location rule, which every backend's reading of a
/// key is held to: one physical key, as each backend reports it, sits at the
/// row's location.
#[cfg(test)]
mod mapping {
    use flui_platform::keyboard::{Code, Location};

    use super::{CHARACTER_WITHOUT_A_KEY, location_for_code, location_of_number, scancode_to_code};
    use crate::input::apple::keys::{keycode_to_code, kvk};

    /// One physical key as each backend reports it.
    struct Key {
        case: &'static str,
        /// Win32's scancode and whether it is extended.
        win32: (u16, bool),
        /// AppKit's virtual key code.
        appkit: u16,
        /// The number the DOM, winit and Android report for its location.
        number: u32,
        location: Location,
    }

    const KEYS: [Key; 7] = [
        Key {
            case: "the A key",
            win32: (0x1E, false),
            appkit: kvk::ANSI_A,
            number: 0,
            location: Location::Standard,
        },
        Key {
            case: "the left Shift",
            win32: (0x2A, false),
            appkit: kvk::SHIFT,
            number: 1,
            location: Location::Left,
        },
        Key {
            case: "the right Shift",
            win32: (0x36, false),
            appkit: kvk::RIGHT_SHIFT,
            number: 2,
            location: Location::Right,
        },
        Key {
            case: "the left Meta",
            win32: (0x5B, true),
            appkit: kvk::COMMAND,
            number: 1,
            location: Location::Left,
        },
        Key {
            case: "the right Meta",
            win32: (0x5C, true),
            appkit: kvk::RIGHT_COMMAND,
            number: 2,
            location: Location::Right,
        },
        Key {
            case: "keypad 0",
            win32: (0x52, false),
            appkit: kvk::ANSI_KEYPAD_0,
            number: 3,
            location: Location::Numpad,
        },
        Key {
            case: "keypad Enter",
            win32: (0x1C, true),
            appkit: kvk::ANSI_KEYPAD_ENTER,
            number: 3,
            location: Location::Numpad,
        },
    ];

    /// What the keyboard-types adapter of the winit and Android hosts reads
    /// for a key whose location keyboard-types numbers `number`.
    #[cfg(any(
        target_os = "android",
        all(
            feature = "winit-backend",
            any(target_os = "windows", target_os = "macos", target_os = "linux")
        )
    ))]
    fn keyboard_types(number: u32, code: Code) -> Location {
        use keyboard_types as upstream;

        let location = [
            upstream::Location::Standard,
            upstream::Location::Left,
            upstream::Location::Right,
            upstream::Location::Numpad,
        ]
        .into_iter()
        .find(|location| *location as u32 == number)
        .expect("BUG: a row numbers a location keyboard-types names");
        let event = upstream::KeyboardEvent {
            state: upstream::KeyState::Down,
            key: upstream::Key::Character("x".to_owned()),
            code: code
                .as_str()
                .parse()
                .expect("BUG: keyboard-types names the code"),
            location,
            modifiers: upstream::Modifiers::empty(),
            repeat: false,
            is_composing: false,
        };
        let flui_platform::PlatformInput::Keyboard(event) =
            super::adapter::keyboard_input(event, 0)
        else {
            panic!("the keyboard adapter produces keyboard input")
        };
        event.location
    }

    #[test]
    fn every_backend_reads_a_key_at_one_location() {
        for key in KEYS {
            let case = key.case;
            let (scancode, extended) = key.win32;
            let code = scancode_to_code(scancode, extended);
            assert_eq!(location_for_code(code), key.location, "Win32: {case}");
            assert_eq!(
                location_for_code(keycode_to_code(key.appkit)),
                key.location,
                "AppKit: {case}"
            );
            assert_eq!(location_of_number(key.number), key.location, "DOM: {case}");
            #[cfg(any(
                target_os = "android",
                all(
                    feature = "winit-backend",
                    any(target_os = "windows", target_os = "macos", target_os = "linux")
                )
            ))]
            assert_eq!(
                keyboard_types(key.number, code),
                key.location,
                "winit and Android: {case}"
            );
        }
        // A location the DOM may number later, and a character no key typed
        // (Win32's Alt+numpad text), are where every other key is.
        assert_eq!(location_of_number(4), Location::Standard);
        assert_eq!(
            location_for_code(Code::Unidentified),
            CHARACTER_WITHOUT_A_KEY
        );
    }
}

#[cfg(test)]
mod input_method {
    use super::taken_by_input_method;

    #[test]
    fn a_processkey_keydown_produces_no_key() {
        let rows: [(&str, u16, bool); 6] = [
            ("VK_PROCESSKEY", 0xE5, true),
            ("VK_PACKET, a character SendInput typed", 0xE7, false),
            ("the A key", 0x41, false),
            ("Space, which converts a composition", 0x20, false),
            ("VK_ICO_00, just before", 0xE4, false),
            ("VK_ICO_CLEAR, just after", 0xE6, false),
        ];
        let failed: Vec<&str> = rows
            .iter()
            .filter(|(_, vk, taken)| taken_by_input_method(*vk) != *taken)
            .map(|(name, ..)| *name)
            .collect();
        assert!(failed.is_empty(), "failing rows: {failed:?}");
    }
}
