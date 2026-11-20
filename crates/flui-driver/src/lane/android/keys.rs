//! The protocol's key names as Android key codes, the keys the lane refuses because they leave the
//! app or change the device, and text as what `input text` can type.
//!
//! A key is named as the protocol names it (`flui_protocol::input::KeyAction`): a W3C key name
//! (`Enter`, `Control`, `ArrowLeft`) or the character an unshifted key types (`a`, `5`, `,`).
//! Each becomes the `KEYCODE_*` name `input keyevent` takes (`KeyEvent.keyCodeFromString`). The
//! W3C `Home` and `End` are the caret's keys, `KEYCODE_MOVE_HOME` and `KEYCODE_MOVE_END`, never
//! the launcher's `KEYCODE_HOME`. A shifted character is no key here: it is typed as text.
//!
//! `input text` types through the virtual keyboard's character map (`KeyCharacterMap
//! .VIRTUAL_KEYBOARD`), which maps printable ASCII and nothing else, and reads `%s` as a space;
//! [`text_strokes`] cuts text into what it can type, line breaks and tabs as their keys.

/// An Android key code, by the name `input` takes and its value in `KeyEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Keycode {
    /// The `KEYCODE_*` name.
    pub name: &'static str,
    /// Its value.
    pub code: u16,
}

const fn key(name: &'static str, code: u16) -> Keycode {
    Keycode { name, code }
}

/// `KEYCODE_ENTER`.
pub const ENTER: Keycode = key("KEYCODE_ENTER", 66);
/// `KEYCODE_TAB`.
pub const TAB: Keycode = key("KEYCODE_TAB", 61);
/// `KEYCODE_SHIFT_LEFT`.
pub const SHIFT: Keycode = key("KEYCODE_SHIFT_LEFT", 59);
/// `KEYCODE_CTRL_LEFT`.
pub const CONTROL: Keycode = key("KEYCODE_CTRL_LEFT", 113);
/// `KEYCODE_ALT_LEFT`.
pub const ALT: Keycode = key("KEYCODE_ALT_LEFT", 57);
/// `KEYCODE_META_LEFT`, the key W3C names `Meta`.
pub const META: Keycode = key("KEYCODE_META_LEFT", 117);
/// `KEYCODE_SPACE`.
pub const SPACE: Keycode = key("KEYCODE_SPACE", 62);

/// The modifiers `input keycombination` holds while it presses the keys after them
/// (`InputShellCommand`'s `MODIFIER` table).
pub const MODIFIERS: [Keycode; 4] = [SHIFT, CONTROL, ALT, META];

/// The W3C key names the lane sends, with their key codes.
const NAMED: &[(&str, Keycode)] = &[
    ("Backspace", key("KEYCODE_DEL", 67)),
    ("Tab", TAB),
    ("Enter", ENTER),
    ("Shift", SHIFT),
    ("Control", CONTROL),
    ("Alt", ALT),
    ("Meta", META),
    ("CapsLock", key("KEYCODE_CAPS_LOCK", 115)),
    ("Escape", key("KEYCODE_ESCAPE", 111)),
    (" ", SPACE),
    ("PageUp", key("KEYCODE_PAGE_UP", 92)),
    ("PageDown", key("KEYCODE_PAGE_DOWN", 93)),
    ("End", key("KEYCODE_MOVE_END", 123)),
    ("Home", key("KEYCODE_MOVE_HOME", 122)),
    ("ArrowLeft", key("KEYCODE_DPAD_LEFT", 21)),
    ("ArrowUp", key("KEYCODE_DPAD_UP", 19)),
    ("ArrowRight", key("KEYCODE_DPAD_RIGHT", 22)),
    ("ArrowDown", key("KEYCODE_DPAD_DOWN", 20)),
    ("Insert", key("KEYCODE_INSERT", 124)),
    ("Delete", key("KEYCODE_FORWARD_DEL", 112)),
    ("ContextMenu", key("KEYCODE_MENU", 82)),
    ("Pause", key("KEYCODE_BREAK", 121)),
    ("ScrollLock", key("KEYCODE_SCROLL_LOCK", 116)),
    ("NumLock", key("KEYCODE_NUM_LOCK", 143)),
    ("F1", key("KEYCODE_F1", 131)),
    ("F2", key("KEYCODE_F2", 132)),
    ("F3", key("KEYCODE_F3", 133)),
    ("F4", key("KEYCODE_F4", 134)),
    ("F5", key("KEYCODE_F5", 135)),
    ("F6", key("KEYCODE_F6", 136)),
    ("F7", key("KEYCODE_F7", 137)),
    ("F8", key("KEYCODE_F8", 138)),
    ("F9", key("KEYCODE_F9", 139)),
    ("F10", key("KEYCODE_F10", 140)),
    ("F11", key("KEYCODE_F11", 141)),
    ("F12", key("KEYCODE_F12", 142)),
];

