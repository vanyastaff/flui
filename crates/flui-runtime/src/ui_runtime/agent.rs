//! The owner's side of the development agent (ADR-0095 §3, ADR-0239): the port a presentation's
//! [`AgentWindow`] calls through, and the owner's halves of what it asks.
//!
//! [`UiRuntime::dev_agent_window`] vends a presentation's [`AgentWindow`]. Its port never
//! touches a tree: it enqueues one command on the UI runtime's bounded inbox, and the owner
//! answers at its next drain, a frame boundary ([`agent_serve`](super::agent_serve)). A read
//! reflects the last committed frame's semantics; an action reaches the same handler an
//! assistive technology's does, through `PresentationState::dispatch_semantics_action`.
//!
//! While any clone of the window is alive the presentation collects semantics (one
//! [`SemanticsHandle`] they share), so the tree an agent reads is the tree assistive technology
//! is published. Dropping the last clone lets collection stop on the next frame; a request still
//! unanswered does not keep it on.

use std::cell::RefCell;
use std::collections::HashMap;
use std::num::{NonZeroU32, NonZeroU64};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use flui_foundation::PresentationId;
use flui_foundation::geometry::DevicePixelRatio;
use flui_foundation::panic::FirstPanic;
use flui_protocol::{
    ActOutcome, ActionName, ActionRequest, Actionability, ActionabilityParams, Effect, EffectKind,
    EffectUnit, ElementId, ElementRef, ErrorCode, ErrorObject, EventFilter, HandleKind, InputPath,
    LocateParams, Matches, ReadQuery, RecordParams, Retry, Snapshot, SnapshotReading, Tree,
    WaitOutcome, WindowId,
};
use flui_rendering::binding::{RendererBinding as _, ViewHitTest};
use flui_semantics::{
    Placement, SemanticsActionError, TreeSnapshot, WireActionError, WireLocateError, WireReadError,
};
use flui_view::__runtime::{AgentPort, LeaseId, Reply, WindowCall};
use flui_view::dev_agent::{AgentAnswer, AgentEvents, AgentRecording, AgentWindow};
use parking_lot::{Mutex, RwLock};

use super::UiRuntime;
use super::agent_serve::PanicAnswer;
use super::commands::{CommandSendError, UiCommand, UiCommandSender};
use crate::presentation::PresentationState;
use crate::semantics_host::SemanticsHandle;

/// Why an agent's request failed, on the owner. Each variant has its ADR-0080 code and retry
/// advice, and becomes the protocol's error object with [`Self::into_error`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AgentError {
    /// The UI runtime's inbox is full; retry soon.
    #[error("the ui_runtime's command inbox is full")]
    InboxFull,
    /// The UI runtime is gone, or dropped the command without answering it.
    #[error("the ui_runtime has shut down")]
    RuntimeGone,
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
        /// gone, if not the handle was never issued to this agent. Exact but
        /// for one bound: once a render slot has shown an agent more than
        /// `IssuedHandles::MAX_RUNS` separate runs of generations, the oldest
        /// gaps between them count as reported.
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
    /// A render object panicked in the hit-test at the element's action
    /// point; the panic continues on the owner thread. Nothing reached the
    /// app: a hit-test only reads.
    #[error("the hit-test at element {element} panicked")]
    HitTestPanicked {
        /// The element addressed.
        element: ElementId,
    },
    /// A render object or view panicked while its tree was being read; the panic continues on
    /// the owner thread. A read changes nothing.
    #[error("reading the tree panicked")]
    InspectPanicked,
    /// The presentation already holds as many waits as it admits; one ends before another is
    /// taken.
    #[error("the window already holds as many waits as it admits")]
    TooManyWaits,
    /// The presentation already has as many live subscriptions as it admits; one ends (its
    /// stream is dropped) before another is taken.
    #[error("the window already has as many event subscriptions as it admits")]
    TooManySubscriptions,
    /// The UI runtime already holds as many recordings as it admits; one ends (its reader is
    /// dropped, or stops it) before another is taken.
    #[error("the window's runtime already holds as many recordings as it admits")]
    TooManyRecordings,
    /// The runtime cannot start a recording: `reason` says why.
    #[error("{reason}")]
    RecordingRefused {
        /// Why, for people.
        reason: &'static str,
    },
    /// The render tree was busy (a layout was in progress) when the hit-test
    /// asked for it; the same call can succeed after the frame.
    #[error("the render tree was busy")]
    RenderTreeBusy,
    /// A tree was in the middle of something when an inspection asked for it (named in the
    /// message, such as a frame having the render tree checked out); the same call can succeed
    /// after it.
    #[error("the tree is busy {0}")]
    TreeBusy(&'static str),
    /// An inspection's root is not in its tree: it was removed, or its slot holds a newer node.
    #[error("{node} is gone")]
    NodeGone {
        /// The root asked for, as a handle.
        node: String,
    },
    /// A node reference names a window that is not this one, or is closed: the node it names is
    /// never looked up here.
    #[error("the node reference names another window")]
    UnknownPresentation {
        /// The window the reference names.
        window: WindowId,
    },
    /// A locator matches no element now.
    #[error("the locator matches no element now")]
    NoMatch,
    /// A strict locator matches several elements.
    #[error("the locator matches {matched} elements")]
    Ambiguous {
        /// How many it matched, at most the candidates looked at.
        matched: usize,
        /// An outline of each match, for the caller to tell them apart.
        candidates: Vec<String>,
    },
    /// The window does not answer this request; `capability` names what does, or the method
    /// this build's windows do not answer yet.
    #[error("{reason}")]
    NotSupported {
        /// The method or capability that does, or that is not answered.
        capability: &'static str,
        /// Why, for people.
        reason: &'static str,
    },
    /// Serving the request panicked outside the steps that name their own failure; the panic
    /// continues on the owner thread.
    #[error("the window panicked while answering")]
    Panicked,
    /// The window already has as much injected input queued as it keeps; one injection ends
    /// before another is taken.
    #[error("the window already has as much injected input queued as it keeps")]
    InputQueueFull,
    /// The input lease the call was asked through was released before the call ran.
    #[error("the input lease it was asked through was released before it ran")]
    LeaseReleased,
    /// An injection stopped part of the way through: `played` of its `total` ticks went out
    /// before `failure` stopped it.
    #[error("{failure} (after {played} of {total} ticks)")]
    InputPartial {
        /// What stopped it.
        failure: Box<AgentError>,
        /// The ticks that went out in full.
        played: u32,
        /// The ticks it had.
        total: u32,
    },
}

