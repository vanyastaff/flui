//! The protocol's key names as macOS virtual key codes (`CGKeyCode`), and text as the Unicode
//! strings a key event carries (`CGEventKeyboardSetUnicodeString`).
//!
//! A key is named as the protocol names it (`flui_protocol::input::KeyAction`): a W3C key name
//! (`Enter`, `Meta`, `ArrowLeft`) or the text the key types (`a`, `+`). A macOS key code names a
//! position on the keyboard, not a character: `0x00` is the key where an ANSI keyboard has `A`,
//! whatever the layout prints on it. So a character key goes out as the ANSI key a US layout
//! types it with *and* the character itself as the event's Unicode string, which is what the app
//! reads as the key's text on every layout. A capital letter or a shifted symbol is that key with
//! Shift, which the lane holds around it unless the sequence already does. A character no ANSI key
//! types (`é`, `日`) has no key code here: it is typed as text, never pressed as a key.
//!
//! Text is not typed key by key: each run of at most [`TEXT_CHUNK_UNITS`] UTF-16 units goes out
//! as one key press and release carrying the run as its Unicode string, which reaches the app as
//! those characters whatever the layout. A line break and a tab are the keys a person presses for
//! them; `\r\n` is one Return.
//!
//! W3C's `Meta` is the Command key and `Alt` the Option key, as browsers on macOS report them.

/// A macOS virtual key code, as `CGEventCreateKeyboardEvent` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyCode(pub u16);

/// `kVK_Shift`.
pub const SHIFT: KeyCode = KeyCode(0x38);
/// `kVK_RightShift`.
pub const RIGHT_SHIFT: KeyCode = KeyCode(0x3C);
/// `kVK_Control`.
pub const CONTROL: KeyCode = KeyCode(0x3B);
/// `kVK_RightControl`.
pub const RIGHT_CONTROL: KeyCode = KeyCode(0x3E);
/// `kVK_Option`.
pub const OPTION: KeyCode = KeyCode(0x3A);
/// `kVK_RightOption`.
pub const RIGHT_OPTION: KeyCode = KeyCode(0x3D);
/// `kVK_Command`.
pub const COMMAND: KeyCode = KeyCode(0x37);
/// `kVK_RightCommand`.
pub const RIGHT_COMMAND: KeyCode = KeyCode(0x36);
/// `kVK_Function`, the Fn or Globe key.
pub const FUNCTION: KeyCode = KeyCode(0x3F);
/// `kVK_CapsLock`.
pub const CAPS_LOCK: KeyCode = KeyCode(0x39);
/// `kVK_Return`.
pub const RETURN: KeyCode = KeyCode(0x24);
/// `kVK_Tab`.
pub const TAB: KeyCode = KeyCode(0x30);
/// `kVK_Space`.
pub const SPACE: KeyCode = KeyCode(0x31);
/// `kVK_Escape`.
pub const ESCAPE: KeyCode = KeyCode(0x35);
/// `kVK_ANSI_Grave`, the key left of `1`.
pub const GRAVE: KeyCode = KeyCode(0x32);
/// `kVK_UpArrow`.
pub const UP: KeyCode = KeyCode(0x7E);
/// `kVK_DownArrow`.
pub const DOWN: KeyCode = KeyCode(0x7D);
/// `kVK_LeftArrow`.
pub const LEFT: KeyCode = KeyCode(0x7B);
/// `kVK_RightArrow`.
pub const RIGHT: KeyCode = KeyCode(0x7C);
/// `kVK_F5`.
pub const F5: KeyCode = KeyCode(0x60);
/// `kVK_F11`.
pub const F11: KeyCode = KeyCode(0x67);
/// `kVK_VolumeUp`, `kVK_VolumeDown` and `kVK_Mute`.
pub const VOLUME: [KeyCode; 3] = [KeyCode(0x48), KeyCode(0x49), KeyCode(0x4A)];
/// The function keys F1 to F8, in order, which the system's keyboard navigation takes with
/// Control.
pub const F1_TO_F8: [KeyCode; 8] = [
    KeyCode(0x7A),
    KeyCode(0x78),
    KeyCode(0x63),
    KeyCode(0x76),
    KeyCode(0x60),
    KeyCode(0x61),
    KeyCode(0x62),
    KeyCode(0x64),
];
/// The keys `1` to `9` on the main block, in order.
pub const DIGITS_1_TO_9: [KeyCode; 9] = [
    KeyCode(0x12),
    KeyCode(0x13),
    KeyCode(0x14),
    KeyCode(0x15),
    KeyCode(0x17),
    KeyCode(0x16),
    KeyCode(0x1A),
    KeyCode(0x1C),
    KeyCode(0x19),
];
/// The keys `-` and `=` on the main block.
pub const MINUS: KeyCode = KeyCode(0x1B);
/// See [`MINUS`].
pub const EQUAL: KeyCode = KeyCode(0x18);
/// The key `Q`.
pub const Q: KeyCode = KeyCode(0x0C);
/// The key `D`.
pub const D: KeyCode = KeyCode(0x02);
/// The key `8` on the main block.
pub const EIGHT: KeyCode = KeyCode(0x1C);

