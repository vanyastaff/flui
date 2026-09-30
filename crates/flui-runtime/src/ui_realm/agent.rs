//! An agent's read of, and actions on, one presentation's semantics tree,
//! through the realm's owner inbox (ADR-0095 §3, the realm half of the
//! in-process backend).
//!
//! A [`SemanticsAgent`] is a `Send + Sync` capability vended by
//! [`UiRealm::semantics_agent`]. It never touches a tree: [`SemanticsAgent::read`]
//! and [`SemanticsAgent::act`] enqueue a command on the realm's bounded inbox
//! and return an [`AgentReply`] that the owner fills at its next drain, a
//! frame boundary. A read reflects the last committed frame's semantics; an
//! action reaches the same handler an assistive technology's does, through
//! `PresentationState::dispatch_semantics_action`.
//!
//! Holding an agent keeps semantics collected for its presentation (a
//! [`SemanticsHandle`] shared by every clone), so the tree an agent reads is
//! the tree assistive technology is published. Dropping the last clone lets
//! collection stop on the next frame; a reply still unanswered does not keep
//! it on.

use std::cell::Cell;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, TrySendError};
use flui_foundation::PresentationId;
use flui_foundation::geometry::DevicePixelRatio;
use flui_protocol::{
    ActionName, ActionRequest, ElementId, ErrorCode, ReadQuery, Retry, Tree, WindowId,
};
use flui_semantics::{Placement, SemanticsActionError, WireActionError, WireReadError};
use flui_view::__runtime::{AgentPort, PendingAnswer};
use flui_view::dev_agent::{AgentAnswer, AgentFault, AgentWindow, HandleKind};
use parking_lot::Mutex;

use super::UiRealm;
use super::commands::{CommandSendError, UiCommand, UiCommandSender};
use crate::semantics_host::SemanticsHandle;

/// Why an agent's read or action failed. Each variant has its ADR-0080 code
/// and retry advice.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AgentError {
    /// The realm's inbox is full; retry soon.
    #[error("the realm's command inbox is full")]
    InboxFull,
    /// The realm is gone, or dropped the command without answering it.
    #[error("the realm has shut down")]
    RealmGone,
    /// No semantics frame has been committed since the agent was vended; one
    /// has been requested.
    #[error("no semantics tree has been committed yet")]
    NoTreeYet,
    /// The presentation the agent addresses has closed.
    #[error("the window has closed")]
    PresentationGone,
    /// The element is not in the tree.
    #[error("element {element} is not in the tree")]
    NodeNotFound {
        /// The element addressed.
        element: ElementId,
        /// Whether this agent's reads ever reported the element: if so it is
        /// gone, if not the handle was never issued to this agent.
        issued: bool,
    },
    /// The element does not advertise the action now.
    #[error("element {element} does not offer {action}")]
    ActionUnsupported {
        /// The element addressed.
        element: ElementId,
        /// The action it does not offer.
        action: ActionName,
    },
    /// The element refuses interaction.
    #[error("element {element} is disabled")]
    Disabled {
        /// The element addressed.
        element: ElementId,
    },
    /// The request is malformed (`set_value` without a value).
    #[error("{reason}")]
    InvalidArgument {
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The published semantics tree breaks AccessKit's contract.
    #[error("the published semantics tree is malformed")]
    Malformed,
    /// The element's handler panicked while the owner invoked it. It may
    /// have run in part; the panic continues on the owner thread. Only a
    /// handler that runs when invoked reports this: a widget that defers the
    /// work to a later frame (a `GestureDetector`) has answered `Ok` by then,
    /// and a panic there is the frame's.
    #[error("the handler of element {element} panicked")]
    HandlerPanicked {
        /// The element addressed.
        element: ElementId,
    },
    /// The owner panicked while resolving the action, before any handler was
    /// invoked; the panic continues on the owner thread.
    #[error("resolving the action on element {element} panicked")]
    ResolvePanicked {
        /// The element addressed.
        element: ElementId,
    },
}

