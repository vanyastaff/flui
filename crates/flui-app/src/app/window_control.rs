//! The application's control over its window (ADR-0237): the commands app code
//! queues through a `WindowHandle`, and their application on the owner thread.
//!
//! A command is checked when it is submitted ([`WindowCommand::admit`] against
//! the window's capabilities), queued, and applied at the start of the owner's
//! next turn, before its frame, in submission order. It is not applied inline:
//! a native call such as `SetWindowPos` re-enters the window procedure
//! synchronously, which would deliver a resize into the middle of the code that
//! asked for it. A command submitted while the queue is being applied runs on
//! the following turn.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak as RcWeak};
use std::sync::{Arc, Weak};

use flui_foundation::PresentationAddress;
use flui_platform::traits::PlatformWindow;
use flui_platform::{
    WindowCapabilities, WindowCommand, WindowCommandError, WindowControl, WindowState,
};
use flui_view::__runtime::WindowSource;
use flui_view::WindowHandle;

use super::close_request::{CloseReason, CloseRequestRouter, CloseResponse};

/// One unit of queued work for a window, applied in the order it was queued.
enum Queued {
    Command(WindowCommand),
    /// Ask the window to close through the application's close-request
    /// handler, which may veto.
    Close,
}

/// A window's queued work and the way back to the window.
pub(crate) struct WindowQueue {
    window: Weak<dyn PlatformWindow>,
    pending: RefCell<VecDeque<Queued>>,
    address: PresentationAddress,
    router: Arc<CloseRequestRouter>,
}

impl WindowQueue {
    /// Apply what was queued before this call, in order. What the applied work
    /// queues goes to the next turn. A failure of one command is reported and
    /// the rest of the queue still applies.
    fn apply_pending(&self) {
        let batch = std::mem::take(&mut *self.pending.borrow_mut());
        if batch.is_empty() {
            return;
        }
        let Some(window) = self.window.upgrade() else {
            return;
        };
        for work in batch {
            match work {
                Queued::Command(command) => match window.apply(command) {
                    Ok(()) => {}
                    Err(WindowCommandError::Backend(message)) => {
                        tracing::warn!(
                            window = ?window.id(),
                            %message,
                            "the window system failed a window command"
                        );
                    }
                    Err(error) => {
                        tracing::debug!(
                            window = ?window.id(),
                            %error,
                            "a queued window command was refused"
                        );
                    }
                },
                Queued::Close => self.close_with_veto(),
            }
        }
    }

    fn close_with_veto(&self) {
        if self.router.consult(self.address, CloseReason::Program) == CloseResponse::Close
            && let Err(error) = self.router.request_close(self.address)
        {
            tracing::debug!(%error, "a queued window close found the window already gone");
        }
    }
}

/// Every window's queue, applied together at the start of an owner turn.
#[derive(Default)]
pub(crate) struct WindowCommandQueues {
    queues: RefCell<Vec<(RcWeak<WindowQueue>, WindowHandle)>>,
    applying: Cell<bool>,
}

impl WindowCommandQueues {
    fn register(&self, queue: &Rc<WindowQueue>, handle: WindowHandle) {
        let mut queues = self.queues.borrow_mut();
        queues.retain(|(queue, _)| queue.strong_count() > 0);
        queues.push((Rc::downgrade(queue), handle));
    }

    /// The handle of the window presented at `address`, while it is open.
    pub(crate) fn handle_for(&self, address: PresentationAddress) -> Option<WindowHandle> {
        self.queues.borrow().iter().find_map(|(queue, handle)| {
            let queue = queue.upgrade()?;
            (queue.address == address).then(|| handle.clone())
        })
    }

    /// Apply every window's pending work. Call on the owner thread, outside any
    /// owner callback: applying may re-enter the window's procedure. A nested
    /// call returns at once and leaves what it finds for the next turn.
    pub(crate) fn apply_pending(&self) {
        if self.applying.replace(true) {
            return;
        }
        let queues: Vec<_> = self
            .queues
            .borrow()
            .iter()
            .filter_map(|(queue, _)| queue.upgrade())
            .collect();
        for queue in queues {
            queue.apply_pending();
        }
        self.applying.set(false);
    }
}

/// [`WindowControl`] over one platform window, backed by its queue.
struct HostWindowControl {
    queue: Rc<WindowQueue>,
    /// Asks the owner for a turn; `false` when it cannot be woken.
    wake: Rc<dyn Fn() -> bool>,
    /// The last state read while the window was alive.
    last_state: RefCell<WindowState>,
}

impl HostWindowControl {
    fn enqueue(&self, work: Queued) -> Result<(), WindowCommandError> {
        let window = self
            .queue
            .window
            .upgrade()
            .ok_or(WindowCommandError::Closed)?;
        self.queue.pending.borrow_mut().push_back(work);
        if !(self.wake)() {
            // No owner wake path: a redraw guarantees a turn on every backend.
            window.request_redraw();
        }
        Ok(())
    }
}

impl WindowControl for HostWindowControl {
    fn state(&self) -> WindowState {
        match self.queue.window.upgrade() {
            Some(window) => {
                let state = window.state();
                self.last_state.replace(state.clone());
                state
            }
            None => self.last_state.borrow().clone(),
        }
    }

    fn capabilities(&self) -> WindowCapabilities {
        self.queue
            .window
            .upgrade()
            .map_or_else(WindowCapabilities::none, |window| window.capabilities())
    }

    fn submit(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        let window = self
            .queue
            .window
            .upgrade()
            .ok_or(WindowCommandError::Closed)?;
        command.admit(&window.capabilities())?;
        self.enqueue(Queued::Command(command))
    }

    fn request_close(&self) {
        // A window that is already gone has nothing left to close.
        let _ = self.enqueue(Queued::Close);
    }
}

/// The window control for the presentation at `address`, registered with
/// `queues` so its commands are applied each owner turn.
pub(crate) fn window_source(
    window: &Arc<dyn PlatformWindow>,
    address: PresentationAddress,
    queues: &WindowCommandQueues,
    wake: Rc<dyn Fn() -> bool>,
    router: Arc<CloseRequestRouter>,
) -> WindowSource {
    let queue = Rc::new(WindowQueue {
        window: Arc::downgrade(window),
        pending: RefCell::new(VecDeque::new()),
        address,
        router,
    });
    let source = WindowSource::new(HostWindowControl {
        last_state: RefCell::new(window.state()),
        queue: Rc::clone(&queue),
        wake,
    });
    queues.register(&queue, source.handle());
    source
}

#[cfg(test)]
#[path = "window_control_tests.rs"]
mod tests;