/// A modifier key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Modifier {
    /// Shift.
    Shift,
    /// Control.
    Control,
    /// Option, W3C's `Alt`.
    Option,
    /// Command, W3C's `Meta`.
    Command,
    /// Fn, the Globe key on newer keyboards.
    Function,
}

impl Modifier {
    /// Every modifier, in the order the flags list them.
    pub const ALL: [Self; 5] = [
        Self::Shift,
        Self::Control,
        Self::Option,
        Self::Command,
        Self::Function,
    ];

    /// The modifier `key` is, if it is one.
    #[must_use]
    pub fn of(key: KeyCode) -> Option<Self> {
        match key {
            SHIFT | RIGHT_SHIFT => Some(Self::Shift),
            CONTROL | RIGHT_CONTROL => Some(Self::Control),
            OPTION | RIGHT_OPTION => Some(Self::Option),
            COMMAND | RIGHT_COMMAND => Some(Self::Command),
            FUNCTION => Some(Self::Function),
            _ => None,
        }
    }

    /// The modifier's bit in `CGEventFlags` (`kCGEventFlagMask*`).
    #[must_use]
    pub const fn flag(self) -> u64 {
        match self {
            Self::Shift => 0x0002_0000,
            Self::Control => 0x0004_0000,
            Self::Option => 0x0008_0000,
            Self::Command => 0x0010_0000,
            Self::Function => 0x0080_0000,
        }
    }
}

/// What a key name names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyName {
    /// A key with no text of its own: a modifier, an arrow, a function key.
    Named(KeyCode),
    /// A key that types `character`, with Shift held around it when `shift`.
    Character {
        /// The ANSI key a US layout types the character with.
        key: KeyCode,
        /// Whether that layout needs Shift for it.
        shift: bool,
        /// The character, which the event carries as its text.
        character: char,
    },
}

impl KeyName {
    /// The key code it presses.
    #[must_use]
    pub fn key(self) -> KeyCode {
        match self {
            Self::Named(key) | Self::Character { key, .. } => key,
        }
    }
}

/// The W3C key names the lane sends, with their key codes.
const NAMED: &[(&str, KeyCode)] = &[
    ("Backspace", KeyCode(0x33)),
    ("Tab", TAB),
    ("Clear", KeyCode(0x47)),
    ("Enter", RETURN),
    ("Shift", SHIFT),
    ("Control", CONTROL),
    ("Alt", OPTION),
    ("Meta", COMMAND),
    ("Fn", FUNCTION),
    ("CapsLock", CAPS_LOCK),
    ("Escape", ESCAPE),
    ("PageUp", KeyCode(0x74)),
    ("PageDown", KeyCode(0x79)),
    ("End", KeyCode(0x77)),
    ("Home", KeyCode(0x73)),
    ("ArrowLeft", LEFT),
    ("ArrowUp", UP),
    ("ArrowRight", RIGHT),
    ("ArrowDown", DOWN),
    // The Help key of older Apple keyboards sits where a PC keyboard has Insert, and
    // macOS reports a PC keyboard's Insert as it.
    ("Insert", KeyCode(0x72)),
    ("Delete", KeyCode(0x75)),
    ("ContextMenu", KeyCode(0x6E)),
    ("F1", KeyCode(0x7A)),
    ("F2", KeyCode(0x78)),
    ("F3", KeyCode(0x63)),
    ("F4", KeyCode(0x76)),
    ("F5", F5),
    ("F6", KeyCode(0x61)),
    ("F7", KeyCode(0x62)),
    ("F8", KeyCode(0x64)),
    ("F9", KeyCode(0x65)),
    ("F10", KeyCode(0x6D)),
    ("F11", F11),
    ("F12", KeyCode(0x6F)),
    ("F13", KeyCode(0x69)),
    ("F14", KeyCode(0x6B)),
    ("F15", KeyCode(0x71)),
    ("F16", KeyCode(0x6A)),
    ("F17", KeyCode(0x40)),
    ("F18", KeyCode(0x4F)),
    ("F19", KeyCode(0x50)),
    ("F20", KeyCode(0x5A)),
    ("AudioVolumeUp", KeyCode(0x48)),
    ("AudioVolumeDown", KeyCode(0x49)),
    ("AudioVolumeMute", KeyCode(0x4A)),
];

