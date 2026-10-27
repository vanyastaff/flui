//! A [`WindowHandler`] for tests: it records every call it receives, in
//! order, and answers with what the test told it to.
//!
//! A window is opened with its handler (ADR-0254 §4), before the test holds
//! the window, so what a method does is set on the handler afterwards
//! ([`RecordingWindowHandler::react_to_input`] and its siblings): a reaction
//! may capture the window to re-enter it. Every call is recorded before its
//! reaction runs, so a reaction that panics is still on the record.
//!
//! ```
//! use flui_platform::{CloseDecision, WindowHandler};
//! use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
//!
//! let handler = RecordingWindowHandler::new();
//! handler.react_to_close_request(|| CloseDecision::KeepOpen);
//! assert_eq!(handler.close_requested(), CloseDecision::KeepOpen);
//! handler.state_changed();
//! assert_eq!(
//!     handler.calls(),
//!     [HandlerCall::CloseRequested, HandlerCall::StateChanged]
//! );
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform::{
    CloseDecision, DispatchEventResult, FrameRequest, PlatformInput, SurfaceChange, WindowHandler,
};

/// One call a [`RecordingWindowHandler`] received.
#[derive(Debug, Clone)]
pub enum HandlerCall {
    /// [`WindowHandler::input`], with the event.
    Input(PlatformInput),
    /// [`WindowHandler::state_changed`].
    StateChanged,
    /// [`WindowHandler::frame_requested`], with the request.
    FrameRequested(FrameRequest),
    /// [`WindowHandler::close_requested`].
    CloseRequested,
    /// [`WindowHandler::closed`].
    Closed,
    /// [`WindowHandler::surface_changed`], with the change.
    SurfaceChanged(SurfaceChange),
}

impl HandlerCall {
    /// Whether this is an input other than a capability snapshot or change.
    #[must_use]
    pub fn is_ordinary_input(&self) -> bool {
        matches!(self, Self::Input(input) if !matches!(input, PlatformInput::Capabilities(_)))
    }
}

/// Compares the kind of call and what it carried, an input by its kind only
/// (input has no equality of its own).
impl PartialEq for HandlerCall {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Input(a), Self::Input(b)) => {
                std::mem::discriminant(a) == std::mem::discriminant(b)
            }
            (Self::FrameRequested(a), Self::FrameRequested(b)) => a == b,
            (Self::SurfaceChanged(a), Self::SurfaceChanged(b)) => a == b,
            (Self::StateChanged, Self::StateChanged)
            | (Self::CloseRequested, Self::CloseRequested)
            | (Self::Closed, Self::Closed) => true,
            _ => false,
        }
    }
}

type Reaction<A, R> = Rc<dyn Fn(A) -> R>;

#[derive(Default)]
struct Reactions {
    input: Option<Reaction<PlatformInput, DispatchEventResult>>,
    state_changed: Option<Reaction<(), ()>>,
    frame_requested: Option<Reaction<FrameRequest, ()>>,
    close_requested: Option<Reaction<(), CloseDecision>>,
    closed: Option<Reaction<(), ()>>,
    surface_changed: Option<Reaction<SurfaceChange, ()>>,
}

/// A [`WindowHandler`] that records its calls and answers as told: by
/// default an input is [`DispatchEventResult::default`] (not taken), a close
/// request is [`CloseDecision::Close`], and the other methods do nothing.
#[derive(Default)]
pub struct RecordingWindowHandler {
    calls: RefCell<Vec<HandlerCall>>,
    reactions: RefCell<Reactions>,
}

impl std::fmt::Debug for RecordingWindowHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordingWindowHandler")
            .field("calls", &self.calls.borrow().len())
            .finish_non_exhaustive()
    }
}

impl RecordingWindowHandler {
    /// A handler with no reactions and no calls yet.
    #[must_use]
    pub fn new() -> Rc<Self> {
        Rc::default()
    }