/// The unshifted characters of a keyboard's keys, with their key codes; the letters and digits
/// are counted from `KEYCODE_A` and `KEYCODE_0`.
const CHARACTERS: &[(char, Keycode)] = &[
    (',', key("KEYCODE_COMMA", 55)),
    ('.', key("KEYCODE_PERIOD", 56)),
    ('`', key("KEYCODE_GRAVE", 68)),
    ('-', key("KEYCODE_MINUS", 69)),
    ('=', key("KEYCODE_EQUALS", 70)),
    ('[', key("KEYCODE_LEFT_BRACKET", 71)),
    (']', key("KEYCODE_RIGHT_BRACKET", 72)),
    ('\\', key("KEYCODE_BACKSLASH", 73)),
    (';', key("KEYCODE_SEMICOLON", 74)),
    ('\'', key("KEYCODE_APOSTROPHE", 75)),
    ('/', key("KEYCODE_SLASH", 76)),
];

const LETTERS: [&str; 26] = [
    "KEYCODE_A",
    "KEYCODE_B",
    "KEYCODE_C",
    "KEYCODE_D",
    "KEYCODE_E",
    "KEYCODE_F",
    "KEYCODE_G",
    "KEYCODE_H",
    "KEYCODE_I",
    "KEYCODE_J",
    "KEYCODE_K",
    "KEYCODE_L",
    "KEYCODE_M",
    "KEYCODE_N",
    "KEYCODE_O",
    "KEYCODE_P",
    "KEYCODE_Q",
    "KEYCODE_R",
    "KEYCODE_S",
    "KEYCODE_T",
    "KEYCODE_U",
    "KEYCODE_V",
    "KEYCODE_W",
    "KEYCODE_X",
    "KEYCODE_Y",
    "KEYCODE_Z",
];

const DIGITS: [&str; 10] = [
    "KEYCODE_0",
    "KEYCODE_1",
    "KEYCODE_2",
    "KEYCODE_3",
    "KEYCODE_4",
    "KEYCODE_5",
    "KEYCODE_6",
    "KEYCODE_7",
    "KEYCODE_8",
    "KEYCODE_9",
];

/// The key codes the lane refuses, by W3C name, each with what it does instead of reaching the
/// app: it leaves the app, or changes the device past the run.
pub const SYSTEM_KEYS: &[(&str, &str)] = &[
    ("GoHome", "goes to the launcher"),
    ("AppSwitch", "opens the recent apps"),
    (
        "GoBack",
        "is the system's back, which leaves a root activity",
    ),
    (
        "BrowserBack",
        "is the system's back, which leaves a root activity",
    ),
    ("Power", "turns the screen off or opens the power menu"),
    ("PowerOff", "turns the device off"),
    ("WakeUp", "wakes the device"),
    ("Standby", "puts the device to sleep"),
    ("Hibernate", "puts the device to sleep"),
    ("AudioVolumeUp", "changes the device's volume"),
    ("AudioVolumeDown", "changes the device's volume"),
    ("AudioVolumeMute", "changes the device's volume"),
    ("MicrophoneVolumeMute", "mutes the device's microphone"),
    ("BrightnessUp", "changes the screen's brightness"),
    ("BrightnessDown", "changes the screen's brightness"),
    ("Settings", "opens the system settings"),
    ("Notification", "opens the notification shade"),
    ("LaunchAssistant", "starts the assistant"),
    ("BrowserSearch", "starts the assistant's search"),
    ("PrintScreen", "takes a screenshot the device keeps"),
    ("Camera", "starts the camera"),
    ("Call", "starts a call"),
    ("EndCall", "ends a call or turns the screen off"),
    ("MediaPlayPause", "controls the device's media session"),
    ("MediaPlay", "controls the device's media session"),
    ("MediaPause", "controls the device's media session"),
    ("MediaStop", "controls the device's media session"),
    ("MediaTrackNext", "controls the device's media session"),
    ("MediaTrackPrevious", "controls the device's media session"),
];