impl AgentError {
    /// The ADR-0080 code an agent branches on.
    #[must_use]
    pub(crate) fn code(&self) -> ErrorCode {
        match self {
            Self::InboxFull
            | Self::NoTreeYet
            | Self::RenderTreeBusy
            | Self::TooManyWaits
            | Self::TooManySubscriptions
            | Self::TooManyRecordings
            | Self::InputQueueFull
            | Self::TreeBusy(_) => ErrorCode::Busy,
            Self::InputPartial { failure, .. } => failure.code(),
            Self::RuntimeGone => ErrorCode::ShuttingDown,
            Self::PresentationGone
            | Self::LeaseReleased
            | Self::NodeNotFound { issued: true, .. }
            | Self::NodeGone { .. } => ErrorCode::Gone,
            Self::NodeNotFound { issued: false, .. } | Self::UnknownPresentation { .. } => {
                ErrorCode::UnknownHandle
            }
            Self::ActionUnsupported { .. } => ErrorCode::ActionUnsupported,
            Self::Disabled { .. } => ErrorCode::Disabled,
            Self::InvalidArgument { .. } => ErrorCode::InvalidArgument,
            Self::NoMatch => ErrorCode::NotFound,
            Self::Ambiguous { .. } => ErrorCode::Ambiguous,
            Self::NotSupported { .. } => ErrorCode::NotSupported,
            Self::Malformed
            | Self::HandlerPanicked { .. }
            | Self::ResolvePanicked { .. }
            | Self::HitTestPanicked { .. }
            | Self::InspectPanicked
            | Self::Panicked
            | Self::RecordingRefused { .. } => ErrorCode::Platform,
        }
    }

    /// Whether the same call can succeed later.
    #[must_use]
    pub(crate) fn retry(&self) -> Retry {
        match self {
            Self::InboxFull
            | Self::NoTreeYet
            | Self::RenderTreeBusy
            | Self::TooManyWaits
            | Self::TooManySubscriptions
            | Self::TooManyRecordings
            | Self::InputQueueFull
            | Self::TreeBusy(_) => Retry::Soon,
            Self::NoMatch => Retry::WhenAppears,
            _ => Retry::Never,
        }
    }

    /// Whether part of the action may have reached the application
    /// (ADR-0080's `may_have_run` effect).
    #[must_use]
    pub(crate) fn may_have_run(&self) -> bool {
        matches!(self, Self::HandlerPanicked { .. })
    }

    /// The element this failure is about, if it is about one.
    #[must_use]
    pub(crate) fn element(&self) -> Option<ElementId> {
        match self {
            Self::NodeNotFound { element, .. }
            | Self::ActionUnsupported { element, .. }
            | Self::Disabled { element }
            | Self::HandlerPanicked { element }
            | Self::ResolvePanicked { element }
            | Self::HitTestPanicked { element } => Some(*element),
            _ => None,
        }
    }

