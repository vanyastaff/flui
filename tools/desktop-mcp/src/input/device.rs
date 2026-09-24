//! The enigo-backed input device.
//!
//! Absolute pointer moves go through [`crate::os::move_pointer`] where the OS
//! offers a virtual-desktop-wide move (Windows), because enigo's absolute
//! move normalizes against the primary monitor only.

use std::thread;
use std::time::Duration;

use enigo::{Axis, Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};

use super::{Guard, MouseButton, Stroke, partial, strokes};
use crate::error::{ToolError, ToolResult};
use crate::keys::{KeyCombo, KeyName, Modifier};
#[cfg(not(target_os = "windows"))]
use enigo::Coordinate;

/// Pause between the parts of a synthesized gesture, so the target's event
/// loop sees distinct events rather than one coalesced burst.
const STEP: Duration = Duration::from_millis(15);

/// A virtual key Windows assigns to nothing (0xE8), tapped to keep a lone
/// Alt or Windows-key release from opening a menu.
#[cfg(target_os = "windows")]
const UNASSIGNED_VK: u32 = 0xE8;

/// The session's input device.
pub struct Input {
    enigo: Enigo,
    /// The mouse button that is down: pressed by a click or a drag whose
    /// release has not gone through yet. Every input call releases it first,
    /// or refuses.
    held_button: Option<Button>,
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
            .map(|enigo| Self {
                enigo,
                held_button: None,
            })
            .map_err(|e| format!("opening the input device failed: {e}"))
    }

    /// Refuses to send anything while a button or a key from an earlier
    /// call is still down, after trying once more to release it: with the
    /// left button held a move is a drag, and with Ctrl held a typed `x` is
    /// Ctrl+X.
    pub fn ready(&mut self) -> ToolResult<()> {
        if self.held_button.is_some() {
            self.release_held().map_err(|e| {
                ToolError::NotSupported(format!(
                    "a mouse button is still held from an earlier failed release ({e}); no input is sent until it is released"
                ))
            })?;
        }
        for key in self.enigo.held().0 {
            self.enigo.key(key, Direction::Release).map_err(|e| {
                ToolError::NotSupported(format!(
                    "{key:?} is still held from an earlier failed release ({e}); no input is sent until it is released"
                ))
            })?;
        }
        Ok(())
    }

    /// Releases every button and key this device holds, as far as the OS
    /// lets it: after a call panicked mid-action, and before the server
    /// exits.
    pub fn release_all(&mut self) {
        if self.held_button.is_some() && self.release_held().is_err() {
            tracing::warn!("a mouse button could not be released");
        }
        for key in self.enigo.held().0 {
            if let Err(e) = self.enigo.key(key, Direction::Release) {
                tracing::warn!("{key:?} could not be released: {e}");
            }
        }
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

    /// Moves to the point and confirms the pointer is there: the OS clamps a
    /// move to a screen edge or an active cursor clip while reporting
    /// success, and a press where the pointer stopped would land on something
    /// the safety check never looked at.
    fn move_verified(&mut self, x: i32, y: i32) -> ToolResult<()> {
        self.move_to(x, y)?;
        self.ensure_at(x, y)
    }

    /// Refuses unless the pointer is exactly at the point now — read again
    /// right before a press, since the user or another program can move it
    /// between the move and the press.
    fn ensure_at(&self, x: i32, y: i32) -> ToolResult<()> {
        match self.position() {
            Some(at) if at == (x, y) => Ok(()),
            Some((ax, ay)) => Err(ToolError::OutsideTarget {
                x,
                y,
                reason: format!(
                    "the pointer is at ({ax}, {ay}) instead (clamped to a screen edge or a cursor clip, or moved by someone else)"
                ),
            }),
            None => Err(ToolError::NotSupported(
                "the pointer position cannot be read back, so the move is not verified".into(),
            )),
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
        self.ready()?;
        // Checked before the first move too: a move is an event (hover,
        // tooltips) and must not reach a window that took the foreground.
        guard(None)?;
        self.move_verified(x, y)?;
        thread::sleep(STEP);
        let button = enigo_button(button);
        let clicks = if double { 2 } else { 1 };
        for sent in 0..clicks {
            // Press and release apart, so a press that went out is known:
            // its release is retried, and a failure counts the click as sent
            // with the button possibly still down.
            // Refused before the press: nothing of this click went out.
            if let Err(cause) = guard(None).and_then(|()| self.ensure_at(x, y)) {
                return Err(partial(cause, sent, clicks, "clicks"));
            }
            self.held_button = Some(button);
            let pressed = self
                .enigo
                .button(button, Direction::Press)
                .map_err(failed("pressing the button"));
            if let Err(cause) = pressed {
                // A press reported failed may still have gone out, and with
                // the release that follows it that is a whole click: say so,
                // whether or not the release went through.
                let released = self.release_held();
                return Err(ToolError::Interrupted {
                    cause: Box::new(cause),
                    what: match released {
                        Ok(()) => format!(
                            "{sent} of {clicks} clicks completed, and click {} may also have gone through (its press reported failure, then it was released); look before retrying",
                            sent + 1
                        ),
                        Err(e) => format!(
                            "{sent} of {clicks} clicks completed, click {} may have been pressed and releasing it failed ({e}); the button may still be held",
                            sent + 1
                        ),
                    },
                });
            }
            if let Err(cause) = self.release_held() {
                return Err(ToolError::Interrupted {
                    cause: Box::new(cause),
                    what: format!(
                        "click {} of {clicks} was pressed but not released; the button may still be held, and the next input releases it first",
                        sent + 1
                    ),
                });
            }
        }
        Ok(())
    }

    /// Presses at `from`, moves in steps over `duration`, releases at `to`.
    /// `guard` runs before the press, before every step and before the
    /// release, with the point about to be reached.
    ///
    /// The release is the drop, so it happens only at a verified point: when
    /// a check fails partway, the pointer goes back to the last point that
    /// passed and the button is released there once it passes again. If even
    /// that fails, the release still goes out (a held button would drag on)
    /// and the error says where; nothing else is sent unverified.
    pub fn drag(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        duration: Duration,
        guard: &mut Guard<'_>,
    ) -> ToolResult<()> {
        self.ready()?;
        guard(None)?;
        self.move_verified(from.0, from.1)?;
        thread::sleep(STEP);
        guard(None)?;
        self.ensure_at(from.0, from.1)?;
        // The primary button, as the user set it: with swapped buttons a
        // physical left press is a secondary one.
        let primary = enigo_button(MouseButton::Left);
        // Marked held before the press: a press that went out but reported
        // failure is released at once, and by the next call if that fails.
        self.held_button = Some(primary);
        if let Err(cause) = self
            .enigo
            .button(primary, Direction::Press)
            .map_err(failed("pressing for a drag"))
        {
            return Err(match self.release_held() {
                Ok(()) => cause,
                Err(e) => ToolError::Interrupted {
                    cause: Box::new(cause),
                    what: format!(
                        "the press may have gone out and releasing it failed ({e}); the button may still be held"
                    ),
                },
            });
        }
        let steps = (duration.as_millis() / STEP.as_millis()).clamp(2, 200) as i32;
        // The steps are bounded; their interval is not, so a long drag lasts
        // as long as it was asked to.
        let interval = (duration / steps.unsigned_abs()).max(STEP);
        let mut last = from;
        let mut moved = Ok(());
        for i in 1..=steps {
            let point = (lerp(from.0, to.0, i, steps), lerp(from.1, to.1, i, steps));
            thread::sleep(interval);
            moved = guard(Some(point)).and_then(|()| self.move_verified(point.0, point.1));
            if moved.is_err() {
                break;
            }
            last = point;
        }
        if moved.is_ok() {
            moved = guard(Some(to)).and_then(|()| self.ensure_at(to.0, to.1));
        }
        match moved {
            Ok(()) => self.release_held().map_err(|cause| ToolError::Interrupted {
                cause: Box::new(cause),
                what: format!(
                    "the drag reached ({}, {}) but the button could not be released; it may still be held, so move nothing until it is released",
                    to.0, to.1
                ),
            }),
            Err(cause) => Err(self.abort_drag(last, cause, guard)),
        }
    }

    /// Releases the held mouse button, retrying: a button left held turns
    /// the next pointer move, the user's included, into a drag.
    fn release_held(&mut self) -> ToolResult<()> {
        let Some(button) = self.held_button else {
            return Ok(());
        };
        let mut result = Ok(());
        for attempt in 0..3 {
            if attempt > 0 {
                thread::sleep(STEP);
            }
            result = self
                .enigo
                .button(button, Direction::Release)
                .map_err(failed("releasing the mouse button"));
            if result.is_ok() {
                self.held_button = None;
                break;
            }
        }
        result
    }

    /// Ends a drag that stopped partway (see [`Self::drag`]) and says how.
    fn abort_drag(
        &mut self,
        last: (i32, i32),
        cause: ToolError,
        guard: &mut Guard<'_>,
    ) -> ToolError {
        // Checked before the move: with the button held, a move is itself a
        // drag, and it must not reach a window that took the foreground.
        let back = guard(Some(last)).and_then(|()| self.move_verified(last.0, last.1));
        // No key goes out unverified: an Esc after the target lost the
        // foreground would reach whatever took it. The release is the one
        // event that must go out regardless, or the button stays held.
        let mut what = if back.is_ok() {
            format!(
                "the drag stopped; the button was released back at ({}, {}), a point verified inside the target",
                last.0, last.1
            )
        } else {
            let at = self.position().map_or_else(
                || "an unknown point".to_owned(),
                |(x, y)| format!("({x}, {y})"),
            );
            format!(
                "the drag stopped where no point could be verified inside the target; the button had to be released where the pointer was, at {at}, which may have dropped there"
            )
        };
        if let Err(e) = self.release_held() {
            what = format!("{what}; releasing the button failed ({e}), so it may still be held");
        }
        ToolError::Interrupted {
            cause: Box::new(cause),
            what,
        }
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
        self.ready()?;
        // Checked before the first move too: a move is an event (hover,
        // tooltips) and must not reach a window that took the foreground.
        guard(None)?;
        self.move_verified(x, y)?;
        thread::sleep(STEP);
        let axes = [(dy, Axis::Vertical), (dx, Axis::Horizontal)];
        let total = axes.iter().filter(|(n, _)| *n != 0).count();
        for (sent, (notches, axis)) in axes.into_iter().filter(|(n, _)| *n != 0).enumerate() {
            let scrolled = guard(None)
                .and_then(|()| self.ensure_at(x, y))
                .and_then(|()| {
                    self.enigo
                        .scroll(notches, axis)
                        .map_err(failed("scrolling"))
                });
            if let Err(cause) = scrolled {
                return Err(partial(cause, sent, total, "scroll axes"));
            }
        }
        Ok(())
    }

    /// Types text one keystroke at a time (see [`strokes`]) with `guard`
    /// before each, so a target that lost the foreground receives none of
    /// the rest; the error then says how much was typed.
    pub fn type_text(&mut self, text: &str, guard: &mut Guard<'_>) -> ToolResult<()> {
        self.ready()?;
        let total = text.chars().count();
        let mut typed = 0;
        let mut buffer = [0_u8; 4];
        for (stroke, chars) in strokes(text) {
            let sent = guard(None).and_then(|()| match stroke {
                Stroke::Key(key) => self.tap(enigo_key(key)?),
                // enigo releases a surrogate pair's low unit with the high
                // one, so a character past the BMP goes out on its own.
                #[cfg(target_os = "windows")]
                Stroke::Char(c) if u32::from(c) > 0xFFFF => crate::os::send_unicode(c),
                Stroke::Char(c) => self
                    .enigo
                    .text(c.encode_utf8(&mut buffer))
                    .map_err(failed("typing text")),
            });
            if let Err(cause) = sent {
                return Err(partial(cause, typed, total, "characters"));
            }
            typed += chars;
        }
        Ok(())
    }

    /// Presses and releases `key` apart: once the press is in, a failed
    /// release leaves it tracked as held (enigo keeps it in its held set), so
    /// the next input releases it first, and the error says it went in.
    fn tap(&mut self, key: Key) -> ToolResult<()> {
        self.enigo
            .key(key, Direction::Press)
            .map_err(failed("pressing a key"))?;
        self.enigo
            .key(key, Direction::Release)
            .map_err(|e| ToolError::Interrupted {
                cause: Box::new(ToolError::platform("releasing a key", e)),
                what: format!("{key:?} went in but its release failed; it may still be held until the next input releases it"),
            })
    }

    /// Presses the combo `repeat` times: modifiers down in order, key
    /// clicked, modifiers up in reverse. A character that needs Shift (or
    /// AltGr) on the current layout gets it added.
    ///
    /// `guard` runs before each repetition, before every modifier press and
    /// again before the key itself; a failed guard sends nothing more,
    /// releases the modifiers already held and stops.
    pub fn key(&mut self, combo: &KeyCombo, repeat: u32, guard: &mut Guard<'_>) -> ToolResult<()> {
        self.ready()?;
        let (key, modifiers) = resolve(combo)?;
        for done in 0..repeat {
            let pressed = self.press_once(key, &modifiers, guard);
            if let Err((cause, sent)) = pressed {
                let sent = done as usize + usize::from(sent);
                return Err(partial(cause, sent, repeat as usize, "presses"));
            }
            thread::sleep(STEP);
        }
        Ok(())
    }

    /// One press of the combo; on failure, whether the key itself had
    /// already gone out (a modifier release failing after it), which the
    /// caller counts as a press sent.
    fn press_once(
        &mut self,
        key: Key,
        modifiers: &[Modifier],
        guard: &mut Guard<'_>,
    ) -> Result<(), (ToolError, bool)> {
        guard(None).map_err(|e| (e, false))?;
        let mut held = Vec::with_capacity(modifiers.len());
        let mut result = Ok(());
        for &m in modifiers {
            let k = modifier_key(m);
            result = guard(None).and_then(|()| {
                self.enigo
                    .key(k, Direction::Press)
                    .map_err(failed("pressing a modifier"))
            });
            if result.is_err() {
                break;
            }
            held.push(k);
        }
        if result.is_ok() {
            result = guard(None);
        }
        let mut sent = false;
        if result.is_ok() {
            // Press and release apart: once the press is in, the key counts
            // as sent even if its release fails (enigo then still tracks it
            // as held, and the next input releases it).
            result = self
                .enigo
                .key(key, Direction::Press)
                .map_err(failed("pressing the key"));
            sent = result.is_ok();
            if sent {
                result = self
                    .enigo
                    .key(key, Direction::Release)
                    .map_err(failed("releasing the key (it may still be held)"));
            }
        }
        // Alt (or the Windows key) released on its own, with nothing pressed
        // while it was down, opens the menu bar (the Start menu) of whatever
        // window is in front now. An unassigned key tapped in between makes
        // it an ordinary chord that does nothing.
        #[cfg(target_os = "windows")]
        if !sent && held.iter().any(|k| matches!(k, Key::Alt | Key::Meta)) {
            let _ = self.enigo.key(Key::Other(UNASSIGNED_VK), Direction::Click);
        }
        for k in held.into_iter().rev() {
            let released = self
                .enigo
                .key(k, Direction::Release)
                .map_err(failed("releasing a modifier (it may still be held)"));
            result = result.and(released);
        }
        result.map_err(|e| (e, sent))
    }
}

/// The OS key for a combo's key and the modifiers to hold for it. On
/// Windows a character goes out as its layout's virtual key with the shift
/// state that layout needs: enigo would send the character's shifted
/// virtual-key code as is, which is no key at all.
fn resolve(combo: &KeyCombo) -> ToolResult<(Key, Vec<Modifier>)> {
    #[cfg(target_os = "windows")]
    if let KeyName::Char(c) = combo.key {
        let (vk, shift) = crate::os::char_key(c).ok_or_else(|| {
            ToolError::InvalidArgument(format!(
                "no key types `{c}` on the current keyboard layout; send it with type_text"
            ))
        })?;
        let mut modifiers = combo.modifiers.clone();
        modifiers.extend(super::implied_modifiers(shift, &combo.modifiers));
        return Ok((Key::Other(u32::from(vk)), modifiers));
    }
    Ok((enigo_key(combo.key)?, combo.modifiers.clone()))
}

/// The point `i/steps` of the way from `a` to `b`, in `i64` so a wide drag
/// cannot overflow after the button is already down. The result lies between
/// `a` and `b`, so it fits back in `i32`.
fn lerp(a: i32, b: i32, i: i32, steps: i32) -> i32 {
    let at = i64::from(a) + (i64::from(b) - i64::from(a)) * i64::from(i) / i64::from(steps);
    i32::try_from(at).expect("BUG: an interpolated point lies between two i32 endpoints")
}

/// The physical button for a logical one: injected button events are
/// physical, so with the buttons swapped a logical left click is a physical
/// right one.
fn enigo_button(button: MouseButton) -> Button {
    let swapped = crate::os::buttons_swapped();
    match button {
        MouseButton::Left if swapped => Button::Right,
        MouseButton::Right if swapped => Button::Left,
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
