//! Real pointer and keyboard input.
//!
//! On Windows and macOS the device is enigo's. Linux is not built in yet:
//! enigo and xcap link libxkbcommon and PipeWire there, which the workspace's
//! Linux builds do not install, so on Linux [`Input`] is uninhabited and
//! [`Input::new`] reports why.

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod device;

#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use device::Input;

/// Runs before each emitted input event and refuses it with an error — how
/// the server keeps a multi-event action (a repeated key, a drag, typed text)
/// from reaching a window that took the foreground partway through. It is
/// given the screen point the event lands on, when it has one (each step of a
/// drag), so that point is checked too.
pub type Guard<'a> = dyn FnMut(Option<(i32, i32)>) -> crate::error::ToolResult<()> + 'a;

/// Which mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// Primary.
    Left,
    /// Secondary.
    Right,
    /// Wheel button.
    Middle,
}

/// One keystroke of typed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos", test)),
    expect(dead_code, reason = "typed by the enigo device, not built on this OS")
)]
pub enum Stroke {
    /// A key pressed for a control character.
    Key(crate::keys::KeyName),
    /// A character entered as itself, layout-independent.
    Char(char),
}

/// `text` as keystrokes, each with how many characters of `text` it types.
/// Line breaks and tabs are keys, as a person types them: entered as
/// characters, a newline reaches a text field as a raw line feed (or, through
/// enigo's own mapping, as both Enter and a line feed), not as a new line.
/// `\r\n` is one Enter.
#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos", test)),
    expect(dead_code, reason = "typed by the enigo device, not built on this OS")
)]
pub fn strokes(text: &str) -> Vec<(Stroke, usize)> {
    use crate::keys::KeyName;
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                (Stroke::Key(KeyName::Enter), 2)
            }
            '\r' | '\n' => (Stroke::Key(KeyName::Enter), 1),
            '\t' => (Stroke::Key(KeyName::Tab), 1),
            c => (Stroke::Char(c), 1),
        });
    }
    out
}

/// The pause between the parts of a synthesized gesture, so the target's
/// event loop sees distinct events rather than one coalesced burst.
pub const STEP: std::time::Duration = std::time::Duration::from_millis(15);

/// The points a drag from `from` to `to` over `duration` passes through,
/// `from` excluded and `to` included: at most 200 steps, at least 2, one
/// per [`STEP`] of the duration. What the device moves through, and what
/// the safety check verifies before the button goes down, so the two agree.
pub fn drag_path(
    from: (i32, i32),
    to: (i32, i32),
    duration: std::time::Duration,
) -> Vec<(i32, i32)> {
    let steps = i32::try_from((duration.as_millis() / STEP.as_millis()).clamp(2, 200))
        .expect("BUG: at most 200 steps");
    (1..=steps)
        .map(|i| (lerp(from.0, to.0, i, steps), lerp(from.1, to.1, i, steps)))
        .collect()
}

/// The point `i/steps` of the way from `a` to `b`, in `i64` so a wide drag
/// cannot overflow after the button is already down. The result lies between
/// `a` and `b`, so it fits back in `i32`.
fn lerp(a: i32, b: i32, i: i32, steps: i32) -> i32 {
    let at = i64::from(a) + (i64::from(b) - i64::from(a)) * i64::from(i) / i64::from(steps);
    i32::try_from(at).expect("BUG: an interpolated point lies between two i32 endpoints")
}

/// The modifiers a combo's character needs beyond `given`, from the shift
/// state a keyboard layout reports for it (bit 1 Shift, 2 Ctrl, 4 Alt —
/// Ctrl+Alt is AltGr): `+` is Shift+`=` on a US layout, so `ctrl+plus` must
/// hold Shift too.
#[cfg_attr(
    not(any(target_os = "windows", test)),
    expect(
        dead_code,
        reason = "only the Windows layout lookup reports shift states"
    )
)]
pub fn implied_modifiers(
    shift_state: u8,
    given: &[crate::keys::Modifier],
) -> Vec<crate::keys::Modifier> {
    use crate::keys::Modifier;
    [
        (1, Modifier::Shift),
        (2, Modifier::Ctrl),
        (4, Modifier::Alt),
    ]
    .into_iter()
    .filter(|&(bit, m)| shift_state & bit != 0 && !given.contains(&m))
    .map(|(_, m)| m)
    .collect()
}