    /// The protocol's error object for this failure in `window`: its code, retry advice and
    /// effect, the handle a `gone` or `unknown_handle` is about, and a message that names ids
    /// only, never a label or a value.
    pub(crate) fn into_error(self, window: WindowId) -> ErrorObject {
        let (code, retry, may_have_run) = (self.code(), self.retry(), self.may_have_run());
        let message = self.to_string();
        let mut error =
            match self {
                // Part of the input went out: the effect counts the ticks.
                Self::InputPartial {
                    failure,
                    played,
                    total,
                } => {
                    return failure.into_error(window).with_effect(Effect::partial(
                        played,
                        total,
                        EffectUnit::Tick,
                    ));
                }
                Self::Ambiguous { candidates, .. } => ErrorObject::ambiguous(message, candidates),
                Self::PresentationGone => ErrorObject::new(code, message)
                    .with_handle(HandleKind::Window, window.to_string()),
                Self::UnknownPresentation { window: named } => ErrorObject::new(code, message)
                    .with_handle(HandleKind::Window, named.to_string()),
                Self::NodeNotFound { element, .. } => ErrorObject::new(code, message)
                    .with_handle(HandleKind::Element, element.to_string()),
                Self::NotSupported { capability, .. } => {
                    let mut error = ErrorObject::new(code, message);
                    error.capability = Some(capability.to_owned());
                    error
                }
                _ => ErrorObject::new(code, message),
            }
            .with_retry(retry);
        if may_have_run {
            error = error.with_effect(Effect::new(EffectKind::MayHaveRun));
        }
        error
    }

    pub(super) fn from_read(error: WireReadError, issued: bool) -> Self {
        match error {
            WireReadError::NoTree => Self::NoTreeYet,
            WireReadError::NotFound { element } => Self::NodeNotFound { element, issued },
            _ => Self::Malformed,
        }
    }

    pub(super) fn from_unscoped_read(error: WireReadError) -> Self {
        match error {
            WireReadError::NoTree => Self::NoTreeYet,
            _ => Self::Malformed,
        }
    }

    pub(super) fn from_locate(error: WireLocateError) -> Self {
        match error {
            WireLocateError::NoTree => Self::NoTreeYet,
            // `locate_wire` lists matches and never answers `NotFound` or
            // `Ambiguous`: resolving one element is `semantics.act`'s, over
            // the matches it lists.
            _ => Self::Malformed,
        }
    }

    pub(super) fn from_act(error: WireActionError, issued: bool) -> Self {
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

    pub(super) fn from_send(error: CommandSendError) -> Self {
        match error {
            CommandSendError::ChannelFull { .. } => Self::InboxFull,
            CommandSendError::OwnerGone { .. } => Self::RuntimeGone,
        }
    }
}

/// The window a presentation is on the wire.
pub(super) fn window_of(presentation: PresentationId) -> WindowId {
    WindowId::from_u64(presentation.as_u64())
        .expect("BUG: a presentation id packs a non-zero generation")
}

/// Where the owner answers one request: the seam's reply, with what the owner records from an
/// answer and the window its errors name. Owner thread only. Clones answer the same request,
/// and the first answer sent is the one taken.
pub(crate) struct ReplySender<T> {
    reply: Reply<T>,
    window: WindowId,
    /// The presentation's record of the element handles its answers reported.
    issued: Rc<RefCell<IssuedHandles>>,
    record: fn(&T, &mut IssuedHandles),
}

impl<T> Clone for ReplySender<T> {
    fn clone(&self) -> Self {
        Self {
            reply: self.reply.clone(),
            window: self.window,
            issued: Rc::clone(&self.issued),
            record: self.record,
        }
    }
}

impl<T> ReplySender<T> {
    pub(super) fn new(
        reply: Reply<T>,
        window: WindowId,
        issued: Rc<RefCell<IssuedHandles>>,
        record: fn(&T, &mut IssuedHandles),
    ) -> Self {
        Self {
            reply,
            window,
            issued,
            record,
        }
    }

    /// Whether anyone can still take the answer: a request the owner holds whose asker is gone
    /// is let go.
    pub(crate) fn is_awaited(&self) -> bool {
        self.reply.is_awaited()
    }
}

impl<T> std::fmt::Debug for ReplySender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplySender")
            .field("window", &self.window)
            .field("awaited", &self.is_awaited())
            .finish_non_exhaustive()
    }
}

/// The record of element handles `state`'s answers reported, shared by every reply.
pub(super) fn issued_of(state: &PresentationState) -> Rc<RefCell<IssuedHandles>> {
    state
        .dev_agent
        .borrow()
        .as_ref()
        .map(|slot| Rc::clone(&slot.issued))
        .unwrap_or_default()
}

