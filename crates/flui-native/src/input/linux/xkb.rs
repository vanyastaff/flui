//! The keyboard as XKB describes it: what a Linux host's key means under the
//! active layout, which physical key it was, which modifiers are held, and
//! the layout's name.
//!
//! Two halves:
//!
//! - the tables, pure and compiled on every host: an evdev key code's
//!   [`Code`](flui_platform::keyboard::Code) ([`code_of`]), a keysym's
//!   [`NamedKey`](flui_platform::keyboard::NamedKey) ([`keysym_key`]), the
//!   [`Key`](flui_platform::keyboard::Key) a press means from its keysym and the text XKB gives it
//!   ([`key_of`]), the [`Modifiers`](flui_platform::keyboard::Modifiers) the named XKB modifiers hold
//!   ([`modifiers_of`]) and a layout's name in a keymap's text
//!   ([`group_name`]);
//! - `keymap`, a compiled keymap and its state over libxkbcommon, loaded at
//!   run time (the `wayland` feature): what the compositor's keymap says of a
//!   key, read through the tables.
//!
//! A Wayland compositor sends the keymap as text and the modifier state as
//! masks; the key code it reports is the kernel's evdev code, which XKB
//! numbers eight higher.

pub use meaning::*;

/// The tables: what a key code, a keysym and the named modifiers mean.
mod meaning {
    #![forbid(unsafe_code)]

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

    /// The name a keymap's text gives layout `group` (zero-based): the
    /// `name[GroupN]` its symbols section declares, as libxkbcommon writes it
    /// out, matched without regard to case. `None` when the text names none.
    #[must_use]
    pub fn group_name(keymap: &str, group: u32) -> Option<String> {
        let wanted = format!("name[group{}]", group.checked_add(1)?);
        let lower = keymap.to_ascii_lowercase();
        let mut from = 0;
        while let Some(found) = lower[from..].find(&wanted) {
            let at = from + found + wanted.len();
            from = at;
            let rest = keymap[at..].trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            let rest = rest.trim_start().strip_prefix('"')?;
            let end = rest.find('"')?;
            return Some(rest[..end].to_owned());
        }
        None
    }
}

#[cfg(wayland_host)]
pub(crate) use keymap::Keymap;

