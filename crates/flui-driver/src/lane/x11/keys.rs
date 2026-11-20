//! The protocol's key names as X keysyms, and text as the keysyms the lane types it with.
//!
//! A key is named as the protocol names it (`flui_protocol::input::KeyAction`): a W3C key name
//! (`Enter`, `Control`, `ArrowLeft`) or the text the key types (`a`, `+`, `ü`). Each becomes the
//! keysym the X server's keymap is searched for; which key carries it, and at which shift level,
//! is the server's keymap's to say, which the lane reads there (`held::Keymap`). A character the
//! keymap has no key for is typed through a spare key the lane maps to it for the stroke, as
//! `xdotool` does, so typed text reaches the app as itself whatever the layout.
//!
//! A line break and a tab are the keys a person presses for them; `\r\n` is one Return.

/// An X keysym, as the core protocol's keymap carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Keysym(pub u32);

/// `XK_Return`.
pub const RETURN: Keysym = Keysym(0xFF0D);
/// `XK_Tab`.
pub const TAB: Keysym = Keysym(0xFF09);
/// `XK_Shift_L`.
pub const SHIFT_L: Keysym = Keysym(0xFFE1);
/// `XK_Control_L`.
pub const CONTROL_L: Keysym = Keysym(0xFFE3);
/// `XK_Alt_L`.
pub const ALT_L: Keysym = Keysym(0xFFE9);
/// `XK_Super_L`, the key W3C names `Meta`.
pub const SUPER_L: Keysym = Keysym(0xFFEB);
/// `NoSymbol`: a key with nothing on it.
pub const NO_SYMBOL: Keysym = Keysym(0);

/// The W3C key names the lane sends, with their keysyms.
const NAMED: &[(&str, Keysym)] = &[
    ("Backspace", Keysym(0xFF08)),
    ("Tab", TAB),
    ("Clear", Keysym(0xFF0B)),
    ("Enter", RETURN),
    ("Shift", SHIFT_L),
    ("Control", CONTROL_L),
    ("Alt", ALT_L),
    ("Pause", Keysym(0xFF13)),
    ("CapsLock", Keysym(0xFFE5)),
    ("Escape", Keysym(0xFF1B)),
    ("PageUp", Keysym(0xFF55)),
    ("PageDown", Keysym(0xFF56)),
    ("End", Keysym(0xFF57)),
    ("Home", Keysym(0xFF50)),
    ("ArrowLeft", Keysym(0xFF51)),
    ("ArrowUp", Keysym(0xFF52)),
    ("ArrowRight", Keysym(0xFF53)),
    ("ArrowDown", Keysym(0xFF54)),
    ("PrintScreen", Keysym(0xFF61)),
    ("Insert", Keysym(0xFF63)),
    ("Delete", Keysym(0xFFFF)),
    ("Help", Keysym(0xFF6A)),
    ("Meta", SUPER_L),
    ("ContextMenu", Keysym(0xFF67)),
    ("F1", Keysym(0xFFBE)),
    ("F2", Keysym(0xFFBF)),
    ("F3", Keysym(0xFFC0)),
    ("F4", Keysym(0xFFC1)),
    ("F5", Keysym(0xFFC2)),
    ("F6", Keysym(0xFFC3)),
    ("F7", Keysym(0xFFC4)),
    ("F8", Keysym(0xFFC5)),
    ("F9", Keysym(0xFFC6)),
    ("F10", Keysym(0xFFC7)),
    ("F11", Keysym(0xFFC8)),
    ("F12", Keysym(0xFFC9)),
    ("F13", Keysym(0xFFCA)),
    ("F14", Keysym(0xFFCB)),
    ("F15", Keysym(0xFFCC)),
    ("F16", Keysym(0xFFCD)),
    ("F17", Keysym(0xFFCE)),
    ("F18", Keysym(0xFFCF)),
    ("F19", Keysym(0xFFD0)),
    ("F20", Keysym(0xFFD1)),
    ("F21", Keysym(0xFFD2)),
    ("F22", Keysym(0xFFD3)),
    ("F23", Keysym(0xFFD4)),
    ("F24", Keysym(0xFFD5)),
    ("NumLock", Keysym(0xFF7F)),
    ("ScrollLock", Keysym(0xFF14)),
    ("AudioVolumeMute", Keysym(0x1008_FF12)),
    ("AudioVolumeDown", Keysym(0x1008_FF11)),
    ("AudioVolumeUp", Keysym(0x1008_FF13)),
    ("MediaTrackNext", Keysym(0x1008_FF17)),
    ("MediaTrackPrevious", Keysym(0x1008_FF16)),
    ("MediaStop", Keysym(0x1008_FF15)),
    ("MediaPlayPause", Keysym(0x1008_FF14)),
];

