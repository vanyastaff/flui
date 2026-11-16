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
//! when the next deadline is. The window arms and stops that timer; its
//! [`ModalPacing`] counts its modal loops and keeps its frames.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use super::{ClassWindows, FrameCount, WindowsLoop};
use crate::owner::app_delivery::AppDelivery;
use crate::owner::signal::OwnerSignal;
use crate::owner::wake_deadline::{TurnDeadline, WakeDeadline};

/// The timer that paces a window's modal loops. Distinct from the touchpad's.
pub(crate) const MODAL_DEADLINE_TIMER: usize = 0x4D4C;

/// What one modal loop's frames are delivered from: the application whose
/// wake deadline is asked, the windows painted, the frame count the
/// deliveries are keyed by and the last deadline this loop delivered. The
/// owner window keeps one for the drag loop and each window one for its own,
/// all over the platform's one application delivery and frame count.
pub(crate) struct ModalFrames {
    app: Arc<AppDelivery>,
    windows: Weak<dyn ClassWindows>,
    frames: Rc<FrameCount>,
    /// The owner whose turn a due turn deadline brings, through its window
    /// message, which a modal loop dispatches like any other.
    signal: Weak<OwnerSignal>,
    deadline: RefCell<WakeDeadline>,
    turn_deadline: RefCell<TurnDeadline>,
}

impl ModalFrames {
    pub(crate) fn new(
        app: Arc<AppDelivery>,
        windows: Weak<dyn ClassWindows>,
        frames: Rc<FrameCount>,
        signal: Weak<OwnerSignal>,
    ) -> Self {
        Self {
            app,
            windows,
            frames,
            signal,
            deadline: RefCell::default(),
            turn_deadline: RefCell::default(),
        }
    }

    /// A modal loop starts: the deadlines an earlier one delivered do not
    /// hold back its first.
    pub(crate) fn start(&self) {
        *self.deadline.borrow_mut() = WakeDeadline::default();
        *self.turn_deadline.borrow_mut() = TurnDeadline::default();
    }

    /// What the main loop would deliver, while the modal loop stands in for it
    /// and dispatches neither deadlines nor paints: a due wake deadline is
    /// delivered as the main loop delivers it, once per frame generation, and
    /// every window with an invalid region is painted, so every runtime on the
    /// thread keeps getting frames. The deadline is armed and taken by the
    /// main loop's own rule ([`WakeDeadline`]), so an instant no frame has
    /// serviced since its delivery is neither delivered again nor waited for.
    /// Returns how many milliseconds to wait before the next request. The
    /// application's handler is user code, asked with no borrow held and in
    /// the main loop's custody; a caller holds its own clone of `self`, since
    /// that code may close the window that keeps it.
    pub(crate) fn deliver(&self) -> u32 {
        let answer = WindowsLoop::ask_wake_deadline(&self.app);
        let now = web_time::Instant::now();
        let frames = self.frames.get();
        let (armed, due) = {
            let mut deadline = self.deadline.borrow_mut();
            let armed = deadline.arm(answer, frames);
            (armed, deadline.take_due(now, frames))
        };
        if due && let Some(windows) = self.windows.upgrade() {
            windows.redraw_for_deadline();
        }
        // The turn deadline: asked with no borrow held, then armed and taken.
        let turn_answer = WindowsLoop::ask_turn_deadline(&self.app);
        let (turn_armed, turn_due) = {
            let mut turn = self.turn_deadline.borrow_mut();
            let armed = turn.arm(turn_answer);
            let due = turn.take_due(now);
            // A delivered instant is no longer waited for.
            (armed.filter(|_| !due), due)
        };
        if turn_due && let Some(signal) = self.signal.upgrade() {
            let _ = signal.wake();
        }
        if let Some(windows) = self.windows.upgrade() {
            windows.paint_invalid();
        }
        crate::owner::modal_pace::modal_frame_wait_ms(
            [armed, turn_armed].into_iter().flatten().min(),
            now,
        )
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

    /// A modal loop for the window starts. `true` when it is the outermost:
    /// its frames start over, and the window arms its pacing timer.
    pub(crate) fn enter(&self) -> bool {
        if self.depth.replace(self.depth.get().saturating_add(1)) == 0 {
            self.frames.start();
            true
        } else {
            false
        }
    }

    /// A modal loop for the window ends. `true` when none is left: the window
    /// stops its pacing timer then.
    pub(crate) fn exit(&self) -> bool {
        let depth = self.depth.get().saturating_sub(1);
        self.depth.set(depth);
        depth == 0
    }

    /// Whether a modal loop runs for the window.
    pub(crate) fn running(&self) -> bool {
        self.depth.get() > 0
    }

    /// The frames the window's modal loops are delivered from, cloned out:
    /// a delivery runs user code, which may close the window that keeps them.
    pub(crate) fn frames(&self) -> Rc<ModalFrames> {
        Rc::clone(&self.frames)
    }
}
