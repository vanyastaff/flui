//! The protocol's key names as WebDriver key values, and text as the keys that type it.
//!
//! A key is named as the protocol names it (`flui_protocol::input::KeyAction`): a W3C key name
//! (`Enter`, `Control`, `ArrowLeft`) or the text the key types (`a`, `+`, `日`). WebDriver's key
//! action takes one grapheme cluster: a named key as the code point its keyboard table gives it
//! (`U+E007` for Enter), a character key as the character itself, which Chrome delivers with the
//! key and code a US layout puts it on. A name with no code point in that table (`CapsLock`,
//! `F13`) has no key value, and the lane refuses it before any event.
//!
//! Text is typed a grapheme cluster at a time, each pressed and released, so `é` written as `e`
//! and a combining accent is one key, as WebDriver requires. A line break is the Enter key and a
//! tab the Tab key, as a person types them; `\r\n` is one Enter. Any other control character
//! types nothing a person could, and is refused.

use icu_segmenter::GraphemeClusterSegmenter;

/// The W3C key names WebDriver's keyboard table gives a code point, with it.
const NAMED: &[(&str, char)] = &[
    ("Unidentified", '\u{E000}'),
    ("Cancel", '\u{E001}'),
    ("Help", '\u{E002}'),
    ("Backspace", '\u{E003}'),
    ("Tab", '\u{E004}'),
    ("Clear", '\u{E005}'),
    ("Enter", '\u{E007}'),
    ("Shift", '\u{E008}'),
    ("Control", '\u{E009}'),
    ("Alt", '\u{E00A}'),
    ("Pause", '\u{E00B}'),
    ("Escape", '\u{E00C}'),
    ("PageUp", '\u{E00E}'),
    ("PageDown", '\u{E00F}'),
    ("End", '\u{E010}'),
    ("Home", '\u{E011}'),
    ("ArrowLeft", '\u{E012}'),
    ("ArrowUp", '\u{E013}'),
    ("ArrowRight", '\u{E014}'),
    ("ArrowDown", '\u{E015}'),
    ("Insert", '\u{E016}'),
    ("Delete", '\u{E017}'),
    ("F1", '\u{E031}'),
    ("F2", '\u{E032}'),
    ("F3", '\u{E033}'),
    ("F4", '\u{E034}'),
    ("F5", '\u{E035}'),
    ("F6", '\u{E036}'),
    ("F7", '\u{E037}'),
    ("F8", '\u{E038}'),
    ("F9", '\u{E039}'),
    ("F10", '\u{E03A}'),
    ("F11", '\u{E03B}'),
    ("F12", '\u{E03C}'),
    ("Meta", '\u{E03D}'),
    ("ZenkakuHankaku", '\u{E040}'),
];

/// The WebDriver key value of the key `name` names; `None` for a name WebDriver's keyboard table
/// has no code point for, and for more than one grapheme cluster that names no key.
#[must_use]
pub fn key_value(name: &str) -> Option<String> {
    if let Some(&(_, code)) = NAMED.iter().find(|(named, _)| *named == name) {
        return Some(code.to_string());
    }
    let mut clusters = graphemes(name).into_iter();
    let cluster = clusters.next()?;
    let printable = !cluster.chars().any(char::is_control);
    (clusters.next().is_none() && printable).then(|| cluster.to_owned())
}

/// The extended grapheme clusters of `text`, through ICU4X, FLUI's one Unicode source
/// (ADR-0092 §6).
fn graphemes(text: &str) -> Vec<&str> {
    let segmenter = const { GraphemeClusterSegmenter::new() };
    let boundaries: Vec<usize> = segmenter.segment_str(text).collect();
    boundaries
        .windows(2)
        .map(|pair| &text[pair[0]..pair[1]])
        .collect()
}

/// The key values that type `text`, one a grapheme cluster: a line break is Enter, a tab is Tab.
///
/// # Errors
///
/// The first control character other than a line break or a tab, which no key types.
pub fn text_keys(text: &str) -> Result<Vec<String>, char> {
    let mut keys = Vec::new();
    for cluster in graphemes(text) {
        let key = match cluster {
            "\r\n" | "\n" | "\r" => '\u{E007}'.to_string(),
            "\t" => '\u{E004}'.to_string(),
            other => {
                if let Some(control) = other.chars().find(|c| c.is_control()) {
                    return Err(control);
                }
                other.to_owned()
            }
        };
        keys.push(key);
    }
    Ok(keys)
}
