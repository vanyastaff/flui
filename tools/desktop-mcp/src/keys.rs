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
                // A control character (a raw ESC, a raw CR) is a named key
                // under another spelling: taken as a character it would slip
                // past the checks that know the key by its name.
                (Some(c), None) if c.is_control() => None,
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
        // `shift+shift` is Shift pressed as a key while held: one modifier,
        // which would slip past checks that look at modifiers and key apart.
        if let KeyName::Modifier(m) = key_name
            && parts.iter().any(|&part| modifier(part) == Some(m))
        {
            return Err(ToolError::InvalidArgument(format!(
                "`{text}` names the same modifier as a modifier and as the key"
            )));
        }
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

    fn has(&self, m: Modifier) -> bool {
        self.modifiers.contains(&m) || self.key == KeyName::Modifier(m)
    }

    /// What handles this combo instead of the foreground window, if the OS
    /// does: the Windows key, the task switcher, Spotlight, the input-language
    /// switch. No foreground check can hold for such a combo, since the window
    /// in front never receives it. `macos` picks the platform's set; `repeat`
    /// matters for Shift, which pressed five times opens the Sticky Keys
    /// prompt.
    pub fn shell_hotkey(&self, macos: bool, repeat: u32) -> Option<&'static str> {
        let only = |mods: &[Modifier]| mods.iter().all(|&m| self.modifiers.contains(&m));
        // A modifier pressed five times in a row is an accessibility
        // shortcut: Shift turns on Sticky Keys (Windows, macOS), Option
        // turns on Mouse Keys (macOS).
        let tapped = |m: Modifier| {
            self.key == KeyName::Modifier(m) && self.modifiers.is_empty() && repeat >= 5
        };
        if tapped(Modifier::Shift) {
            return Some("the accessibility shortcut (Sticky Keys)");
        }
        if macos && tapped(Modifier::Alt) {
            return Some("the accessibility shortcut (Mouse Keys)");
        }
        if macos {
            return match self.key {
                KeyName::Tab if self.has(Modifier::Meta) => Some("the macOS app switcher"),
                KeyName::Space if self.has(Modifier::Meta) || self.has(Modifier::Ctrl) => {
                    Some("Spotlight, the Character Viewer or the input-source switch")
                }
                KeyName::Char('d') if only(&[Modifier::Meta, Modifier::Alt]) => {
                    Some("the Dock (show or hide)")
                }
                KeyName::F(2 | 3) if self.has(Modifier::Ctrl) => {
                    Some("macOS keyboard navigation (the menu bar or the Dock)")
                }
                KeyName::F(3 | 4 | 11) if self.modifiers.is_empty() => {
                    Some("Mission Control, Launchpad or Show Desktop")
                }
                KeyName::Char('1'..='9') if only(&[Modifier::Ctrl]) => {
                    Some("Mission Control (switching Spaces)")
                }
                KeyName::Char('q') if only(&[Modifier::Meta, Modifier::Ctrl]) => {
                    Some("macOS (Lock Screen)")
                }
                KeyName::Char('q') if only(&[Modifier::Meta, Modifier::Shift]) => {
                    Some("macOS (Log Out)")
                }
                KeyName::Escape if only(&[Modifier::Meta, Modifier::Alt]) => Some("Force Quit"),
                KeyName::Up | KeyName::Down | KeyName::Left | KeyName::Right
                    if self.has(Modifier::Ctrl) =>
                {
                    Some("Mission Control")
                }
                KeyName::Char('3' | '4' | '5') if only(&[Modifier::Meta, Modifier::Shift]) => {
                    Some("the macOS screenshot tool")
                }
                _ => None,
            };
        }
        if self.has(Modifier::Meta) {
            return Some("the Windows shell (the Windows key)");
        }
        // Caps Lock changes a state every window shares, not the target's.
        if self.key == KeyName::CapsLock {
            return Some("the keyboard's lock state, shared by every window");
        }
        match self.key {
            KeyName::Tab | KeyName::Escape if self.has(Modifier::Alt) => {
                Some("the Windows task switcher")
            }
            KeyName::Escape if only(&[Modifier::Ctrl, Modifier::Shift]) => Some("Task Manager"),
            KeyName::Escape if self.has(Modifier::Ctrl) => Some("the Start menu"),
            KeyName::Delete if only(&[Modifier::Ctrl, Modifier::Alt]) => {
                Some("the Windows secure attention sequence")
            }
            // Alt+Shift and Ctrl+Shift switch the input language, for every
            // window at once.
            KeyName::Modifier(Modifier::Shift)
                if self.has(Modifier::Alt) || self.has(Modifier::Ctrl) =>
            {
                Some("the Windows input-language switch")
            }
            KeyName::Modifier(Modifier::Alt | Modifier::Ctrl) if self.has(Modifier::Shift) => {
                Some("the Windows input-language switch")
            }
            _ => None,
        }
    }
}

impl Modifier {
    fn name(self) -> &'static str {
        match self {
            Self::Ctrl => "ctrl",
            Self::Shift => "shift",
            Self::Alt => "alt",
            Self::Meta => "meta",
        }
    }
}

