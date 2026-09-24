//! The enigo-backed input device.
//!
//! Absolute pointer moves go through [`crate::os::move_pointer`] where the OS
//! offers a virtual-desktop-wide move (Windows), because enigo's absolute
//! move normalizes against the primary monitor only.

use std::thread;
use std::time::Duration;

use enigo::{Axis, Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};

use super::{Guard, MouseButton};
use crate::error::{ToolError, ToolResult};
use crate::keys::{KeyCombo, KeyName, Modifier};
#[cfg(not(target_os = "windows"))]
use enigo::Coordinate;

/// Pause between the parts of a synthesized gesture, so the target's event
/// loop sees distinct events rather than one coalesced burst.
const STEP: Duration = Duration::from_millis(15);

/// The session's input device.
pub struct Input {
    enigo: Enigo,
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Input").finish_non_exhaustive()
    }
}

fn failed(what: &'static str) -> impl FnOnce(enigo::InputError) -> ToolError {
    move |e| ToolError::platform(what, e)
}

impl Input {
    /// Opens the OS input connection.
    pub fn new() -> Result<Self, String> {
        let settings = Settings {
            release_keys_when_dropped: true,
            ..Settings::default()
        };
        Enigo::new(&settings)
            .map(|enigo| Self { enigo })
            .map_err(|e| format!("opening the input device failed: {e}"))
    }

    /// Moves the pointer to a physical screen point.
    #[cfg_attr(
        target_os = "windows",
        expect(clippy::unused_self, reason = "the Windows path bypasses enigo")
    )]
    pub fn move_to(&mut self, x: i32, y: i32) -> ToolResult<()> {
        #[cfg(target_os = "windows")]
        {
            crate::os::move_pointer(x, y)
        }
        #[cfg(not(target_os = "windows"))]
        {
            self.enigo
                .move_mouse(x, y, Coordinate::Abs)
                .map_err(failed("moving the pointer"))
        }
    }

    /// Where the pointer is now.
    pub fn position(&self) -> Option<(i32, i32)> {
        crate::os::cursor().or_else(|| self.enigo.location().ok())
    }

    /// Moves to the point and clicks once or twice. `guard` runs before each
    /// click, so a target that lost the foreground receives no further one.
    pub fn click(
        &mut self,
        x: i32,
        y: i32,
        button: MouseButton,
        double: bool,
        guard: &mut Guard<'_>,
    ) -> ToolResult<()> {
        self.move_to(x, y)?;
        thread::sleep(STEP);
        let button = enigo_button(button);
        for _ in 0..if double { 2 } else { 1 } {
            guard()?;
            self.enigo
                .button(button, Direction::Click)
                .map_err(failed("clicking"))?;
        }
        Ok(())
    }

    /// Presses at `from`, moves in steps over `duration`, releases at `to`.
    /// `guard` runs before the press and before every step; a failed guard
    /// stops the drag, and the button is released either way.
    pub fn drag(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        duration: Duration,
        guard: &mut Guard<'_>,
    ) -> ToolResult<()> {
        self.move_to(from.0, from.1)?;
        thread::sleep(STEP);
        guard()?;
        self.enigo
            .button(Button::Left, Direction::Press)
            .map_err(failed("pressing for a drag"))?;
        let steps = (duration.as_millis() / STEP.as_millis()).clamp(2, 200) as i32;
        let moved = (1..=steps).try_for_each(|i| {
            let x = from.0 + (to.0 - from.0) * i / steps;
            let y = from.1 + (to.1 - from.1) * i / steps;
            thread::sleep(STEP);
            guard()?;
            self.move_to(x, y)
        });
        // Release even if a move failed, so no button stays held.
        let released = self
            .enigo
            .button(Button::Left, Direction::Release)
            .map_err(failed("releasing after a drag"));
        moved.and(released)
    }

    /// Moves to the point, then scrolls `dx`/`dy` wheel notches (positive
    /// is right/down).
    pub fn scroll(
        &mut self,
        x: i32,
        y: i32,
        dx: i32,
        dy: i32,
        guard: &mut Guard<'_>,
    ) -> ToolResult<()> {
        self.move_to(x, y)?;
        thread::sleep(STEP);
        if dy != 0 {
            guard()?;
            self.enigo
                .scroll(dy, Axis::Vertical)
                .map_err(failed("scrolling"))?;
        }
        if dx != 0 {
            guard()?;
            self.enigo
                .scroll(dx, Axis::Horizontal)
                .map_err(failed("scrolling"))?;
        }
        Ok(())
    }

    /// Types text as characters, independent of the keyboard layout, one
    /// character at a time with `guard` before each, so a target that lost
    /// the foreground receives none of the rest.
    pub fn type_text(&mut self, text: &str, guard: &mut Guard<'_>) -> ToolResult<()> {
        if text.contains('\0') {
            return Err(ToolError::InvalidArgument(
                "text must not contain NUL characters".into(),
            ));
        }
        let mut buffer = [0_u8; 4];
        for ch in text.chars() {
            guard()?;
            self.enigo
                .text(ch.encode_utf8(&mut buffer))
                .map_err(failed("typing text"))?;
        }
        Ok(())
    }

    /// Presses the combo `repeat` times: modifiers down in order, key
    /// clicked, modifiers up in reverse.
    ///
    /// `guard` runs before each repetition and again before the key itself,
    /// once the modifiers are down; a failed guard sends no key, releases
    /// the held modifiers and stops.
    pub fn key(&mut self, combo: &KeyCombo, repeat: u32, guard: &mut Guard<'_>) -> ToolResult<()> {
        let key = enigo_key(combo.key)?;
        for _ in 0..repeat {
            guard()?;
            let mut held = Vec::with_capacity(combo.modifiers.len());
            let mut result = Ok(());
            for &m in &combo.modifiers {
                let k = modifier_key(m);
                result = self
                    .enigo
                    .key(k, Direction::Press)
                    .map_err(failed("pressing a modifier"));
                if result.is_err() {
                    break;
                }
                held.push(k);
            }
            if result.is_ok() {
                result = guard();
            }
            if result.is_ok() {
                result = self
                    .enigo
                    .key(key, Direction::Click)
                    .map_err(failed("pressing the key"));
            }
            for k in held.into_iter().rev() {
                let released = self
                    .enigo
                    .key(k, Direction::Release)
                    .map_err(failed("releasing a modifier"));
                result = result.and(released);
            }
            result?;
            thread::sleep(STEP);
        }
        Ok(())
    }
}