impl AgentError {
    /// The ADR-0080 code an agent branches on.
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InboxFull | Self::NoTreeYet => ErrorCode::Busy,
            Self::RealmGone => ErrorCode::ShuttingDown,
            Self::PresentationGone | Self::NodeNotFound { issued: true, .. } => ErrorCode::Gone,
            Self::NodeNotFound { issued: false, .. } => ErrorCode::UnknownHandle,
            Self::ActionUnsupported { .. } => ErrorCode::ActionUnsupported,
            Self::Disabled { .. } => ErrorCode::Disabled,
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::Malformed | Self::HandlerPanicked { .. } | Self::ResolvePanicked { .. } => {
                ErrorCode::Platform
            }
        }
    }

    /// Whether the same call can succeed later.
    #[must_use]
    pub fn retry(&self) -> Retry {
        match self {
            Self::InboxFull | Self::NoTreeYet => Retry::Soon,
            _ => Retry::Never,
        }
    }

    /// Whether part of the action may have reached the application
    /// (ADR-0080's `may_have_run` effect).
    #[must_use]
    pub fn may_have_run(&self) -> bool {
        matches!(self, Self::HandlerPanicked { .. })
    }

    /// The kind of handle a `gone` or `unknown_handle` names.
    #[must_use]
    pub fn handle_kind(&self) -> Option<&'static str> {
        match self {
            Self::PresentationGone => Some("window"),
            Self::NodeNotFound { .. } => Some("element"),
            _ => None,
        }
    }

    fn from_read(error: WireReadError, issued: bool) -> Self {
        match error {
            WireReadError::NoTree => Self::NoTreeYet,
            WireReadError::NotFound { element } => Self::NodeNotFound { element, issued },
            _ => Self::Malformed,
        }
    }

    fn from_act(error: WireActionError, issued: bool) -> Self {
        match error {
            WireActionError::NoTree => Self::NoTreeYet,
            WireActionError::NotFound { element } => Self::NodeNotFound { element, issued },
            WireActionError::Disabled { element } => Self::Disabled { element },
            WireActionError::ActionUnsupported { element, action } => {
                Self::ActionUnsupported { element, action }
            }
            WireActionError::InvalidArgument { reason } => Self::InvalidArgument { reason },
            _ => Self::Malformed,
        }
    }

    fn from_send(error: CommandSendError) -> Self {
        match error {
            CommandSendError::ChannelFull { .. } => Self::InboxFull,
            CommandSendError::OwnerGone { .. } => Self::RealmGone,
        }
    }
}

impl From<AgentError> for AgentFault {
    /// The development seam's view of the error: its code, retry advice and
    /// effect, and a message that names element ids only.
    fn from(error: AgentError) -> Self {
        let kind = match &error {
            AgentError::PresentationGone => Some(HandleKind::Window),
            AgentError::NodeNotFound { .. } => Some(HandleKind::Element),
            _ => None,
        };
        let fault = AgentFault::new(error.code(), error.retry(), error.to_string())
            .with_may_have_run(error.may_have_run());
        match kind {
            Some(kind) => fault.with_kind(kind),
            None => fault,
        }
    }
}

/// The owner's half of an [`AgentReply`].
pub(super) type ReplySender<T> = Sender<Result<T, AgentError>>;

/// What an agent's reads have reported, enough to tell a handle that is
/// gone from one that was never issued, in memory bounded by the render
/// slots the presentation has used rather than by how many elements came and
/// went.
///
/// An element handle is the generational accessibility id of its render
/// object: the render slot in the low 32 bits, the slot's generation in the
/// high 32 (`RenderId::new_gen`), and a removed element's slot is reused only
/// under a higher generation. So the record keeps, per slot, the newest
/// generation a read reported: a handle at or below it was issued (the
/// element it names is gone if the tree no longer shows it), and one above it,
/// or at a slot no read reported, was not.
#[derive(Debug, Default)]
pub(super) struct IssuedHandles {
    newest: HashMap<u32, NonZeroU32>,
}

impl IssuedHandles {
    fn split(element: ElementId) -> (u32, Option<NonZeroU32>) {
        let packed = element.get();
        let slot = u32::try_from(packed & u64::from(u32::MAX))
            .expect("BUG: the low half of a u64 fits a u32");
        let generation =
            u32::try_from(packed >> 32).expect("BUG: the high half of a u64 fits a u32");
        (slot, NonZeroU32::new(generation))
    }