/// What the lane does with a key name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyName {
    /// A key it sends.
    Key(Keycode),
    /// A key it refuses, and what the key does instead.
    System(&'static str),
    /// A name it has no key for.
    Unknown,
}

/// What the lane does with the key `name`: see the [module docs](self).
#[must_use]
pub fn resolve(name: &str) -> KeyName {
    if let Some((_, effect)) = SYSTEM_KEYS.iter().find(|(system, _)| *system == name) {
        return KeyName::System(effect);
    }
    if let Some((_, code)) = NAMED.iter().find(|(named, _)| *named == name) {
        return KeyName::Key(*code);
    }
    let mut characters = name.chars();
    let (Some(character), None) = (characters.next(), characters.next()) else {
        return KeyName::Unknown;
    };
    character_key(character).map_or(KeyName::Unknown, KeyName::Key)
}

/// The key an unshifted `character` is on.
fn character_key(character: char) -> Option<Keycode> {
    let offset = |from: char| u16::try_from(u32::from(character) - u32::from(from)).ok();
    match character {
        'a'..='z' => {
            let index = offset('a')?;
            Some(key(LETTERS[usize::from(index)], 29 + index))
        }
        '0'..='9' => {
            let index = offset('0')?;
            Some(key(DIGITS[usize::from(index)], 7 + index))
        }
        _ => CHARACTERS
            .iter()
            .find(|(on, _)| *on == character)
            .map(|(_, code)| *code),
    }
}

/// Whether `code` is a modifier `input keycombination` holds.
#[must_use]
pub fn is_modifier(code: Keycode) -> bool {
    MODIFIERS.contains(&code)
}

/// One piece of typed text: a run `input text` types, or a key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextStroke {
    /// Printable ASCII for `input text`, holding no `%s`, which it would read as a space.
    Run(String),
    /// A key: a line break's Enter, a tab's Tab.
    Key(Keycode),
}

/// `text` as what the lane types it with: printable ASCII in runs, a line break (`\n`, `\r\n` or
/// `\r`) as Enter, a tab as Tab. A run is cut between a `%` and an `s` after it, so neither run
/// holds the `%s` that `input text` reads as a space.
///
/// # Errors
///
/// The first character the virtual keyboard's character map cannot type.
pub fn text_strokes(text: &str) -> std::result::Result<Vec<TextStroke>, char> {
    let mut strokes = Vec::new();
    let mut run = String::new();
    let flush = |run: &mut String, strokes: &mut Vec<TextStroke>| {
        if !run.is_empty() {
            strokes.push(TextStroke::Run(std::mem::take(run)));
        }
    };
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                flush(&mut run, &mut strokes);
                strokes.push(TextStroke::Key(ENTER));
            }
            '\n' => {
                flush(&mut run, &mut strokes);
                strokes.push(TextStroke::Key(ENTER));
            }
            '\t' => {
                flush(&mut run, &mut strokes);
                strokes.push(TextStroke::Key(TAB));
            }
            ' '..='~' => {
                if character == 's' && run.ends_with('%') {
                    flush(&mut run, &mut strokes);
                }
                run.push(character);
            }
            other => return Err(other),
        }
    }
    flush(&mut run, &mut strokes);
    Ok(strokes)
}