/// What an agent's reads have reported, enough to tell a handle that is
/// gone from one that was never issued, in memory bounded by the render
/// slots the presentation has used rather than by how many elements came and
/// went.
///
/// An element handle is the generational accessibility id of its render
/// object: the render slot in the low 32 bits, the slot's generation in the
/// high 32 (`RenderId::new_gen`), and a removed element's slot is reused only
/// under a higher generation. So the record keeps, per slot, the runs of
/// consecutive generations reads reported, as inclusive ranges: a handle in
/// one was issued (the element it names is gone if the tree no longer shows
/// it), and one outside every run, or at a slot no read reported, was not.
///
/// A slot's generations reported one read after another form one run, so a
/// slot usually holds one or two. Each generation that came and went between
/// two reads without either reporting it splits a run; past
/// [`Self::MAX_RUNS`] a slot's two oldest runs merge, and the old gap between
/// them counts as issued from then on (`gone` rather than `unknown_handle`).
/// Recent gaps stay exact, and the record stays bounded by the slots.
#[derive(Debug, Default)]
pub(super) struct IssuedHandles {
    runs: HashMap<u32, Vec<(u32, u32)>>,
}

impl IssuedHandles {
    /// The most runs of reported generations one slot keeps.
    pub(super) const MAX_RUNS: usize = 16;

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
        let generation = generation.get();
        let runs = self.runs.entry(slot).or_default();
        // The first run that ends at or after the generation's predecessor:
        // the only runs the generation can fall in, extend or join.
        let at = runs.partition_point(|&(_, end)| u64::from(end) + 1 < u64::from(generation));
        match runs.get(at).copied() {
            Some((start, end)) if start <= generation && generation <= end => return,
            Some((start, end)) if u64::from(end) + 1 == u64::from(generation) => {
                runs[at] = (start, generation);
                if let Some(&(next_start, next_end)) = runs.get(at + 1)
                    && u64::from(generation) + 1 == u64::from(next_start)
                {
                    runs[at] = (start, next_end);
                    runs.remove(at + 1);
                }
            }
            Some((start, end)) if u64::from(generation) + 1 == u64::from(start) => {
                runs[at] = (generation, end);
            }
            _ => runs.insert(at, (generation, generation)),
        }
        if runs.len() > Self::MAX_RUNS {
            let (_, end) = runs.remove(1);
            runs[0].1 = end;
        }
    }

    pub(super) fn was_issued(&self, element: ElementId) -> bool {
        let (slot, generation) = Self::split(element);
        generation.is_some_and(|generation| {
            let generation = generation.get();
            self.runs.get(&slot).is_some_and(|runs| {
                let at = runs.partition_point(|&(_, end)| end < generation);
                runs.get(at).is_some_and(|&(start, _)| start <= generation)
            })
        })
    }

    #[cfg(test)]
    pub(super) fn slots(&self) -> usize {
        self.runs.len()
    }

    #[cfg(test)]
    pub(super) fn runs(&self) -> usize {
        self.runs.values().map(Vec::len).sum()
    }
}

/// Records the element handles a read reported, so a later call naming one
/// that is no longer in the tree answers `gone` rather than `unknown_handle`.
pub(super) fn record_issued(tree: &Tree, issued: &mut IssuedHandles) {
    fn walk(nodes: &[flui_protocol::Node], issued: &mut IssuedHandles) {
        for node in nodes {
            issued.record(node.id);
            walk(&node.children, issued);
        }
    }
    walk(&tree.roots, issued);
}

/// Records the element an action reported acting on, which a locator may have named, and the
/// handles its reading reported, as a read does.
pub(super) fn record_issued_outcome(outcome: &ActOutcome, issued: &mut IssuedHandles) {
    if let Some(element) = outcome.element {
        issued.record(element.element);
    }
    match &outcome.snapshot {
        Some(SnapshotReading::Full(tree)) => record_issued(tree, issued),
        Some(SnapshotReading::Delta(delta)) => {
            for node in &delta.appeared {
                issued.record(node.id);
            }
            for change in &delta.changed {
                issued.record(change.node.id);
            }
        }
        _ => {}
    }
}

/// Records the element handles a locate reported, as a read does.
pub(super) fn record_issued_matches(matches: &Matches, issued: &mut IssuedHandles) {
    for found in &matches.matches {
        issued.record(found.element.element);
    }
}

/// Records the element handles a wait's last observation reported.
pub(super) fn record_issued_wait(outcome: &WaitOutcome, issued: &mut IssuedHandles) {
    for node in &outcome.observation.matched {
        issued.record(node.id);
    }
}

