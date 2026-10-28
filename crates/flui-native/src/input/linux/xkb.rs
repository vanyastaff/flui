//! XKB, the keyboard of both Linux hosts: the keymap a key's keysym comes
//! from, the state of its modifiers and layout, and compose.
//!
//! The pure half ([`keys`]) is what a key event is made of: the FLUI key a
//! keysym names, the physical key a kernel key code names, and the modifiers
//! a state holds. It is checked on every host.
//!
//! The live half (`Keyboard`) holds libxkbcommon's keymap, state and compose
//! state, loaded at run time. The X server keeps the state of its keyboard
//! itself and reports it (XKB's `StateNotify`), so the X11 host sets the
//! state from those reports rather than from the keys it sees; the keymap
//! comes from the server's core keyboard device. A key press feeds compose:
//! a dead key or a compose sequence in progress is [`NamedKey::Dead`], and the
//! key that completes it carries the composed text.
//!
//! [`NamedKey::Dead`]: flui_platform::keyboard::NamedKey::Dead

pub mod keys {
    //! Keysyms and kernel key codes as FLUI's keys.
    //!
    //! A keysym is XKB's name for what a key produces at its level: a named
    //! key (`Return`, `Left`, `F10`, a dead key) or a character. A kernel key
    //! code (`linux/input-event-codes.h`) is where the key sits, which X11
    //! reports offset by 8. Both tables are written from the headers, as raw
    //! numbers, so they compile and are tested on every host.

    #![forbid(unsafe_code)]

    use flui_platform::keyboard::{Code, Key, Modifiers, NamedKey};

