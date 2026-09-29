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
//! collection stop on the next frame.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, TrySendError};
use flui_foundation::PresentationId;
use flui_foundation::geometry::DevicePixelRatio;
use flui_protocol::{
    ActionName, ActionRequest, ElementId, ErrorCode, ReadQuery, Retry, Tree, WindowId,
};
use flui_semantics::{Placement, SemanticsActionError, WireActionError, WireReadError};
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
    /// The element's handler panicked. It may have run in part; the panic
    /// continues on the owner thread.
    #[error("the handler of element {element} panicked")]
    HandlerPanicked {
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
            Self::Malformed | Self::HandlerPanicked { .. } => ErrorCode::Platform,
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

    fn from_read(error: WireReadError) -> Self {
        match error {
            WireReadError::NoTree => Self::NoTreeYet,
            WireReadError::NotFound { element } => Self::NodeNotFound {
                element,
                issued: true,
            },
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

/// The owner's half of an [`AgentReply`].
pub(super) type ReplySender<T> = Sender<Result<T, AgentError>>;

/// Records the element handles a read reported, so a later action on one
/// that is no longer in the tree answers `gone` rather than `unknown_handle`.
fn record_issued(tree: &Tree, shared: &Shared) {
    fn walk(nodes: &[flui_protocol::Node], issued: &mut HashSet<ElementId>) {
        for node in nodes {
            issued.insert(node.id);
            walk(&node.children, issued);
        }
    }
    walk(&tree.roots, &mut shared.issued.lock());
}

/// The answer to one [`SemanticsAgent::read`] or [`SemanticsAgent::act`],
/// filled by the realm's owner at its next drain.
#[must_use = "a reply carries the result of the read or action"]
#[derive(Debug)]
pub struct AgentReply<T> {
    rx: Receiver<Result<T, AgentError>>,
    shared: Arc<Shared>,
    /// What the agent learns from a successful answer.
    record: fn(&T, &Shared),
    delivered: bool,
}

impl<T> AgentReply<T> {
    fn deliver(&mut self, received: Result<T, AgentError>) -> Result<T, AgentError> {
        self.delivered = true;
        if let Ok(answer) = &received {
            (self.record)(answer, &self.shared);
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

/// What every clone of one agent shares.
#[derive(Debug)]
struct Shared {
    /// Keeps semantics collected while any clone is alive.
    _semantics: SemanticsHandle,
    /// Every element handle a read has reported to this agent, so a handle
    /// that is not in the tree can be told `gone` from `unknown_handle`.
    /// Touched only on the agent's side, never by the owner.
    issued: Mutex<HashSet<ElementId>>,
}

/// A capability to read one presentation's semantics tree and act on its
/// elements, from any thread, through the realm's owner inbox.
///
/// Vended by [`UiRealm::semantics_agent`]. `Clone + Send + Sync`; clones
/// share one semantics handle and one record of issued element handles.
#[derive(Debug, Clone)]
pub struct SemanticsAgent {
    sender: UiCommandSender,
    shared: Arc<Shared>,
}

impl SemanticsAgent {
    fn reply<T>(&self, record: fn(&T, &Shared)) -> (ReplySender<T>, AgentReply<T>) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        (
            tx,
            AgentReply {
                rx,
                shared: Arc::clone(&self.shared),
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
        let (reply, answer) = self.reply(record_issued);
        self.sender
            .send(UiCommand::SemanticsRead {
                presentation_id: self.sender.presentation_id,
                query,
                reply,
            })
            .map_err(AgentError::from_send)?;
        Ok(answer)
    }

    /// Perform `request` on its element, checked against the element as the
    /// last committed frame shows it.
    ///
    /// The reply comes at the owner's next drain, once the element's handler
    /// has been invoked; what the handler changes shows in a read after the
    /// next frame.
    ///
    /// # Errors
    ///
    /// [`AgentError::InboxFull`] or [`AgentError::RealmGone`] when the
    /// request cannot be enqueued; the reply carries the rest.
    pub fn act(&self, request: ActionRequest) -> Result<AgentReply<()>, AgentError> {
        let issued = self.shared.issued.lock().contains(&request.element);
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
            shared: Arc::new(Shared {
                _semantics: handle,
                issued: Mutex::new(HashSet::new()),
            }),
        })
    }

    /// The owner half of [`SemanticsAgent::read`].
    pub(super) fn serve_semantics_read(
        &self,
        presentation: PresentationId,
        query: &ReadQuery,
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
                .map_err(AgentError::from_read)
        })
    }

    /// The owner half of [`SemanticsAgent::act`]: resolves the wire action
    /// against the committed tree, then invokes the handler through the path
    /// an assistive technology's action takes. The caller contains a
    /// handler's panic.
    pub(super) fn serve_semantics_act(
        &self,
        presentation: PresentationId,
        request: &ActionRequest,
        issued: bool,
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