/// What a presentation keeps for the development agent: see
/// [`UiRuntime::dev_agent_window`]. Owner thread only.
pub(crate) struct DevAgentSlot {
    /// The port every window handle answers through. Closed, and dropped,
    /// with the presentation.
    port: Arc<DevAgentPort>,
    /// The semantics handle the window handles share; dead once the hook
    /// has dropped them all.
    collecting: Weak<SemanticsHandle>,
    /// The element handles this presentation's answers reported.
    issued: Rc<RefCell<IssuedHandles>>,
    /// The input the window was delivered since the slot was made.
    pub(super) journal: super::agent_journal::InputJournal,
}

/// The development seam's port: a [`flui_view::dev_agent::AgentWindow`]
/// enqueues its calls through it, while its presentation is open.
///
/// A call on another thread can hold the port (upgraded from the window's
/// weak reference) past the presentation's close, so the port's lifetime
/// does not say whether the window is open; the flag does, cleared when the
/// presentation's [`DevAgentSlot`] goes.
///
/// Admission and close are serialized: a call enqueues while holding the
/// flag's read lock, and the close takes its write lock, so once the close
/// returns no call is still enqueueing and none that starts later is
/// admitted. The lock is held across a non-blocking enqueue only.
pub(crate) struct DevAgentPort {
    sender: UiCommandSender,
    window: WindowId,
    open: RwLock<bool>,
    /// The last input lease minted; ids are never reused within the port.
    leases: AtomicU64,
    /// Leases let go since the owner last took them: each is released once,
    /// so the list holds at most one entry per lease minted, and the owner
    /// empties it at each drain.
    released: Mutex<Vec<LeaseId>>,
}

// A refusal is the protocol's error object itself, which the window answers with.
// The error object is under the lint's size only where a pointer is 32 bits.
#[cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::result_large_err,
        reason = "the error object is the answer, returned once per call"
    )
)]
impl DevAgentPort {
    /// Enqueue `command` if the window is open, holding admission open for it.
    fn admit(&self, command: UiCommand) -> Result<(), ErrorObject> {
        let admission = {
            let open = self.open.read();
            if !*open {
                return Err(AgentError::PresentationGone.into_error(self.window));
            }
            self.sender.enqueue(command)
        };
        // A wake can re-enter this port or synchronously close its presentation.
        // Only the enqueue belongs inside the close/admission fence.
        self.sender
            .deliver_enqueued(admission)
            .map_err(|error| AgentError::from_send(error).into_error(self.window))
    }

    /// Close admission, waiting for any call already admitted to finish
    /// enqueueing.
    fn close(&self) {
        *self.open.write() = false;
    }
}

impl AgentPort for DevAgentPort {
    fn is_open(&self) -> bool {
        *self.open.read()
    }

    fn call(&self, call: WindowCall) -> Result<(), ErrorObject> {
        self.admit(UiCommand::Agent {
            presentation_id: self.sender.presentation_id,
            call,
        })
    }

    fn subscribe(
        &self,
        filters: Vec<EventFilter>,
    ) -> Result<AgentAnswer<AgentEvents>, ErrorObject> {
        let (reply, answer) = flui_view::__runtime::reply_pair();
        self.admit(UiCommand::AgentSubscribe {
            presentation_id: self.sender.presentation_id,
            filters,
            reply,
        })?;
        Ok(answer)
    }

    fn lease(&self) -> Result<LeaseId, ErrorObject> {
        if !self.is_open() {
            return Err(AgentError::PresentationGone.into_error(self.window));
        }
        let last = self
            .leases
            .try_update(Ordering::AcqRel, Ordering::Acquire, |last| {
                last.checked_add(1)
            })
            .map_err(|_| {
                ErrorObject::new(
                    ErrorCode::Busy,
                    "the window has handed out every input lease",
                )
                .with_retry(Retry::Never)
            })?;
        let minted = last
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .expect("BUG: the update refused the last lease id");
        Ok(LeaseId::new(minted))
    }

    fn release(&self, lease: LeaseId) {
        if !self.is_open() {
            // The presentation released everything as it closed.
            return;
        }
        self.released.lock().push(lease);
        // Recorded first: the wake is the platform's, and a release it fails to deliver is
        // taken at the owner's next drain all the same.
        self.sender.wake_owner();
    }

    fn record(&self, params: RecordParams) -> Result<AgentAnswer<AgentRecording>, ErrorObject> {
        let (reply, answer) = flui_view::__runtime::reply_pair();
        self.admit(UiCommand::AgentRecord {
            presentation_id: self.sender.presentation_id,
            params,
            reply,
        })?;
        Ok(answer)
    }
}

impl DevAgentSlot {
    pub(crate) fn withdraw(&self) {
        self.port.close();
    }

    /// How many leases the window's handles have let go that the owner has not released yet.
    pub(super) fn released_count(&self) -> usize {
        self.port.released.lock().len()
    }

