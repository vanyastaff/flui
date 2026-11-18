//! What a Win32 window holds to while the user acts on it: the pointer it
//! holds (`CursorGrab`, through `cursor`) and the steps and proportion the
//! user's resize keeps (`WM_SIZING`, through `sizing`), as methods of the
//! window's context the window procedure calls.

use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
};

use super::proc::WindowContext;
use super::set::checked_logical_to_device;
use crate::owner::windows::WindowStep;

impl WindowContext {
    /// The grab the window holds.
    pub(crate) fn cursor_grab(&self) -> flui_platform::CursorGrab {
        match self.pointer_hold.get() {
            None => flui_platform::CursorGrab::None,
            Some(super::cursor::Held::Confined) => flui_platform::CursorGrab::Confined,
            Some(super::cursor::Held::Locked(_)) => flui_platform::CursorGrab::Locked,
        }
    }

    /// Hold the pointer as `grab` asks: only while the window has the
    /// keyboard focus, which a grab ends with, as every window system ends
    /// one. A grab the window cannot take is not held. The window holds what
    /// it holds when this returns, a refusal included, and its caller
    /// publishes that. Owner thread only.
    ///
    /// # Errors
    /// [`PlatformFailure::Failed`](flui_platform::PlatformFailure::Failed)
    /// when no display shows the client area, or Win32 refuses the clip or
    /// the raw input; the window then holds nothing.
    pub(crate) fn hold_pointer(
        &self,
        hwnd: HWND,
        grab: flui_platform::CursorGrab,
    ) -> Result<(), flui_platform::PlatformFailure> {
        use flui_platform::CursorGrab;
        use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;

        self.let_pointer_go(hwnd);
        // SAFETY: a plain query of this thread's focus.
        let focused = unsafe { GetFocus() } == hwnd;
        if grab == CursorGrab::None || !focused {
            return Ok(());
        }
        let locked = grab == CursorGrab::Locked;
        self.pointer_hold
            .set(Some(super::cursor::hold(hwnd, locked)?));
        if locked {
            self.step(hwnd, WindowStep::PointerLocked(true));
            // A locked pointer is not shown; the next `WM_SETCURSOR` keeps it so.
            // SAFETY: `SetCursor(None)` takes no pointer.
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SetCursor(None);
            }
        }
        Ok(())
    }

    /// Let go of whatever grab the window holds: the window lost the focus,
    /// is going, or holds another one now. The clip is the desktop's, so it
    /// must not outlive the window's focus.
    pub(crate) fn let_pointer_go(&self, hwnd: HWND) {
        let Some(held) = self.pointer_hold.take() else {
            return;
        };
        let locked = matches!(held, super::cursor::Held::Locked(_));
        super::cursor::let_go(locked);
        if locked {
            self.step(hwnd, WindowStep::PointerLocked(false));
        }
    }

    /// The client area moved, resized or changed scale, or the displays
    /// changed: clip the pointer the window holds where the client area is
    /// now, a lock at its pixel of the client area a display shows. A clip no
    /// display can show, or one Win32 refuses, lets the grab go, which the
    /// next snapshot says.
    pub(crate) fn hold_again(&self, hwnd: HWND) {
        if let Some(held) = self.pointer_hold.get()
            && super::cursor::clip(hwnd, held).is_err()
        {
            self.let_pointer_go(hwnd);
        }
    }

    /// `WM_SIZING`: hold the rectangle the user drags `edge` to
    /// (`proposed`, which Win32 reads back) to the steps and the proportion
    /// the application asked for, at the window's scale. Whether it held it;
    /// with nothing asked, the rectangle is left to the default procedure.
    pub(crate) fn hold_sizing(&self, hwnd: HWND, edge: usize, proposed: &mut RECT) -> bool {
        use super::limits::PixelSize;
        use super::sizing::{ResizeRule, SizingEdge, SizingRect};

        let (increments, aspect) = (self.resize_increments.get(), self.aspect_ratio.get());
        let Some(edge) = SizingEdge::from_wmsz(edge) else {
            return false;
        };
        if increments.is_none() && aspect.is_none() {
            return false;
        }
        let scale = self.scale.scale();
        let device = |logical: f64| {
            checked_logical_to_device(logical, scale)
                .unwrap_or(0)
                .max(0)
        };
        let rule = ResizeRule {
            increments: increments.map(|step| {
                PixelSize::new(device(step.width()).max(1), device(step.height()).max(1))
            }),
            base: self
                .constraints
                .get()
                .min
                .map_or(PixelSize::new(0, 0), |min| {
                    PixelSize::new(device(min.width()), device(min.height()))
                }),
            aspect,
        };
        let mut window = RECT::default();
        let mut client = RECT::default();
        // SAFETY: plain queries of the live window into live locals.
        let measured = unsafe {
            GetWindowRect(hwnd, &raw mut window).is_ok()
                && GetClientRect(hwnd, &raw mut client).is_ok()
        };
        if !measured {
            return false;
        }
        let frame = PixelSize::new(
            ((window.right - window.left) - client.right).max(0),
            ((window.bottom - window.top) - client.bottom).max(0),
        );
        let held = rule.hold(
            edge,
            SizingRect {
                left: proposed.left,
                top: proposed.top,
                right: proposed.right,
                bottom: proposed.bottom,
            },
            frame,
        );
        *proposed = RECT {
            left: held.left,
            top: held.top,
            right: held.right,
            bottom: held.bottom,
        };
        true
    }
}
