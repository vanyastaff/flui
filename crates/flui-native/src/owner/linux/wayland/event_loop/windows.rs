//! The windows a loop holds, by identity, and what the loop asks of each.

use std::cell::{Cell, RefCell};
use std::collections::hash_map::Entry;
use std::rc::Rc;

use flui_platform::WindowId;
use rustc_hash::FxHashMap;
use web_time::Instant;

use crate::owner::OpenWindowError;
use crate::owner::linux::wayland::activation::ActivationToken;
use crate::owner::linux::wayland::wait::FrameHold;
use crate::owner::panic_boundary::contain_owner_callback;

/// What the loop asks of a window it holds: the window feature's
/// (`window::linux::wayland`), which implements it.
pub(crate) trait LoopWindow {
    /// Carry out what the compositor and other threads asked of the
    /// window since the last turn: acknowledge a configure, publish the
    /// state, ask about a close and deliver a frame that is due. Calls the
    /// window's handler; the loop calls it with none of its locks held.
    fn settle(&self);

    /// Whether the window has a frame to deliver now.
    fn frame_due(&self) -> bool;

    /// When the window gives up on a frame callback that has not come,
    /// with a frame asked for behind it: a present that was skipped never
    /// commits the callback it armed.
    fn frame_wait(&self) -> Option<Instant>;

    /// What holds back a frame asked of the window now.
    fn frame_hold(&self) -> FrameHold;

    /// When the window stops waiting for the compositor to carry out a
    /// mode change it asked for: the turn after it publishes what the
    /// window reads.
    fn mode_due(&self) -> Option<Instant>;

    /// A wake deadline came due: ask the window for a frame.
    fn request_frame(&self);

    /// The loop ends: let go of the window's handler and its surface.
    fn release(&self);

    /// Another instance of the application was launched and handed its
    /// launch over: come forward with `token`, the one its launcher gave
    /// it, or, without one, a token for the last user input.
    fn activate_forwarded(&self, token: Option<ActivationToken>);
}

/// The windows a loop holds, by identity, on the owner thread, which
/// alone holds the table (ADR-0327 §5).
pub(crate) struct WindowTable {
    pub(super) windows: RefCell<FxHashMap<WindowId, Rc<dyn LoopWindow>>>,
    /// The window the compositor activated last, whether or not it still
    /// is: the one a forwarded launch brings forward.
    last_active: Cell<Option<WindowId>>,
    /// The last window closed since the loop last looked: it asks the
    /// application's exit policy.
    emptied: Cell<bool>,
    /// Issues the id of every window, never one twice.
    ids: RefCell<flui_platform::WindowIdMint>,
    /// The activation token the process was launched with, until the
    /// first window is held.
    launch_token: RefCell<Option<ActivationToken>>,
}

impl WindowTable {
    /// A table whose first window takes `launch_token`.
    pub(super) fn new(launch_token: Option<ActivationToken>) -> Self {
        Self {
            windows: RefCell::default(),
            last_active: Cell::new(None),
            emptied: Cell::new(false),
            ids: RefCell::default(),
            launch_token: RefCell::new(launch_token),
        }
    }

    /// An identity no window of this loop had.
    ///
    /// # Errors
    ///
    /// [`OpenWindowError::IdsExhausted`] once the space is used up, never
    /// wrapped onto a window something still names.
    pub(super) fn issue(&self) -> Result<WindowId, OpenWindowError> {
        Ok(self.ids.borrow_mut().next()?)
    }

    /// Hold `window` as `id`. The first window held is given the
    /// activation token the process was launched with; a window refused
    /// leaves it for the next.
    pub(super) fn register(
        &self,
        id: WindowId,
        window: Rc<dyn LoopWindow>,
    ) -> Result<Option<ActivationToken>, OpenWindowError> {
        let refused = match self.windows.borrow_mut().entry(id) {
            Entry::Vacant(vacant) => {
                vacant.insert(window);
                None
            }
            Entry::Occupied(_) => Some(window),
        };
        match refused {
            None => Ok(self.launch_token.borrow_mut().take()),
            Some(refused) => {
                // Dropped with the table's borrow released: its drop may
                // release what its handler held.
                contain_owner_callback(|| drop(refused));
                Err(OpenWindowError::Backend {
                    message: format!("the Wayland loop already held a window as {id:?}"),
                })
            }
        }
    }

    /// Let go of the window `id`, which closed. When it was the last, the
    /// loop asks the application's exit policy on its next turn.
    pub(crate) fn forget(&self, id: WindowId) {
        let (window, empty) = {
            let mut windows = self.windows.borrow_mut();
            let window = windows.remove(&id);
            (window, windows.is_empty())
        };
        if window.is_some() && empty {
            self.emptied.set(true);
        }
        // The window's own drop may release what its handler held.
        contain_owner_callback(|| drop(window));
    }

    /// The compositor activated window `id`, or took the activation away.
    pub(crate) fn activation(&self, id: WindowId, activated: bool) {
        if activated {
            self.last_active.set(Some(id));
        }
    }

    /// Another instance handed its launch over: bring forward the window
    /// activated last, or another one it still holds, with `token`.
    /// Before the first window is held, `token` waits for it in place of
    /// the launch's own, which the compositor may since have expired.
    pub(crate) fn activate_forwarded(&self, token: Option<ActivationToken>) {
        let window = {
            let windows = self.windows.borrow();
            self.last_active
                .get()
                .and_then(|id| windows.get(&id))
                .or_else(|| windows.values().next())
                .cloned()
        };
        match window {
            // Called with the table's borrow released: a window may close
            // or open another while it comes forward.
            Some(window) => contain_owner_callback(|| window.activate_forwarded(token)),
            None => {
                if let Some(token) = token {
                    *self.launch_token.borrow_mut() = Some(token);
                }
            }
        }
    }

    /// The windows held now, as a snapshot a window's settle may change
    /// the table under.
    pub(super) fn values(&self) -> Vec<Rc<dyn LoopWindow>> {
        self.windows
            .borrow()
            .values()
            .cloned()
            .collect::<Vec<Rc<dyn LoopWindow>>>()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.windows.borrow().is_empty()
    }

    pub(super) fn take_emptied(&self) -> bool {
        self.emptied.replace(false)
    }
}
