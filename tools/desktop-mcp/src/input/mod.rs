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