    /// The modifiers an XKB state holds, by the names every keymap gives
    /// them.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct ActiveModifiers {
        /// `Shift`.
        pub shift: bool,
        /// `Lock`: Caps Lock.
        pub caps_lock: bool,
        /// `Control`.
        pub control: bool,
        /// `Mod1`: Alt.
        pub alt: bool,
        /// `Mod2`: Num Lock.
        pub num_lock: bool,
        /// `Mod4`: Super, which FLUI calls Meta.
        pub meta: bool,
        /// `Mod5`: the third level, AltGr.
        pub alt_graph: bool,
    }

    impl ActiveModifiers {
        /// The modifiers of an X11 modifier mask (`ShiftMask` … `Mod5Mask`),
        /// as XInput 2 events carry them.
        #[must_use]
        pub const fn from_x11_mask(mask: u32) -> Self {
            Self {
                shift: mask & 0x01 != 0,
                caps_lock: mask & 0x02 != 0,
                control: mask & 0x04 != 0,
                alt: mask & 0x08 != 0,
                num_lock: mask & 0x10 != 0,
                meta: mask & 0x40 != 0,
                alt_graph: mask & 0x80 != 0,
            }
        }

        /// FLUI's modifiers.
        #[must_use]
        pub fn modifiers(self) -> Modifiers {
            [
                (self.shift, Modifiers::SHIFT),
                (self.caps_lock, Modifiers::CAPS_LOCK),
                (self.control, Modifiers::CONTROL),
                (self.alt, Modifiers::ALT),
                (self.num_lock, Modifiers::NUM_LOCK),
                (self.meta, Modifiers::META),
                (self.alt_graph, Modifiers::ALT_GRAPH),
            ]
            .into_iter()
            .filter(|(active, _)| *active)
            .fold(Modifiers::NONE, |set, (_, modifier)| set | modifier)
        }
    }

    /// The first and last dead-key keysyms (`dead_grave` … `dead_greek`,
    /// `keysymdef.h`).
    const DEAD_KEYS: std::ops::RangeInclusive<u32> = 0xfe50..=0xfe8f;

    /// The FLUI key `keysym` names, when it names one rather than a
    /// character.
    #[must_use]
    pub fn named_key(keysym: u32) -> Option<NamedKey> {
        use NamedKey as N;
        if DEAD_KEYS.contains(&keysym) {
            return Some(N::Dead);
        }
        // F1 (0xffbe) … F35 (0xffe0).
        if let Some(index) = keysym.checked_sub(0xffbe).filter(|index| *index < 35) {
            return Some(function_key(index));
        }
        let named = match keysym {
            0xff08 => N::Backspace,
            0xff09 | 0xfe20 | 0xff89 => N::Tab,
            0xff0b | 0xff58 | 0xff9d => N::Clear,
            0xff0d | 0xff8d | 0xff0a => N::Enter,
            0xff13 | 0xff6b => N::Pause,
            0xff14 => N::ScrollLock,
            0xff15 | 0xff61 => N::PrintScreen,
            0xff1b => N::Escape,
            0xffff | 0xff9f => N::Delete,
            0xff20 => N::Compose,
            0xff21 => N::KanjiMode,
            0xff22 => N::NonConvert,
            0xff23 => N::Convert,
            0xff24 => N::Romaji,
            0xff25 => N::Hiragana,
            0xff26 => N::Katakana,
            0xff27 => N::HiraganaKatakana,
            0xff28 => N::Zenkaku,
            0xff29 => N::Hankaku,
            0xff2a => N::ZenkakuHankaku,
            0xff30 => N::Eisu,
            0xff31 => N::HangulMode,
            0xff34 => N::HanjaMode,
            0xff50 | 0xff95 => N::Home,
            0xff51 | 0xff96 => N::ArrowLeft,
            0xff52 | 0xff97 => N::ArrowUp,
            0xff53 | 0xff98 => N::ArrowRight,
            0xff54 | 0xff99 => N::ArrowDown,
            0xff55 | 0xff9a => N::PageUp,
            0xff56 | 0xff9b => N::PageDown,
            0xff57 | 0xff9c => N::End,
            0xff60 => N::Select,
            0xff62 => N::Execute,
            0xff63 | 0xff9e => N::Insert,
            0xff65 => N::Undo,
            0xff66 => N::Redo,
            0xff67 => N::ContextMenu,
            0xff68 => N::Find,
            0xff69 => N::Cancel,
            0xff6a => N::Help,
            0xff7e => N::ModeChange,
            0xff7f => N::NumLock,
            0xffe1 | 0xffe2 | 0xffe6 => N::Shift,
            0xffe3 | 0xffe4 => N::Control,
            0xffe5 => N::CapsLock,
            0xffe7..=0xffea => N::Alt,
            0xffeb..=0xffee => N::Meta,
            0xfe03 => N::AltGraph,
            0xfe08 => N::GroupNext,
            0xfe0a => N::GroupPrevious,
            0xfe0c => N::GroupFirst,
            0xfe0e => N::GroupLast,
            // KP_F1 … KP_F4.
            0xff91 => N::F1,
            0xff92 => N::F2,
            0xff93 => N::F3,
            0xff94 => N::F4,
            // XF86keysym.h.
            0x1008_ff02 => N::BrightnessUp,
            0x1008_ff03 => N::BrightnessDown,
            0x1008_ff11 => N::AudioVolumeDown,
            0x1008_ff12 => N::AudioVolumeMute,
            0x1008_ff13 => N::AudioVolumeUp,
            0x1008_ff14 => N::MediaPlayPause,
            0x1008_ff15 => N::MediaStop,
            0x1008_ff16 => N::MediaTrackPrevious,
            0x1008_ff17 => N::MediaTrackNext,
            0x1008_ff18 => N::BrowserHome,
            0x1008_ff19 => N::LaunchMail,
            0x1008_ff1b => N::BrowserSearch,
            0x1008_ff1c => N::MediaRecord,
            0x1008_ff1d => N::LaunchApplication2,
            0x1008_ff26 => N::BrowserBack,
            0x1008_ff27 => N::BrowserForward,
            0x1008_ff28 => N::BrowserStop,
            0x1008_ff29 => N::BrowserRefresh,
            0x1008_ff2a => N::PowerOff,
            0x1008_ff2b => N::WakeUp,
            0x1008_ff2c => N::Eject,
            0x1008_ff2d => N::LaunchScreenSaver,
            0x1008_ff2e => N::LaunchWebBrowser,
            0x1008_ff2f => N::Standby,
            0x1008_ff30 => N::BrowserFavorites,
            0x1008_ff31 => N::MediaPause,
            0x1008_ff33 => N::LaunchApplication1,
            0x1008_ff3e => N::MediaRewind,
            0x1008_ff56 => N::Close,
            0x1008_ff57 => N::Copy,
            0x1008_ff58 => N::Cut,
            0x1008_ff68 => N::New,
            0x1008_ff6b => N::Open,
            0x1008_ff6d => N::Paste,
            0x1008_ff72 => N::MailReply,
            0x1008_ff77 => N::Save,
            0x1008_ff7b => N::MailSend,
            0x1008_ff7c => N::SpellCheck,
            0x1008_ff8b => N::ZoomIn,
            0x1008_ff8c => N::ZoomOut,
            0x1008_ff90 => N::MailForward,
            0x1008_ff97 => N::MediaFastForward,
            0x1008_ffb2 => N::MicrophoneVolumeMute,
            _ => return None,
        };
        Some(named)
    }

    /// `F1` … `F35` by index from `F1`.
    fn function_key(index: u32) -> NamedKey {
        use NamedKey as N;
        const KEYS: [NamedKey; 35] = [
            N::F1,
            N::F2,
            N::F3,
            N::F4,
            N::F5,
            N::F6,
            N::F7,
            N::F8,
            N::F9,
            N::F10,
            N::F11,
            N::F12,
            N::F13,
            N::F14,
            N::F15,
            N::F16,
            N::F17,
            N::F18,
            N::F19,
            N::F20,
            N::F21,
            N::F22,
            N::F23,
            N::F24,
            N::F25,
            N::F26,
            N::F27,
            N::F28,
            N::F29,
            N::F30,
            N::F31,
            N::F32,
            N::F33,
            N::F34,
            N::F35,
        ];
        KEYS.get(index as usize)
            .copied()
            .unwrap_or(NamedKey::Unidentified)
    }

    /// The key a keysym makes, given `text`, what the keysym itself spells
    /// (`xkb_keysym_to_utf8`, which no modifier rewrites into a control
    /// character): the named key, else the character it spells, else
    /// unidentified.
    #[must_use]
    pub fn key(keysym: u32, text: &str) -> Key {
        if let Some(named) = named_key(keysym) {
            return Key::Named(named);
        }
        if text.is_empty() || text.chars().any(char::is_control) {
            return Key::Named(NamedKey::Unidentified);
        }
        Key::character(text)
    }

    /// The physical key a kernel key code names (X11 reports it plus 8).
    #[must_use]
    pub fn code(evdev: u32) -> Code {
        use Code as C;
        // `linux/input-event-codes.h`, KEY_ESC (1) … KEY_KPDOT (83).
        const FIRST: [Code; 83] = [
            C::Escape,
            C::Digit1,
            C::Digit2,
            C::Digit3,
            C::Digit4,
            C::Digit5,
            C::Digit6,
            C::Digit7,
            C::Digit8,
            C::Digit9,
            C::Digit0,
            C::Minus,
            C::Equal,
            C::Backspace,
            C::Tab,
            C::KeyQ,
            C::KeyW,
            C::KeyE,
            C::KeyR,
            C::KeyT,
            C::KeyY,
            C::KeyU,
            C::KeyI,
            C::KeyO,
            C::KeyP,
            C::BracketLeft,
            C::BracketRight,
            C::Enter,
            C::ControlLeft,
            C::KeyA,
            C::KeyS,
            C::KeyD,
            C::KeyF,
            C::KeyG,
            C::KeyH,
            C::KeyJ,
            C::KeyK,
            C::KeyL,
            C::Semicolon,
            C::Quote,
            C::Backquote,
            C::ShiftLeft,
            C::Backslash,
            C::KeyZ,
            C::KeyX,
            C::KeyC,
            C::KeyV,
            C::KeyB,
            C::KeyN,
            C::KeyM,
            C::Comma,
            C::Period,
            C::Slash,
            C::ShiftRight,
            C::NumpadMultiply,
            C::AltLeft,
            C::Space,
            C::CapsLock,
            C::F1,
            C::F2,
            C::F3,
            C::F4,
            C::F5,
            C::F6,
            C::F7,
            C::F8,
            C::F9,
            C::F10,
            C::NumLock,
            C::ScrollLock,
            C::Numpad7,
            C::Numpad8,
            C::Numpad9,
            C::NumpadSubtract,
            C::Numpad4,
            C::Numpad5,
            C::Numpad6,
            C::NumpadAdd,
            C::Numpad1,
            C::Numpad2,
            C::Numpad3,
            C::Numpad0,
            C::NumpadDecimal,
        ];
        if let Some(code) = evdev
            .checked_sub(1)
            .and_then(|index| FIRST.get(index as usize))
        {
            return *code;
        }
        match evdev {
            85 => C::Lang5,
            86 => C::IntlBackslash,
            87 => C::F11,
            88 => C::F12,
            89 => C::IntlRo,
            90 => C::Lang3,
            91 => C::Lang4,
            92 => C::Convert,
            93 => C::KanaMode,
            94 => C::NonConvert,
            95 | 121 => C::NumpadComma,
            96 => C::NumpadEnter,
            97 => C::ControlRight,
            98 => C::NumpadDivide,
            99 => C::PrintScreen,
            100 => C::AltRight,
            102 => C::Home,
            103 => C::ArrowUp,
            104 => C::PageUp,
            105 => C::ArrowLeft,
            106 => C::ArrowRight,
            107 => C::End,
            108 => C::ArrowDown,
            109 => C::PageDown,
            110 => C::Insert,
            111 => C::Delete,
            113 => C::AudioVolumeMute,
            114 => C::AudioVolumeDown,
            115 => C::AudioVolumeUp,
            116 => C::Power,
            117 => C::NumpadEqual,
            119 => C::Pause,
            122 => C::Lang1,
            123 => C::Lang2,
            124 => C::IntlYen,
            125 => C::MetaLeft,
            126 => C::MetaRight,
            127 => C::ContextMenu,
            128 => C::BrowserStop,
            129 => C::Again,
            130 => C::Props,
            131 => C::Undo,
            132 => C::Select,
            133 => C::Copy,
            134 => C::Open,
            135 => C::Paste,
            136 => C::Find,
            137 => C::Cut,
            138 => C::Help,
            140 => C::LaunchApp2,
            142 => C::Sleep,
            143 => C::WakeUp,
            144 => C::LaunchApp1,
            155 => C::LaunchMail,
            156 => C::BrowserFavorites,
            158 => C::BrowserBack,
            159 => C::BrowserForward,
            161 => C::Eject,
            163 => C::MediaTrackNext,
            164 => C::MediaPlayPause,
            165 => C::MediaTrackPrevious,
            166 => C::MediaStop,
            172 => C::BrowserHome,
            173 => C::BrowserRefresh,
            183 => C::F13,
            184 => C::F14,
            185 => C::F15,
            186 => C::F16,
            187 => C::F17,
            188 => C::F18,
            189 => C::F19,
            190 => C::F20,
            191 => C::F21,
            192 => C::F22,
            193 => C::F23,
            194 => C::F24,
            217 => C::BrowserSearch,
            224 => C::BrightnessDown,
            225 => C::BrightnessUp,
            _ => C::Unidentified,
        }
    }

    #[cfg(test)]
    mod tests {
        use flui_platform::keyboard::{Code, Key, Modifiers, NamedKey};

        use super::{ActiveModifiers, code, key, named_key};

        /// Keysyms, from `keysymdef.h` and `XF86keysym.h`, as the keys they
        /// name; one that spells a character is that character.
        #[test]
        fn a_keysym_is_the_key_it_names_or_the_character_it_spells() {
            // (case, keysym, what it spells, key)
            let table: &[(&str, u32, &str, Key)] = &[
                ("Return", 0xff0d, "\r", Key::Named(NamedKey::Enter)),
                ("KP_Enter", 0xff8d, "\r", Key::Named(NamedKey::Enter)),
                (
                    "BackSpace",
                    0xff08,
                    "\u{8}",
                    Key::Named(NamedKey::Backspace),
                ),
                ("ISO_Left_Tab", 0xfe20, "", Key::Named(NamedKey::Tab)),
                ("Menu", 0xff67, "", Key::Named(NamedKey::ContextMenu)),
                ("F1", 0xffbe, "", Key::Named(NamedKey::F1)),
                ("F10", 0xffc7, "", Key::Named(NamedKey::F10)),
                ("F35", 0xffe0, "", Key::Named(NamedKey::F35)),
                ("Shift_R", 0xffe2, "", Key::Named(NamedKey::Shift)),
                ("Super_L", 0xffeb, "", Key::Named(NamedKey::Meta)),
                (
                    "ISO_Level3_Shift",
                    0xfe03,
                    "",
                    Key::Named(NamedKey::AltGraph),
                ),
                ("dead_acute", 0xfe51, "", Key::Named(NamedKey::Dead)),
                ("KP_Left", 0xff96, "", Key::Named(NamedKey::ArrowLeft)),
                (
                    "XF86AudioRaiseVolume",
                    0x1008_ff13,
                    "",
                    Key::Named(NamedKey::AudioVolumeUp),
                ),
                ("XF86Copy", 0x1008_ff57, "", Key::Named(NamedKey::Copy)),
                ("Print", 0xff61, "", Key::Named(NamedKey::PrintScreen)),
                ("a", 0x61, "a", Key::character("a")),
                ("A", 0x41, "A", Key::character("A")),
                ("Cyrillic_ef", 0x6c6, "ф", Key::character("ф")),
                ("KP_1", 0xffb1, "1", Key::character("1")),
                ("NoSymbol", 0, "", Key::Named(NamedKey::Unidentified)),
                (
                    "a control character is no key's text",
                    0x1234,
                    "\u{3}",
                    Key::Named(NamedKey::Unidentified),
                ),
            ];
            for (case, keysym, text, expected) in table {
                assert_eq!(key(*keysym, text), *expected, "{case}");
            }
            assert_eq!(named_key(0xffe1 + 100), None);
        }

        /// Kernel key codes as the physical keys they are.
        #[test]
        fn a_kernel_key_code_is_the_key_where_it_sits() {
            // (case, evdev code, code)
            let table = [
                ("KEY_ESC", 1, Code::Escape),
                ("KEY_A", 30, Code::KeyA),
                ("KEY_KPDOT", 83, Code::NumpadDecimal),
                ("the key beside the left Shift", 86, Code::IntlBackslash),
                ("KEY_F12", 88, Code::F12),
                ("KEY_RIGHTALT", 100, Code::AltRight),
                ("KEY_LEFTMETA", 125, Code::MetaLeft),
                ("KEY_COMPOSE", 127, Code::ContextMenu),
                ("KEY_F24", 194, Code::F24),
                ("KEY_RESERVED", 0, Code::Unidentified),
                ("past the table", 900, Code::Unidentified),
            ];
            for (case, evdev, expected) in table {
                assert_eq!(code(evdev), expected, "{case}");
            }
        }

        #[test]
        fn an_x11_modifier_mask_is_flui_modifiers() {
            let all = ActiveModifiers::from_x11_mask(0xff).modifiers();
            assert_eq!(
                all,
                Modifiers::SHIFT
                    | Modifiers::CAPS_LOCK
                    | Modifiers::CONTROL
                    | Modifiers::ALT
                    | Modifiers::NUM_LOCK
                    | Modifiers::META
                    | Modifiers::ALT_GRAPH
            );
            // Mod3 (0x20) is no modifier FLUI names.
            assert_eq!(
                ActiveModifiers::from_x11_mask(0x20).modifiers(),
                Modifiers::NONE
            );
            assert_eq!(
                ActiveModifiers::from_x11_mask(0x05).modifiers(),
                Modifiers::SHIFT | Modifiers::CONTROL
            );
        }
    }
}