/// The combo in the syntax [`KeyCombo::parse`] reads: `ctrl+shift+s`.
impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for m in &self.modifiers {
            write!(f, "{}+", m.name())?;
        }
        match self.key {
            KeyName::Char('+') => f.write_str("plus"),
            KeyName::Char(c) => write!(f, "{c}"),
            KeyName::F(n) => write!(f, "f{n}"),
            KeyName::Modifier(m) => f.write_str(m.name()),
            KeyName::Enter => f.write_str("enter"),
            KeyName::Tab => f.write_str("tab"),
            KeyName::Escape => f.write_str("esc"),
            KeyName::Space => f.write_str("space"),
            KeyName::Backspace => f.write_str("backspace"),
            KeyName::Delete => f.write_str("delete"),
            KeyName::Insert => f.write_str("insert"),
            KeyName::Home => f.write_str("home"),
            KeyName::End => f.write_str("end"),
            KeyName::PageUp => f.write_str("pageup"),
            KeyName::PageDown => f.write_str("pagedown"),
            KeyName::Up => f.write_str("up"),
            KeyName::Down => f.write_str("down"),
            KeyName::Left => f.write_str("left"),
            KeyName::Right => f.write_str("right"),
            KeyName::CapsLock => f.write_str("capslock"),
            KeyName::Menu => f.write_str("menu"),
        }
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
            "shift+shift",
            "cmd+meta",
            "s+ctrl",
            "hyper+s",
            "enterr",
        ] {
            assert!(KeyCombo::parse(bad).is_err(), "`{bad}` should be rejected");
        }
    }

    /// A combo prints in the syntax it is parsed from, so messages quote
    /// what the agent can send back.
    #[test]
    fn a_combo_prints_as_it_parses() {
        for text in [
            "ctrl+shift+s",
            "alt+f4",
            "meta+r",
            "ctrl+plus",
            "enter",
            "shift",
            "pagedown",
        ] {
            let combo = parse(text);
            assert_eq!(combo.to_string(), text);
            assert_eq!(parse(&combo.to_string()), combo);
        }
    }

    /// Combos the OS shell takes before the foreground window sees them are
    /// named, per platform; ordinary shortcuts are not.
    #[test]
    fn shell_hotkeys_are_recognized() {
        for shell in [
            "win",
            "win+r",
            "win+d",
            "ctrl+esc",
            "alt+tab",
            "alt+shift+tab",
            "alt+esc",
            "ctrl+shift+esc",
            "ctrl+alt+delete",
        ] {
            assert!(
                parse(shell).shell_hotkey(false, 1).is_some(),
                "`{shell}` on Windows"
            );
        }
        for app in [
            "ctrl+s",
            "alt+f4",
            "ctrl+tab",
            "esc",
            "shift+tab",
            "ctrl+shift+s",
            "alt+d",
        ] {
            assert_eq!(
                parse(app).shell_hotkey(false, 1),
                None,
                "`{app}` on Windows"
            );
        }
        for shell in [
            "cmd+tab",
            "cmd+space",
            "cmd+alt+esc",
            "ctrl+left",
            "cmd+shift+4",
            "ctrl+cmd+q",
            "cmd+shift+q",
            "cmd+alt+shift+q",
            "ctrl+f3",
            "ctrl+f2",
            "cmd+ctrl+space",
            "ctrl+space",
            "cmd+alt+d",
            "ctrl+2",
            "f11",
        ] {
            assert!(
                parse(shell).shell_hotkey(true, 1).is_some(),
                "`{shell}` on macOS"
            );
        }
        for app in ["cmd+q", "cmd+s", "cmd+shift+s", "esc", "cmd+w"] {
            assert_eq!(parse(app).shell_hotkey(true, 1), None, "`{app}` on macOS");
        }
    }

    /// Windows switches the input language on Alt+Shift and Ctrl+Shift, and
    /// Shift pressed five times opens the Sticky Keys prompt.
    #[test]
    fn language_switch_and_sticky_keys_are_shell_hotkeys() {
        for shell in ["alt+shift", "ctrl+shift", "shift+alt"] {
            assert!(parse(shell).shell_hotkey(false, 1).is_some(), "`{shell}`");
        }
        assert!(parse("shift").shell_hotkey(false, 5).is_some());
        assert!(parse("shift").shell_hotkey(true, 5).is_some());
        assert_eq!(parse("shift").shell_hotkey(false, 4), None);
        assert!(parse("alt").shell_hotkey(true, 5).is_some(), "Mouse Keys");
        assert!(
            parse("capslock").shell_hotkey(false, 1).is_some(),
            "Caps Lock"
        );
    }

    /// A control character is not a key of its own: a raw ESC would
    /// otherwise reach the keyboard as Escape without the checks that know
    /// Escape by name (ctrl+esc opens Start).
    #[test]
    fn control_characters_are_not_keys() {
        for raw in ["ctrl+\u{1b}", "alt+\u{1b}", "\u{7f}", "ctrl+\u{0}"] {
            assert!(KeyCombo::parse(raw).is_err(), "{raw:?} must be refused");
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
