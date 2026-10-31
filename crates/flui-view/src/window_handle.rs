//! The application's handle on its window (ADR-0237).
//!
//! A [`WindowHandle`] reads the window's published [`WindowState`], asks for
//! changes as [`WindowCommand`]s and observes state changes. It is owner-local
//! and weak: it does not keep the window alive, and once the window closes every
//! call answers closed. The host owns the window through a [`WindowSource`].

use std::cell::Cell;
use std::panic::resume_unwind;
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    SizeConstraints, SubmitError, WindowCommand, WindowControl, WindowFrame, WindowMode,
    WindowRevision, WindowState,
};

use crate::observation::{Observation, Phase, Subscription, WeakObservation};

/// The window has closed, or its host is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the window is closed")]
pub struct WindowClosed;

impl From<WindowClosed> for SubmitError {
    fn from(WindowClosed: WindowClosed) -> Self {
        Self::Closed
    }
}

struct Shared {
    control: Box<dyn WindowControl>,
    closed: Cell<bool>,
}

/// Weak, owner-local capability for one window.
///
/// Acquire it from `LifecycleContext::window` in `init_state` or
/// `did_change_dependencies`, never in build/layout/paint. Commands are
/// admitted at once against the window's capabilities
/// ([`SubmitError::Admission`]) and applied on the owner's next turn, in the
/// order they were made; their effect shows in the next [`WindowState`].
#[derive(Clone)]
pub struct WindowHandle {
    shared: Weak<Shared>,
    states: WeakObservation<WindowState>,
}

impl std::fmt::Debug for WindowHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowHandle").finish_non_exhaustive()
    }
}

impl WindowHandle {
    fn open(&self) -> Result<Rc<Shared>, WindowClosed> {
        self.shared
            .upgrade()
            .filter(|shared| !shared.closed.get())
            .ok_or(WindowClosed)
    }

    fn submit(&self, command: WindowCommand) -> Result<(), SubmitError> {
        self.open()?.control.submit(command)
    }

    /// The window's last published state, with the
    /// [`capabilities`](WindowState::capabilities) that say which commands it
    /// honours.
    ///
    /// # Errors
    /// [`WindowClosed`] once the window has closed.
    pub fn state(&self) -> Result<WindowState, WindowClosed> {
        Ok(self.open()?.control.state())
    }

    /// Set the title.
    ///
    /// # Errors
    /// [`SubmitError::Admission`] with
    /// [`Unsupported`](flui_platform::AdmissionError::Unsupported) where the
    /// window cannot, [`SubmitError::Closed`] once it has closed.
    pub fn set_title(&self, title: impl Into<String>) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetTitle(title.into()))
    }

    /// Move the window's outer top-left corner to a global position in logical
    /// pixels.
    ///
    /// # Errors
    /// [`InvalidGeometry`](flui_platform::AdmissionError::InvalidGeometry) for
    /// a non-finite position; otherwise as [`Self::set_title`].
    pub fn set_position(&self, at: Point<f64>) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetPosition(at))
    }

    /// Resize the window's content area, in logical pixels.
    ///
    /// # Errors
    /// [`InvalidGeometry`](flui_platform::AdmissionError::InvalidGeometry) for
    /// a size that is not finite and positive; otherwise as
    /// [`Self::set_title`].
    pub fn set_size(&self, size: Size<f64>) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetSize(size))
    }

    /// Set the content-size limits.
    ///
    /// # Errors
    /// [`InvalidGeometry`](flui_platform::AdmissionError::InvalidGeometry) for
    /// a bad bound or a minimum above the maximum; otherwise as
    /// [`Self::set_title`].
    pub fn set_constraints(&self, constraints: SizeConstraints) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetConstraints(constraints))
    }

    /// Allow or forbid user resizing.
    ///
    /// # Errors
    /// As [`Self::set_title`].
    pub fn set_resizable(&self, on: bool) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetResizable(on))
    }

    /// Frame the window: the system's frame, a title bar the application draws
    /// inside a frame the system keeps, or none.
    ///
    /// # Errors
    /// As [`Self::set_title`].
    pub fn set_frame(&self, frame: WindowFrame) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetFrame(frame))
    }

    /// Enter a mode; [`WindowMode::Normal`] restores from any other.
    ///
    /// # Errors
    /// As [`Self::set_title`].
    pub fn set_mode(&self, mode: WindowMode) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetMode(mode))
    }

    /// Show the window.
    ///
    /// # Errors
    /// As [`Self::set_title`].
    pub fn show(&self) -> Result<(), SubmitError> {
        self.submit(WindowCommand::Show)
    }

    /// Hide the window.
    ///
    /// # Errors
    /// As [`Self::set_title`].
    pub fn hide(&self) -> Result<(), SubmitError> {
        self.submit(WindowCommand::Hide)
    }

    /// Bring the window to the front and request focus.
    ///
    /// # Errors
    /// As [`Self::set_title`].
    pub fn activate(&self) -> Result<(), SubmitError> {
        self.submit(WindowCommand::Activate)
    }

    /// Have the window system draw `material` behind the window's content
    /// (ADR-0254 §3).
    ///
    /// # Errors
    /// [`Unsupported`](flui_platform::AdmissionError::Unsupported) where the
    /// window draws no material
    /// ([`WindowCapabilities::material`](flui_platform::WindowCapabilities::material));
    /// otherwise as [`Self::set_title`].
    pub fn set_material(&self, material: flui_platform::WindowMaterial) -> Result<(), SubmitError> {
        self.submit(WindowCommand::SetMaterial(material))
    }

    /// Ask the window to close through the vetoed close path; the window may
    /// stay open. Asking is not closing: the application's close-request
    /// handler decides, on the owner's next turn.
    ///
    /// # Errors
    /// [`WindowClosed`] once the window has closed.
    pub fn request_close(&self) -> Result<(), WindowClosed> {
        self.open()?.control.request_close();
        Ok(())
    }

    /// Register for future state changes and return the state now. The
    /// callback is never invoked synchronously by this method; it runs in FIFO
    /// order on the owner's turn after a new state is published, a reentrant
    /// publish queues behind the active walk, and a callback panic is held until
    /// every eligible callback has run. Retain the returned token.
    ///
    /// # Errors
    /// [`WindowClosed`] once the window has closed.
    pub fn subscribe(
        &self,
        callback: impl FnMut(&WindowState) + 'static,
    ) -> Result<(WindowState, WindowSubscription), WindowClosed> {
        // Registered first, read second: a state published while the snapshot
        // is read is delivered to the new subscriber instead of falling between
        // an old snapshot and a registration that came too late.
        let (_, token) = self
            .states
            .subscribe(Box::new(callback))
            .map_err(|_| WindowClosed)?;
        let state = self.open()?.control.state();
        Ok((state, WindowSubscription { _token: token }))
    }
}