#[cfg(x11_host)]
pub(crate) use live::{Keyboard, KeyboardSource};

#[cfg(x11_host)]
mod live {
    //! libxkbcommon's keymap, state and compose, loaded at run time.
    #![expect(unsafe_code, reason = "libxkbcommon is called through its C API")]

    use std::ffi::{CStr, CString, c_char};
    use std::ptr::NonNull;

    use flui_platform::EventTime;
    use flui_platform::keyboard::{Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey};
    use xkbcommon_dl::{
        XkbCommon, XkbCommonCompose, xkb_compose_feed_result, xkb_compose_state,
        xkb_compose_status, xkb_compose_table, xkb_context, xkb_keymap, xkb_mod_index_t, xkb_state,
        xkb_state_component,
    };

    use super::keys::{self, ActiveModifiers};
    use crate::input::keys::location_for_code;

    /// Where a keymap comes from.
    #[derive(Debug, Clone, Copy)]
    pub(crate) enum KeyboardSource {
        /// The X server's core keyboard, on the libxcb connection `connection`
        /// points to.
        X11 {
            connection: NonNull<std::ffi::c_void>,
            device: i32,
        },
    }

    /// A libxkbcommon object held by one reference, released on drop.
    struct Held<T> {
        pointer: NonNull<T>,
        release: unsafe extern "C" fn(*mut T),
    }

