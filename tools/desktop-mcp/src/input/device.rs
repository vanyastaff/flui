//! The enigo-backed input device.
//!
//! Absolute pointer moves go through [`crate::os::move_pointer`] where the OS
//! offers a virtual-desktop-wide move (Windows), because enigo's absolute
//! move normalizes against the primary monitor only.

use std::thread;
use std::time::Duration;

use enigo::{Axis, Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};

use super::{Guard, MouseButton, STEP, Stroke, drag_path, partial, strokes};
use crate::error::{Effect, ToolError, ToolResult};
use crate::keys::{KeyCombo, KeyName, Modifier};

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
    /// A Unicode unit a partial send left down (Windows), outside enigo's
    /// held set: released before any further input.
    #[cfg(target_os = "windows")]
    stuck_unit: Option<u16>,
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
                #[cfg(target_os = "windows")]
                stuck_unit: None,
            })
            .map_err(|e| format!("opening the input device failed: {e}"))
    }

    /// Refuses to send anything while a button or a key from an earlier
    /// call is still down, after trying once more to release it: with the
    /// left button held a move is a drag, and with Ctrl held a typed `x` is
    /// Ctrl+X.
    ///
    /// A release that goes through now lands wherever the pointer and the
    /// focus are now: a dropped drag, a click, a lone Alt opening a menu. So
    /// the call stops there and says so, rather than going on as if nothing
    /// had been sent.
    pub fn ready(&mut self) -> ToolResult<()> {
        // Every one is tried: the ones that go out are side effects to
        // report even when another stays down.
        let mut released = Vec::new();
        let mut still_held = Vec::new();
        #[cfg(target_os = "windows")]
        if let Some(unit) = self.stuck_unit {
            if crate::os::release_unicode(unit) {
                self.stuck_unit = None;
                released.push("a typed character's key".to_owned());
            } else {
                still_held.push("a typed character's key".to_owned());
            }
        }
        if let Some(button) = self.held_button {
            match self.release_held() {
                Ok(()) => released.push(format!("the {button:?} mouse button")),
                Err(e) => still_held.push(format!("the {button:?} mouse button ({e})")),
            }
        }
        self.mask_lone_modifiers();
        for key in self.enigo.held().0 {
            match self.enigo.key(key, Direction::Release) {
                // The mask key itself does nothing: releasing it is no
                // side effect to report.
                #[cfg(target_os = "windows")]
                Ok(()) if key == Key::Other(UNASSIGNED_VK) => {}
                Ok(()) => released.push(format!("{key:?}")),
                Err(e) => still_held.push(format!("{key:?} ({e})")),
            }
        }
        if released.is_empty() && still_held.is_empty() {
            return Ok(());
        }
        let stuck = (!still_held.is_empty()).then(|| ToolError::InputHeld(still_held.join(", ")));
        if released.is_empty() {
            return Err(stuck.expect("BUG: something was held"));
        }
        let cause = stuck.unwrap_or_else(|| {
            ToolError::Busy(format!(
                "{} held from an earlier failed release {} released first",
                released.join(", "),
                if released.len() == 1 { "was" } else { "were" }
            ))
        });
        Err(cause.after(
            Effect::Incidental,
            format!(
                "{} went to wherever the pointer and focus are now and may have dropped, clicked or opened a menu there; nothing of this call was sent, so look, then retry",
                released.join(", ")
            ),
        ))
    }

    /// Releases every button and key this device holds, as far as the OS
    /// lets it: after a call panicked mid-action, and before the server
    /// exits.
    pub fn release_all(&mut self) {
        #[cfg(target_os = "windows")]
        // Cleared only once released: `ready` keeps refusing input while it
        // is still down.
        if let Some(unit) = self.stuck_unit {
            if crate::os::release_unicode(unit) {
                self.stuck_unit = None;
            } else {
                tracing::warn!("a typed character's key could not be released");
            }
        }
        if self.held_button.is_some() && self.release_held().is_err() {
            tracing::warn!("a mouse button could not be released");
        }
        self.mask_lone_modifiers();
        for key in self.enigo.held().0 {
            if let Err(e) = self.enigo.key(key, Direction::Release) {
                tracing::warn!("{key:?} could not be released: {e}");
            }
        }
    }

    /// Before held modifiers are released on their own: the mask tap, as a
    /// chord does it, so the release does not open the menu bar or Start
    /// menu, or switch the input language, in whatever window is in front.
    #[cfg_attr(
        not(target_os = "windows"),
        expect(clippy::unused_self, reason = "the masked gestures are Windows ones")
    )]
    fn mask_lone_modifiers(&mut self) {
        #[cfg(target_os = "windows")]
        if self
            .enigo
            .held()
            .0
            .iter()
            .any(|k| matches!(k, Key::Alt | Key::Meta | Key::Control | Key::Shift))
            && !self.mask()
        {
            tracing::warn!("could not mask held modifiers before releasing them");
        }
    }

    /// Moves the pointer to a physical screen point.
    #[cfg_attr(
        not(target_os = "windows"),
        expect(
            clippy::unused_self,
            reason = "unsupported platforms refuse movement before accessing device state"
        )
    )]
    pub fn move_to(&mut self, x: i32, y: i32) -> ToolResult<()> {
        #[cfg(target_os = "windows")]
        {
            // With a button the user holds, a move is a drag (or its drop)
            // in whatever has the mouse: refused. Our own held button (a
            // drag in progress) is the one exception, that button only: a
            // second one pressed during the drag refuses the next step.
            let ours = self.held_button.map_or(0, physical_bit);
            super::only_owned_button(crate::os::mouse_buttons_down(), ours)?;
            crate::os::move_pointer(x, y)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (x, y);
            Err(ToolError::NotSupported(
                "pointer movement is refused until the OS can verify that no physical mouse button is held".into(),
            ))
        }
    }

    /// Moves to the point and confirms the pointer is there: the OS clamps a
    /// move to a screen edge or an active cursor clip while reporting
    /// success, and a press where the pointer stopped would land on something
    /// the safety check never looked at.
    fn move_verified(&mut self, x: i32, y: i32) -> ToolResult<()> {
        self.move_to(x, y)?;
        self.verify_moved(x, y)
    }

    fn verify_moved(&self, x: i32, y: i32) -> ToolResult<()> {
        match self.position() {
            // Right after our own move: the OS clamped it (a screen edge, a
            // cursor clip), which a retry repeats.
            Some(at) if at != (x, y) => Err(ToolError::OutsideTarget {
                x,
                y,
                reason: format!(
                    "the pointer stopped at ({}, {}): the OS keeps it out of that point (a screen edge, or a cursor clip the application holds)",
                    at.0, at.1
                ),
                covered_by: None,
            }),
            _ => self.ensure_at(x, y),
        }
    }

    /// Refuses unless the pointer is exactly at the point now — read again
    /// right before a press, since the user or another program can move it
    /// between the move and the press.
    fn ensure_at(&self, x: i32, y: i32) -> ToolResult<()> {
        match self.position() {
            Some(at) if at == (x, y) => Ok(()),
            // Passing when someone moved it; a clamp repeats, and the
            // message says which to suspect.
            Some((ax, ay)) => Err(ToolError::Busy(format!(
                "the pointer is at ({ax}, {ay}) instead of ({x}, {y}) (moved by someone else, or clamped to a screen edge or a cursor clip, which a retry repeats)"
            ))),
            None => Err(ToolError::NotSupported(
                "the pointer position cannot be read back, so the move is not verified".into(),
            )),
        }
    }

    /// Where the pointer is now.
    pub fn position(&self) -> Option<(i32, i32)> {
        crate::os::cursor().or_else(|| self.enigo.location().ok())
    }

    /// Moves before a pointer action, then accounts for that event on every
    /// later failure, including a clamped move or a refused first press.
    fn pointer_action(
        &mut self,
        point: (i32, i32),
        guard: &mut Guard<'_>,
        action: impl FnOnce(&mut Self, &mut Guard<'_>) -> ToolResult<()>,
    ) -> ToolResult<()> {
        self.ready()?;
        guard(None)?;
        self.move_to(point.0, point.1)?;
        super::after_pointer_move(point, || {
            self.verify_moved(point.0, point.1)?;
            action(self, guard)
        })
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
        self.pointer_action((x, y), guard, |this, guard| {
            thread::sleep(STEP);
            let button = enigo_button(button);
            let clicks = if double { 2 } else { 1 };
            for sent in 0..clicks {
                // A completed click needs both press and release. A failed
                // release leaves this click uncertain, even though its down is
                // known; only earlier completed clicks contribute to progress.
                // Refused before the press: nothing of this click went out.
                // The button wait first: it can take a moment, and the target
                // and position are checked after it, right before the press.
                if let Err(cause) = nothing_held().and_then(|()| {
                    super::guarded_input_event(
                        nothing_held_now,
                        || guard(None),
                        || this.ensure_at(x, y),
                    )
                }) {
                    return Err(partial(cause, sent, clicks, "clicks"));
                }
                this.held_button = Some(button);
                let pressed = this
                    .enigo
                    .button(button, Direction::Press)
                    .map_err(failed("pressing the button"));
                super::complete_click(pressed, || this.release_held(), sent, clicks)?;
            }
            Ok(())
        })
    }

    /// Presses at `from`, moves in steps over `duration`, releases at `to`.
    /// `guard` runs before the press, before every step and before the
    /// release, with the point about to be reached.
    ///
    /// Keyboard state is checked around every target guard: a physical key
    /// must not turn later movement into a modified drag. Recovery moves to
    /// the last verified point only with a clear keyboard and verified target.
    /// The mouse button is always released, even if recovery is refused: a
    /// generic drag has no guaranteed cancellation, and keeping our button
    /// down would modify the user's next movement. Physical keys are never
    /// released on the user's behalf; an unavoidable modified drop is reported.
    pub fn drag(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        duration: Duration,
        guard: &mut Guard<'_>,
    ) -> ToolResult<()> {
        self.pointer_action(from, guard, |this, guard| {
            thread::sleep(STEP);
            nothing_held()?;
            super::guarded_input_event(
                nothing_held_now,
                || guard(None),
                || this.ensure_at(from.0, from.1),
            )?;
            // The primary button, as the user set it: with swapped buttons a
            // physical left press is a secondary one.
            let primary = enigo_button(MouseButton::Left);
            // Marked held before the press: a press that went out but reported
            // failure is released at once, and by the next call if that fails.
            this.held_button = Some(primary);
            if let Err(cause) = this
                .enigo
                .button(primary, Direction::Press)
                .map_err(failed("pressing for a drag"))
            {
                // A press that went out, then released, is a click at the drag's
                // start: say so either way.
                let released = this.release_held();
                return Err(cause.after(
                    Effect::MayHaveRun,
                    match released {
                        Ok(()) => format!(
                            "the press may have gone out and was released, which is a click at ({}, {}); look before retrying",
                            from.0, from.1
                        ),
                        Err(e) => format!(
                            "the press may have gone out and releasing it failed ({e}); the button may still be held"
                        ),
                    },
                ));
            }
            let path = drag_path(from, to, duration);
            let steps = u32::try_from(path.len()).expect("BUG: at most 200 steps");
            // The steps are bounded; their interval is not, so a long drag lasts
            // as long as it was asked to.
            let interval = (duration / steps).max(STEP);
            let mut last = from;
            let mut done = 0;
            // SendInput can return before async button state reflects the down.
            // Only this initial transition gets a bounded settling wait. Once
            // observed held, a later missing bit means the drag was interrupted.
            #[cfg(target_os = "windows")]
            let mut moved = super::settle_owned_button(
                physical_bit(primary),
                || {
                    no_keyboard_input()?;
                    Ok(crate::os::mouse_buttons_down())
                },
                || thread::sleep(Duration::from_millis(5)),
            );
            #[cfg(not(target_os = "windows"))]
            let mut moved = Ok(());
            let held_button = this.held_button;
            for point in path {
                if moved.is_err() {
                    break;
                }
                thread::sleep(interval);
                moved = super::guarded_input_event(
                    || no_drag_interference(held_button),
                    || guard(Some(point)),
                    || this.move_verified(point.0, point.1),
                );
                if moved.is_err() {
                    break;
                }
                last = point;
                done += 1;
            }
            if moved.is_ok() {
                moved = super::guarded_input_event(
                    || no_drag_interference(held_button),
                    || guard(Some(to)),
                    || this.ensure_at(to.0, to.1),
                );
            }
            match moved {
                Ok(()) => this.release_held().map_err(|cause| {
                    cause.after(
                        Effect::Ran,
                        format!(
                            "the drag reached ({}, {}) but the button could not be released; it may still be held, so move nothing until it is released",
                            to.0, to.1
                        ),
                    )
                }),
                Err(cause) => {
                    let went = Effect::Partial {
                        sent: done,
                        total: steps as usize,
                        unit: "drag steps",
                    };
                    Err(this.abort_drag(last, cause, went, guard))
                }
            }
        })
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
        went: Effect,
        guard: &mut Guard<'_>,
    ) -> ToolError {
        // Never move during a modified drag, even for recovery. Being at
        // the last point already does not exempt a drop from checking other
        // physical buttons and the keyboard around the target lookup.
        let held_button = self.held_button;
        let back = super::guarded_input_event(
            || no_drag_interference(held_button),
            || guard(Some(last)),
            || {
                if self.position() == Some(last) {
                    Ok(())
                } else {
                    self.move_verified(last.0, last.1)
                }
            },
        )
        .and_then(|()| {
            super::guarded_input_event(
                || no_drag_interference(held_button),
                || guard(Some(last)),
                || self.ensure_at(last.0, last.1),
            )
        });
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
                "the drag stopped and recovery was refused; the button had to be released where the pointer was, at {at}, which may have dropped there; any physical keys or additional mouse buttons still held may modify that drop and were not released by this tool"
            )
        };
        if let Err(e) = self.release_held() {
            what = format!("{what}; releasing the button failed ({e}), so it may still be held");
        }
        cause.after(went, what)
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
        self.pointer_action((x, y), guard, |this, guard| {
            thread::sleep(STEP);
            let axes = [(dy, Axis::Vertical), (dx, Axis::Horizontal)];
            let total = axes.iter().filter(|(n, _)| *n != 0).count();
            for (sent, (notches, axis)) in axes.into_iter().filter(|(n, _)| *n != 0).enumerate() {
                let scrolled = nothing_held().and_then(|()| {
                    super::guarded_input_event(
                        nothing_held_now,
                        || guard(None),
                        || {
                            this.ensure_at(x, y)?;
                            this.enigo
                                .scroll(notches, axis)
                                .map_err(failed("scrolling"))
                        },
                    )
                });
                if let Err(cause) = scrolled {
                    return Err(partial(cause, sent, total, "scroll axes"));
                }
            }
            Ok(())
        })
    }

    /// Types text one keystroke at a time (see [`strokes`]) with `guard`
    /// before each, so a target that lost the foreground receives none of
    /// the rest; the error then says how much was typed.
    pub fn type_text(&mut self, text: &str, guard: &mut Guard<'_>) -> ToolResult<()> {
        self.ready()?;
        let total = text.chars().count();
        let mut typed = 0;
        #[cfg(not(target_os = "windows"))]
        let mut buffer = [0_u8; 4];
        for (stroke, chars) in strokes(text) {
            // Only a confirmed press contributes to progress. An uncertain
            // Enter still carries may_have_run, so it must not be retried blindly.
            let mut tapped = false;
            // A held Ctrl would turn a typed Enter into Ctrl+Enter.
            let clear = nothing_held;
            // The key-state wait first: it can take a moment, in which the
            // foreground can change, so the target is checked after it,
            // right before the keystroke.
            let sent = clear().and_then(|()| {
                super::guarded_input_event(
                    nothing_held_now,
                    || guard(None),
                    || match stroke {
                        Stroke::Key(key) => {
                            let key = enigo_key(key)?;
                            let (result, pressed) = self.tap(key);
                            tapped = pressed;
                            result
                        }
                        // On Windows every character goes out through our own
                        // `SendInput`, which knows how much of it went in: enigo's
                        // reports only failure (and releases a surrogate pair's low
                        // unit with the high one).
                        #[cfg(target_os = "windows")]
                        Stroke::Char(c) => {
                            crate::os::send_unicode(c).map_err(|(e, stuck, typed)| {
                                // Not in enigo's held set: kept here, released first
                                // by the next input and at shutdown.
                                if stuck.is_some() {
                                    self.stuck_unit = stuck;
                                }
                                // Its last unit's key-down went in: that typed it.
                                tapped = typed;
                                e
                            })
                        }
                        #[cfg(not(target_os = "windows"))]
                        Stroke::Char(c) => self
                            .enigo
                            .text(c.encode_utf8(&mut buffer))
                            .map_err(failed("typing text")),
                    },
                )
            });
            if let Err(cause) = sent {
                let typed = typed + if tapped { chars } else { 0 };
                return Err(partial(cause, typed, total, "characters"));
            }
            typed += chars;
        }
        Ok(())
    }

    /// Presses and releases `key` apart: once the press is in, a failed
    /// release leaves it tracked as held (enigo keeps it in its held set), so
    /// the next input releases it first, and the error says it went in.
    ///
    /// A press that reports failure may still have gone out (an Enter can
    /// submit a form), so it is released regardless and reported as having
    /// possibly gone in.
    fn tap(&mut self, key: Key) -> (ToolResult<()>, bool) {
        super::press_and_release(|down| {
            self.enigo
                .key(
                    key,
                    if down {
                        Direction::Press
                    } else {
                        Direction::Release
                    },
                )
                .map_err(failed(if down {
                    "pressing the key"
                } else {
                    "releasing the key"
                }))
        })
    }

    /// Taps the unassigned key that turns modifiers released on their own
    /// into a chord that does nothing; whether it masked them. A press and a
    /// release, not one click: the press going in is what masks, and if only
    /// its release fails, enigo tracks the key as held and the next input
    /// releases it (a key that does nothing, so no side effect).
    #[cfg(target_os = "windows")]
    fn mask(&mut self) -> bool {
        if self
            .enigo
            .key(Key::Other(UNASSIGNED_VK), Direction::Press)
            .is_err()
        {
            return false;
        }
        let _ = self
            .enigo
            .key(Key::Other(UNASSIGNED_VK), Direction::Release);
        true
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
        for done in 0..repeat {
            let pressed = self.press_once(combo, guard);
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
        combo: &KeyCombo,
        guard: &mut Guard<'_>,
    ) -> Result<(), (ToolError, bool)> {
        // Resolved on every press, since the previous one can have moved
        // focus to a thread with another keyboard layout, and then checked:
        // the target must be in front, and be the window whose layout chose
        // the key (a window that was in front for a moment has another).
        guard(None).map_err(|e| (e, false))?;
        let (key, modifiers, layout_of) = resolve(combo).map_err(|e| (e, false))?;
        #[cfg(target_os = "windows")]
        if let Some(changed) = layout_of.and_then(owner_changed) {
            return Err((changed, false));
        }
        #[cfg(not(target_os = "windows"))]
        let _ = layout_of;
        // A modifier the user holds would join the chord (Shift turning
        // ctrl+z into Redo), and releasing ours would lift theirs.
        #[cfg(target_os = "windows")]
        only_modifiers(&[]).map_err(|e| (e, false))?;
        let modifiers = modifiers.as_slice();
        let mut held: Vec<Key> = Vec::with_capacity(modifiers.len());
        let mut result = Ok(());
        for &m in modifiers {
            let k = modifier_key(m);
            // Our preceding modifier may still be reaching async key state.
            // Wait before the target lookup, then only snapshot afterwards.
            #[cfg(target_os = "windows")]
            {
                result = only_modifiers(&held);
            }
            if result.is_ok() {
                result = super::guarded_input_event(
                    || only_our_keys_now(&held),
                    || guard(None),
                    || Ok(()),
                );
            }
            if result.is_err() {
                break;
            }
            // Counted as held before the press: one reported failed may still
            // have gone down, and the release below then lifts it too.
            held.push(k);
            result = self
                .enigo
                .key(k, Direction::Press)
                .map_err(failed("pressing a modifier"));
            if result.is_err() {
                break;
            }
        }
        // Exactly ours are down before the key. Polled, so it can take a
        // moment in which the foreground can change: the target and the
        // layout owner are checked after it, right before the key.
        #[cfg(target_os = "windows")]
        if result.is_ok() {
            result = only_modifiers(&held);
        }
        // Again after the modifiers: pressing one can move focus to a control
        // with another layout, where the key chosen would be another one.
        // Before the guard, which is the last check before the key.
        #[cfg(target_os = "windows")]
        if result.is_ok()
            && let Some(changed) = layout_of.and_then(owner_changed)
        {
            result = Err(changed);
        }
        if result.is_ok() {
            result = super::guarded_input_event(
                || only_our_keys_now(&held),
                || {
                    guard(None)?;
                    // A provider lookup can also outlive the keyboard layout
                    // used to resolve this character. Recheck after it.
                    #[cfg(target_os = "windows")]
                    if let Some(changed) = layout_of.and_then(owner_changed) {
                        return Err(changed);
                    }
                    Ok(())
                },
                || Ok(()),
            );
        }
        let mut sent = false;
        let attempted = result.is_ok();
        // Whether the key surely went down: only then is the chord a real
        // shortcut rather than modifiers on their own, which get masked.
        #[cfg(target_os = "windows")]
        let mut emitted = false;
        if result.is_ok() {
            let (pressed, confirmed) = self.tap(key);
            sent = confirmed;
            #[cfg(target_os = "windows")]
            {
                emitted = confirmed;
            }
            result = pressed;
        }
        // Modifiers released with nothing pressed while they were down are a
        // gesture of their own: a lone Alt opens the menu bar, the Windows
        // key the Start menu, Ctrl+Shift switches the input language, and
        // lone Shift taps add up to Sticky Keys. An unassigned key tapped in
        // between makes it an ordinary chord that does nothing; if even that
        // fails, the error says the gesture may have gone out.
        #[cfg(target_os = "windows")]
        if !emitted && !held.is_empty() && !self.mask() {
            let what = format!(
                "{held:?} may have been pressed without a confirmed main key and could not be masked, so releasing them may act as a shortcut of their own (menu bar, Start, language switch); look before retrying"
            );
            result = Err(match result {
                Ok(()) => ToolError::platform("masking released modifiers", &what),
                Err(cause) => cause,
            }
            .after(Effect::Incidental, what));
        }
        // Every release is tried; one that fails is reported even when an
        // earlier failure is the cause, since a modifier left down changes
        // the user's next keystroke.
        let went_down = held.clone();
        let stuck: Vec<Key> = held
            .into_iter()
            .rev()
            .filter(|&k| self.enigo.key(k, Direction::Release).is_err())
            .collect();
        if !stuck.is_empty() {
            let what = format!(
                "{stuck:?} could not be released and may still be held; the next input releases them first"
            );
            result = Err(match result {
                Ok(()) => ToolError::platform("releasing a modifier", &what),
                Err(cause) => cause,
            }
            .after(Effect::Incidental, what));
        }
        // Stopped with modifiers already down: they reached the target and
        // came back up (masked), which the caller is told rather than
        // "nothing was sent".
        if !attempted
            && !went_down.is_empty()
            && let Err(cause) = result
        {
            result = Err(cause.after(
                Effect::Incidental,
                format!(
                    "pressing {went_down:?} was attempted and their releases were attempted without sending the main key"
                ),
            ));
        }
        result.map_err(|e| (e, sent))
    }
}