fn enigo_button(button: MouseButton) -> Button {
    match button {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
    }
}

fn modifier_key(m: Modifier) -> Key {
    match m {
        Modifier::Ctrl => Key::Control,
        Modifier::Shift => Key::Shift,
        Modifier::Alt => Key::Alt,
        Modifier::Meta => Key::Meta,
    }
}

fn enigo_key(key: KeyName) -> ToolResult<Key> {
    Ok(match key {
        KeyName::Char(c) => Key::Unicode(c),
        KeyName::F(n) => function_key(n)?,
        KeyName::Modifier(m) => modifier_key(m),
        KeyName::Enter => Key::Return,
        KeyName::Tab => Key::Tab,
        KeyName::Escape => Key::Escape,
        KeyName::Space => Key::Space,
        KeyName::Backspace => Key::Backspace,
        KeyName::Delete => Key::Delete,
        #[cfg(not(target_os = "macos"))]
        KeyName::Insert => Key::Insert,
        #[cfg(target_os = "macos")]
        KeyName::Insert => return Err(unavailable("insert")),
        KeyName::Home => Key::Home,
        KeyName::End => Key::End,
        KeyName::PageUp => Key::PageUp,
        KeyName::PageDown => Key::PageDown,
        KeyName::Up => Key::UpArrow,
        KeyName::Down => Key::DownArrow,
        KeyName::Left => Key::LeftArrow,
        KeyName::Right => Key::RightArrow,
        KeyName::CapsLock => Key::CapsLock,
        #[cfg(target_os = "windows")]
        KeyName::Menu => Key::Apps,
        #[cfg(not(target_os = "windows"))]
        KeyName::Menu => return Err(unavailable("menu")),
    })
}

fn unavailable(name: &str) -> ToolError {
    ToolError::NotSupported(format!(
        "the {name} key cannot be synthesized on {}",
        std::env::consts::OS
    ))
}

fn function_key(n: u8) -> ToolResult<Key> {
    Ok(match n {
        1 => Key::F1,
        2 => Key::F2,
        3 => Key::F3,
        4 => Key::F4,
        5 => Key::F5,
        6 => Key::F6,
        7 => Key::F7,
        8 => Key::F8,
        9 => Key::F9,
        10 => Key::F10,
        11 => Key::F11,
        12 => Key::F12,
        13 => Key::F13,
        14 => Key::F14,
        15 => Key::F15,
        16 => Key::F16,
        17 => Key::F17,
        18 => Key::F18,
        19 => Key::F19,
        20 => Key::F20,
        #[cfg(not(target_os = "macos"))]
        21 => Key::F21,
        #[cfg(not(target_os = "macos"))]
        22 => Key::F22,
        #[cfg(not(target_os = "macos"))]
        23 => Key::F23,
        #[cfg(not(target_os = "macos"))]
        24 => Key::F24,
        _ => return Err(unavailable(&format!("F{n}"))),
    })
}
