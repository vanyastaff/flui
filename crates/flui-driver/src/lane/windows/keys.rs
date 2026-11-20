//! The protocol's key names as Windows virtual keys, and text as the Unicode packets
//! (`KEYEVENTF_UNICODE`, which Windows delivers as `VK_PACKET`) the lane types it with.
//!
//! A key is named as the protocol names it (`flui_protocol::input::KeyAction`): a W3C key name
//! (`Enter`, `Control`, `ArrowLeft`) or the text the key types (`a`, `+`). A named key and an
//! ASCII letter, digit or space map to a virtual key here, on every host; any other character's
//! key depends on the keyboard layout of the window it goes to, which the lane looks up there
//! ([`KeyName::Character`]). A capital letter is its letter's key with Shift, which the lane holds
//! around it unless the sequence already does.
//!
//! Text is not typed key by key: each UTF-16 unit is a packet pressed and released with itself,
//! so it reaches the app as that character whatever the layout. A line break and a tab are the
//! keys a person presses for them, since a packet carrying one arrives as a raw control code;
//! `\r\n` is one Enter.

/// A Windows virtual key, as `SendInput` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VirtualKey {
    /// The `VK_*` code.
    pub code: u16,
    /// Whether the key sits in the extended block (the arrows, the editing keys, the right-hand
    /// modifiers), which `KEYEVENTF_EXTENDEDKEY` tells the app.
    pub extended: bool,
}

impl VirtualKey {
    const fn plain(code: u16) -> Self {
        Self {
            code,
            extended: false,
        }
    }

    const fn extended(code: u16) -> Self {
        Self {
            code,
            extended: true,
        }
    }
}

/// `VK_SHIFT`.
pub const SHIFT: VirtualKey = VirtualKey::plain(0x10);
/// `VK_CONTROL`.
pub const CONTROL: VirtualKey = VirtualKey::plain(0x11);
/// `VK_MENU`, the Alt key.
pub const ALT: VirtualKey = VirtualKey::plain(0x12);
/// `VK_LWIN`, the Windows key.
pub const META: VirtualKey = VirtualKey::extended(0x5B);
/// `VK_RETURN`.
pub const ENTER: VirtualKey = VirtualKey::plain(0x0D);
/// `VK_TAB`.
pub const TAB: VirtualKey = VirtualKey::plain(0x09);

/// A modifier key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Modifier {
    /// Shift.
    Shift,
    /// Control.
    Control,
    /// Alt.
    Alt,
    /// The Windows key.
    Meta,
}

impl Modifier {
    /// The modifier `key` is, if it is one.
    #[must_use]
    pub fn of(key: VirtualKey) -> Option<Self> {
        match key.code {
            0x10 | 0xA0 | 0xA1 => Some(Self::Shift),
            0x11 | 0xA2 | 0xA3 => Some(Self::Control),
            0x12 | 0xA4 | 0xA5 => Some(Self::Alt),
            0x5B | 0x5C => Some(Self::Meta),
            _ => None,
        }
    }
}

/// What a key name names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyName {
    /// A virtual key, with Shift when `shift` (a capital letter).
    Virtual {
        /// The key.
        key: VirtualKey,
        /// Whether Shift is held around it.
        shift: bool,
    },
    /// A character whose key the target window's keyboard layout decides.
    Character(char),
}