/// A drag must still hold exactly its own physical mouse button. Both this button
/// snapshot and the keyboard snapshot run around every potentially slow
/// guard, including the final drop and recovery without pointer movement.
fn no_drag_interference(held: Option<Button>) -> ToolResult<()> {
    no_keyboard_input()?;
    #[cfg(target_os = "windows")]
    super::only_owned_button(
        crate::os::mouse_buttons_down(),
        held.map_or(0, physical_bit),
    )?;
    #[cfg(not(target_os = "windows"))]
    let _ = held;
    Ok(())
}

/// A non-blocking keyboard preflight for an in-progress drag. Waiting for
/// a key to clear could miss a modified gesture; stop as soon as one is seen.
#[cfg_attr(
    not(target_os = "windows"),
    expect(
        clippy::unnecessary_wraps,
        reason = "physical key state is available only on Windows; other input backends refuse pointer movement"
    )
)]
fn no_keyboard_input() -> ToolResult<()> {
    #[cfg(target_os = "windows")]
    if crate::os::modifiers_down() != 0 || crate::os::other_key_down().is_some() {
        return Err(ToolError::Busy(
            "a physical key is held during the drag; further movement is refused because it could modify the gesture".into(),
        ));
    }
    Ok(())
}

/// Snapshot immediately after a target lookup, without sleeping after the
/// lookup. Unlike `nothing_held`, this cannot stale the target while polling.
fn nothing_held_now() -> ToolResult<()> {
    only_our_keys_now(&[])
}