/// The characters the ANSI keys type on a US layout: each key with its character unshifted and
/// shifted.
const ANSI: &[(KeyCode, char, char)] = &[
    (KeyCode(0x00), 'a', 'A'),
    (KeyCode(0x01), 's', 'S'),
    (KeyCode(0x02), 'd', 'D'),
    (KeyCode(0x03), 'f', 'F'),
    (KeyCode(0x04), 'h', 'H'),
    (KeyCode(0x05), 'g', 'G'),
    (KeyCode(0x06), 'z', 'Z'),
    (KeyCode(0x07), 'x', 'X'),
    (KeyCode(0x08), 'c', 'C'),
    (KeyCode(0x09), 'v', 'V'),
    (KeyCode(0x0B), 'b', 'B'),
    (KeyCode(0x0C), 'q', 'Q'),
    (KeyCode(0x0D), 'w', 'W'),
    (KeyCode(0x0E), 'e', 'E'),
    (KeyCode(0x0F), 'r', 'R'),
    (KeyCode(0x10), 'y', 'Y'),
    (KeyCode(0x11), 't', 'T'),
    (KeyCode(0x12), '1', '!'),
    (KeyCode(0x13), '2', '@'),
    (KeyCode(0x14), '3', '#'),
    (KeyCode(0x15), '4', '$'),
    (KeyCode(0x16), '6', '^'),
    (KeyCode(0x17), '5', '%'),
    (KeyCode(0x18), '=', '+'),
    (KeyCode(0x19), '9', '('),
    (KeyCode(0x1A), '7', '&'),
    (KeyCode(0x1B), '-', '_'),
    (KeyCode(0x1C), '8', '*'),
    (KeyCode(0x1D), '0', ')'),
    (KeyCode(0x1E), ']', '}'),
    (KeyCode(0x1F), 'o', 'O'),
    (KeyCode(0x20), 'u', 'U'),
    (KeyCode(0x21), '[', '{'),
    (KeyCode(0x22), 'i', 'I'),
    (KeyCode(0x23), 'p', 'P'),
    (KeyCode(0x25), 'l', 'L'),
    (KeyCode(0x26), 'j', 'J'),
    (KeyCode(0x27), '\'', '"'),
    (KeyCode(0x28), 'k', 'K'),
    (KeyCode(0x29), ';', ':'),
    (KeyCode(0x2A), '\\', '|'),
    (KeyCode(0x2B), ',', '<'),
    (KeyCode(0x2C), '/', '?'),
    (KeyCode(0x2D), 'n', 'N'),
    (KeyCode(0x2E), 'm', 'M'),
    (KeyCode(0x2F), '.', '>'),
    (GRAVE, '`', '~'),
    (SPACE, ' ', ' '),
];

/// What the protocol's key name `name` names; `None` for a word that is no W3C key name the lane
/// sends, a character no ANSI key types, or an empty name.
#[must_use]
pub fn resolve(name: &str) -> Option<KeyName> {
    if let Some(&(_, key)) = NAMED.iter().find(|(known, _)| *known == name) {
        return Some(KeyName::Named(key));
    }
    let mut chars = name.chars();
    let (Some(character), None) = (chars.next(), chars.next()) else {
        return None;
    };
    ANSI.iter().find_map(|&(key, plain, shifted)| {
        if character == plain {
            Some(KeyName::Character {
                key,
                shift: false,
                character,
            })
        } else if character == shifted {
            Some(KeyName::Character {
                key,
                shift: true,
                character,
            })
        } else {
            None
        }
    })
}

/// The most UTF-16 units one key event carries as its text: Quartz takes no more than twenty
/// (`CGEventKeyboardSetUnicodeString`), and a longer string is cut.
pub const TEXT_CHUNK_UNITS: usize = 20;

/// Up to [`TEXT_CHUNK_UNITS`] UTF-16 units one key event carries as its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextChunk {
    units: [u16; TEXT_CHUNK_UNITS],
    len: usize,
}

impl TextChunk {
    /// The chunk's units.
    #[must_use]
    pub fn units(&self) -> &[u16] {
        &self.units[..self.len]
    }

    /// The chunk as text.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf16_lossy(self.units())
    }
}

/// One keystroke of typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TextStroke {
    /// A key a person presses for a control character: Return, Tab.
    Key(KeyCode),
    /// A run of characters, pressed and released as one key event carrying them as its text.
    Text(TextChunk),
}

/// `text` as the strokes the lane types it with: see the [module docs](self). A run is cut before
/// a character that would take it past [`TEXT_CHUNK_UNITS`] units, so a surrogate pair is never
/// split between two events.
#[must_use]
pub fn text_strokes(text: &str) -> Vec<TextStroke> {
    let mut strokes = Vec::new();
    let mut run = TextChunk {
        units: [0; TEXT_CHUNK_UNITS],
        len: 0,
    };
    let flush = |run: &mut TextChunk, strokes: &mut Vec<TextStroke>| {
        if run.len > 0 {
            strokes.push(TextStroke::Text(*run));
            run.len = 0;
        }
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let key = match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                Some(RETURN)
            }
            '\n' => Some(RETURN),
            '\t' => Some(TAB),
            _ => None,
        };
        if let Some(key) = key {
            flush(&mut run, &mut strokes);
            strokes.push(TextStroke::Key(key));
            continue;
        }
        let mut units = [0_u16; 2];
        let encoded = c.encode_utf16(&mut units);
        if run.len + encoded.len() > TEXT_CHUNK_UNITS {
            flush(&mut run, &mut strokes);
        }
        for &unit in encoded.iter() {
            run.units[run.len] = unit;
            run.len += 1;
        }
    }
    flush(&mut run, &mut strokes);
    strokes
}