    impl<T> Held<T> {
        /// `pointer` as returned by a constructor; `None` for a null one.
        fn new(pointer: *mut T, release: unsafe extern "C" fn(*mut T)) -> Option<Self> {
            NonNull::new(pointer).map(|pointer| Self { pointer, release })
        }

        fn as_ptr(&self) -> *mut T {
            self.pointer.as_ptr()
        }
    }

    impl<T> Drop for Held<T> {
        fn drop(&mut self) {
            // SAFETY: the pointer is a live reference this value owns, taken
            // from the constructor `release` pairs with.
            unsafe { (self.release)(self.pointer.as_ptr()) };
        }
    }

    /// The indices of the modifiers [`ActiveModifiers`] reads, in one keymap.
    #[derive(Debug, Clone, Copy)]
    struct ModifierIndices([xkb_mod_index_t; 7]);

    /// Compose: the locale's table and the sequence in progress.
    struct Compose {
        api: &'static XkbCommonCompose,
        state: Held<xkb_compose_state>,
        _table: Held<xkb_compose_table>,
    }

    /// The keyboard of one Linux host: its keymap, its state and compose.
    pub(crate) struct Keyboard {
        api: &'static XkbCommon,
        // Field order: the state goes before the keymap it names, the keymap
        // before the context.
        state: Held<xkb_state>,
        keymap: Held<xkb_keymap>,
        compose: Option<Compose>,
        context: Held<xkb_context>,
        modifiers: ModifierIndices,
    }