/// Cancels a window-state callback on drop. Does not retain the window or the
/// callback.
#[must_use = "dropping the subscription cancels window observation"]
pub struct WindowSubscription {
    _token: Subscription<WindowState>,
}

impl std::fmt::Debug for WindowSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowSubscription").finish_non_exhaustive()
    }
}

/// Composition-root owner of one window's [`WindowHandle`]s.
///
/// The host publishes each new [`WindowState`] with [`Self::publish`] and
/// delivers it with [`Self::drain`], outside any lock; it closes the source
/// before the window's terminal notification.
#[doc(hidden)]
pub struct WindowSource {
    shared: Rc<Shared>,
    states: Observation<WindowState>,
    /// The revision subscribers last received.
    published: Cell<Option<WindowRevision>>,
}

impl std::fmt::Debug for WindowSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowSource").finish_non_exhaustive()
    }
}

impl WindowSource {
    /// A source whose handles reach the window through `control`.
    #[must_use]
    pub fn new(control: impl WindowControl + 'static) -> Self {
        Self {
            shared: Rc::new(Shared {
                control: Box::new(control),
                closed: Cell::new(false),
            }),
            states: Observation::transient(),
            published: Cell::new(None),
        }
    }

    /// A weak handle for the window's content.
    #[must_use]
    pub fn handle(&self) -> WindowHandle {
        WindowHandle {
            shared: Rc::downgrade(&self.shared),
            states: self.states.downgrade(),
        }
    }

    /// Queue a newly published state for the subscribers. A closed source
    /// ignores it.
    pub fn publish(&self, state: WindowState) {
        let _ = self.states.commit(state, Phase::Open, |_| true);
    }

    /// Publish the window's current state, unless subscribers already have its
    /// revision, and deliver it. For the host to call when the window reports
    /// that its state may have changed; the report carries no payload, so a
    /// repeated report costs nothing.
    pub fn publish_current(&self) {
        if self.queue_current().is_some() {
            self.drain();
        }
    }

    /// Queue the window's current state for the subscribers when its revision
    /// is new, without delivering it, and return that state. A host that acts
    /// on the new state itself does so before calling [`Self::drain`], so the
    /// subscribers' code runs after the host's own bookkeeping.
    pub fn queue_current(&self) -> Option<WindowState> {
        let state = self.shared.control.state();
        if self.published.replace(Some(state.revision)) == Some(state.revision) {
            return None;
        }
        self.publish(state.clone());
        Some(state)
    }

    /// Deliver queued states in FIFO order. Nested drains defer to the active
    /// walk; the first callback panic resumes after every eligible callback ran.
    pub fn drain(&self) {
        if let Some(payload) = self.states.drain() {
            resume_unwind(payload);
        }
    }

    /// Close the window's handles: later calls answer closed and every
    /// subscription is released. Idempotent.
    pub fn close(&self) {
        self.shared.closed.set(true);
        self.states.begin_close();
        self.states.request_finish();
        self.drain();
    }
}

impl Drop for WindowSource {
    fn drop(&mut self) {
        let mut first = None;
        self.states.release(&mut first);
        if let Some(payload) = first {
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static_assertions::assert_not_impl_any!(WindowHandle: Send, Sync);
    static_assertions::assert_not_impl_any!(WindowSubscription: Send, Sync);
}