    pub(super) fn record(&mut self, element: ElementId) {
        let (slot, Some(generation)) = Self::split(element) else {
            return;
        };
        let newest = self.newest.entry(slot).or_insert(generation);
        *newest = (*newest).max(generation);
    }

    pub(super) fn was_issued(&self, element: ElementId) -> bool {
        let (slot, generation) = Self::split(element);
        generation.is_some_and(|generation| {
            self.newest
                .get(&slot)
                .is_some_and(|newest| generation <= *newest)
        })
    }

    #[cfg(test)]
    pub(super) fn slots(&self) -> usize {
        self.newest.len()
    }
}

/// Records the element handles a read reported, so a later call naming one
/// that is no longer in the tree answers `gone` rather than `unknown_handle`.
fn record_issued(tree: &Tree, issued: &Mutex<IssuedHandles>) {
    fn walk(nodes: &[flui_protocol::Node], issued: &mut IssuedHandles) {
        for node in nodes {
            issued.record(node.id);
            walk(&node.children, issued);
        }
    }
    walk(&tree.roots, &mut issued.lock());
}

/// The answer to one [`SemanticsAgent::read`] or [`SemanticsAgent::act`],
/// filled by the realm's owner at its next drain.
#[must_use = "a reply carries the result of the read or action"]
#[derive(Debug)]
pub struct AgentReply<T> {
    rx: Receiver<Result<T, AgentError>>,
    /// The agent's record of issued handles, and not its semantics handle:
    /// an unanswered reply does not keep collection on.
    issued: Arc<Mutex<IssuedHandles>>,
    /// What the agent learns from a successful answer.
    record: fn(&T, &Mutex<IssuedHandles>),
    delivered: bool,
}

impl<T> AgentReply<T> {
    fn deliver(&mut self, received: Result<T, AgentError>) -> Result<T, AgentError> {
        self.delivered = true;
        if let Ok(answer) = &received {
            (self.record)(answer, &self.issued);
        }
        received
    }

    /// The answer, if the owner has sent it; `None` while it has not, and
    /// after the answer has been taken once.
    pub fn try_take(&mut self) -> Option<Result<T, AgentError>> {
        if self.delivered {
            return None;
        }
        match self.rx.try_recv() {
            Ok(received) => Some(self.deliver(received)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(self.deliver(Err(AgentError::RealmGone))),
        }
    }

    /// The answer, waiting up to `timeout` for it; `None` if it has not come
    /// by then, and after the answer has been taken once. Never call this on
    /// the realm's owner thread: the owner answers only at its drain.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<Result<T, AgentError>> {
        if self.delivered {
            return None;
        }
        match self.rx.recv_timeout(timeout) {
            Ok(received) => Some(self.deliver(received)),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(self.deliver(Err(AgentError::RealmGone))),
        }
    }
}

impl<T: Send + 'static> PendingAnswer<T> for AgentReply<T> {
    fn try_take(&mut self) -> Option<Result<T, AgentFault>> {
        AgentReply::try_take(self).map(|answer| answer.map_err(AgentFault::from))
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Option<Result<T, AgentFault>> {
        AgentReply::recv_timeout(self, timeout).map(|answer| answer.map_err(AgentFault::from))
    }
}

/// A capability to read one presentation's semantics tree and act on its
/// elements, from any thread, through the realm's owner inbox.
///
/// Vended by [`UiRealm::semantics_agent`]. `Clone + Send + Sync`; clones
/// share one semantics handle and one record of issued element handles.
///
/// Its [`ElementId`]s are scoped to the one presentation it was vended for:
/// they are render identities, so another window's agent can report the same
/// `e<n>` for a different element. A server that addresses several windows
/// through one session keeps its own handle table over these (ADR-0095 §3).
#[derive(Debug, Clone)]
pub struct SemanticsAgent {
    sender: UiCommandSender,
    /// Keeps semantics collected while any clone is alive.
    _semantics: Arc<SemanticsHandle>,
    /// What this agent's reads reported. Touched only on the agent's side,
    /// never by the owner.
    issued: Arc<Mutex<IssuedHandles>>,
}

