//! Key-combo parsing: `"ctrl+shift+s"`, `"enter"`, `"alt+f4"`.
//!
//! The parser is platform-neutral; [`crate::input`] maps the result onto the
//! OS keys. Names are case-insensitive and `+`-separated: every part but the
//! last is a modifier, the last is the key. A lone modifier (`"shift"`) is a
//! key of its own.

use std::fmt;

use crate::error::{ToolError, ToolResult};

/// A modifier held while the main key is pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    /// Control.
    Ctrl,
    /// Shift.
    Shift,
    /// Alt (Option on macOS).
    Alt,
    /// The Windows key, Command on macOS, Super on Linux.
    Meta,
}

/// The key a combo ends with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyName {
    /// A printable character, lower-cased for letters.
    Char(char),
    /// A function key, F1..=F24.
    F(u8),
    /// A modifier pressed on its own.
    Modifier(Modifier),
    /// Enter / Return.
    Enter,
    /// Tab.
    Tab,
    /// Escape.
    Escape,
    /// Space bar.
    Space,
    /// Backspace.
    Backspace,
    /// Forward delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Caps Lock.
    CapsLock,
    /// The context-menu key.
    Menu,
}

/// A parsed combo: modifiers in the order given, then one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCombo {
    /// Held in order, released in reverse.
    pub modifiers: Vec<Modifier>,
    /// Clicked while the modifiers are held.
    pub key: KeyName,
}

fn modifier(name: &str) -> Option<Modifier> {
    Some(match name {
        "ctrl" | "control" => Modifier::Ctrl,
        "shift" => Modifier::Shift,
        "alt" | "option" | "opt" => Modifier::Alt,
        "meta" | "win" | "windows" | "super" | "cmd" | "command" => Modifier::Meta,
        _ => return None,
    })
}

fn key(name: &str) -> Option<KeyName> {
    if let Some(m) = modifier(name) {
        return Some(KeyName::Modifier(m));
    }
    let named = match name {
        "enter" | "return" => KeyName::Enter,
        "tab" => KeyName::Tab,
        "esc" | "escape" => KeyName::Escape,
        "space" => KeyName::Space,
        "backspace" => KeyName::Backspace,
        "delete" | "del" => KeyName::Delete,
        "insert" | "ins" => KeyName::Insert,
        "home" => KeyName::Home,
        "end" => KeyName::End,
        "pageup" | "pgup" => KeyName::PageUp,
        "pagedown" | "pgdn" => KeyName::PageDown,
        "up" => KeyName::Up,
        "down" => KeyName::Down,
        "left" => KeyName::Left,
        "right" => KeyName::Right,
        "capslock" => KeyName::CapsLock,
        "menu" | "apps" => KeyName::Menu,
        "plus" => KeyName::Char('+'),
        _ => {
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) => Some(KeyName::Char(c)),
                _ => function_key(name),
            };
        }
    };
    Some(named)
}

fn function_key(name: &str) -> Option<KeyName> {
    let n: u8 = name.strip_prefix('f')?.parse().ok()?;
    (1..=24).contains(&n).then_some(KeyName::F(n))
}

impl KeyCombo {
    /// Parses `"ctrl+shift+s"`-style text.
    pub fn parse(text: &str) -> ToolResult<Self> {
        let lower = text.trim().to_lowercase();
        if lower.is_empty() {
            return Err(ToolError::InvalidArgument("key combo is empty".into()));
        }
        // A trailing "+" is the plus key itself ("ctrl++"); a lone "+" too.
        let (body, plus_key) = match lower.strip_suffix("++") {
            Some(rest) => (rest.to_owned(), true),
            None if lower == "+" => (String::new(), true),
            None => (lower.clone(), false),
        };
        let mut parts: Vec<&str> = if body.is_empty() {
            Vec::new()
        } else {
            body.split('+').map(str::trim).collect()
        };
        if parts.iter().any(|p| p.is_empty()) {
            return Err(ToolError::InvalidArgument(format!(
                "key combo `{text}` has an empty part; write it like `ctrl+shift+s`"
            )));
        }
        let key_name = if plus_key {
            KeyName::Char('+')
        } else {
            let last = parts
                .pop()
                .expect("BUG: a non-empty combo has at least one part");
            key(last).ok_or_else(|| {
                ToolError::InvalidArgument(format!(
                    "unknown key `{last}` in `{text}`; use a character, f1-f24, or one of \
                     enter tab esc space backspace delete insert home end pageup pagedown \
                     up down left right capslock menu"
                ))
            })?
        };
        let mut modifiers = Vec::with_capacity(parts.len());
        for part in parts {
            let m = modifier(part).ok_or_else(|| {
                ToolError::InvalidArgument(format!(
                    "`{part}` in `{text}` is not a modifier; modifiers are ctrl, shift, alt, meta (win/cmd/super)"
                ))
            })?;
            if modifiers.contains(&m) {
                return Err(ToolError::InvalidArgument(format!(
                    "modifier `{part}` appears twice in `{text}`"
                )));
            }
            modifiers.push(m);
        }
        Ok(Self {
            modifiers,
            key: key_name,
        })
    }
}

impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for m in &self.modifiers {
            write!(f, "{m:?}+")?;
        }
        write!(f, "{:?}", self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> KeyCombo {
        KeyCombo::parse(text).expect("BUG: test combo should parse")
    }

    #[test]
    fn single_named_keys() {
        assert_eq!(parse("enter").key, KeyName::Enter);
        assert_eq!(parse("Return").key, KeyName::Enter);
        assert_eq!(parse("TAB").key, KeyName::Tab);
        assert_eq!(parse("esc").key, KeyName::Escape);
        assert_eq!(parse("pagedown").key, KeyName::PageDown);
        assert!(parse("enter").modifiers.is_empty());
    }

    #[test]
    fn modifiers_keep_their_order() {
        let combo = parse("ctrl+shift+s");
        assert_eq!(combo.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
        assert_eq!(combo.key, KeyName::Char('s'));
        let combo = parse("Shift + Ctrl + S");
        assert_eq!(combo.modifiers, vec![Modifier::Shift, Modifier::Ctrl]);
        assert_eq!(combo.key, KeyName::Char('s'));
    }

    #[test]
    fn modifier_aliases() {
        assert_eq!(parse("cmd+q").modifiers, vec![Modifier::Meta]);
        assert_eq!(parse("win+r").modifiers, vec![Modifier::Meta]);
        assert_eq!(parse("option+a").modifiers, vec![Modifier::Alt]);
        assert_eq!(parse("control+c").modifiers, vec![Modifier::Ctrl]);
    }

    #[test]
    fn function_keys_in_range_only() {
        assert_eq!(parse("alt+f4").key, KeyName::F(4));
        assert_eq!(parse("f24").key, KeyName::F(24));
        assert!(KeyCombo::parse("f25").is_err());
        assert!(KeyCombo::parse("f0").is_err());
    }

    #[test]
    fn lone_modifier_is_a_key() {
        let combo = parse("shift");
        assert!(combo.modifiers.is_empty());
        assert_eq!(combo.key, KeyName::Modifier(Modifier::Shift));
    }

    #[test]
    fn plus_key_spellings() {
        assert_eq!(parse("+").key, KeyName::Char('+'));
        let combo = parse("ctrl++");
        assert_eq!(combo.modifiers, vec![Modifier::Ctrl]);
        assert_eq!(combo.key, KeyName::Char('+'));
        assert_eq!(parse("ctrl+plus").key, KeyName::Char('+'));
    }

    #[test]
    fn digits_and_punctuation() {
        assert_eq!(parse("ctrl+1").key, KeyName::Char('1'));
        assert_eq!(parse("ctrl+/").key, KeyName::Char('/'));
    }

    #[test]
    fn rejects_malformed_combos() {
        for bad in [
            "",
            "   ",
            "ctrl+",
            "+s",
            "ctrl++s",
            "ctrl+ctrl+s",
            "s+ctrl",
            "hyper+s",
            "enterr",
        ] {
            assert!(KeyCombo::parse(bad).is_err(), "`{bad}` should be rejected");
        }
    }

    #[test]
    fn error_names_the_bad_part() {
        let err = KeyCombo::parse("ctrl+enterr")
            .expect_err("BUG: unknown key must fail")
            .to_string();
        assert!(err.contains("enterr"), "{err}");
    }
}