/// The keysym the W3C name `name` names; `None` for a name the lane does not send.
#[must_use]
pub fn named(name: &str) -> Option<Keysym> {
    NAMED
        .iter()
        .find(|(known, _)| *known == name)
        .map(|&(_, keysym)| keysym)
}

/// The keysym that types `c`: the character itself for Latin-1, the Unicode keysym
/// (`0x0100_0000` plus the code point) for the rest; `None` for a control character, which no
/// key types.
#[must_use]
pub fn of_char(c: char) -> Option<Keysym> {
    let code = u32::from(c);
    match code {
        0x20..=0x7E | 0xA0..=0xFF => Some(Keysym(code)),
        0..=0x1F | 0x7F..=0x9F => None,
        _ => Some(Keysym(0x0100_0000 | code)),
    }
}

/// `keysym` for people, as [`resolve`] names it: its W3C name, the character it types, or its
/// number.
#[must_use]
pub fn name_of(keysym: Keysym) -> String {
    if let Some((name, _)) = NAMED.iter().find(|(_, known)| *known == keysym) {
        return (*name).to_owned();
    }
    let code = if keysym.0 & 0xFF00_0000 == 0x0100_0000 {
        keysym.0 & 0x00FF_FFFF
    } else {
        keysym.0
    };
    match char::from_u32(code).filter(|c| of_char(*c) == Some(keysym)) {
        Some(c) => c.to_string(),
        None => format!("keysym {:#x}", keysym.0),
    }
}

/// The keysym the protocol's key name `name` names: a W3C name the lane sends, or a single
/// character; `None` for anything else, an empty name included.
#[must_use]
pub fn resolve(name: &str) -> Option<Keysym> {
    if let Some(keysym) = named(name) {
        return Some(keysym);
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => of_char(c),
        _ => None,
    }
}

/// A modifier, as the shell's shortcuts read them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Modifier {
    /// Either Shift.
    Shift,
    /// Either Control.
    Control,
    /// Either Alt, or Meta (which X keymaps put on the Alt keys).
    Alt,
    /// Either Super or Hyper: the key W3C names `Meta`.
    Super,
    /// The third or fifth level's shift (AltGr, `Mode_switch`), which types a key's other
    /// characters and takes part in no shortcut.
    Level,
}

impl Modifier {
    /// The modifier `keysym` is, if it is one.
    #[must_use]
    pub fn of(keysym: Keysym) -> Option<Self> {
        match keysym.0 {
            0xFFE1 | 0xFFE2 => Some(Self::Shift),
            0xFFE3 | 0xFFE4 => Some(Self::Control),
            0xFFE7..=0xFFEA => Some(Self::Alt),
            0xFFEB..=0xFFEE => Some(Self::Super),
            0xFE03 | 0xFE11 | 0xFF7E => Some(Self::Level),
            _ => None,
        }
    }
}

/// One keystroke of typed text, as the keysym it types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TextStroke {
    /// A key a person presses for a control character: Return, Tab.
    Key(Keysym),
    /// A character.
    Char(Keysym),
}

impl TextStroke {
    /// The keysym the stroke types.
    #[must_use]
    pub fn keysym(self) -> Keysym {
        match self {
            Self::Key(keysym) | Self::Char(keysym) => keysym,
        }
    }
}

/// `text` as the strokes the lane types it with: see the [module docs](self).
///
/// # Errors
///
/// The first control character other than a line break and a tab, which no key types.
pub fn text_strokes(text: &str) -> Result<Vec<TextStroke>, char> {
    let mut strokes = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                strokes.push(TextStroke::Key(RETURN));
            }
            '\n' => strokes.push(TextStroke::Key(RETURN)),
            '\t' => strokes.push(TextStroke::Key(TAB)),
            c => strokes.push(TextStroke::Char(of_char(c).ok_or(c)?)),
        }
    }
    Ok(strokes)
}
