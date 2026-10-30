//! The keyboard as XKB describes it, for both Linux hosts: what a key means
//! under the active layout, which physical key it was, which modifiers are
//! held, and compose.
//!
//! Two halves:
//!
//! - the tables, pure and compiled on every host: an evdev key code's
//!   [`Code`](flui_platform::keyboard::Code) ([`code_of`]), a keysym's
//!   [`NamedKey`](flui_platform::keyboard::NamedKey) ([`keysym_key`]), the
//!   [`Key`](flui_platform::keyboard::Key) a press means from its keysym and the text it spells
//!   ([`key_of`]), the [`Modifiers`](flui_platform::keyboard::Modifiers) the named XKB modifiers hold
//!   ([`modifiers_of`]) and a layout's name in a keymap's text
//!   ([`group_names`]);
//! - `keyboard`, a keymap, its state and compose over libxkbcommon, loaded at
//!   run time: what a key means now, read through the tables. Its keymap comes
//!   from a `KeymapSource`, which names where a host reads it: the X server's
//!   core keyboard (the X11 host, whose server keeps the state and reports it
//!   in XKB's `StateNotify`). A press feeds compose: a dead key or a sequence in
//!   progress is [`NamedKey::Dead`](flui_platform::keyboard::NamedKey::Dead),
//!   and the key that completes it carries the composed text; a window losing
//!   the keyboard ends the sequence.
//!
//! XKB numbers a key eight higher than the kernel's evdev code.

pub use meaning::*;

/// The tables: what a key code, a keysym and the named modifiers mean.
mod meaning {
    #![forbid(unsafe_code)]

    use std::ops::Range;

    use flui_platform::keyboard::{Code, Key, Modifiers, NamedKey};

    use crate::input::keys::scancode_to_code;

    /// What XKB adds to a kernel key code to name the key.
    pub const EVDEV_OFFSET: u32 = 8;

    /// The keysyms the conformance table and the tests name.
    pub mod keysym {
        /// `Menu`, the keyboard's menu key.
        pub const MENU: u32 = 0xff67;
        /// `F10`.
        pub const F10: u32 = 0xffc7;
        /// `Escape`.
        pub const ESCAPE: u32 = 0xff1b;
        /// `Return`.
        pub const RETURN: u32 = 0xff0d;
        /// `Shift_L`.
        pub const SHIFT_L: u32 = 0xffe1;
        /// `dead_acute`.
        pub const DEAD_ACUTE: u32 = 0xfe51;
        /// `a`.
        pub const LATIN_A: u32 = 0x61;
    }

    /// The physical key the kernel's evdev `code` names (`linux/input-event-codes.h`).
    ///
    /// The codes up to `KEY_KPDOT` (83) are the PC keyboard's set-1 scan codes,
    /// which the one scan-code table reads; the keys above it are numbered by the
    /// kernel alone.
    #[must_use]
    pub fn code_of(code: u32) -> Code {
        match code {
            1..=83 => {
                u16::try_from(code).map_or(Code::Unidentified, |code| scancode_to_code(code, false))
            }
            85 => Code::Lang5,
            86 => Code::IntlBackslash,
            87 => Code::F11,
            88 => Code::F12,
            89 => Code::IntlRo,
            90 => Code::Katakana,
            91 => Code::Hiragana,
            92 => Code::Convert,
            93 => Code::KanaMode,
            94 => Code::NonConvert,
            95 | 121 => Code::NumpadComma,
            96 => Code::NumpadEnter,
            97 => Code::ControlRight,
            98 => Code::NumpadDivide,
            99 => Code::PrintScreen,
            100 => Code::AltRight,
            102 => Code::Home,
            103 => Code::ArrowUp,
            104 => Code::PageUp,
            105 => Code::ArrowLeft,
            106 => Code::ArrowRight,
            107 => Code::End,
            108 => Code::ArrowDown,
            109 => Code::PageDown,
            110 => Code::Insert,
            111 => Code::Delete,
            113 => Code::AudioVolumeMute,
            114 => Code::AudioVolumeDown,
            115 => Code::AudioVolumeUp,
            116 => Code::Power,
            117 => Code::NumpadEqual,
            119 => Code::Pause,
            122 => Code::Lang1,
            123 => Code::Lang2,
            124 => Code::IntlYen,
            125 => Code::MetaLeft,
            126 => Code::MetaRight,
            127 => Code::ContextMenu,
            129 => Code::Again,
            130 => Code::Props,
            131 => Code::Undo,
            133 => Code::Copy,
            134 => Code::Open,
            135 => Code::Paste,
            136 => Code::Find,
            137 => Code::Cut,
            138 => Code::Help,
            140 => Code::LaunchApp2,
            142 => Code::Sleep,
            143 => Code::WakeUp,
            155 => Code::LaunchMail,
            156 => Code::BrowserFavorites,
            157 => Code::LaunchApp1,
            158 => Code::BrowserBack,
            159 => Code::BrowserForward,
            161 => Code::Eject,
            163 => Code::MediaTrackNext,
            164 => Code::MediaPlayPause,
            165 => Code::MediaTrackPrevious,
            166 => Code::MediaStop,
            172 => Code::BrowserHome,
            173 => Code::BrowserRefresh,
            183 => Code::F13,
            184 => Code::F14,
            185 => Code::F15,
            186 => Code::F16,
            187 => Code::F17,
            188 => Code::F18,
            189 => Code::F19,
            190 => Code::F20,
            191 => Code::F21,
            192 => Code::F22,
            193 => Code::F23,
            194 => Code::F24,
            217 => Code::BrowserSearch,
            224 => Code::BrightnessDown,
            225 => Code::BrightnessUp,
            226 => Code::MediaSelect,
            0x1d0 => Code::Fn,
            _ => Code::Unidentified,
        }
    }

