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
use flui_foundation::panic::FirstPanic;
use flui_platform::PlatformWindow;
use flui_platform::{WindowCommand, WindowCommandError, WindowControl, WindowState};
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
    /// Asks the owner for a turn; `false` when it cannot be woken.
    wake: Rc<dyn Fn() -> bool>,
}

impl WindowQueue {
    /// Ask the owner for the turn that applies this queue.
    fn ask_for_turn(&self, window: &dyn PlatformWindow) {
        if !(self.wake)() {
            // No owner wake path: a redraw guarantees a turn on every backend.
            window.request_redraw();
        }
    }

    /// Ask for a turn again if work is still waiting: the turn that should have
    /// applied it did not.
    fn rearm(&self) {
        if self.pending.borrow().is_empty() {
            return;
        }
        if let Some(window) = self.window.upgrade() {
            self.ask_for_turn(&*window);
        }
    }

    /// Apply what was queued before this call, in order. What the applied work
    /// queues goes to the next turn. A failure of one command is reported and
    /// the rest of the queue still applies. A close that goes ahead ends the
    /// batch: what was queued after it is dropped with the window.
    fn apply_pending(&self) {
        let batch = std::mem::take(&mut *self.pending.borrow_mut());
        if batch.is_empty() {
            return;
        }
        // A window that is gone takes its commands with it: they are dropped,
        // not given back to a queue no turn can ever apply them from.
        let Some(window) = self.window.upgrade() else {
            return;
        };
        // From here the batch is owned by a guard that gives back whatever is
        // still unapplied if a command panics: the rest waits for the next
        // turn instead of being lost with the unwind.
        let mut batch = UnappliedBatch {
            queue: self,
            rest: batch,
        };
        while let Some(work) = batch.rest.pop_front() {
            match work {
                Queued::Command(command) => match window.apply(command) {
                    Ok(()) => {}
                    Err(WindowCommandError::Backend { message }) => {
                        tracing::warn!(
                            window = ?window.id(),
                            %message,
                            "the window system failed a window command"
                        );
                    }
                    Err(error) => {
                        // Why the window does not take it, and whether asking
                        // again could: a refusal for good is not worth
                        // retrying from the application.
                        if let Some(absent) = error.unsupported() {
                            tracing::debug!(
                                window = ?window.id(),
                                reason = ?absent.reason,
                                retryable = absent.is_retryable(),
                                "the window does not take a queued window command"
                            );
                        } else {
                            tracing::debug!(
                                window = ?window.id(),
                                %error,
                                "a queued window command was refused"
                            );
                        }
                    }
                },
                Queued::Close => {
                    if self.close_with_veto() {
                        batch.rest.clear();
                    }
                }
            }
        }
    }

    /// Close the window unless the application's close-request handler vetoes.
    /// `true` when the window is closed or was already gone.
    fn close_with_veto(&self) -> bool {
        if self.router.consult(self.address, CloseReason::Program) != CloseResponse::Close {
            return false;
        }
        if let Err(error) = self.router.request_close(self.address) {
            tracing::debug!(%error, "a queued window close found the window already gone");
        }
        true
    }
}

/// The part of a batch not yet applied. If the walk unwinds, dropping this puts
/// it back at the front of the queue, ahead of anything queued meanwhile, so the
/// next turn applies it in the order it was submitted.
struct UnappliedBatch<'a> {
    queue: &'a WindowQueue,
    rest: VecDeque<Queued>,
}

impl Drop for UnappliedBatch<'_> {
    fn drop(&mut self) {
        if self.rest.is_empty() {
            return;
        }
        let mut pending = self.queue.pending.borrow_mut();
        let newer = std::mem::replace(&mut *pending, std::mem::take(&mut self.rest));
        pending.extend(newer);
    }
}

/// Marks a walk over the queues as in progress, and unmarks it however the walk
/// ends.
struct Applying<'a>(&'a Cell<bool>);

impl Drop for Applying<'_> {
    fn drop(&mut self) {
        self.0.set(false);
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
            // A source that closed keeps its queue until it is dropped; its
            // handle answers closed, and a closed window has no handle to give.
            (queue.address == address && handle.state().is_ok()).then(|| handle.clone())
        })
    }

    /// The window presented at `address`, while it is open.
    #[cfg(feature = "native")]
    pub(crate) fn window_for(
        &self,
        address: PresentationAddress,
    ) -> Option<Arc<dyn PlatformWindow>> {
        self.queues.borrow().iter().find_map(|(queue, handle)| {
            let queue = queue.upgrade()?;
            // A closed window keeps its queue until it is dropped; it is not
            // handed out.
            (queue.address == address && handle.state().is_ok())
                .then(|| queue.window.upgrade())
                .flatten()
        })
    }

    fn live(&self) -> Vec<Rc<WindowQueue>> {
        self.queues
            .borrow()
            .iter()
            .filter_map(|(queue, _)| queue.upgrade())
            .collect()
    }

    /// Apply every window's pending work. Call on the owner thread, outside any
    /// owner callback: applying may re-enter the window's procedure. A nested
    /// call returns at once and leaves what it finds for the next turn.
    ///
    /// A window whose command panics keeps the rest of its batch and asks for
    /// another turn; the other windows still apply theirs, and the first panic
    /// is raised once every window has had its turn.
    pub(crate) fn apply_pending(&self) {
        if self.applying.replace(true) {
            return;
        }
        let _applying = Applying(&self.applying);
        let mut first = FirstPanic::new();
        for queue in self.live() {
            if first.contain(|| queue.apply_pending()).is_none() {
                queue.rearm();
            }
        }
        first.resume();
    }
}

/// [`WindowControl`] over one platform window, backed by its queue.
struct HostWindowControl {
    queue: Rc<WindowQueue>,
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
        self.queue.ask_for_turn(&*window);
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

    fn submit(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        let window = self
            .queue
            .window
            .upgrade()
            .ok_or(WindowCommandError::Closed)?;
        command.admit(&window.state().capabilities)?;
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
        wake,
    });
    let source = WindowSource::new(HostWindowControl {
        last_state: RefCell::new(window.state()),
        queue: Rc::clone(&queue),
    });
    queues.register(&queue, source.handle());
    source
}

#[cfg(test)]
#[path = "window_control_tests.rs"]
mod tests;