    /// The first `count` leases let go, oldest first, taken to be released.
    pub(super) fn take_released(&self, count: usize) -> Vec<LeaseId> {
        let mut released = self.port.released.lock();
        let count = count.min(released.len());
        released.drain(..count).collect()
    }
}

impl Drop for DevAgentSlot {
    /// The presentation closed: every window handle answers `gone` from
    /// now on, even through a port a call still holds, and nothing is
    /// enqueued for the closed window after this returns.
    fn drop(&mut self) {
        self.port.close();
    }
}

/// Sends `result` to an agent that may have stopped waiting: an answer's element handles are
/// recorded first, and an error becomes the protocol's error object. A receiver that is gone is
/// traced by the element and code alone: the answer can carry labels and values, which stay out
/// of the log.
pub(super) fn send_reply<T>(
    reply: &ReplySender<T>,
    element: Option<ElementId>,
    result: Result<T, AgentError>,
) {
    let code = result.as_ref().err().map(AgentError::code);
    // The element a failure names is traced even where the caller named none.
    let element = element.or_else(|| result.as_ref().err().and_then(AgentError::element));
    let answer = match result {
        Ok(value) => {
            (reply.record)(&value, &mut reply.issued.borrow_mut());
            Ok(value)
        }
        Err(error) => Err(error.into_error(reply.window)),
    };
    if !reply.reply.send(answer) {
        tracing::trace!(
            element = element.map(ElementId::get),
            code = code.map(ErrorCode::name),
            "dropping a development agent reply nobody waits for"
        );
    }
}

/// The most entries one action's delta reports across appeared, gone and
/// changed nodes; past it the delta says `truncated` and the caller reads the
/// tree.
const DELTA_CAP: usize = 256;

/// The most nodes a full reading after an action reports.
const READBACK_NODES: usize = 4096;

/// The most readings one presentation owes at once: past it the oldest is
/// answered as failed rather than kept for a frame that may never come (a
/// hidden window runs none).
const MAX_OWED_READINGS: usize = 256;

/// What serving an action that owes a reading came to.
pub(super) enum Served {
    /// The action ran and nothing is owed.
    Answered(ActOutcome),
    /// The action ran; the reading is owed once its effect has settled.
    Reading(ReadingOwed),
}

/// The reading an action owes, with what the action itself answers.
pub(super) struct ReadingOwed {
    outcome: ActOutcome,
    snapshot: Snapshot,
    /// The tree the action started on, for a delta.
    before: Option<TreeSnapshot>,
}

impl ReadingOwed {
    /// What an action that answered `outcome` owes once a frame has passed: the reading
    /// `snapshot` asks for, against `before` for a delta.
    pub(super) fn new(
        outcome: ActOutcome,
        snapshot: Snapshot,
        before: Option<TreeSnapshot>,
    ) -> Self {
        Self {
            outcome,
            snapshot,
            before,
        }
    }
}

/// The tree an action that asks for `snapshot` starts on: read for a delta, `None` otherwise.
pub(super) fn tree_before(
    state: &PresentationState,
    window: WindowId,
    snapshot: Snapshot,
) -> Result<Option<TreeSnapshot>, AgentError> {
    if snapshot != Snapshot::Delta {
        return Ok(None);
    }
    let placement = placement_of(state, window);
    state
        .pipeline()
        .with(|pipeline| {
            pipeline
                .semantics_owner()
                .ok_or(AgentError::NoTreeYet)?
                .snapshot_wire(placement)
                .map_err(AgentError::from_unscoped_read)
        })
        .map(Some)
}

/// An action's reply, parked on its presentation until a frame has passed.
/// Owner thread only.
pub(crate) struct PendingActRead {
    owed: ReadingOwed,
    reply: ReplySender<ActOutcome>,
}

impl PendingActRead {
    /// The window or the frame is gone before the reading could be taken: the
    /// action still ran, and the outcome says the tree could not be read back.
    pub(crate) fn abandon(mut self) {
        self.owed.outcome.readback_failed = true;
        let element = self.owed.outcome.element.map(|element| element.element);
        send_reply(&self.reply, element, Ok(self.owed.outcome));
    }
}

/// How `state` places its semantics on the wire: its device pixel ratio, and `window`.
fn placement_of(state: &PresentationState, window: WindowId) -> Placement {
    state.pipeline().with(|pipeline| {
        let ratio = DevicePixelRatio::try_new(pipeline.device_pixel_ratio())
            .unwrap_or(DevicePixelRatio::ONE);
        Placement::new(ratio, window)
    })
}