    /// The keysyms that name a key rather than type text (`xkbcommon-keysyms.h`),
    /// each with the W3C key it is. The function keys and the dead keys are
    /// ranges ([`keysym_key`]).
    const NAMED: &[(u32, NamedKey)] = &[
        (0xff08, NamedKey::Backspace),
        (0xff09, NamedKey::Tab),
        // ISO_Left_Tab: Shift+Tab.
        (0xfe20, NamedKey::Tab),
        (0xff0b, NamedKey::Clear),
        (keysym::RETURN, NamedKey::Enter),
        (0xff13, NamedKey::Pause),
        (0xff14, NamedKey::ScrollLock),
        (0xff15, NamedKey::PrintScreen),
        (keysym::ESCAPE, NamedKey::Escape),
        (0xffff, NamedKey::Delete),
        // Multi_key.
        (0xff20, NamedKey::Compose),
        (0xff21, NamedKey::KanjiMode),
        (0xff22, NamedKey::NonConvert),
        (0xff23, NamedKey::Convert),
        (0xff24, NamedKey::Romaji),
        (0xff25, NamedKey::Hiragana),
        (0xff26, NamedKey::Katakana),
        (0xff27, NamedKey::HiraganaKatakana),
        (0xff28, NamedKey::Zenkaku),
        (0xff29, NamedKey::Hankaku),
        (0xff2a, NamedKey::ZenkakuHankaku),
        (0xff2d, NamedKey::KanaMode),
        (0xff2f, NamedKey::Eisu),
        (0xff31, NamedKey::HangulMode),
        (0xff34, NamedKey::HanjaMode),
        (0xff37, NamedKey::CodeInput),
        (0xff3c, NamedKey::SingleCandidate),
        (0xff3d, NamedKey::AllCandidates),
        (0xff3e, NamedKey::PreviousCandidate),
        (0xff50, NamedKey::Home),
        (0xff51, NamedKey::ArrowLeft),
        (0xff52, NamedKey::ArrowUp),
        (0xff53, NamedKey::ArrowRight),
        (0xff54, NamedKey::ArrowDown),
        (0xff55, NamedKey::PageUp),
        (0xff56, NamedKey::PageDown),
        (0xff57, NamedKey::End),
        (0xff60, NamedKey::Select),
        (0xff61, NamedKey::PrintScreen),
        (0xff62, NamedKey::Execute),
        (0xff63, NamedKey::Insert),
        (0xff65, NamedKey::Undo),
        (0xff66, NamedKey::Redo),
        (keysym::MENU, NamedKey::ContextMenu),
        (0xff68, NamedKey::Find),
        (0xff69, NamedKey::Cancel),
        (0xff6a, NamedKey::Help),
        (0xff6b, NamedKey::Pause),
        (0xff7e, NamedKey::ModeChange),
        (0xff7f, NamedKey::NumLock),
        // The keypad's keys that do not type: Tab, Enter, F1 to F4 and the
        // keys under NumLock's other half.
        (0xff89, NamedKey::Tab),
        (0xff8d, NamedKey::Enter),
        (0xff91, NamedKey::F1),
        (0xff92, NamedKey::F2),
        (0xff93, NamedKey::F3),
        (0xff94, NamedKey::F4),
        (0xff95, NamedKey::Home),
        (0xff96, NamedKey::ArrowLeft),
        (0xff97, NamedKey::ArrowUp),
        (0xff98, NamedKey::ArrowRight),
        (0xff99, NamedKey::ArrowDown),
        (0xff9a, NamedKey::PageUp),
        (0xff9b, NamedKey::PageDown),
        (0xff9c, NamedKey::End),
        (0xff9d, NamedKey::Clear),
        (0xff9e, NamedKey::Insert),
        (0xff9f, NamedKey::Delete),
        (keysym::SHIFT_L, NamedKey::Shift),
        (0xffe2, NamedKey::Shift),
        (0xffe3, NamedKey::Control),
        (0xffe4, NamedKey::Control),
        (0xffe5, NamedKey::CapsLock),
        (0xffe6, NamedKey::CapsLock),
        (0xffe7, NamedKey::Meta),
        (0xffe8, NamedKey::Meta),
        (0xffe9, NamedKey::Alt),
        (0xffea, NamedKey::Alt),
        // Super is the W3C's Meta.
        (0xffeb, NamedKey::Meta),
        (0xffec, NamedKey::Meta),
        // ISO_Level3_Shift, the AltGr key.
        (0xfe03, NamedKey::AltGraph),
        (0xfe08, NamedKey::GroupNext),
        (0xfe0a, NamedKey::GroupPrevious),
        (0xfe0c, NamedKey::GroupFirst),
        (0xfe0e, NamedKey::GroupLast),
        // The XF86 keys of multimedia keyboards.
        (0x1008_ff02, NamedKey::BrightnessUp),
        (0x1008_ff03, NamedKey::BrightnessDown),
        (0x1008_ff11, NamedKey::AudioVolumeDown),
        (0x1008_ff12, NamedKey::AudioVolumeMute),
        (0x1008_ff13, NamedKey::AudioVolumeUp),
        (0x1008_ff14, NamedKey::MediaPlay),
        (0x1008_ff15, NamedKey::MediaStop),
        (0x1008_ff16, NamedKey::MediaTrackPrevious),
        (0x1008_ff17, NamedKey::MediaTrackNext),
        (0x1008_ff18, NamedKey::BrowserHome),
        (0x1008_ff19, NamedKey::LaunchMail),
        (0x1008_ff1b, NamedKey::BrowserSearch),
        (0x1008_ff1c, NamedKey::MediaRecord),
        (0x1008_ff1d, NamedKey::LaunchApplication2),
        (0x1008_ff26, NamedKey::BrowserBack),
        (0x1008_ff27, NamedKey::BrowserForward),
        (0x1008_ff28, NamedKey::BrowserStop),
        (0x1008_ff29, NamedKey::BrowserRefresh),
        (0x1008_ff2a, NamedKey::PowerOff),
        (0x1008_ff2b, NamedKey::WakeUp),
        (0x1008_ff2c, NamedKey::Eject),
        (0x1008_ff2f, NamedKey::Standby),
        (0x1008_ff30, NamedKey::BrowserFavorites),
        (0x1008_ff31, NamedKey::MediaPause),
        (0x1008_ff33, NamedKey::LaunchApplication1),
        (0x1008_ff56, NamedKey::Close),
        (0x1008_ff57, NamedKey::Copy),
        (0x1008_ff58, NamedKey::Cut),
        (0x1008_ff6d, NamedKey::Paste),
    ];