/// `cause`, or when `sent` of `total` `unit` already went out, `cause`
/// with that count, so the caller does not repeat them.
#[cfg_attr(
    not(any(target_os = "windows", target_os = "macos", test)),
    expect(dead_code, reason = "raised by the enigo device, not built on this OS")
)]
pub fn partial(
    cause: crate::error::ToolError,
    sent: usize,
    total: usize,
    unit: &'static str,
) -> crate::error::ToolError {
    if sent == 0 {
        return cause;
    }
    cause.counted(sent, total, unit)
}

/// Records a preparatory move even when verification or the first semantic
/// event fails. Existing press uncertainty and progress remain the primary
/// effect; the pointer event is additional context, not a replacement count.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn after_pointer_move<T>(
    point: (i32, i32),
    action: impl FnOnce() -> crate::error::ToolResult<T>,
) -> crate::error::ToolResult<T> {
    action().map_err(|cause| {
        cause.after(
            crate::error::Effect::Incidental,
            format!(
                "a preparatory pointer move toward ({}, {}) reached the OS and may have triggered hover or focus changes; the pointer may have been clamped or moved again; inspect before retrying",
                point.0, point.1
            ),
        )
    })
}

/// Always releases a click's button, but advances click progress only after
/// both halves succeed. A failed release can leave the button down or can
/// have delivered the click before reporting failure; neither is completion.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn complete_click(
    pressed: crate::error::ToolResult<()>,
    release: impl FnOnce() -> crate::error::ToolResult<()>,
    completed: usize,
    total: usize,
) -> crate::error::ToolResult<()> {
    use crate::error::Effect;
    let released = release();
    let (cause, detail) = match (pressed, released) {
        (Ok(()), Ok(())) => return Ok(()),
        (Ok(()), Err(cause)) => (
            cause,
            format!(
                "click {} of {total} was pressed but its release was not confirmed; it may have completed or the button may still be held, and the next input releases it first; look before retrying",
                completed + 1,
            ),
        ),
        (Err(cause), Ok(())) => (
            cause,
            format!(
                "click {} of {total} may have gone through (its press reported failure, then it was released); look before retrying",
                completed + 1,
            ),
        ),
        (Err(cause), Err(release)) => (
            cause,
            format!(
                "click {} of {total} may have been pressed and releasing it failed ({release}); the button may still be held; look before retrying",
                completed + 1,
            ),
        ),
    };
    Err(partial(
        cause.after(Effect::MayHaveRun, detail),
        completed,
        total,
        "clicks",
    ))
}

/// Only the session's own physical button is exempt during a drag. A
/// second button belongs to another gesture, including at a stationary drop.
#[cfg(any(target_os = "windows", test))]
fn only_owned_button(down: u8, owned: u8) -> crate::error::ToolResult<()> {
    if down == owned {
        Ok(())
    } else if down & !owned != 0 {
        Err(crate::error::ToolError::Busy(
            "another physical mouse button is held; further movement or the requested drop would join its gesture".into(),
        ))
    } else {
        Err(crate::error::ToolError::Busy(
            "the drag's mouse button is no longer down; it may already have been released outside this call".into(),
        ))
    }
}

/// Only the first observation after our press may lag behind SendInput.
/// Settle before target guards; later snapshots must detect a lost hold
/// immediately instead of waiting for another physical press to replace it.
#[cfg(any(target_os = "windows", test))]
fn settle_owned_button(
    owned: u8,
    mut read: impl FnMut() -> crate::error::ToolResult<u8>,
    mut pause: impl FnMut(),
) -> crate::error::ToolResult<()> {
    for attempt in 0..10 {
        let down = read()?;
        if down == owned || down & !owned != 0 || attempt == 9 {
            return only_owned_button(down, owned);
        }
        pause();
    }
    unreachable!("BUG: the last settling attempt returns")
}