#[cfg_attr(
    not(target_os = "windows"),
    expect(
        clippy::unnecessary_wraps,
        reason = "native input state is Windows-only; other backends refuse targeted input"
    )
)]
fn only_our_keys_now(ours: &[Key]) -> ToolResult<()> {
    #[cfg(target_os = "windows")]
    {
        if crate::os::mouse_button_down() {
            return Err(ToolError::Busy(
                "a mouse button is held down and would join the input sent".into(),
            ));
        }
        only_modifiers_now(ours)?;
    }
    #[cfg(not(target_os = "windows"))]
    let _ = ours;
    Ok(())
}

/// Refuses right before a synthetic press or wheel turn while the person at
/// the desk holds a mouse button or a key: a held Ctrl turns a click into
/// Ctrl+click and a wheel turn into zoom, a held button completes or drops
/// its own gesture. The target and position are checked after it.
fn nothing_held() -> ToolResult<()> {
    no_button_down()?;
    #[cfg(target_os = "windows")]
    only_modifiers(&[])?;
    Ok(())
}

/// Refuses right before a synthetic press while a mouse button the user
/// holds is down: the press and its release would complete or drop their
/// gesture. Polled briefly, since this session's own last release (the first
/// click of a double click) reaches the OS's state a moment late. Callers
/// check the target and position after it, never before.
#[cfg_attr(
    not(target_os = "windows"),
    expect(
        clippy::unnecessary_wraps,
        reason = "the physical-button read is Windows only for now"
    )
)]
fn no_button_down() -> ToolResult<()> {
    #[cfg(target_os = "windows")]
    {
        for attempt in 0..10 {
            if !crate::os::mouse_button_down() {
                return Ok(());
            }
            if attempt < 9 {
                thread::sleep(Duration::from_millis(5));
            }
        }
        Err(ToolError::Busy(
            "a mouse button is held down, and a click now would complete or drop its gesture"
                .into(),
        ))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

/// Refuses unless the modifiers down on the keyboard are exactly `ours` and
/// no other key is down: one the user holds would join the keys sent (a held
/// Shift makes ctrl+z Redo; an auto-repeating `w` plus our Ctrl is Ctrl+W).
/// Our own presses and releases reach the OS's key state a moment after they
/// are sent, so it is polled briefly before concluding.
#[cfg(target_os = "windows")]
fn only_modifiers(ours: &[Key]) -> ToolResult<()> {
    let mut result = Ok(());
    for attempt in 0..10 {
        result = only_modifiers_now(ours);
        if result.is_ok() {
            return result;
        }
        if attempt < 9 {
            thread::sleep(Duration::from_millis(5));
        }
    }
    result
}

/// The non-blocking half of `only_modifiers`, also used after slow guards.
#[cfg(target_os = "windows")]
fn only_modifiers_now(ours: &[Key]) -> ToolResult<()> {
    let expected = ours.iter().fold(0_u8, |bits, key| {
        bits | match key {
            Key::Shift => 1,
            Key::Control => 2,
            Key::Alt => 4,
            Key::Meta => 8,
            _ => 0,
        }
    });
    let other = crate::os::other_key_down();
    if crate::os::modifiers_down() == expected && other.is_none() {
        return Ok(());
    }
    Err(ToolError::Busy(match other {
        Some(vk) => format!(
            "a key (virtual key 0x{vk:02X}) is held down on the keyboard and would join the keys sent"
        ),
        None => {
            "the modifier keys down differ from this chord's own keys; further input is refused"
                .into()
        }
    }))
}

/// Why a key chosen for `owner` must not go out now, if it must not: other
/// windows would get it, or the layout or Caps Lock changed under it (the
/// same key would type something else; passing, so worth a retry).
#[cfg(target_os = "windows")]
fn owner_changed(owner: LayoutOwner) -> Option<ToolError> {
    // Passing either way: the key is chosen again on a retry, and a target
    // that lost the foreground or its focus is refused by the guard, with
    // the code that says so.
    let now = crate::os::keyboard_owner();
    if now == owner {
        None
    } else if now.same_windows(&owner) {
        Some(ToolError::Busy(
            "the keyboard layout or Caps Lock changed while the key was chosen".into(),
        ))
    } else {
        Some(ToolError::Busy(
            "the foreground window or its focused control changed while the key was chosen".into(),
        ))
    }
}

/// The window, focus, layout and Caps Lock a character's key was chosen
/// for (Windows); nothing elsewhere.
#[cfg(target_os = "windows")]
type LayoutOwner = crate::os::KeyboardOwner;
#[cfg(not(target_os = "windows"))]
type LayoutOwner = ();

/// The OS key for a combo's key and the modifiers to hold for it. On
/// Windows a character goes out as its layout's virtual key with the shift
/// state that layout needs: enigo would send the character's shifted
/// virtual-key code as is, which is no key at all.
/// The key and modifiers for `combo`, and for a character the
/// [`LayoutOwner`] whose layout chose its key.
fn resolve(combo: &KeyCombo) -> ToolResult<(Key, Vec<Modifier>, Option<LayoutOwner>)> {
    #[cfg(target_os = "windows")]
    if let KeyName::Char(c) = combo.key {
        // With a command modifier the key is a shortcut: the layout's key as
        // is, Caps Lock left out (ctrl+c must not become ctrl+shift+c).
        let command = combo
            .modifiers
            .iter()
            .any(|m| !matches!(m, Modifier::Shift));
        let (vk, shift, window) = crate::os::char_key(c, command).ok_or_else(|| {
            ToolError::InvalidArgument(format!(
                "no key types `{c}` on the current keyboard layout; send it with type_text"
            ))
        })?;
        let mut modifiers = combo.modifiers.clone();
        modifiers.extend(super::implied_modifiers(shift, &combo.modifiers));
        return Ok((Key::Other(u32::from(vk)), modifiers, Some(window)));
    }
    Ok((enigo_key(combo.key)?, combo.modifiers.clone(), None))
}

/// The bit [`crate::os::mouse_buttons_down`] reports for an injected
/// button: both are physical.
#[cfg(target_os = "windows")]
fn physical_bit(button: Button) -> u8 {
    match button {
        Button::Left => 1,
        Button::Right => 2,
        Button::Middle => 4,
        Button::Back => 8,
        Button::Forward => 16,
        _ => 0,
    }
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

#[cfg(all(test, target_os = "windows"))]
mod live_state_tests {
    use super::*;
    use std::cell::Cell;

    /// No pointer move or click is sent. A temporary Ctrl goes through the
    /// real OS state reader inside the guard, then is released explicitly
    /// (and by Enigo's drop cleanup if an assertion unwinds).
    #[test]
    #[ignore = "temporarily presses Ctrl; requires an idle interactive Windows desktop"]
    fn physical_state_change_inside_guard_blocks_event() {
        let mut input = Input::new().expect("BUG: native input device opens");
        nothing_held().expect("BUG: the live test needs an idle keyboard and mouse");
        let event_sent = Cell::new(false);
        let result = super::super::guarded_input_event(
            nothing_held_now,
            || {
                input
                    .enigo
                    .key(Key::Control, Direction::Press)
                    .map_err(failed("test Ctrl press"))?;
                only_modifiers(&[Key::Control])
            },
            || {
                event_sent.set(true);
                Ok(())
            },
        );
        input
            .enigo
            .key(Key::Control, Direction::Release)
            .expect("BUG: temporary Ctrl releases");
        only_modifiers(&[]).expect("BUG: temporary Ctrl is no longer held");
        assert!(matches!(result, Err(ToolError::Busy(_))));
        assert!(
            !event_sent.get(),
            "the post-guard snapshot must block the event"
        );
    }
}