    /// The first and last function-key keysyms, `F1` to `F35`.
    const FUNCTION_KEYS: (u32, u32) = (0xffbe, 0xffe0);
    /// The first and last dead-key keysyms, `dead_grave` to `dead_greek` and
    /// their kin.
    const DEAD_KEYS: (u32, u32) = (0xfe50, 0xfe93);

    /// The function keys in order, so a keysym's offset from `F1` names its key.
    const FUNCTIONS: [NamedKey; 35] = [
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
        NamedKey::F13,
        NamedKey::F14,
        NamedKey::F15,
        NamedKey::F16,
        NamedKey::F17,
        NamedKey::F18,
        NamedKey::F19,
        NamedKey::F20,
        NamedKey::F21,
        NamedKey::F22,
        NamedKey::F23,
        NamedKey::F24,
        NamedKey::F25,
        NamedKey::F26,
        NamedKey::F27,
        NamedKey::F28,
        NamedKey::F29,
        NamedKey::F30,
        NamedKey::F31,
        NamedKey::F32,
        NamedKey::F33,
        NamedKey::F34,
        NamedKey::F35,
    ];

    /// The W3C key `keysym` names, when it names one rather than typing text.
    #[must_use]
    pub fn keysym_key(keysym: u32) -> Option<NamedKey> {
        if (FUNCTION_KEYS.0..=FUNCTION_KEYS.1).contains(&keysym) {
            let index = usize::try_from(keysym - FUNCTION_KEYS.0).ok()?;
            return FUNCTIONS.get(index).copied();
        }
        if (DEAD_KEYS.0..=DEAD_KEYS.1).contains(&keysym) {
            return Some(NamedKey::Dead);
        }
        NAMED
            .iter()
            .find(|(named, _)| *named == keysym)
            .map(|(_, key)| *key)
    }

    /// What a key whose keysym is `keysym` means, given the text XKB says it
    /// types (`xkb_state_key_get_utf8`): its name when the keysym names one, its
    /// text when it types some, and `Unidentified` otherwise. A control
    /// character (Ctrl+S types `0x13`) is no text.
    #[must_use]
    pub fn key_of(keysym: u32, text: &str) -> Key {
        if let Some(named) = keysym_key(keysym) {
            return Key::Named(named);
        }
        if text.chars().any(char::is_control) {
            return Key::Named(NamedKey::Unidentified);
        }
        Key::character(text)
    }

    /// The XKB modifiers FLUI reads, by their names in every keymap
    /// (`xkbcommon-names.h`): what the live keymap asks of its state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum XkbModifier {
        /// `Shift`.
        Shift,
        /// `Lock`, Caps Lock.
        Lock,
        /// `Control`.
        Control,
        /// `Mod1`, Alt.
        Mod1,
        /// `Mod2`, Num Lock.
        Mod2,
        /// `Mod4`, Super.
        Mod4,
        /// `Mod5`, ISO Level 3 (AltGr).
        Mod5,
    }

    impl XkbModifier {
        /// Every modifier FLUI reads.
        pub const ALL: [Self; 7] = [
            Self::Shift,
            Self::Lock,
            Self::Control,
            Self::Mod1,
            Self::Mod2,
            Self::Mod4,
            Self::Mod5,
        ];

        /// Its name, as libxkbcommon takes it.
        #[must_use]
        pub const fn name(self) -> &'static std::ffi::CStr {
            match self {
                Self::Shift => c"Shift",
                Self::Lock => c"Lock",
                Self::Control => c"Control",
                Self::Mod1 => c"Mod1",
                Self::Mod2 => c"Mod2",
                Self::Mod4 => c"Mod4",
                Self::Mod5 => c"Mod5",
            }
        }

        const fn modifiers(self) -> Modifiers {
            match self {
                Self::Shift => Modifiers::SHIFT,
                Self::Lock => Modifiers::CAPS_LOCK,
                Self::Control => Modifiers::CONTROL,
                Self::Mod1 => Modifiers::ALT,
                Self::Mod2 => Modifiers::NUM_LOCK,
                Self::Mod4 => Modifiers::META,
                Self::Mod5 => Modifiers::ALT_GRAPH,
            }
        }
    }

    /// The modifiers of the XKB modifiers `active` says are in effect.
    #[must_use]
    pub fn modifiers_of(active: impl Fn(XkbModifier) -> bool) -> Modifiers {
        XkbModifier::ALL
            .into_iter()
            .filter(|modifier| active(*modifier))
            .fold(Modifiers::NONE, |held, modifier| {
                held.union(modifier.modifiers())
            })
    }

    /// The layouts a keymap's text names at most: XKB's groups, which
    /// libxkbcommon counts up to 32.
    const MAX_LAYOUTS: usize = 32;

    #[cfg(test)]
    thread_local! {
        /// How many keymap texts this thread read layout names from.
        #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
        pub(crate) static GROUP_NAME_READS: std::cell::Cell<usize> =
            const { std::cell::Cell::new(0) };
    }

    /// Where in a keymap's text the names of its layouts are, by zero-based
    /// index: the `name[GroupN]` its symbols section declares, as libxkbcommon
    /// writes it out, matched without regard to case; an empty range for a
    /// layout it names none. Ranges into the text rather than copies of it,
    /// so a keymap whose names are most of its text holds them once. Read
    /// once per keymap, in one pass over its text, so naming the layout in
    /// effect is a lookup.
    #[must_use]
    pub fn group_names(keymap: &str) -> Vec<Range<usize>> {
        const WANTED: &str = "name[group";
        #[cfg(test)]
        GROUP_NAME_READS.with(|reads| reads.set(reads.get() + 1));
        let lower = keymap.to_ascii_lowercase();
        let mut names: Vec<Range<usize>> = Vec::new();
        let mut from = 0;
        while let Some(found) = lower[from..].find(WANTED) {
            let at = from + found + WANTED.len();
            from = at;
            let digits = lower[at..].bytes().take_while(u8::is_ascii_digit).count();
            let Some(index) = lower[at..at + digits]
                .parse::<usize>()
                .ok()
                .and_then(|number| number.checked_sub(1))
                .filter(|index| *index < MAX_LAYOUTS)
            else {
                continue;
            };
            let Some(rest) = keymap[at + digits..].strip_prefix(']') else {
                continue;
            };
            let Some(rest) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            // A declaration without a quoted value names nothing: the next
            // one may.
            let Some(rest) = rest.trim_start().strip_prefix('"') else {
                continue;
            };
            let Some(end) = rest.find('"') else {
                continue;
            };
            if names.len() <= index {
                names.resize(index + 1, 0..0);
            }
            if names[index].is_empty() {
                let start = keymap.len() - rest.len();
                names[index] = start..start + end;
            }
        }
        names
    }

    /// The largest keymap a host reads: a desktop keymap is some tens of
    /// kilobytes, so a size past this is a compositor's fault, not a keymap.
    pub const KEYMAP_LIMIT: usize = 8 * 1024 * 1024;

    /// The bytes to read for a keymap a compositor says is `size` long;
    /// `None` for none, or past [`KEYMAP_LIMIT`].
    #[must_use]
    pub fn keymap_length(size: u32) -> Option<usize> {
        usize::try_from(size)
            .ok()
            .filter(|length| (1..=KEYMAP_LIMIT).contains(length))
    }

    /// The keymap text in `bytes`: up to its terminating NUL, which the
    /// Wayland protocol counts in the size; `None` when it is not UTF-8. The
    /// one place a keymap's NUL is stripped.
    #[must_use]
    pub fn keymap_text(mut bytes: Vec<u8>) -> Option<String> {
        if let Some(end) = bytes.iter().position(|byte| *byte == 0) {
            bytes.truncate(end);
        }
        String::from_utf8(bytes).ok()
    }
}