/// The owner half of `semantics.read`: the committed tree as wire nodes.
pub(super) fn read_tree(
    state: &PresentationState,
    window: WindowId,
    query: &ReadQuery,
    issued: bool,
) -> Result<Tree, AgentError> {
    let placement = placement_of(state, window);
    state.pipeline().with(|pipeline| {
        pipeline
            .semantics_owner()
            .ok_or(AgentError::NoTreeYet)?
            .read_wire(query, placement)
            .map_err(|error| AgentError::from_read(error, issued))
    })
}

/// The owner half of `semantics.locate`: every match in the committed tree, or the ones `pick`
/// and `max_matches` leave.
pub(super) fn locate(
    state: &PresentationState,
    window: WindowId,
    params: &LocateParams,
) -> Result<Matches, AgentError> {
    let placement = placement_of(state, window);
    state.pipeline().with(|pipeline| {
        pipeline
            .semantics_owner()
            .ok_or(AgentError::NoTreeYet)?
            .locate_wire(params, placement)
            .map_err(AgentError::from_locate)
    })
}

/// The owner half of `element.actionability`.
///
/// Three steps, none inside another's borrow. The semantics owner says where the pointer would
/// land; the render tree's hit-test runs there, outside every semantics borrow because render
/// objects are user code; then the owner reads the facts against the path the hit-test found. A
/// render object that panics in the hit-test leaves its payload in `hit_panic`, for the caller to
/// answer and resume.
pub(super) fn actionability(
    state: &PresentationState,
    window: WindowId,
    params: &ActionabilityParams,
    hit_panic: &mut FirstPanic,
) -> Result<Actionability, AgentError> {
    let placement = placement_of(state, window);
    let point = state.pipeline().with(|pipeline| {
        pipeline
            .semantics_owner()
            .ok_or(AgentError::NoTreeYet)?
            .actionability_point(params.element, params.point, placement)
            .map_err(AgentError::from_unscoped_read)
    })?;

    let mut hit = Vec::new();
    if let Some(point) = point {
        let mut result = flui_interaction::pointer::HitTestResult::new();
        // No physical contact: the check asks about the element, not
        // about the reach of a finger or a stylus.
        match state.renderer().hit_test_in_view(
            &mut result,
            flui_foundation::geometry::Offset::new(point.x, point.y),
            flui_interaction::events::PointerKind::Virtual,
            0,
        ) {
            ViewHitTest::Answered {
                panic: Some(payload),
            } => {
                hit_panic.record(payload);
                return Err(AgentError::HitTestPanicked {
                    element: params.element.element,
                });
            }
            ViewHitTest::Answered { panic: None } => {}
            ViewHitTest::TreeBusy => return Err(AgentError::RenderTreeBusy),
        }
        hit.extend(result.path().iter().map(|entry| entry.target));
    }

    let read = |previous| {
        state.pipeline().with(|pipeline| {
            pipeline
                .semantics_owner()
                .ok_or(AgentError::NoTreeYet)?
                .actionability_wire(params.element, point, previous, &hit, placement)
                .map_err(AgentError::from_unscoped_read)
        })
    };
    let facts = read(None)?;
    let previous = state.agent_stability.borrow_mut().observe(
        params.element,
        state.tree_revision(),
        !state.has_pending_work_snapshot(),
        facts.rect,
    );
    match previous {
        Some(_) => read(previous),
        None => Ok(facts),
    }
}

/// The owner half of `semantics.act` once its target is an element: the tree the action starts
/// on is read first (for a delta), then the action is resolved against the committed tree and
/// its handler invoked through the path an assistive technology's action takes. A panic is the
/// caller's to contain; `if_panics` says how far the action got.
pub(super) fn act(
    state: &PresentationState,
    window: WindowId,
    request: &ActionRequest,
    snapshot: Snapshot,
    issued: bool,
    if_panics: &PanicAnswer,
) -> Result<Served, AgentError> {
    let before = tree_before(state, window, snapshot)?;
    let resolved = state.pipeline().with(|pipeline| {
        pipeline
            .semantics_owner()
            .ok_or(AgentError::NoTreeYet)?
            .resolve_wire_action(request)
            .map_err(|error| AgentError::from_act(error, issued))
    })?;
    let element = request.element;
    if_panics.set(AgentError::HandlerPanicked { element });
    state
        .dispatch_semantics_action(resolved)
        .map_err(|error| match error {
            SemanticsActionError::PresentationClosed => AgentError::PresentationGone,
            SemanticsActionError::SemanticsUnavailable => AgentError::NoTreeYet,
            SemanticsActionError::UnsupportedAction { .. } => AgentError::ActionUnsupported {
                element,
                action: request.action,
            },
            SemanticsActionError::Disabled { .. } => AgentError::Disabled { element },
            _ => AgentError::NodeNotFound { element, issued },
        })?;
    let mut outcome = ActOutcome::new(InputPath::Semantic, Effect::new(EffectKind::Ran));
    outcome.element = Some(ElementRef::new(window, element));
    Ok(match snapshot {
        Snapshot::Delta | Snapshot::Full => Served::Reading(ReadingOwed {
            outcome,
            snapshot,
            before,
        }),
        _ => Served::Answered(outcome),
    })
}