impl SemanticsAgent {
    fn reply<T>(&self, record: fn(&T, &Mutex<IssuedHandles>)) -> (ReplySender<T>, AgentReply<T>) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        (
            tx,
            AgentReply {
                rx,
                issued: Arc::clone(&self.issued),
                record,
                delivered: false,
            },
        )
    }

    /// Read the presentation's tree as ADR-0080 wire nodes, as of the last
    /// committed frame.
    ///
    /// # Errors
    ///
    /// [`AgentError::InboxFull`] or [`AgentError::RealmGone`] when the
    /// request cannot be enqueued; the reply carries the rest.
    pub fn read(&self, query: ReadQuery) -> Result<AgentReply<Tree>, AgentError> {
        let issued = query
            .root
            .is_none_or(|root| self.issued.lock().was_issued(root));
        let (reply, answer) = self.reply(record_issued);
        self.sender
            .send(UiCommand::SemanticsRead {
                presentation_id: self.sender.presentation_id,
                query,
                issued,
                reply,
            })
            .map_err(AgentError::from_send)?;
        Ok(answer)
    }

    /// Perform `request` on its element, checked against the element as the
    /// last committed frame shows it.
    ///
    /// The reply comes at the owner's next drain. `Ok` means the action was
    /// delivered to the element's semantics handler, not that its effect has
    /// happened: a handler may do its work later. A `GestureDetector`, behind
    /// most tappable widgets, runs a semantics tap in the frame after the
    /// drain, so its effect shows in a read two frames on, and a tap it drops
    /// (the widget unmounted first) still answered `Ok`. Read the tree for
    /// the effect rather than acting again, since a second action is a second
    /// press.
    ///
    /// # Errors
    ///
    /// [`AgentError::InboxFull`] or [`AgentError::RealmGone`] when the
    /// request cannot be enqueued; the reply carries the rest.
    pub fn act(&self, request: ActionRequest) -> Result<AgentReply<()>, AgentError> {
        let issued = self.issued.lock().was_issued(request.element);
        let (reply, answer) = self.reply(|(), _| {});
        self.sender
            .send(UiCommand::SemanticsAgentAction {
                presentation_id: self.sender.presentation_id,
                request,
                issued,
                reply,
            })
            .map_err(AgentError::from_send)?;
        Ok(answer)
    }
}

/// The development seam's port: a [`flui_view::dev_agent::AgentWindow`]
/// reads and acts through the agent the realm vended for it.
impl AgentPort for SemanticsAgent {
    fn read(&self, query: ReadQuery) -> Result<AgentAnswer<Tree>, AgentFault> {
        SemanticsAgent::read(self, query)
            .map(flui_view::__runtime::agent_answer)
            .map_err(AgentFault::from)
    }

    fn act(&self, request: ActionRequest) -> Result<AgentAnswer<()>, AgentFault> {
        SemanticsAgent::act(self, request)
            .map(flui_view::__runtime::agent_answer)
            .map_err(AgentFault::from)
    }
}

/// Sends `result` to an agent that may have stopped waiting. A receiver that
/// is gone is traced by the element and code alone: the answer can carry
/// labels and values, which stay out of the log.
pub(super) fn send_reply<T>(
    reply: &ReplySender<T>,
    element: Option<ElementId>,
    result: Result<T, AgentError>,
) {
    let code = result.as_ref().err().map(AgentError::code);
    if let Err(TrySendError::Disconnected(_) | TrySendError::Full(_)) = reply.try_send(result) {
        tracing::trace!(
            element = element.map(ElementId::get),
            code = code.map(ErrorCode::name),
            "dropping a semantics agent reply nobody waits for"
        );
    }
}