/// Checks the keyboard on both sides of a potentially slow target lookup.
/// Shared by pointer and keyboard input as well as drag recovery. A physical
/// key or button pressed during a slow guard must prevent the next event.
/// `state` is a non-blocking snapshot; any settling wait belongs before this.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn guarded_input_event(
    mut state: impl FnMut() -> crate::error::ToolResult<()>,
    guard: impl FnOnce() -> crate::error::ToolResult<()>,
    event: impl FnOnce() -> crate::error::ToolResult<()>,
) -> crate::error::ToolResult<()> {
    state()?;
    guard()?;
    state()?;
    event()
}

/// Sends one key down and always attempts its release, even when the down
/// failed ambiguously. The boolean counts only a confirmed down; the error
/// retains uncertainty separately from progress through a larger request.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn press_and_release(
    mut send: impl FnMut(bool) -> crate::error::ToolResult<()>,
) -> (crate::error::ToolResult<()>, bool) {
    use crate::error::Effect;
    let pressed = send(true);
    let confirmed = pressed.is_ok();
    let released = send(false);
    let result = match (pressed, released) {
        (Ok(()), Ok(())) => Ok(()),
        (Ok(()), Err(cause)) => Err(cause.after(
            Effect::Ran,
            "the key went out but its release failed; it may still be held until the next input releases it",
        )),
        (Err(cause), released) => Err(cause.after(
            Effect::MayHaveRun,
            if released.is_ok() {
                "the key may have gone out before its press reported failure, and it was released; look before retrying"
            } else {
                "the key may have gone out and could not be released; it may still be held"
            },
        )),
    };
    (result, confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{KeyName, Modifier};

    #[test]
    fn a_preparatory_move_is_reported_when_verification_or_the_action_refuses() {
        use crate::error::{Effect, ToolError};
        for cause in [
            ToolError::Busy("the first press guard refused".into()),
            ToolError::OutsideTarget {
                x: 20,
                y: 30,
                reason: "the pointer was clamped".into(),
                covered_by: None,
            },
        ] {
            let code = cause.code();
            let error = after_pointer_move((20, 30), || Err::<(), _>(cause))
                .expect_err("BUG: the action refused after movement");
            assert_eq!(error.code(), code);
            let ToolError::Interrupted { effect, detail, .. } = error else {
                panic!("the pointer move must be reported");
            };
            assert_eq!(effect, Effect::Incidental);
            assert!(detail.contains("(20, 30)"));
            assert!(detail.contains("hover or focus"));
        }
    }

    #[test]
    fn a_preparatory_move_keeps_press_uncertainty_and_completed_progress() {
        use crate::error::{Effect, ToolError};
        for expected in [
            Effect::MayHaveRun,
            Effect::Ran,
            Effect::Partial {
                sent: 1,
                total: 2,
                unit: "clicks",
            },
        ] {
            let error = after_pointer_move((20, 30), || {
                Err::<(), _>(
                    ToolError::Busy("release failed".into())
                        .after(expected.clone(), "original action detail"),
                )
            })
            .expect_err("BUG: action failure persists");
            let ToolError::Interrupted { effect, detail, .. } = error else {
                panic!("the original effect must be kept");
            };
            assert_eq!(effect, expected);
            assert!(detail.contains("original action detail"));
            assert!(detail.contains("preparatory pointer move"));
        }
        assert!(after_pointer_move((20, 30), || Ok(())).is_ok());
    }

    #[test]
    fn drag_refuses_movement_when_a_key_is_pressed_during_the_target_guard() {
        use std::cell::Cell;
        let key_down = Cell::new(false);
        let moved = Cell::new(false);
        let result = guarded_input_event(
            || {
                if key_down.get() {
                    Err(crate::error::ToolError::Busy("Ctrl held".into()))
                } else {
                    Ok(())
                }
            },
            || {
                key_down.set(true);
                Ok(())
            },
            || {
                moved.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!moved.get(), "no modified movement may follow the guard");
        assert!(key_down.get(), "the user's key must not be released");
        let recovery = guarded_input_event(
            || {
                if key_down.get() {
                    Err(crate::error::ToolError::Busy("Ctrl still held".into()))
                } else {
                    Ok(())
                }
            },
            || Ok(()),
            || {
                moved.set(true);
                Ok(())
            },
        );
        assert!(recovery.is_err());
        assert!(
            !moved.get(),
            "recovery must not move under the physical modifier either"
        );
    }

    #[test]
    fn drag_checks_keyboard_again_after_a_successful_target_lookup() {
        use std::cell::RefCell;
        let events = RefCell::new(Vec::new());
        guarded_input_event(
            || {
                events.borrow_mut().push("keyboard");
                Ok(())
            },
            || {
                events.borrow_mut().push("target");
                Ok(())
            },
            || {
                events.borrow_mut().push("move");
                Ok(())
            },
        )
        .expect("BUG: a clear keyboard and valid target allow the drag step");
        assert_eq!(*events.borrow(), ["keyboard", "target", "keyboard", "move"]);
    }

    #[test]
    fn a_physical_button_pressed_during_a_guard_blocks_the_pending_event() {
        use std::cell::Cell;
        let button_down = Cell::new(false);
        let emitted = Cell::new(false);
        let result = guarded_input_event(
            || {
                if button_down.get() {
                    Err(crate::error::ToolError::Busy("button held".into()))
                } else {
                    Ok(())
                }
            },
            || {
                button_down.set(true);
                Ok(())
            },
            || {
                emitted.set(true);
                Ok(())
            },
        );
        assert!(matches!(result, Err(crate::error::ToolError::Busy(_))));
        assert!(
            !emitted.get(),
            "a press or wheel event must not join the user's gesture"
        );
        assert!(
            button_down.get(),
            "the physical button is not ours to release"
        );
    }

    #[test]
    fn a_second_mouse_button_during_the_drop_guard_refuses_normal_completion() {
        use std::cell::Cell;
        // Both primary-button configurations, and every additional button.
        for owned in [1_u8, 2] {
            for extra in [1_u8, 2, 4, 8, 16]
                .into_iter()
                .filter(|extra| *extra != owned)
            {
                let down = Cell::new(owned);
                let dropped = Cell::new(false);
                let result = guarded_input_event(
                    || only_owned_button(down.get(), owned),
                    || {
                        down.set(owned | extra);
                        Ok(())
                    },
                    || {
                        dropped.set(true);
                        Ok(())
                    },
                );
                assert!(matches!(result, Err(crate::error::ToolError::Busy(_))));
                assert!(
                    !dropped.get(),
                    "normal drop must be refused even without another move"
                );
                assert_eq!(
                    down.get(),
                    owned | extra,
                    "the other button is not ours to release"
                );
                let recovered = guarded_input_event(
                    || only_owned_button(down.get(), owned),
                    || Ok(()),
                    || {
                        dropped.set(true);
                        Ok(())
                    },
                );
                assert!(recovered.is_err());
                assert!(
                    !dropped.get(),
                    "stationary recovery cannot bypass the button check"
                );
            }
        }
    }

    #[test]
    fn an_initial_drag_press_can_settle_but_a_lost_hold_cannot_resume() {
        use std::cell::Cell;
        for owned in [1_u8, 2] {
            let mut observations = [0, 0, owned].into_iter();
            let pauses = Cell::new(0);
            settle_owned_button(
                owned,
                || Ok(observations.next().expect("BUG: bounded sequence")),
                || pauses.set(pauses.get() + 1),
            )
            .expect("BUG: the synthetic press became visible");
            assert_eq!(pauses.get(), 2);
            let down = Cell::new(owned);
            let sent = Cell::new(false);
            let result = guarded_input_event(
                || only_owned_button(down.get(), owned),
                || {
                    down.set(0);
                    Ok(())
                },
                || {
                    sent.set(true);
                    Ok(())
                },
            );
            assert!(result.is_err());
            assert!(
                !sent.get(),
                "a release during a guard must stop movement or drop"
            );
            assert!(
                only_owned_button(0, owned).is_err(),
                "recovery cannot turn a lost drag into hover"
            );
        }
    }

    #[test]
    fn initial_drag_settling_is_bounded_and_rejects_additional_buttons() {
        use std::cell::Cell;
        for down in [0_u8, 3] {
            let pauses = Cell::new(0);
            let result = settle_owned_button(1, || Ok(down), || pauses.set(pauses.get() + 1));
            assert!(result.is_err());
            assert_eq!(pauses.get(), if down == 0 { 9 } else { 0 });
        }
        assert!(
            only_owned_button(0, 0).is_ok(),
            "targetless moves require no button"
        );
    }

    #[test]
    fn an_unconfirmed_click_release_counts_only_prior_completed_clicks() {
        use crate::error::{Effect, ToolError};
        for (failed_at, total) in [(0, 1), (0, 2), (1, 2)] {
            for completed in 0..total {
                let result = complete_click(
                    Ok(()),
                    || {
                        if completed == failed_at {
                            Err(ToolError::Busy("release failed".into()))
                        } else {
                            Ok(())
                        }
                    },
                    completed,
                    total,
                );
                if completed < failed_at {
                    result.expect("BUG: earlier click completed");
                } else {
                    let error = result.expect_err("BUG: release failure must interrupt");
                    match error {
                        ToolError::Interrupted { effect, detail, .. } => {
                            if failed_at == 0 {
                                assert_eq!(effect, Effect::MayHaveRun);
                            } else {
                                assert_eq!(
                                    effect,
                                    Effect::Partial {
                                        sent: 1,
                                        total: 2,
                                        unit: "clicks"
                                    }
                                );
                                assert!(detail.contains("may_have_run"), "{detail}");
                            }
                            assert!(detail.contains("release was not confirmed"), "{detail}");
                            assert!(detail.contains("button may still be held"), "{detail}");
                        }
                        error => panic!("BUG: missing uncertain effect: {error}"),
                    }
                    break;
                }
            }
        }
    }

    #[test]
    fn an_uncertain_click_press_always_attempts_release_without_claiming_completion() {
        use crate::error::{Effect, ToolError};
        use std::cell::Cell;
        for release_fails in [false, true] {
            let released = Cell::new(false);
            let result = complete_click(
                Err(ToolError::Busy("press failed".into())),
                || {
                    released.set(true);
                    if release_fails {
                        Err(ToolError::Busy("release failed".into()))
                    } else {
                        Ok(())
                    }
                },
                0,
                1,
            );
            assert!(released.get());
            assert!(matches!(
                result,
                Err(ToolError::Interrupted {
                    effect: Effect::MayHaveRun,
                    ..
                })
            ));
        }
    }

    #[test]
    fn failed_presses_are_released_but_not_counted_as_confirmed() {
        use crate::error::{Effect, ToolError};
        for release_fails in [false, true] {
            let mut events = Vec::new();
            let (result, confirmed) = press_and_release(|down| {
                events.push(down);
                if down || release_fails {
                    Err(ToolError::Busy("injected failure".into()))
                } else {
                    Ok(())
                }
            });
            assert_eq!(events, [true, false]);
            assert!(!confirmed);
            let error = result.expect_err("press failed");
            assert!(matches!(
                &error,
                ToolError::Interrupted {
                    effect: Effect::MayHaveRun,
                    ..
                }
            ));
            let counted = partial(error, 2 + usize::from(confirmed), 4, "presses");
            assert!(matches!(counted, ToolError::Interrupted {
                effect: Effect::Partial { sent: 2, total: 4, .. }, detail, ..
            } if detail.contains("may_have_run")));
        }
    }

    #[test]
    fn failed_release_counts_the_confirmed_control_stroke() {
        use crate::error::{Effect, ToolError};
        let (result, confirmed) = press_and_release(|down| {
            if down {
                Ok(())
            } else {
                Err(ToolError::Busy("release failed".into()))
            }
        });
        assert!(confirmed);
        let error = result.expect_err("release failed");
        assert!(matches!(
            &error,
            ToolError::Interrupted {
                effect: Effect::Ran,
                ..
            }
        ));
        // CRLF is a single Enter but accounts for two input characters.
        let (_, chars) = strokes("\r\n")[0];
        let counted = partial(error, usize::from(confirmed) * chars, 3, "characters");
        assert!(matches!(
            counted,
            ToolError::Interrupted {
                effect: Effect::Partial {
                    sent: 2,
                    total: 3,
                    ..
                },
                ..
            }
        ));
    }

    /// Control characters become the keys a person presses, once each:
    /// sent as characters they arrive doubled or as raw control codes.
    #[test]
    fn line_breaks_and_tabs_are_keys() {
        let enter = Stroke::Key(KeyName::Enter);
        let tab = Stroke::Key(KeyName::Tab);
        assert_eq!(
            strokes("a\nb\tc"),
            [
                (Stroke::Char('a'), 1),
                (enter, 1),
                (Stroke::Char('b'), 1),
                (tab, 1),
                (Stroke::Char('c'), 1)
            ]
        );
        assert_eq!(
            strokes("x\r\ny"),
            [(Stroke::Char('x'), 1), (enter, 2), (Stroke::Char('y'), 1)]
        );
        assert_eq!(strokes("\r"), [(enter, 1)]);
        let typed: usize = strokes("日本\r\n語\t").iter().map(|&(_, n)| n).sum();
        assert_eq!(
            typed,
            "日本\r\n語\t".chars().count(),
            "every character is accounted for"
        );
    }

    #[test]
    fn a_layouts_shift_state_adds_only_missing_modifiers() {
        assert_eq!(implied_modifiers(1, &[Modifier::Ctrl]), [Modifier::Shift]);
        assert_eq!(implied_modifiers(1, &[Modifier::Shift]), []);
        assert_eq!(implied_modifiers(6, &[]), [Modifier::Ctrl, Modifier::Alt]);
        assert_eq!(implied_modifiers(0, &[Modifier::Alt]), []);
    }

    /// A count is kept when the cause already carries an effect of its
    /// own (a key whose release failed partway through typed text).
    #[test]
    fn the_count_outranks_an_inner_effect() {
        use crate::error::{Effect, ToolError};
        let inner = ToolError::Busy("x".into()).after(Effect::Ran, "the Enter release failed");
        let err = partial(inner, 2, 5, "characters");
        assert!(
            matches!(
                &err,
                ToolError::Interrupted {
                    effect: Effect::Partial { sent: 2, total: 5, .. },
                    detail,
                    ..
                } if detail.contains("(ran)") && detail.contains("Enter release")
            ),
            "{err:?}"
        );
    }

    #[test]
    fn a_partial_send_says_how_much_went_out() {
        let cause = || crate::error::ToolError::NotFound("gone".into());
        assert!(matches!(
            partial(cause(), 0, 5, "characters"),
            crate::error::ToolError::NotFound(_)
        ));
        let err = partial(cause(), 3, 5, "characters").to_string();
        assert!(
            err.contains("gone") && err.contains("3 of 5 characters"),
            "{err}"
        );
    }
}

/// No input device on this OS: the type has no values.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[derive(Debug)]
pub enum Input {}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Input {
    /// Why there is no input device.
    pub fn new() -> Result<Self, String> {
        Err(format!(
            "input is not supported on {} yet (Windows and macOS only)",
            std::env::consts::OS
        ))
    }

    /// Unreachable: no `Input` exists.
    pub fn move_to(&mut self, _: i32, _: i32) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn ready(&mut self) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn release_all(&mut self) {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn position(&self) -> Option<(i32, i32)> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn click(
        &mut self,
        _: i32,
        _: i32,
        _: MouseButton,
        _: bool,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn drag(
        &mut self,
        _: (i32, i32),
        _: (i32, i32),
        _: std::time::Duration,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn scroll(
        &mut self,
        _: i32,
        _: i32,
        _: i32,
        _: i32,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn type_text(&mut self, _: &str, _: &mut Guard<'_>) -> crate::error::ToolResult<()> {
        match *self {}
    }

    /// Unreachable: no `Input` exists.
    pub fn key(
        &mut self,
        _: &crate::keys::KeyCombo,
        _: u32,
        _: &mut Guard<'_>,
    ) -> crate::error::ToolResult<()> {
        match *self {}
    }
}