/// A compiled keymap and its state over libxkbcommon.
#[cfg(wayland_host)]
mod keymap {
    #![expect(
        unsafe_code,
        reason = "libxkbcommon is a C library, loaded at run time"
    )]

    use std::ffi::{CString, c_char};
    use std::ptr::NonNull;

    use flui_platform::keyboard::Modifiers;
    use xkbcommon_dl::{
        XkbCommon, xkb_context, xkb_context_flags, xkb_keymap, xkb_keymap_compile_flags,
        xkb_keymap_format, xkb_state, xkb_state_component, xkbcommon_option,
    };

    use super::{EVDEV_OFFSET, XkbModifier, group_name, modifiers_of};

    /// What the keymap says of one key in the current state.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct KeyFacts {
        /// The keysym the key produces.
        pub(crate) keysym: u32,
        /// The text it types; empty for none.
        pub(crate) text: String,
        /// Whether holding it repeats.
        pub(crate) repeats: bool,
    }

    /// A keymap the compositor sent, compiled, with the state its modifier
    /// masks put it in. Owned by one thread: libxkbcommon's objects are not
    /// shared.
    pub(crate) struct Keymap {
        api: &'static XkbCommon,
        context: NonNull<xkb_context>,
        compiled: NonNull<xkb_keymap>,
        state: NonNull<xkb_state>,
        /// The keymap's text, which names its layouts.
        text: String,
    }

    impl std::fmt::Debug for Keymap {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Keymap").finish_non_exhaustive()
        }
    }

    impl Keymap {
        /// Compile the keymap `text` (XKB's text format v1); `None` when
        /// libxkbcommon cannot be loaded or refuses the text.
        pub(crate) fn compile(text: &str) -> Option<Self> {
            let api = xkbcommon_option()?;
            let source = CString::new(text.trim_end_matches('\0')).ok()?;
            // SAFETY: `xkb_context_new` takes flags only and returns an owned
            // context or null.
            let context = NonNull::new(unsafe {
                (api.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS)
            })?;
            // SAFETY: `context` is the live context made above and `source`
            // a NUL-terminated string that outlives the call; the keymap
            // copies what it reads.
            let keymap = NonNull::new(unsafe {
                (api.xkb_keymap_new_from_string)(
                    context.as_ptr(),
                    source.as_ptr(),
                    xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                    xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
                )
            });
            let Some(keymap) = keymap else {
                // SAFETY: the context made above, released once.
                unsafe { (api.xkb_context_unref)(context.as_ptr()) };
                return None;
            };
            // SAFETY: `keymap` is the live keymap compiled above.
            let state = NonNull::new(unsafe { (api.xkb_state_new)(keymap.as_ptr()) });
            let Some(state) = state else {
                // SAFETY: the keymap and context made above, each released once.
                unsafe {
                    (api.xkb_keymap_unref)(keymap.as_ptr());
                    (api.xkb_context_unref)(context.as_ptr());
                }
                return None;
            };
            Some(Self {
                api,
                context,
                compiled: keymap,
                state,
                text: source.into_string().unwrap_or_default(),
            })
        }

        /// Put the state in the modifier and layout masks the compositor
        /// sent. Returns whether the effective layout changed.
        pub(crate) fn update(
            &mut self,
            depressed: u32,
            latched: u32,
            locked: u32,
            group: u32,
        ) -> bool {
            let before = self.group();
            // SAFETY: `state` is this keymap's live state; the masks are
            // plain numbers libxkbcommon validates.
            unsafe {
                (self.api.xkb_state_update_mask)(
                    self.state.as_ptr(),
                    depressed,
                    latched,
                    locked,
                    0,
                    0,
                    group,
                );
            }
            self.group() != before
        }

        fn group(&self) -> u32 {
            // SAFETY: `state` is this keymap's live state.
            unsafe {
                (self.api.xkb_state_serialize_layout)(
                    self.state.as_ptr(),
                    xkb_state_component::XKB_STATE_LAYOUT_EFFECTIVE,
                )
            }
        }

        /// What the key with kernel code `code` means now.
        pub(crate) fn key(&self, code: u32) -> KeyFacts {
            let key = code.saturating_add(EVDEV_OFFSET);
            // SAFETY: `state` is this keymap's live state; any key code is
            // valid to ask about.
            let keysym = unsafe { (self.api.xkb_state_key_get_one_sym)(self.state.as_ptr(), key) };
            let mut buffer = [0 as c_char; 64];
            // SAFETY: `buffer` is writable for its whole length, which is
            // passed; libxkbcommon writes at most that many bytes, NUL
            // included, and returns the length it needed.
            let length = unsafe {
                (self.api.xkb_state_key_get_utf8)(
                    self.state.as_ptr(),
                    key,
                    buffer.as_mut_ptr(),
                    buffer.len(),
                )
            };
            let text = usize::try_from(length)
                .ok()
                .filter(|length| *length < buffer.len())
                .map(|length| {
                    // `c_char` is signed on some targets and not on others.
                    let bytes: Vec<u8> = buffer[..length]
                        .iter()
                        .map(|byte| u8::from_ne_bytes(byte.to_ne_bytes()))
                        .collect();
                    String::from_utf8_lossy(&bytes).into_owned()
                })
                .unwrap_or_default();
            // SAFETY: `keymap` is the live keymap; any key code is valid.
            let repeats =
                unsafe { (self.api.xkb_keymap_key_repeats)(self.compiled.as_ptr(), key) } != 0;
            KeyFacts {
                keysym,
                text,
                repeats,
            }
        }

        /// The modifiers in effect.
        pub(crate) fn modifiers(&self) -> Modifiers {
            modifiers_of(|modifier: XkbModifier| {
                // SAFETY: `state` is this keymap's live state and the name a
                // NUL-terminated static string.
                unsafe {
                    (self.api.xkb_state_mod_name_is_active)(
                        self.state.as_ptr(),
                        modifier.name().as_ptr(),
                        xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
                    ) > 0
                }
            })
        }

        /// The effective layout's name, as the keymap names it; empty when
        /// it names none.
        pub(crate) fn layout(&self) -> String {
            group_name(&self.text, self.group()).unwrap_or_default()
        }
    }

    impl Drop for Keymap {
        fn drop(&mut self) {
            // SAFETY: each object was made by `compile` and is released once,
            // the state before the keymap it reads and the keymap before its
            // context.
            unsafe {
                (self.api.xkb_state_unref)(self.state.as_ptr());
                (self.api.xkb_keymap_unref)(self.compiled.as_ptr());
                (self.api.xkb_context_unref)(self.context.as_ptr());
            }
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
        assert_eq!(group_name(keymap, 0).as_deref(), Some("English (US)"));
        assert_eq!(group_name(keymap, 1).as_deref(), Some("Russian"));
        assert_eq!(group_name(keymap, 2), None);
        assert_eq!(group_name("name[group1]", 0), None, "no value");
        assert_eq!(group_name(keymap, u32::MAX), None);
    }
}