    /// Every call received so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<HandlerCall> {
        self.calls.borrow().clone()
    }

    /// Every call received so far, leaving none on the record.
    pub fn take_calls(&self) -> Vec<HandlerCall> {
        std::mem::take(&mut self.calls.borrow_mut())
    }

    /// The inputs received so far, capability snapshots and changes included.
    #[must_use]
    pub fn inputs(&self) -> Vec<PlatformInput> {
        self.calls
            .borrow()
            .iter()
            .filter_map(|call| match call {
                HandlerCall::Input(input) => Some(input.clone()),
                _ => None,
            })
            .collect()
    }

    /// How many calls equal to `call` were received.
    #[must_use]
    pub fn count(&self, call: &HandlerCall) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|seen| *seen == call)
            .count()
    }

    /// Answer each later input with `reaction`.
    pub fn react_to_input(
        &self,
        reaction: impl Fn(PlatformInput) -> DispatchEventResult + 'static,
    ) {
        self.reactions.borrow_mut().input = Some(Rc::new(reaction));
    }

    /// Run `reaction` on each later state change.
    pub fn react_to_state_change(&self, reaction: impl Fn() + 'static) {
        self.reactions.borrow_mut().state_changed = Some(Rc::new(move |()| reaction()));
    }

    /// Run `reaction` on each later frame request.
    pub fn react_to_frame(&self, reaction: impl Fn(FrameRequest) + 'static) {
        self.reactions.borrow_mut().frame_requested = Some(Rc::new(reaction));
    }

    /// Answer each later close request with `reaction`.
    pub fn react_to_close_request(&self, reaction: impl Fn() -> CloseDecision + 'static) {
        self.reactions.borrow_mut().close_requested = Some(Rc::new(move |()| reaction()));
    }

    /// Run `reaction` when the window has closed.
    pub fn react_to_close(&self, reaction: impl Fn() + 'static) {
        self.reactions.borrow_mut().closed = Some(Rc::new(move |()| reaction()));
    }

    /// Run `reaction` on each later surface change.
    pub fn react_to_surface_change(&self, reaction: impl Fn(SurfaceChange) + 'static) {
        self.reactions.borrow_mut().surface_changed = Some(Rc::new(reaction));
    }

    /// Record `call`, then hand out the reaction `pick` names, for the caller
    /// to run with no borrow held: a reaction may re-enter this handler.
    fn record<A, R>(
        &self,
        call: HandlerCall,
        pick: fn(&Reactions) -> &Option<Reaction<A, R>>,
    ) -> Option<Reaction<A, R>> {
        self.calls.borrow_mut().push(call);
        pick(&self.reactions.borrow()).clone()
    }
}

impl WindowHandler for RecordingWindowHandler {
    fn input(&self, event: PlatformInput) -> DispatchEventResult {
        self.record(HandlerCall::Input(event.clone()), |reactions| {
            &reactions.input
        })
        .map_or_else(DispatchEventResult::default, |reaction| reaction(event))
    }

    fn state_changed(&self) {
        if let Some(reaction) = self.record(HandlerCall::StateChanged, |reactions| {
            &reactions.state_changed
        }) {
            reaction(());
        }
    }

    fn frame_requested(&self, request: FrameRequest) {
        if let Some(reaction) = self.record(HandlerCall::FrameRequested(request), |reactions| {
            &reactions.frame_requested
        }) {
            reaction(request);
        }
    }

    fn close_requested(&self) -> CloseDecision {
        self.record(HandlerCall::CloseRequested, |reactions| {
            &reactions.close_requested
        })
        .map_or(CloseDecision::Close, |reaction| reaction(()))
    }

    fn closed(&self) {
        if let Some(reaction) = self.record(HandlerCall::Closed, |reactions| &reactions.closed) {
            reaction(());
        }
    }

    fn surface_changed(&self, change: SurfaceChange) {
        if let Some(reaction) = self.record(HandlerCall::SurfaceChanged(change), |reactions| {
            &reactions.surface_changed
        }) {
            reaction(change);
        }
    }
}