impl UiRealm {
    /// A capability to read `presentation`'s semantics tree and act on its
    /// elements, or `None` when this realm does not host it.
    ///
    /// Vending one turns semantics collection on for the presentation and
    /// requests a frame, which builds the first tree; until it commits, a
    /// read answers [`AgentError::NoTreeYet`].
    #[must_use]
    pub fn semantics_agent(&self, presentation: PresentationId) -> Option<SemanticsAgent> {
        let state = self.presentations.get(presentation)?;
        let handle = state.semantics_host().ensure_semantics();
        let mut sender = self.sender_prototype.clone();
        sender.presentation_id = presentation;
        self.request_redraw_for(state);
        Some(SemanticsAgent {
            sender,
            _semantics: Arc::new(handle),
            issued: Arc::new(Mutex::new(IssuedHandles::default())),
        })
    }

    /// The development agent's window for `presentation`, or `None` when this
    /// realm does not host it.
    ///
    /// The first call vends a [`SemanticsAgent`] (turning collection on and
    /// requesting a frame, as [`Self::semantics_agent`] does) and keeps it on
    /// the presentation; later calls hand out the same agent, so every
    /// window handle shares one semantics handle and one record of issued
    /// element handles. The returned [`AgentWindow`] holds the agent weakly:
    /// closing the presentation drops the agent, collection stops on the next
    /// frame, and every call on the window answers `gone`.
    #[must_use]
    pub fn dev_agent_window(&self, presentation: PresentationId) -> Option<AgentWindow> {
        let state = self.presentations.get(presentation)?;
        let kept = state.dev_agent.borrow().as_ref().map(Arc::clone);
        let agent = if let Some(agent) = kept {
            agent
        } else {
            let agent = Arc::new(self.semantics_agent(presentation)?);
            *state.dev_agent.borrow_mut() = Some(Arc::clone(&agent));
            agent
        };
        let window = WindowId::from_u64(presentation.as_u64())
            .expect("BUG: a presentation id packs a non-zero generation");
        let port: std::sync::Weak<dyn AgentPort> = Arc::downgrade(&agent) as _;
        Some(flui_view::__runtime::agent_window(window, port))
    }

    /// The owner half of [`SemanticsAgent::read`].
    pub(super) fn serve_semantics_read(
        &self,
        presentation: PresentationId,
        query: &ReadQuery,
        issued: bool,
    ) -> Result<Tree, AgentError> {
        let state = self
            .presentations
            .get(presentation)
            .ok_or(AgentError::PresentationGone)?;
        let window = WindowId::from_u64(presentation.as_u64())
            .expect("BUG: a presentation id packs a non-zero generation");
        state.pipeline().with(|pipeline| {
            let ratio = DevicePixelRatio::new(pipeline.device_pixel_ratio())
                .unwrap_or(DevicePixelRatio::ONE);
            pipeline
                .semantics_owner()
                .ok_or(AgentError::NoTreeYet)?
                .read_wire(query, Placement::new(ratio, window))
                .map_err(|error| AgentError::from_read(error, issued))
        })
    }

    /// The owner half of [`SemanticsAgent::act`]: resolves the wire action
    /// against the committed tree, then invokes the handler through the path
    /// an assistive technology's action takes. The caller contains a panic;
    /// `reached_handler` is set just before the handler is invoked, so the
    /// caller can tell a handler's panic from one while resolving.
    pub(super) fn serve_semantics_act(
        &self,
        presentation: PresentationId,
        request: &ActionRequest,
        issued: bool,
        reached_handler: &Cell<bool>,
    ) -> Result<(), AgentError> {
        let state = self
            .presentations
            .get(presentation)
            .ok_or(AgentError::PresentationGone)?;
        let resolved = state.pipeline().with(|pipeline| {
            pipeline
                .semantics_owner()
                .ok_or(AgentError::NoTreeYet)?
                .resolve_wire_action(request)
                .map_err(|error| AgentError::from_act(error, issued))
        })?;
        let element = request.element;
        reached_handler.set(true);
        state
            .dispatch_semantics_action(resolved)
            .map_err(|error| match error {
                SemanticsActionError::PresentationClosed => AgentError::PresentationGone,
                SemanticsActionError::SemanticsUnavailable => AgentError::NoTreeYet,
                SemanticsActionError::UnsupportedAction { .. } => AgentError::ActionUnsupported {
                    element,
                    action: request.action,
                },
                _ => AgentError::NodeNotFound { element, issued },
            })
    }
}
