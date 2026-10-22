//! Wake deadlines while the system runs a modal loop for a window: moving or
//! sizing it, and its menus (the system menu included).
//!
//! `DefWindowProc`'s loops wait in `GetMessage`, so the platform's own loop,
//! which delivers deadlines, does not run: with the pointer still, caret
//! blink, timers and the reveal fallback stop. Those loops do dispatch
//! `WM_TIMER`, so a timer of the window delivers what is due the way the
//! drag-and-drop loop does (`WindowsLoop::deliver_modal_frames`), and is
//! armed again for when the next one is.

use std::cell::Cell;
use std::rc::Rc;

use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{KillTimer, SetTimer},
};

use super::{FrameCount, ModalDelivery, WindowsLoop};
use crate::platforms::windows::platform::WindowContext;

/// The timer that paces a window's modal loops. Distinct from the touchpad's.
pub(crate) const MODAL_DEADLINE_TIMER: usize = 0x4D4C;

/// A window's modal-loop pacing: the frame count deliveries are keyed by and
/// the last deadline delivered.
pub(crate) struct ModalPacing {
    frames: Rc<FrameCount>,
    delivered: Rc<ModalDelivery>,
    /// How many modal loops are running for the window.
    depth: Cell<u32>,
}

impl ModalPacing {
    pub(crate) fn new(frames: Rc<FrameCount>) -> Self {
        Self {
            frames,
            delivered: Rc::default(),
            depth: Cell::new(0),
        }
    }
}

impl WindowContext {
    /// A modal loop for the window starts.
    pub(crate) fn enter_modal_loop(&self, hwnd: HWND) {
        let pacing = &self.modal_pacing;
        if pacing.depth.replace(pacing.depth.get().saturating_add(1)) == 0 {
            pacing.delivered.clear();
            arm(hwnd, super::modal_pace::MODAL_FRAME_PACE_MS);
        }
    }

    /// A modal loop for the window ends.
    pub(crate) fn exit_modal_loop(&self, hwnd: HWND) {
        let pacing = &self.modal_pacing;
        let depth = pacing.depth.get().saturating_sub(1);
        pacing.depth.set(depth);
        if depth == 0 {
            // SAFETY: a plain call on the window's handle; a timer that is
            // not set fails harmlessly.
            let _ = unsafe { KillTimer(Some(hwnd), MODAL_DEADLINE_TIMER) };
        }
    }

    /// The pacing timer fired: deliver what is due and arm it again.
    pub(crate) fn pace_modal_loop(&self, hwnd: HWND) {
        if self.modal_pacing.depth.get() == 0 {
            // SAFETY: as in `exit_modal_loop`.
            let _ = unsafe { KillTimer(Some(hwnd), MODAL_DEADLINE_TIMER) };
            return;
        }
        // Cloned out: the delivery runs user code, which may close the window.
        let handlers = Rc::clone(&self.handlers);
        let windows = self.windows.clone();
        let frames = Rc::clone(&self.modal_pacing.frames);
        let delivered = Rc::clone(&self.modal_pacing.delivered);
        let wait = WindowsLoop::deliver_modal_frames(&handlers, &windows, &frames, &delivered);
        if self.modal_pacing.depth.get() > 0 {
            arm(hwnd, wait);
        }
    }
}

fn arm(hwnd: HWND, millis: u32) {
    // SAFETY: a plain call on the window's handle with no callback: the
    // timer posts `WM_TIMER`, and setting it again replaces it.
    let _ = unsafe { SetTimer(Some(hwnd), MODAL_DEADLINE_TIMER, millis, None) };
}