#[cfg(any(x11_host, wayland_host))]
pub(crate) use keyboard::{Keyboard, KeymapSource};

/// A keymap, its state and compose over libxkbcommon.
#[cfg(any(x11_host, wayland_host))]
mod keyboard {
    #![expect(
        unsafe_code,
        reason = "libxkbcommon is a C library, loaded at run time"
    )]

    use std::ffi::{CStr, CString, c_char};
    use std::ptr::NonNull;
    #[cfg(x11_host)]
    use std::sync::Arc;

    use flui_platform::EventTime;
    use flui_platform::keyboard::{Key, KeyEvent, KeyRepeat, KeyState, Modifiers, NamedKey};
    use xkbcommon_dl::{
        XkbCommon, XkbCommonCompose, xkb_compose_feed_result, xkb_compose_state,
        xkb_compose_status, xkb_compose_table, xkb_context, xkb_keymap, xkb_mod_index_t, xkb_state,
        xkb_state_component,
    };

    use super::{EVDEV_OFFSET, XkbModifier, code_of, key_of, modifiers_of};
    #[cfg(wayland_host)]
    use super::{group_names, keymap_text};
    use crate::input::keys::location_for_code;
    #[cfg(x11_host)]
    use crate::owner::linux::x11::X11Connection;

    /// Where a keyboard's keymap comes from.
    #[derive(Debug, Clone)]
    pub(crate) enum KeymapSource {
        /// The X server's core keyboard, device `device` on `connection`,
        /// which the source holds: every libxkbcommon-x11 call through it
        /// names a connection that is open.
        #[cfg(x11_host)]
        X11 {
            connection: Arc<X11Connection>,
            device: i32,
        },
        /// The keymap a Wayland compositor shared, as text (XKB's text format
        /// v1), its NUL stripped; the state is the masks the compositor
        /// sends.
        #[cfg(wayland_host)]
        Compositor {
            /// The keymap's text.
            text: String,
            /// Where in the text it names its layouts, by index, read once:
            /// naming the layout in effect is a lookup, however often the
            /// compositor switches.
            layouts: Vec<std::ops::Range<usize>>,
        },
    }

    impl KeymapSource {
        /// The keymap a compositor shared as `bytes` (read within
        /// [`super::KEYMAP_LIMIT`]); `None` when it is not UTF-8.
        #[cfg(wayland_host)]
        pub(crate) fn compositor(bytes: Vec<u8>) -> Option<Self> {
            keymap_text(bytes).map(|text| {
                let layouts = group_names(&text);
                Self::Compositor { text, layouts }
            })
        }

        /// The X server's keyboard device the keymap is read from; `None` for
        /// a keymap of another source.
        #[cfg(x11_host)]
        #[cfg_attr(
            not(wayland_host),
            expect(
                clippy::unnecessary_wraps,
                reason = "with the Wayland host built, a keymap may come from a compositor"
            )
        )]
        pub(crate) fn x11_device(&self) -> Option<i32> {
            match self {
                Self::X11 { device, .. } => Some(*device),
                #[cfg(wayland_host)]
                Self::Compositor { .. } => None,
            }
        }

        /// The first `chars` characters of the keymap's text, for a
        /// diagnostic.
        #[cfg(wayland_host)]
        pub(crate) fn head(&self, chars: usize) -> String {
            match self {
                Self::Compositor { text, .. } => text.chars().take(chars).collect(),
                #[cfg(x11_host)]
                Self::X11 { .. } => String::new(),
            }
        }

        /// The name the compositor's keymap gives layout `layout`, as XKB
        /// names the group; `None` where it names none.
        #[cfg(wayland_host)]
        pub(crate) fn layout_name(&self, layout: u32) -> Option<&str> {
            match self {
                Self::Compositor { text, layouts } => usize::try_from(layout)
                    .ok()
                    .and_then(|index| layouts.get(index))
                    .and_then(|range| text.get(range.clone()))
                    .filter(|name| !name.is_empty()),
                #[cfg(x11_host)]
                Self::X11 { .. } => None,
            }
        }

        /// The X server's core keyboard on `connection`, once XKB is set up
        /// on it; `None` when libxkbcommon-x11 cannot be loaded or the server
        /// has no XKB.
        #[cfg(x11_host)]
        pub(crate) fn x11_core_keyboard(connection: &Arc<X11Connection>) -> Option<Self> {
            let x11 = xkbcommon_dl::x11::xkbcommon_x11_option()?;
            let raw = connection.xcb().get_raw_xcb_connection();
            // SAFETY: `raw` is the libxcb connection `connection` holds open
            // for the call; the version and error outputs are optional and
            // passed null.
            let set_up = unsafe {
                (x11.xkb_x11_setup_xkb_extension)(
                    raw,
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
            // SAFETY: as above, with XKB set up on it.
            let device = unsafe { (x11.xkb_x11_get_core_keyboard_device_id)(raw) };
            (device >= 0).then(|| Self::X11 {
                connection: Arc::clone(connection),
                device,
            })
        }
    }

    /// A libxkbcommon object held by one reference, released on drop.
    struct Held<T> {
        pointer: NonNull<T>,
        release: unsafe extern "C" fn(*mut T),
    }

    impl<T> Held<T> {
        /// `pointer` as a constructor returned it; `None` for a null one.
        fn try_new(pointer: *mut T, release: unsafe extern "C" fn(*mut T)) -> Option<Self> {
            NonNull::new(pointer).map(|pointer| Self { pointer, release })
        }

        fn as_ptr(&self) -> *mut T {
            self.pointer.as_ptr()
        }
    }

    impl<T> Drop for Held<T> {
        fn drop(&mut self) {
            // SAFETY: the pointer is a reference this value owns, taken from
            // the constructor `release` pairs with, released once.
            unsafe { (self.release)(self.pointer.as_ptr()) };
        }
    }

    /// Compose: the locale's table and the sequence in progress.
    struct Compose {
        api: &'static XkbCommonCompose,
        // Field order: the state goes before the table it reads.
        state: Held<xkb_compose_state>,
        _table: Held<xkb_compose_table>,
    }

    /// A keyboard: its keymap, the state its source reports and compose, and
    /// the keys pressed and not released. Owned by one thread: libxkbcommon's
    /// objects are not shared.
    pub(crate) struct Keyboard {
        api: &'static XkbCommon,
        // Field order: the state goes before the keymap it names, the
        // keymap before the context.
        state: Held<xkb_state>,
        keymap: Held<xkb_keymap>,
        compose: Option<Compose>,
        context: Held<xkb_context>,
        modifiers: [xkb_mod_index_t; 7],
        /// The key codes the keymap defines.
        keycodes: std::ops::RangeInclusive<u32>,
        /// The XKB key codes pressed and not released, in order; at most
        /// [`MAX_HELD_KEYS`].
        held: Vec<u32>,
    }

    /// The keys a keyboard holds down at once. A full-size keyboard has 104
    /// to 108 keys and one with n-key rollover reports every one held; past
    /// this the source is pressing keys it never releases, and a further
    /// press is refused rather than held, and scanned, forever. A keymap's
    /// key codes alone do not bound it: they may run past 255.
    pub(crate) const MAX_HELD_KEYS: usize = 128;

    impl std::fmt::Debug for Keyboard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Keyboard")
                .field("compose", &self.compose.is_some())
                .field("held", &self.held)
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

    impl Keyboard {
        /// The keyboard `source` describes; `None` when libxkbcommon cannot
        /// be loaded or the keymap cannot be read.
        pub(crate) fn try_new(source: &KeymapSource) -> Option<Self> {
            let api = xkbcommon_dl::xkbcommon_option()?;
            // SAFETY: a constructor that takes flags only.
            let context = Held::try_new(
                unsafe {
                    (api.xkb_context_new)(xkbcommon_dl::xkb_context_flags::XKB_CONTEXT_NO_FLAGS)
                },
                *api.xkb_context_unref,
            )?;
            let (keymap, state) = Self::read(api, &context, source)?;
            let compose = Self::open_compose(&context);
            let modifiers = Self::indices(api, &keymap);
            let keycodes = Self::keycodes(api, &keymap);
            Some(Self {
                api,
                state,
                keymap,
                compose,
                context,
                modifiers,
                keycodes,
                held: Vec::new(),
            })
        }

        fn keycodes(api: &XkbCommon, keymap: &Held<xkb_keymap>) -> std::ops::RangeInclusive<u32> {
            // SAFETY: a live keymap.
            unsafe {
                (api.xkb_keymap_min_keycode)(keymap.as_ptr())
                    ..=(api.xkb_keymap_max_keycode)(keymap.as_ptr())
            }
        }

        /// The keymap and state `source` holds now.
        fn read(
            api: &'static XkbCommon,
            context: &Held<xkb_context>,
            source: &KeymapSource,
        ) -> Option<(Held<xkb_keymap>, Held<xkb_state>)> {
            match source {
                #[cfg(wayland_host)]
                KeymapSource::Compositor { text, .. } => {
                    let source = CString::new(text.as_str()).ok()?;
                    // SAFETY: the live context and a NUL-terminated string that
                    // outlives the call; the keymap copies what it reads.
                    let keymap = Held::try_new(
                        unsafe {
                            (api.xkb_keymap_new_from_string)(
                                context.as_ptr(),
                                source.as_ptr(),
                                xkbcommon_dl::xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                                xkbcommon_dl::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
                            )
                        },
                        *api.xkb_keymap_unref,
                    )?;
                    // SAFETY: the keymap just made.
                    let state = Held::try_new(
                        unsafe { (api.xkb_state_new)(keymap.as_ptr()) },
                        *api.xkb_state_unref,
                    )?;
                    Some((keymap, state))
                }
                #[cfg(x11_host)]
                KeymapSource::X11 { connection, device } => {
                    let x11 = xkbcommon_dl::x11::xkbcommon_x11_option()?;
                    let raw = connection.xcb().get_raw_xcb_connection();
                    // SAFETY: the libxcb connection the source holds open, and
                    // the live context.
                    let keymap = Held::try_new(
                        unsafe {
                            (x11.xkb_x11_keymap_new_from_device)(
                                context.as_ptr(),
                                raw,
                                *device,
                                xkbcommon_dl::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
                            )
                        },
                        *api.xkb_keymap_unref,
                    )?;
                    // SAFETY: as above, with the keymap just made.
                    let state = Held::try_new(
                        unsafe {
                            (x11.xkb_x11_state_new_from_device)(keymap.as_ptr(), raw, *device)
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
            // SAFETY: a live context and a NUL-terminated locale that
            // outlives the call.
            let table = Held::try_new(
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
            let state = Held::try_new(
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

        fn indices(api: &XkbCommon, keymap: &Held<xkb_keymap>) -> [xkb_mod_index_t; 7] {
            // SAFETY: a live keymap and NUL-terminated static names.
            XkbModifier::ALL.map(|modifier| unsafe {
                (api.xkb_keymap_mod_get_index)(keymap.as_ptr(), modifier.name().as_ptr())
            })
        }

        /// Read the keymap and state again from `source`: its keymap
        /// changed. The compose sequence in progress ends.
        pub(crate) fn reload(&mut self, source: &KeymapSource) -> bool {
            let Some((keymap, state)) = Self::read(self.api, &self.context, source) else {
                return false;
            };
            self.modifiers = Self::indices(self.api, &keymap);
            self.keycodes = Self::keycodes(self.api, &keymap);
            // The old state names the old keymap: it goes first.
            self.state = state;
            self.keymap = keymap;
            self.reset_compose();
            true
        }

        /// Set the state its source reports; whether the layout in effect
        /// changed.
        pub(crate) fn update_mask(
            &mut self,
            depressed: u32,
            latched: u32,
            locked: u32,
            layouts: [u32; 3],
        ) -> bool {
            let before = self.layout();
            // SAFETY: the live state; libxkbcommon validates the masks.
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
            self.layout() != before
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

        /// Whether holding the key with XKB key code `keycode` repeats, as the
        /// keymap says: a Wayland client repeats keys itself.
        #[cfg(wayland_host)]
        pub(crate) fn repeats(&self, keycode: u32) -> bool {
            // SAFETY: the live keymap; any key code may be asked.
            unsafe { (self.api.xkb_keymap_key_repeats)(self.keymap.as_ptr(), keycode) != 0 }
        }

        /// The modifiers in effect.
        pub(crate) fn modifiers(&self) -> Modifiers {
            modifiers_of(|modifier| {
                let index = self.modifiers[XkbModifier::ALL
                    .iter()
                    .position(|known| *known == modifier)
                    .expect("BUG: every modifier is in ALL")];
                index != xkbcommon_dl::XKB_MOD_INVALID
                    // SAFETY: the live state and an index of its keymap.
                    && unsafe {
                        (self.api.xkb_state_mod_index_is_active)(
                            self.state.as_ptr(),
                            index,
                            xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
                        )
                    } > 0
            })
        }

        /// What `keysym` spells by itself, no modifier applied.
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
                    self.reset_compose();
                    Some(if text.is_empty() {
                        key_of(sym, &self.keysym_text(sym))
                    } else {
                        Key::character(text)
                    })
                }
                xkb_compose_status::XKB_COMPOSE_CANCELLED => {
                    self.reset_compose();
                    // The key that broke the sequence is itself.
                    None
                }
                xkb_compose_status::XKB_COMPOSE_NOTHING => None,
            }
        }

        /// End the compose sequence in progress, if any.
        pub(crate) fn reset_compose(&self) {
            if let Some(compose) = &self.compose {
                // SAFETY: the live compose state.
                unsafe { (compose.api.xkb_compose_state_reset)(compose.state.as_ptr()) };
            }
        }

        /// The key event of XKB key code `keycode` (the kernel's plus
        /// [`EVDEV_OFFSET`]) going `state` at `time`; a press feeds compose.
        /// `None` for a key code the keymap does not define, unless it is the
        /// release of a key held from an older keymap, and for a press past
        /// [`MAX_HELD_KEYS`] held.
        pub(crate) fn key(
            &mut self,
            keycode: u32,
            state: KeyState,
            repeat: bool,
            time: EventTime,
        ) -> Option<KeyEvent> {
            let releases_held = state == KeyState::Up && self.held.contains(&keycode);
            if !self.keycodes.contains(&keycode) && !releases_held {
                return None;
            }
            match state {
                KeyState::Down if !self.held.contains(&keycode) => {
                    if self.held.len() >= MAX_HELD_KEYS {
                        return None;
                    }
                    self.held.push(keycode);
                }
                KeyState::Down => {}
                KeyState::Up => self.held.retain(|held| *held != keycode),
            }
            // SAFETY: the live state; any key code may be asked.
            let keysym =
                unsafe { (self.api.xkb_state_key_get_one_sym)(self.state.as_ptr(), keycode) };
            let composed = if state == KeyState::Down && !repeat {
                self.compose(keysym)
            } else {
                None
            };
            let key = composed.unwrap_or_else(|| key_of(keysym, &self.keysym_text(keysym)));
            let code = code_of(keycode.saturating_sub(EVDEV_OFFSET));
            Some(
                KeyEvent::new(state, key, code, time)
                    .with_location(location_for_code(code))
                    .with_modifiers(self.modifiers())
                    .with_repeat(if repeat {
                        KeyRepeat::AutoRepeat
                    } else {
                        KeyRepeat::First
                    }),
            )
        }

        /// The keyboard leaves its window: the compose sequence in progress
        /// ends, and each key still held is released there, in the order
        /// they were pressed.
        pub(crate) fn leave(&mut self, time: EventTime) -> Vec<KeyEvent> {
            self.reset_compose();
            self.held
                .clone()
                .into_iter()
                .filter_map(|keycode| self.key(keycode, KeyState::Up, false, time))
                .collect()
        }
    }

    #[cfg(all(test, wayland_host))]
    mod tests {
        use flui_platform::EventTime;
        use flui_platform::keyboard::{Key, KeyState, NamedKey};

        use super::{Keyboard, KeymapSource, MAX_HELD_KEYS};
        use crate::input::linux::xkb::EVDEV_OFFSET;

        /// A whole keymap as a compositor shares it, NUL included, and named
        /// here rather than compiled from the system's rules: the apostrophe
        /// key (evdev 40) is dead acute, `E` (evdev 18) types `e`, and its one
        /// group is named. Its every part is spelled out, so it compiles alike
        /// on any xkeyboard-config.
        const COMPOSITOR_KEYMAP: &[u8] = b"xkb_keymap {
xkb_keycodes \"flui\" {
    minimum = 8;
    maximum = 255;
    <AC11> = 48;
    <AD03> = 26;
};
xkb_types \"flui\" {
    type \"ONE_LEVEL\" {
        modifiers = none;
        level_name[Level1] = \"Any\";
    };
};
xkb_compatibility \"flui\" {
};
xkb_symbols \"flui\" {
    name[Group1] = \"FLUI (dead keys)\";
    key <AC11> { type = \"ONE_LEVEL\", [ dead_acute ] };
    key <AD03> { type = \"ONE_LEVEL\", [ e ] };
};
};
\0";

        /// The keymap a compositor shares reads through the shared keyboard,
        /// compose included: dead acute then `e` is `é`, and the layout is
        /// named as the keymap names its group.
        #[test]
        fn a_compositor_keymap_composes_dead_keys() {
            if xkbcommon_dl::xkbcommon_option().is_none() {
                eprintln!("skipped: libxkbcommon is not installed");
                return;
            }
            let source =
                KeymapSource::compositor(COMPOSITOR_KEYMAP.to_vec()).expect("the keymap is text");
            let mut keyboard = Keyboard::try_new(&source).expect("the keymap compiles");
            assert_eq!(
                source.layout_name(keyboard.layout()),
                Some("FLUI (dead keys)")
            );
            let time = EventTime::from_nanos(1);
            let press = |keyboard: &mut Keyboard, code: u32| {
                let key = keyboard
                    .key(code + EVDEV_OFFSET, KeyState::Down, false, time)
                    .expect("the keymap defines the key")
                    .key;
                keyboard.key(code + EVDEV_OFFSET, KeyState::Up, false, time);
                key
            };
            assert_eq!(press(&mut keyboard, 40), Key::Named(NamedKey::Dead));
            assert_eq!(press(&mut keyboard, 18), Key::character("é"));
            assert_eq!(
                press(&mut keyboard, 18),
                Key::character("e"),
                "composed once"
            );
        }

        /// A keymap with every key code a keyboard has, and more than the cap
        /// lets it hold (libxkbcommon may clamp the maximum to 255).
        const WIDE_KEYMAP: &[u8] = b"xkb_keymap {
xkb_keycodes \"flui\" { minimum = 8; maximum = 20000; };
xkb_types \"flui\" { };
xkb_compatibility \"flui\" { };
xkb_symbols \"flui\" { };
};
";

        /// A source that presses keys it never releases leaves at most
        /// `MAX_HELD_KEYS` held: further presses, and key codes the keymap
        /// does not define, are refused, and leaving releases the held ones.
        #[test]
        fn a_keyboard_holds_a_bounded_number_of_keys() {
            if xkbcommon_dl::xkbcommon_option().is_none() {
                eprintln!("skipped: libxkbcommon is not installed");
                return;
            }
            let source =
                KeymapSource::compositor(WIDE_KEYMAP.to_vec()).expect("the keymap is text");
            let mut keyboard = Keyboard::try_new(&source).expect("the keymap compiles");
            let time = EventTime::from_nanos(1);
            for outside in [0, 30_000] {
                assert!(
                    keyboard.key(outside, KeyState::Down, false, time).is_none(),
                    "key code {outside}, outside the keymap's, is refused"
                );
            }
            let pressed = (0..10_000)
                .filter_map(|code| keyboard.key(code + EVDEV_OFFSET, KeyState::Down, false, time))
                .count();
            assert_eq!(pressed, MAX_HELD_KEYS);
            assert_eq!(keyboard.leave(time).len(), MAX_HELD_KEYS);
            assert!(
                keyboard
                    .key(EVDEV_OFFSET, KeyState::Down, false, time)
                    .is_some(),
                "leaving frees the keys"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::keyboard::{Code, Key, Modifiers, NamedKey};

    use super::*;

    /// A key code names its physical key: the codes the kernel shares with
    /// the PC scan codes through the one table, the rest by the kernel's own
    /// numbers.
    #[test]
    fn an_evdev_code_names_its_physical_key() {
        for (code, expected) in [
            (1, Code::Escape),
            (30, Code::KeyA),
            (42, Code::ShiftLeft),
            (68, Code::F10),
            (82, Code::Numpad0),
            (86, Code::IntlBackslash),
            (96, Code::NumpadEnter),
            (97, Code::ControlRight),
            (100, Code::AltRight),
            (103, Code::ArrowUp),
            (111, Code::Delete),
            (125, Code::MetaLeft),
            (127, Code::ContextMenu),
            (183, Code::F13),
            (84, Code::Unidentified),
            (0, Code::Unidentified),
            (u32::MAX, Code::Unidentified),
        ] {
            assert_eq!(code_of(code), expected, "code {code}");
        }
    }

    /// A keysym that names a key is that key, function and dead keys
    /// included; one that types text is its text; a control character is no
    /// text.
    #[test]
    fn a_keysym_names_its_key_or_types_its_text() {
        for (case, keysym, text, expected) in [
            ("menu", keysym::MENU, "", Key::Named(NamedKey::ContextMenu)),
            ("F10", keysym::F10, "", Key::Named(NamedKey::F10)),
            ("F1", 0xffbe, "", Key::Named(NamedKey::F1)),
            ("F35", 0xffe0, "", Key::Named(NamedKey::F35)),
            (
                "escape types ESC",
                keysym::ESCAPE,
                "\u{1b}",
                Key::Named(NamedKey::Escape),
            ),
            (
                "return types CR",
                keysym::RETURN,
                "\r",
                Key::Named(NamedKey::Enter),
            ),
            ("shift", keysym::SHIFT_L, "", Key::Named(NamedKey::Shift)),
            (
                "dead acute",
                keysym::DEAD_ACUTE,
                "",
                Key::Named(NamedKey::Dead),
            ),
            ("super", 0xffeb, "", Key::Named(NamedKey::Meta)),
            ("AltGr", 0xfe03, "", Key::Named(NamedKey::AltGraph)),
            ("a", keysym::LATIN_A, "a", Key::character("a")),
            ("Cyrillic", 0x6c1, "а", Key::character("а")),
            ("keypad 1", 0xffb1, "1", Key::character("1")),
            ("Ctrl+S", 0x73, "\u{13}", Key::Named(NamedKey::Unidentified)),
            ("nothing", 0, "", Key::Named(NamedKey::Unidentified)),
        ] {
            assert_eq!(key_of(keysym, text), expected, "{case}");
        }
    }

    /// The modifiers held are the named XKB modifiers in effect.
    #[test]
    fn the_named_xkb_modifiers_are_the_modifiers_held() {
        assert_eq!(modifiers_of(|_| false), Modifiers::NONE);
        assert_eq!(
            modifiers_of(|modifier| matches!(modifier, XkbModifier::Shift | XkbModifier::Mod4)),
            Modifiers::SHIFT.union(Modifiers::META)
        );
        let all = modifiers_of(|_| true);
        for held in [
            Modifiers::SHIFT,
            Modifiers::CAPS_LOCK,
            Modifiers::CONTROL,
            Modifiers::ALT,
            Modifiers::NUM_LOCK,
            Modifiers::META,
            Modifiers::ALT_GRAPH,
        ] {
            assert!(all.contains(held), "{held:?}");
        }
    }

    /// A layout's name is what the keymap's symbols declare for its group,
    /// as libxkbcommon writes a keymap out.
    #[test]
    fn a_layout_is_named_by_its_group() {
        let keymap = "xkb_keymap {\n\
            xkb_symbols \"pc+us+ru:2+inet(evdev)\" {\n\
            \tname[Group1]=\"English (US)\";\n\
            \tname[Group2]= \"Russian\";\n\
            \tkey <AE01> { [ 1, exclam ] };\n\
            };\n};";
        let named = |keymap: &str| -> Vec<String> {
            group_names(keymap)
                .into_iter()
                .map(|range| keymap[range].to_owned())
                .collect()
        };
        assert_eq!(named(keymap), ["English (US)", "Russian"]);
        assert!(named("name[group1]").is_empty(), "no value");
        assert_eq!(
            named("name[Group1]=unquoted;\n name[Group1]=\"German\";"),
            ["German"],
            "a declaration without quotes is passed over for the next"
        );
        assert_eq!(
            named("name[Group3]=\"Third\"; name[Group99]=\"Past\"; name[Group]=\"None\";"),
            ["", "", "Third"],
            "an unnamed group is empty, and one past the limit or unnumbered is passed over"
        );
    }

    /// A keymap whose layout names are nearly all of its text holds them
    /// once: the names are where the text has them, not copies.
    #[test]
    #[cfg(wayland_host)]
    fn layout_names_are_held_in_the_keymap_text() {
        use std::fmt::Write as _;

        let long = "x".repeat(1024 * 1024);
        let mut text = String::new();
        for group in 1..=4 {
            writeln!(text, "name[Group{group}]=\"{long}\";").expect("a string takes text");
        }
        let source = KeymapSource::compositor(text.into_bytes()).expect("the keymap is text");
        let KeymapSource::Compositor { text, .. } = &source else {
            panic!("a compositor's keymap");
        };
        let held = text.as_bytes().as_ptr_range();
        for layout in 0..4 {
            let name = source.layout_name(layout).expect("the layout is named");
            assert_eq!(name.len(), long.len());
            assert!(
                held.contains(&name.as_ptr()),
                "layout {layout}'s name is a copy, not the keymap's text"
            );
        }
    }

    /// A keymap is read up to its NUL and only within the limit.
    #[test]
    fn a_keymap_is_read_within_its_limit() {
        assert_eq!(keymap_length(0), None);
        assert_eq!(keymap_length(64_756), Some(64_756));
        let limit = u32::try_from(KEYMAP_LIMIT).expect("fits");
        assert_eq!(keymap_length(limit), Some(KEYMAP_LIMIT));
        assert_eq!(keymap_length(limit + 1), None);
        assert_eq!(
            keymap_text(b"xkb_keymap {};\0\0".to_vec()).as_deref(),
            Some("xkb_keymap {};")
        );
        assert_eq!(keymap_text(vec![0xff, 0xfe, 0]), None);
    }
}