impl UiRuntime {
    /// The development agent's window for `presentation`, or `None` when this
    /// UI runtime does not host it.
    ///
    /// The presentation keeps one port for the development agent, so every
    /// window handle shares one record of issued element handles. The
    /// returned [`AgentWindow`] holds that port weakly: closing the
    /// presentation drops it, and every call on the window answers `gone`.
    /// It holds the presentation's semantics handle strongly: while any
    /// window handle is alive the presentation collects semantics, and once
    /// the hook has dropped them all collection stops on the next frame.
    /// Vending one while none is alive turns collection back on and requests
    /// a frame, which builds the tree; until it commits, a read answers
    /// `busy`.
    #[must_use]
    pub fn dev_agent_window(&self, presentation: PresentationId) -> Option<AgentWindow> {
        let state = self.presentations.get(presentation)?;
        let window = window_of(presentation);
        let (port, collecting, fresh) = {
            let mut slot = state.dev_agent.borrow_mut();
            let slot = slot.get_or_insert_with(|| {
                let mut sender = self.sender_prototype.clone();
                sender.presentation_id = presentation;
                DevAgentSlot {
                    port: Arc::new(DevAgentPort {
                        sender,
                        window,
                        open: RwLock::new(true),
                        leases: AtomicU64::new(0),
                        released: Mutex::default(),
                    }),
                    collecting: Weak::new(),
                    issued: Rc::default(),
                    journal: super::agent_journal::InputJournal::default(),
                }
            });
            let kept = slot.collecting.upgrade();
            let fresh = kept.is_none();
            let collecting = kept.unwrap_or_else(|| {
                let handle = Arc::new(state.semantics_host().ensure_semantics());
                slot.collecting = Arc::downgrade(&handle);
                handle
            });
            (Arc::clone(&slot.port), collecting, fresh)
        };
        if fresh {
            self.request_redraw_for(state);
        }
        let port: Weak<dyn AgentPort> = Arc::downgrade(&port) as _;
        Some(flui_view::__runtime::agent_window(window, port, collecting))
    }

    /// Holds an action's reply until a frame has passed, and asks for one, so
    /// the reading comes even when the action dirtied nothing.
    pub(super) fn park_act_read(
        &self,
        state: &PresentationState,
        owed: ReadingOwed,
        reply: ReplySender<ActOutcome>,
    ) {
        let pending = PendingActRead { owed, reply };
        let displaced = {
            let mut parked = state.agent_reads.borrow_mut();
            parked.push(pending);
            (parked.len() > MAX_OWED_READINGS).then(|| parked.remove(0))
        };
        if let Some(oldest) = displaced {
            oldest.abandon();
        }
        self.request_redraw_for(state);
    }

    /// Takes the readings the presentation owes, now that a frame has passed
    /// since the actions that owe them ran: the committed tree, as the frame
    /// left it, against the tree each action started on.
    pub(super) fn complete_agent_reads(state: &PresentationState) {
        let owed = std::mem::take(&mut *state.agent_reads.borrow_mut());
        if owed.is_empty() {
            return;
        }
        let placement = placement_of(state, window_of(state.id()));
        let now = state.pipeline().with(|pipeline| {
            pipeline
                .semantics_owner()
                .map(|owner| owner.snapshot_wire(placement))
        });
        for PendingActRead { owed, reply } in owed {
            let ReadingOwed {
                mut outcome,
                snapshot,
                before,
            } = owed;
            let asked = snapshot;
            let reading = match (snapshot, before, &now) {
                (Snapshot::Delta, Some(before), Some(Ok(now))) => Some(SnapshotReading::Delta(
                    before.delta_to(now, false, DELTA_CAP),
                )),
                (Snapshot::Full, ..) => {
                    let mut query = ReadQuery::new();
                    query.max_nodes = Some(READBACK_NODES);
                    state
                        .pipeline()
                        .with(|pipeline| {
                            pipeline
                                .semantics_owner()
                                .map(|owner| owner.read_wire(&query, placement))
                        })
                        .and_then(Result::ok)
                        .map(SnapshotReading::Full)
                }
                _ => None,
            };
            outcome.readback_failed = reading.is_none() && asked != Snapshot::None;
            outcome.snapshot = reading;
            let element = outcome.element.map(|element| element.element);
            send_reply(&reply, element, Ok(outcome));
        }
    }
}

#[cfg(test)]
#[path = "tests/agent_admission.rs"]
mod admission_tests;
