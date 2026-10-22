//! What the owner delivers while a modal loop stands in for its message loop:
//! OLE's `DoDragDrop`, run from the owner window, and the system's loops for a
//! window, moving or sizing it and its menus (the system menu included).
//!
//! A modal loop waits in `GetMessage`, so the platform's own loop, which
//! delivers deadlines, does not run: with the pointer still, caret blink,
//! timers and the reveal fallback stop. [`ModalFrames`] delivers what that
//! loop would, each time the modal loop asks: the owner window's from the
//! drag loop's frame requests, a window's from a timer of its own, which
//! `DefWindowProc`'s loops dispatch as `WM_TIMER` and which is armed again for
//! when the next deadline is.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{KillTimer, SetTimer},
};

use super::{FrameCount, ModalDelivery, WindowsLoop};
use crate::owner::handlers::PlatformHandlers;
use crate::platforms::windows::platform::{WindowContext, WindowMap};

/// The timer that paces a window's modal loops. Distinct from the touchpad's.
pub(crate) const MODAL_DEADLINE_TIMER: usize = 0x4D4C;

/// What one modal loop's frames are delivered from: the handlers whose
/// wake-deadline hook is asked, the windows painted, the frame count the
/// deliveries are keyed by and the last deadline this loop delivered. The
/// owner window keeps one for the drag loop and each window one for its own,
/// all over the platform's one handler set and frame count.
pub(crate) struct ModalFrames {
    handlers: Rc<RefCell<PlatformHandlers>>,
    windows: Weak<WindowMap>,
    frames: Rc<FrameCount>,
    delivered: ModalDelivery,
}

impl ModalFrames {
    pub(crate) fn new(
        handlers: Rc<RefCell<PlatformHandlers>>,
        windows: Weak<WindowMap>,
        frames: Rc<FrameCount>,
    ) -> Self {
        Self {
            handlers,
            windows,
            frames,
            delivered: ModalDelivery::default(),
        }
    }

    /// A modal loop starts: the deadline an earlier one delivered does not
    /// hold back its first.
    pub(crate) fn start(&self) {
        self.delivered.clear();
    }

    /// What the main loop would deliver, while the modal loop stands in for it
    /// and dispatches neither deadlines nor paints: a due wake deadline is
    /// delivered as the main loop delivers it, once per frame generation, and
    /// every window with an invalid region is painted (`UpdateWindow` sends
    /// its `WM_PAINT` directly), so every runtime on the thread keeps getting
    /// frames. Returns how many milliseconds to wait before the next request.
    /// The hook is user code, run with no borrow held and in the main loop's
    /// custody; a caller holds its own clone of `self`, since that code may
    /// close the window that keeps it.
    pub(crate) fn deliver(&self) -> u32 {
        // A replaced hook's final captures may replace the registration
        // again: the query retires its clone before the answer is checked
        // against it.
        let queried = WindowsLoop::ask_wake_deadline(&self.handlers);
        // The answer of a hook that is no longer installed is not its
        // replacement's: as in the main loop, the delivery record is keyed by
        // the hook as well as the instant.
        let current = queried.and_then(|(deadline, identity)| {
            let installed = self
                .handlers
                .borrow()
                .wake_deadline
                .as_ref()
                .is_some_and(|hook| Weak::ptr_eq(&identity, &Arc::downgrade(hook)));
            installed.then_some((deadline?, identity))
        });
        let now = web_time::Instant::now();
        let deadline = current.as_ref().map(|(deadline, _)| *deadline);
        match current {
            Some((deadline, identity))
                if deadline <= now
                    && self
                        .delivered
                        .admits(deadline, &identity, self.frames.get()) =>
            {
                if let Some(windows) = self.windows.upgrade() {
                    self.delivered.record(deadline, identity, self.frames.get());
                    WindowsLoop::redraw_deadline_windows(&windows);
                }
            }
            Some(_) => {}
            None => self.delivered.clear(),
        }
        // A snapshot: a paint runs user code, which may open or close windows.
        let painted: Vec<HWND> = self
            .windows
            .upgrade()
            .map(|windows| {
                windows
                    .lock()
                    .values()
                    .map(|window| window.hwnd())
                    .collect()
            })
            .unwrap_or_default();
        for hwnd in painted {
            // SAFETY: a window this thread created; a handle closed by an
            // earlier paint fails the call harmlessly. Nothing is borrowed.
            let _ = unsafe { windows::Win32::Graphics::Gdi::UpdateWindow(hwnd) };
        }
        crate::owner::modal_pace::modal_frame_wait_ms(deadline, now)
    }
}

/// A window's modal-loop pacing: what its loops' frames are delivered from,
/// and how many of them run.
pub(crate) struct ModalPacing {
    frames: Rc<ModalFrames>,
    /// How many modal loops are running for the window.
    depth: Cell<u32>,
}

impl ModalPacing {
    pub(crate) fn new(frames: ModalFrames) -> Self {
        Self {
            frames: Rc::new(frames),
            depth: Cell::new(0),
        }
    }
}

impl WindowContext {
    /// A modal loop for the window starts.
    pub(crate) fn enter_modal_loop(&self, hwnd: HWND) {
        let pacing = &self.modal_pacing;
        if pacing.depth.replace(pacing.depth.get().saturating_add(1)) == 0 {
            pacing.frames.start();
            arm(hwnd, crate::owner::modal_pace::MODAL_FRAME_PACE_MS);
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
        let frames = Rc::clone(&self.modal_pacing.frames);
        let wait = frames.deliver();
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