    impl std::fmt::Debug for Keyboard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Keyboard")
                .field("compose", &self.compose.is_some())
                .finish_non_exhaustive()
        }
    }

    /// The locale compose sequences are read for: the first of `LC_ALL`,
    /// `LC_CTYPE` and `LANG` that is set, else `C`.
    fn compose_locale() -> CString {
        ["LC_ALL", "LC_CTYPE", "LANG"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
            .and_then(|value| CString::new(value).ok())
            .unwrap_or_else(|| c"C".to_owned())
    }

    impl KeyboardSource {
        /// The X server's core keyboard on the libxcb connection
        /// `connection` points to, once XKB is set up on it; `None` when
        /// libxkbcommon-x11 cannot be loaded or the server has no XKB.
        pub(crate) fn x11_core_keyboard(connection: NonNull<std::ffi::c_void>) -> Option<Self> {
            let x11 = xkbcommon_dl::x11::xkbcommon_x11_option()?;
            // SAFETY: the caller's live libxcb connection; the version and
            // error outputs are optional and passed null.
            let set_up = unsafe {
                (x11.xkb_x11_setup_xkb_extension)(
                    connection.as_ptr(),
                    xkbcommon_dl::x11::XKB_X11_MIN_MAJOR_XKB_VERSION,
                    xkbcommon_dl::x11::XKB_X11_MIN_MINOR_XKB_VERSION,
                    xkbcommon_dl::x11::xkb_x11_setup_xkb_extension_flags::XKB_X11_SETUP_XKB_EXTENSION_NO_FLAGS,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if set_up != 1 {
                return None;
            }
            // SAFETY: as above, with XKB set up.
            let device = unsafe { (x11.xkb_x11_get_core_keyboard_device_id)(connection.as_ptr()) };
            (device >= 0).then_some(Self::X11 { connection, device })
        }

        /// The keyboard's XKB device id.
        pub(crate) const fn device(self) -> i32 {
            match self {
                Self::X11 { device, .. } => device,
            }
        }
    }

    impl Keyboard {
        /// The keyboard `source` describes; `None` when libxkbcommon cannot be
        /// loaded or the keymap cannot be read.
        pub(crate) fn new(source: KeyboardSource) -> Option<Self> {
            let api = xkbcommon_dl::xkbcommon_option()?;
            // SAFETY: a constructor with no arguments to uphold.
            let context = Held::new(
                unsafe {
                    (api.xkb_context_new)(xkbcommon_dl::xkb_context_flags::XKB_CONTEXT_NO_FLAGS)
                },
                *api.xkb_context_unref,
            )?;
            let (keymap, state) = Self::read(api, &context, source)?;
            let compose = Self::open_compose(&context);
            let modifiers = Self::indices(api, &keymap);
            Some(Self {
                api,
                state,
                keymap,
                compose,
                context,
                modifiers,
            })
        }

        /// The keymap and state `source` holds now.
        fn read(
            api: &'static XkbCommon,
            context: &Held<xkb_context>,
            source: KeyboardSource,
        ) -> Option<(Held<xkb_keymap>, Held<xkb_state>)> {
            match source {
                KeyboardSource::X11 { connection, device } => {
                    let x11 = xkbcommon_dl::x11::xkbcommon_x11_option()?;
                    // SAFETY: `connection` is the live libxcb connection the
                    // caller holds for the call; the context is live.
                    let keymap = Held::new(
                        unsafe {
                            (x11.xkb_x11_keymap_new_from_device)(
                                context.as_ptr(),
                                connection.as_ptr(),
                                device,
                                xkbcommon_dl::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
                            )
                        },
                        *api.xkb_keymap_unref,
                    )?;
                    // SAFETY: as above, with the keymap just made.
                    let state = Held::new(
                        unsafe {
                            (x11.xkb_x11_state_new_from_device)(
                                keymap.as_ptr(),
                                connection.as_ptr(),
                                device,
                            )
                        },
                        *api.xkb_state_unref,
                    )?;
                    Some((keymap, state))
                }
            }
        }

        /// Compose for the locale, when libxkbcommon has its module and the
        /// locale a table.
        fn open_compose(context: &Held<xkb_context>) -> Option<Compose> {
            let api = xkbcommon_dl::xkbcommon_compose_option()?;
            let locale = compose_locale();
            // SAFETY: a live context and a NUL-terminated locale that outlives
            // the call.
            let table = Held::new(
                unsafe {
                    (api.xkb_compose_table_new_from_locale)(
                        context.as_ptr(),
                        locale.as_ptr(),
                        xkbcommon_dl::xkb_compose_compile_flags::XKB_COMPOSE_COMPILE_NO_FLAGS,
                    )
                },
                *api.xkb_compose_table_unref,
            )?;
            // SAFETY: the live table just made.
            let state = Held::new(
                unsafe {
                    (api.xkb_compose_state_new)(
                        table.as_ptr(),
                        xkbcommon_dl::xkb_compose_state_flags::XKB_COMPOSE_STATE_NO_FLAGS,
                    )
                },
                *api.xkb_compose_state_unref,
            )?;
            Some(Compose {
                api,
                state,
                _table: table,
            })
        }

        fn indices(api: &XkbCommon, keymap: &Held<xkb_keymap>) -> ModifierIndices {
            let names: [&CStr; 7] = [
                c"Shift", c"Lock", c"Control", c"Mod1", c"Mod2", c"Mod4", c"Mod5",
            ];
            // SAFETY: a live keymap and NUL-terminated names.
            ModifierIndices(names.map(|name| unsafe {
                (api.xkb_keymap_mod_get_index)(keymap.as_ptr(), name.as_ptr())
            }))
        }

        /// Read the keymap and state again from `source`: the server's
        /// keyboard changed. The compose sequence in progress ends.
        pub(crate) fn reload(&mut self, source: KeyboardSource) -> bool {
            let Some((keymap, state)) = Self::read(self.api, &self.context, source) else {
                return false;
            };
            self.modifiers = Self::indices(self.api, &keymap);
            // The old state names the old keymap: it goes first.
            self.state = state;
            self.keymap = keymap;
            if let Some(compose) = &self.compose {
                // SAFETY: the live compose state.
                unsafe { (compose.api.xkb_compose_state_reset)(compose.state.as_ptr()) };
            }
            true
        }

        /// Set the state the server reports.
        pub(crate) fn update_mask(
            &mut self,
            depressed: u32,
            latched: u32,
            locked: u32,
            layouts: [u32; 3],
        ) {
            // SAFETY: the live state.
            unsafe {
                (self.api.xkb_state_update_mask)(
                    self.state.as_ptr(),
                    depressed,
                    latched,
                    locked,
                    layouts[0],
                    layouts[1],
                    layouts[2],
                );
            }
        }

        /// The layout in effect, by its index in the keymap.
        pub(crate) fn layout(&self) -> u32 {
            // SAFETY: the live state.
            unsafe {
                (self.api.xkb_state_serialize_layout)(
                    self.state.as_ptr(),
                    xkb_state_component::XKB_STATE_LAYOUT_EFFECTIVE,
                )
            }
        }

        /// The modifiers in effect.
        pub(crate) fn modifiers(&self) -> Modifiers {
            let [shift, caps_lock, control, alt, num_lock, meta, alt_graph] =
                self.modifiers.0.map(|index| {
                    index != xkbcommon_dl::XKB_MOD_INVALID
                    // SAFETY: the live state and an index of its keymap.
                        && unsafe {
                            (self.api.xkb_state_mod_index_is_active)(
                                self.state.as_ptr(),
                                index,
                                xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
                            )
                        } > 0
                });
            ActiveModifiers {
                shift,
                caps_lock,
                control,
                alt,
                num_lock,
                meta,
                alt_graph,
            }
            .modifiers()
        }

        /// What `keysym` spells by itself.
        fn keysym_text(&self, keysym: u32) -> String {
            let mut buffer = [0 as c_char; 64];
            // SAFETY: the buffer and its length.
            let written =
                unsafe { (self.api.xkb_keysym_to_utf8)(keysym, buffer.as_mut_ptr(), buffer.len()) };
            if written <= 0 {
                return String::new();
            }
            // SAFETY: libxkbcommon NUL-terminates what it wrote.
            unsafe { CStr::from_ptr(buffer.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        }

        /// What compose makes of a press of `keysym`: `None` when compose did
        /// not take it, else the key the press is.
        fn compose(&self, keysym: u32) -> Option<Key> {
            let compose = self.compose.as_ref()?;
            let state = compose.state.as_ptr();
            // SAFETY: the live compose state.
            let fed = unsafe { (compose.api.xkb_compose_state_feed)(state, keysym) };
            if fed == xkb_compose_feed_result::XKB_COMPOSE_FEED_IGNORED {
                return None;
            }
            // SAFETY: the live compose state.
            match unsafe { (compose.api.xkb_compose_state_get_status)(state) } {
                xkb_compose_status::XKB_COMPOSE_COMPOSING => Some(Key::Named(NamedKey::Dead)),
                xkb_compose_status::XKB_COMPOSE_COMPOSED => {
                    let mut buffer = [0 as c_char; 64];
                    // SAFETY: the live compose state, the buffer and its length.
                    let written = unsafe {
                        (compose.api.xkb_compose_state_get_utf8)(
                            state,
                            buffer.as_mut_ptr(),
                            buffer.len(),
                        )
                    };
                    let text = (written > 0)
                        // SAFETY: libxkbcommon NUL-terminates what it wrote.
                        .then(|| unsafe { CStr::from_ptr(buffer.as_ptr()) })
                        .map(|text| text.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    // SAFETY: the live compose state.
                    let sym = unsafe { (compose.api.xkb_compose_state_get_one_sym)(state) };
                    // SAFETY: the live compose state.
                    unsafe { (compose.api.xkb_compose_state_reset)(state) };
                    Some(if text.is_empty() {
                        keys::key(sym, &self.keysym_text(sym))
                    } else {
                        Key::character(text)
                    })
                }
                xkb_compose_status::XKB_COMPOSE_CANCELLED => {
                    // SAFETY: the live compose state.
                    unsafe { (compose.api.xkb_compose_state_reset)(state) };
                    // The key that broke the sequence is itself.
                    None
                }
                xkb_compose_status::XKB_COMPOSE_NOTHING => None,
            }
        }

        /// The key event of X keycode or `keycode` (the kernel's plus 8) going
        /// `state` at `time`; a press feeds compose.
        pub(crate) fn key(
            &mut self,
            keycode: u32,
            state: KeyState,
            repeat: bool,
            time: EventTime,
        ) -> KeyEvent {
            // SAFETY: the live state; any keycode may be asked.
            let keysym =
                unsafe { (self.api.xkb_state_key_get_one_sym)(self.state.as_ptr(), keycode) };
            let composed = if state == KeyState::Down && !repeat {
                self.compose(keysym)
            } else {
                None
            };
            let key = composed.unwrap_or_else(|| keys::key(keysym, &self.keysym_text(keysym)));
            let code = keys::code(keycode.saturating_sub(8));
            KeyEvent::new(state, key, code, time)
                .with_location(location_for_code(code))
                .with_modifiers(self.modifiers())
                .with_repeat(if repeat {
                    KeyRepeat::AutoRepeat
                } else {
                    KeyRepeat::First
                })
        }
    }
}