/// The W3C key names the lane sends, with their virtual keys.
const NAMED: &[(&str, VirtualKey)] = &[
    ("Backspace", VirtualKey::plain(0x08)),
    ("Tab", TAB),
    ("Clear", VirtualKey::plain(0x0C)),
    ("Enter", ENTER),
    ("Shift", SHIFT),
    ("Control", CONTROL),
    ("Alt", ALT),
    ("Pause", VirtualKey::plain(0x13)),
    ("CapsLock", VirtualKey::plain(0x14)),
    ("Escape", VirtualKey::plain(0x1B)),
    ("PageUp", VirtualKey::extended(0x21)),
    ("PageDown", VirtualKey::extended(0x22)),
    ("End", VirtualKey::extended(0x23)),
    ("Home", VirtualKey::extended(0x24)),
    ("ArrowLeft", VirtualKey::extended(0x25)),
    ("ArrowUp", VirtualKey::extended(0x26)),
    ("ArrowRight", VirtualKey::extended(0x27)),
    ("ArrowDown", VirtualKey::extended(0x28)),
    ("PrintScreen", VirtualKey::extended(0x2C)),
    ("Insert", VirtualKey::extended(0x2D)),
    ("Delete", VirtualKey::extended(0x2E)),
    ("Help", VirtualKey::plain(0x2F)),
    ("Meta", META),
    ("ContextMenu", VirtualKey::extended(0x5D)),
    ("F1", VirtualKey::plain(0x70)),
    ("F2", VirtualKey::plain(0x71)),
    ("F3", VirtualKey::plain(0x72)),
    ("F4", VirtualKey::plain(0x73)),
    ("F5", VirtualKey::plain(0x74)),
    ("F6", VirtualKey::plain(0x75)),
    ("F7", VirtualKey::plain(0x76)),
    ("F8", VirtualKey::plain(0x77)),
    ("F9", VirtualKey::plain(0x78)),
    ("F10", VirtualKey::plain(0x79)),
    ("F11", VirtualKey::plain(0x7A)),
    ("F12", VirtualKey::plain(0x7B)),
    ("F13", VirtualKey::plain(0x7C)),
    ("F14", VirtualKey::plain(0x7D)),
    ("F15", VirtualKey::plain(0x7E)),
    ("F16", VirtualKey::plain(0x7F)),
    ("F17", VirtualKey::plain(0x80)),
    ("F18", VirtualKey::plain(0x81)),
    ("F19", VirtualKey::plain(0x82)),
    ("F20", VirtualKey::plain(0x83)),
    ("F21", VirtualKey::plain(0x84)),
    ("F22", VirtualKey::plain(0x85)),
    ("F23", VirtualKey::plain(0x86)),
    ("F24", VirtualKey::plain(0x87)),
    ("NumLock", VirtualKey::extended(0x90)),
    ("ScrollLock", VirtualKey::plain(0x91)),
    ("AudioVolumeMute", VirtualKey::extended(0xAD)),
    ("AudioVolumeDown", VirtualKey::extended(0xAE)),
    ("AudioVolumeUp", VirtualKey::extended(0xAF)),
    ("MediaTrackNext", VirtualKey::extended(0xB0)),
    ("MediaTrackPrevious", VirtualKey::extended(0xB1)),
    ("MediaStop", VirtualKey::extended(0xB2)),
    ("MediaPlayPause", VirtualKey::extended(0xB3)),
];

/// The virtual key the W3C name `name` names; `None` for a name the lane does not send.
#[must_use]
pub fn named(name: &str) -> Option<VirtualKey> {
    NAMED
        .iter()
        .find(|(known, _)| *known == name)
        .map(|&(_, key)| key)
}

/// The virtual key `code` as people read it: its W3C name when the lane sends it by name, a
/// letter or digit as itself, and otherwise its code.
#[must_use]
pub fn name_of(code: u16) -> String {
    if let Some((name, _)) = NAMED.iter().find(|(_, key)| key.code == code) {
        return (*name).to_owned();
    }
    match u8::try_from(code).ok().map(char::from) {
        Some(c @ ('0'..='9' | 'A'..='Z')) => c.to_string(),
        _ => format!("virtual key {code:#04x}"),
    }
}

/// Whether the virtual key `code` sits in the extended block, as a [`VirtualKey`]'s `extended` says of
/// the named keys; a letter, a digit or a layout's key is not.
#[must_use]
pub fn is_extended(code: u16) -> bool {
    NAMED
        .iter()
        .any(|(_, key)| key.code == code && key.extended)
}

/// What the protocol's key name `name` names; `None` for a word that is no W3C key name the
/// lane sends, or an empty name.
#[must_use]
pub fn resolve(name: &str) -> Option<KeyName> {
    if let Some(key) = named(name) {
        return Some(KeyName::Virtual { key, shift: false });
    }
    let mut chars = name.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let ascii = u8::try_from(u32::from(c)).ok();
    Some(match ascii {
        Some(b' ') => KeyName::Virtual {
            key: VirtualKey::plain(0x20),
            shift: false,
        },
        Some(byte @ (b'a'..=b'z')) => KeyName::Virtual {
            key: VirtualKey::plain(u16::from(byte.to_ascii_uppercase())),
            shift: false,
        },
        Some(byte @ (b'A'..=b'Z')) => KeyName::Virtual {
            key: VirtualKey::plain(u16::from(byte)),
            shift: true,
        },
        Some(byte @ (b'0'..=b'9')) => KeyName::Virtual {
            key: VirtualKey::plain(u16::from(byte)),
            shift: false,
        },
        _ => KeyName::Character(c),
    })
}

/// One keystroke of typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TextStroke {
    /// A key a person presses for a control character: Enter, Tab.
    Key(VirtualKey),
    /// A UTF-16 unit, pressed and released as a Unicode packet.
    Unit(u16),
}

/// `text` as the strokes the lane types it with: see the [module docs](self).
#[must_use]
pub fn text_strokes(text: &str) -> Vec<TextStroke> {
    let mut strokes = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                strokes.push(TextStroke::Key(ENTER));
            }
            '\n' => strokes.push(TextStroke::Key(ENTER)),
            '\t' => strokes.push(TextStroke::Key(TAB)),
            c => {
                let mut units = [0_u16; 2];
                strokes.extend(
                    c.encode_utf16(&mut units)
                        .iter()
                        .map(|&unit| TextStroke::Unit(unit)),
                );
            }
        }
    }
    strokes
}
