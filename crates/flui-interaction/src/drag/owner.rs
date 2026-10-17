//! Starting, driving and delivering drag sessions (ADR-0211).
//!
//! [`DragOwner`] belongs to one UI runtime. Source code (the `Draggable` widget)
//! reaches it through a [`DragHandle`] and holds a [`DragSession`] per drag; the
//! runtime attaches its presentations, enters the OS sessions the table decided
//! on, and delivers the OS's drag events to the receivers under them.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Offset;
use flui_foundation::{DataTransferId, DragSessionId, IdMint, PresentationId, UiRuntimeId};
use flui_platform_api::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, Mime, RepresentationIndex,
    TransferActions, TransferError, TransferPayload,
};
use flui_platform_api::drag::{
    AllowedActions, DragError, DragMoment, DragReach, DragSessionKey, OsDragEnd, OsDragRequest,
    OsDragStart, OsOffer, OsSessionPlan, ProcessNonce, plan_os_session, validate_session,
};
use flui_platform_api::pointer::PointerInfo;
use flui_platform_api::{DragDropEvent, EventTime, PlatformWindow, WindowAt, WindowId};

use super::item::{DragItem, SessionData};
use super::receiver::{DragPoint, receivers_on_path};
use super::sessions::{
    DragPhase, DragSessions, Entry, Incoming, PendingOsDrag, Shared, Surface, WeakShared,
    pointer_kind,
};
use super::tracker::{DropTracker, Dropped, Ticked};
use super::view::{DragScope, DragView};
use crate::close::ClosePanic;
use crate::events::PointerKind;
use crate::pointer::{HitTestEntry, HitTestHandle, PointerCapture};

/// How a drag ended, as its source is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragOutcome {
    /// A receiver took the drop with this action.
    Dropped {
        /// The action the drop resolved to.
        action: TransferActions,
    },
    /// Nothing took the drop, or it was cancelled.
    Cancelled,
}

/// Hears about the life of a session its source started.
///
/// Called on the owner thread with no session borrow held, so a method may use
/// the session it hears about.
pub trait DragObserver {
    /// The drag moved to `at` while it ran in its window.
    fn moved(&self, at: DragPoint) {
        let _ = at;
    }

    /// The drag needed an OS session and the backend has none. It stays in the
    /// window; leaving the window is a move with no target.
    fn escalation_refused(&self, error: DragError) {
        let _ = error;
    }

    /// The drag is over.
    fn ended(&self, outcome: DragOutcome) {
        let _ = outcome;
    }
}

/// What a session is doing, without the data each stage carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragStage {
    /// Started; the pointer has not moved yet.
    Armed,
    /// Running in its window on pointer capture.
    InWindow,
    /// Decided to become an OS session; the host has not entered it yet.
    EscalationPending,
    /// The OS runs it.
    System,
    /// Over.
    Concluded,
}

/// What one pointer move did to a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveOutcome {
    /// The drag runs in its window; the innermost accepting receiver would take
    /// the drop with this action, if any accepts.
    Tracking(Option<TransferActions>),
    /// The pointer left the window and the session became an OS session, waiting
    /// for the host to enter it.
    Escalating,
    /// The session needed an OS session and the backend cannot start one. It
    /// stays in the window with no target.
    Refused(DragError),
    /// The OS owns the drag; pointer input no longer drives it.
    OsOwned,
}

/// What it takes to start a drag.
#[must_use = "a DragStart does nothing until it starts a session"]
pub struct DragStart {
    source: PresentationId,
    items: Vec<DragItem>,
    allowed: AllowedActions,
    reach: Option<DragReach>,
    pointer: Option<PointerInfo>,
    window: Option<std::sync::Weak<dyn PlatformWindow>>,
    hit_test: Option<HitTestHandle>,
    capture: Option<PointerCapture>,
    observer: Option<Rc<dyn DragObserver>>,
}

impl DragStart {
    /// A drag of `items` from `source`'s window, allowing `allowed`.
    pub fn new(source: PresentationId, items: Vec<DragItem>, allowed: AllowedActions) -> Self {
        Self {
            source,
            items,
            allowed,
            reach: None,
            pointer: None,
            window: None,
            hit_test: None,
            capture: None,
            observer: None,
        }
    }

    /// How far the session may reach. Without this it derives from the actions
    /// ([`AllowedActions::derived_reach`]); reorder sets [`DragReach::Window`].
    pub fn reach(mut self, reach: DragReach) -> Self {
        self.reach = Some(reach);
        self
    }

    /// The contact that drags, and so the kind hit tests ask with.
    pub fn pointer(mut self, pointer: PointerInfo) -> Self {
        self.pointer = Some(pointer);
        self
    }

    /// The window the drag starts in: it says when the pointer leaves, and
    /// starts the OS session.
    pub fn window(mut self, window: std::sync::Weak<dyn PlatformWindow>) -> Self {
        self.window = Some(window);
        self
    }

    /// How to find the receivers under the pointer while the drag runs in the
    /// window. Without it the drag hit-tests the tree its runtime attached for
    /// the source presentation, and finds none when there is no such tree.
    pub fn hit_test(mut self, hit_test: HitTestHandle) -> Self {
        self.hit_test = Some(hit_test);
        self
    }

    /// The capture the contact's drag holds; the session keeps it until the
    /// drag is over or the OS takes it.
    pub fn capture(mut self, capture: PointerCapture) -> Self {
        self.capture = Some(capture);
        self
    }

    /// Who hears about the session.
    pub fn observer(mut self, observer: Rc<dyn DragObserver>) -> Self {
        self.observer = Some(observer);
        self
    }
}

impl std::fmt::Debug for DragStart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragStart")
            .field("source", &self.source)
            .field("items", &self.items.len())
            .field("allowed", &self.allowed)
            .finish_non_exhaustive()
    }
}

/// The drag machinery of one UI runtime.
pub struct DragOwner {
    shared: Shared,
    source: Arc<dyn DataTransferSource>,
}

impl std::fmt::Debug for DragOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragOwner").finish_non_exhaustive()
    }
}

impl DragOwner {
    /// The machinery of `runtime`, in the platform instance named by `nonce`,
    /// answering the OS through `source`.
    #[must_use]
    pub fn new(
        nonce: ProcessNonce,
        runtime: UiRuntimeId,
        source: Arc<dyn DataTransferSource>,
    ) -> Self {
        Self {
            shared: Rc::new(std::cell::RefCell::new(DragSessions::new(nonce, runtime))),
            source,
        }
    }

    /// This owner issuing session identities from `mint`, so a test can reach the
    /// terminal identity.
    #[doc(hidden)]
    #[must_use]
    pub fn with_session_mint(self, mint: IdMint<DragSessionId>) -> Self {
        self.shared.borrow_mut().mint = mint;
        self
    }

    /// A weak handle for source code.
    #[must_use]
    pub fn handle(&self) -> DragHandle {
        DragHandle {
            shared: Rc::downgrade(&self.shared),
        }
    }

    /// Note that `presentation` is open over `window`, its receivers found
    /// through `hit_test`. A session of this runtime moving over that window on
    /// its source's capture reaches them without an OS session; where the
    /// backend cannot place its windows, a session whose runtime has another
    /// open presentation may become an identity-only OS session instead.
    pub fn attach_presentation(
        &self,
        presentation: PresentationId,
        window: WindowId,
        hit_test: HitTestHandle,
    ) {
        let mut sessions = self.shared.borrow_mut();
        if !sessions
            .presentations
            .iter()
            .any(|open| open.presentation == presentation)
        {
            sessions.presentations.push(Surface {
                presentation,
                window,
                hit_test,
            });
        }
    }

    /// `presentation` is closing: its sessions are cancelled, the offers it was
    /// shown are left and concluded, and a session from another window hovering
    /// it leaves its receivers while its tree still stands. An OS session its
    /// window started keeps running; the OS ends it.
    pub fn detach_presentation(&self, presentation: PresentationId) {
        let mut failure = ClosePanic::new();
        let (sessions, incoming, hovering) = {
            let mut state = self.shared.borrow_mut();
            state
                .presentations
                .retain(|open| open.presentation != presentation);
            let hovering: Vec<(usize, DragSessionId)> = state
                .entries
                .iter()
                .filter(|(_, entry)| {
                    entry.source != presentation
                        && entry.over == presentation
                        && matches!(entry.phase, DragPhase::InWindow { .. })
                })
                .map(|(slot, entry)| (slot, entry.id))
                .collect();
            let sessions: Vec<(usize, DragSessionId)> = state
                .entries
                .iter()
                .filter(|(_, entry)| {
                    entry.source == presentation
                        && matches!(
                            entry.phase,
                            DragPhase::Armed
                                | DragPhase::InWindow { .. }
                                | DragPhase::EscalationPending { .. }
                        )
                })
                .map(|(slot, entry)| (slot, entry.id))
                .collect();
            let incoming: Vec<DataTransferId> = state
                .incoming
                .iter()
                .filter(|incoming| incoming.presentation == presentation)
                .map(|incoming| incoming.offer.id())
                .collect();
            (sessions, incoming, hovering)
        };
        for (slot, id) in hovering {
            failure.invoke(|| leave_hovered(&self.shared, slot, id));
        }
        for (slot, id) in sessions {
            failure.invoke(|| cancel_session(&self.shared, slot, id));
        }
        for id in incoming {
            failure.invoke(|| self.exit_incoming(id));
        }
        failure.finish();
    }

    /// The OS sessions decided on since the last call, now entered: each moves to
    /// the system phase before the host starts it, because a backend may run a
    /// modal loop inside `start` and the session must already be the OS's when
    /// its first event arrives. Call at the outermost quiescent point.
    #[must_use]
    pub fn take_pending_os_drags(&self) -> Vec<PendingOsDrag> {
        let mut state = self.shared.borrow_mut();
        let mut taken = Vec::new();
        let mut capture_release = Vec::new();
        while let Some((id, pending)) = state.pending.pop_front() {
            let slot = state
                .entries
                .iter()
                .find(|(_, entry)| entry.id == id)
                .map(|(slot, _)| slot);
            let Some(entry) = slot.and_then(|slot| state.entries.get_mut(slot)) else {
                continue;
            };
            let DragPhase::EscalationPending { plan } = entry.phase else {
                continue;
            };
            entry.phase = DragPhase::System { plan };
            capture_release.extend(entry.capture.take());
            taken.push(pending);
        }
        drop(state);
        // Releasing a capture defers its cancellation; no user code runs here.
        drop(capture_release);
        taken
    }

    /// Whether a session decided on an OS session the host has not entered yet.
    #[must_use]
    pub fn has_pending_os_drags(&self) -> bool {
        !self.shared.borrow().pending.is_empty()
    }

    /// The host could not start `session`'s OS session: it returns to its window.
    pub fn os_drag_refused(&self, session: DragSessionKey, error: DragError) {
        let observer = {
            let mut state = self.shared.borrow_mut();
            if session.nonce() != state.nonce || session.runtime() != state.runtime {
                return;
            }
            let Some(entry) = state
                .entries
                .iter_mut()
                .map(|(_, entry)| entry)
                .find(|entry| entry.id == session.session())
            else {
                return;
            };
            if !matches!(entry.phase, DragPhase::System { .. }) {
                return;
            }
            entry.phase = DragPhase::InWindow { accepted: None };
            if std::mem::replace(&mut entry.refusal_told, true) {
                None
            } else {
                entry.observer.clone()
            }
        };
        if let Some(observer) = observer {
            observer.escalation_refused(error);
        }
    }

    /// Advance every session and offer to `now`: receivers whose widgets
    /// unregistered them leave, spring-loading that came due runs, one
    /// autoscroll frame runs, and the drops whose requests have resolved are
    /// concluded.
    ///
    /// Runs in the owner's lane scope, as frames do; outside it a receiver's
    /// registration cannot be checked and it stays entered.
    pub fn tick(&self, now: EventTime) {
        let mut failure = ClosePanic::new();
        let live: Vec<(Hovered, DropTracker, ViewParts)> = {
            let state = self.shared.borrow();
            let sessions = state
                .entries
                .iter()
                .filter(|(_, entry)| {
                    matches!(entry.phase, DragPhase::Armed | DragPhase::InWindow { .. })
                })
                .map(|(slot, entry)| {
                    (
                        Hovered::Session(slot, entry.id),
                        entry.tracker.clone(),
                        ViewParts::of_entry(&state, entry),
                    )
                });
            let incoming = state.incoming.iter().map(|incoming| {
                (
                    Hovered::Offer(incoming.offer.id()),
                    incoming.tracker.clone(),
                    ViewParts::of_incoming(incoming),
                )
            });
            sessions.chain(incoming).collect()
        };
        for (hovered, tracker, parts) in live {
            let ticked = failure
                .invoke(|| parts.with_view(&self.source, |view| tracker.tick(view, now)))
                .unwrap_or(Ticked::Unchanged);
            if ticked == Ticked::AcceptingGone {
                self.accepting_gone(hovered);
            }
        }
        failure.invoke(|| self.conclude_resolved_drops());
        failure.finish();
    }

    /// The receiver that accepted `hovered`'s last move went away: nothing
    /// would take a drop now, and the OS is told so for an offer.
    fn accepting_gone(&self, hovered: Hovered) {
        match hovered {
            Hovered::Session(slot, id) => {
                let mut state = self.shared.borrow_mut();
                if let Some(entry) = state.entry_mut(slot, id)
                    && matches!(entry.phase, DragPhase::InWindow { .. })
                {
                    entry.phase = DragPhase::InWindow { accepted: None };
                }
            }
            Hovered::Offer(id) => {
                let waiting = self
                    .shared
                    .borrow_mut()
                    .incoming_mut(id)
                    .is_some_and(|incoming| !incoming.dropped);
                if waiting {
                    self.source
                        .update_drop_feedback(id, DropFeedback { accept: None });
                }
            }
        }
    }

    /// When [`tick`](Self::tick) next has work: the earliest pending
    /// spring-loading, the next frame of an autoscroll that is scrolling, or
    /// at once when a hover has no time yet or a drop's last request has
    /// resolved and the OS is still waiting to hear that the target is done
    /// with it.
    #[must_use]
    pub fn next_deadline(&self) -> Option<EventTime> {
        let state = self.shared.borrow();
        let concludable = state
            .incoming
            .iter()
            .any(|incoming| incoming.dropped && incoming.in_flight.load(Ordering::Acquire) == 0);
        if concludable {
            return Some(EventTime::from_nanos(0));
        }
        state
            .entries
            .iter()
            .map(|(_, entry)| &entry.tracker)
            .chain(state.incoming.iter().map(|incoming| &incoming.tracker))
            .filter_map(DropTracker::next_deadline)
            .min()
    }

    /// The identity of the live session `key` names, if this owner holds it.
    #[must_use]
    pub fn is_live(&self, key: DragSessionKey) -> bool {
        self.shared.borrow().find_live(key).is_some()
    }

    /// Deliver an OS drag event to `presentation`'s receivers.
    ///
    /// `hit` returns the hit-test path under a window position, innermost first.
    /// Runs in the owner's lane scope, where dispatch runs. The target's answer
    /// to each enter and move reaches the OS as drop feedback.
    pub fn deliver(
        &self,
        presentation: PresentationId,
        event: DragDropEvent,
        hit: &mut dyn FnMut(Offset<f64>) -> Option<Vec<HitTestEntry>>,
    ) {
        match event {
            DragDropEvent::Entered {
                offer,
                allowed,
                position,
            } => {
                let id = offer.id();
                self.admit_offer(presentation, offer, allowed);
                if let Some(position) = position {
                    self.update_incoming(id, Offset::new(position.x, position.y), hit);
                }
            }
            DragDropEvent::Moved {
                id,
                allowed,
                position,
            } => {
                if let Some(incoming) = self.shared.borrow_mut().incoming_mut(id) {
                    incoming.os_allowed = allowed;
                }
                self.update_incoming(id, Offset::new(position.x, position.y), hit);
            }
            DragDropEvent::Dropped {
                id,
                action,
                position,
            } => {
                if let Some(position) = position {
                    self.update_incoming(id, Offset::new(position.x, position.y), hit);
                }
                self.drop_incoming(id, action);
            }
            DragDropEvent::Exited { id } => self.exit_incoming(id),
        }
    }

    fn admit_offer(
        &self,
        presentation: PresentationId,
        offer: DataTransferOffer,
        os_allowed: TransferActions,
    ) {
        let mut state = self.shared.borrow_mut();
        // The same offer announced again over the same window is the same drag:
        // one tracker, so its receivers still hear one enter and one leave or drop.
        if let Some(incoming) = state.incoming.iter_mut().find(|incoming| {
            incoming.offer.id() == offer.id() && incoming.presentation == presentation
        }) {
            incoming.os_allowed = os_allowed;
            return;
        }
        let key = DragSessionKey::from_offer(&offer);
        let (scope, data, source_allowed) = match key {
            None => (DragScope::External, None, None),
            Some(key) if key.nonce() != state.nonce => (DragScope::External, None, None),
            Some(key) => match state.find_live(key) {
                Some(entry) => (
                    DragScope::Runtime,
                    Some(Rc::clone(&entry.data)),
                    Some(entry.allowed.within_app),
                ),
                None => (DragScope::OtherRuntime, None, None),
            },
        };
        state.incoming.push(Incoming {
            offer,
            presentation,
            scope,
            key,
            data,
            source_allowed,
            os_allowed,
            tracker: DropTracker::new(),
            in_flight: Arc::new(AtomicUsize::new(0)),
            dropped: false,
            position: None,
        });
    }

    fn update_incoming(
        &self,
        id: DataTransferId,
        position: Offset<f64>,
        hit: &mut dyn FnMut(Offset<f64>) -> Option<Vec<HitTestEntry>>,
    ) {
        let found = {
            let mut state = self.shared.borrow_mut();
            let Some(incoming) = state.incoming_mut(id) else {
                return;
            };
            if incoming.dropped {
                return;
            }
            incoming.position = Some(position);
            (incoming.tracker.clone(), ViewParts::of_incoming(incoming))
        };
        let (tracker, parts) = found;
        let hits = hit(position)
            .and_then(|path| receivers_on_path(&path, position).ok())
            .unwrap_or_default();
        let accepted = parts.with_view(&self.source, |view| {
            tracker.update(hits, view, position, None)
        });
        self.source
            .update_drop_feedback(id, DropFeedback { accept: accepted });
    }

    fn drop_incoming(&self, id: DataTransferId, action: TransferActions) {
        let (tracker, parts) = {
            let mut state = self.shared.borrow_mut();
            let Some(incoming) = state.incoming_mut(id) else {
                return;
            };
            if std::mem::replace(&mut incoming.dropped, true) {
                return;
            }
            (incoming.tracker.clone(), ViewParts::of_incoming(incoming))
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parts.with_view(&self.source, |view| tracker.finish(view, Some(action)))
        }));
        // The OS is told once every request made from the drop has resolved.
        self.conclude_resolved_drops();
        match result {
            Ok(Dropped::Delivered | Dropped::NoRecipient) => {}
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    fn exit_incoming(&self, id: DataTransferId) {
        let removed = {
            let mut state = self.shared.borrow_mut();
            let Some(index) = state
                .incoming
                .iter()
                .position(|incoming| incoming.offer.id() == id)
            else {
                return;
            };
            let incoming = state.incoming.remove(index);
            (!incoming.dropped).then_some(incoming)
        };
        if let Some(incoming) = removed {
            let parts = ViewParts::of_incoming(&incoming);
            parts.with_view(&self.source, |view| incoming.tracker.finish(view, None));
        }
    }

    /// Tell the OS every dropped offer whose requests have all resolved that the
    /// target is done with it.
    fn conclude_resolved_drops(&self) {
        let done: Vec<DataTransferId> = {
            let mut state = self.shared.borrow_mut();
            let mut done = Vec::new();
            state.incoming.retain(|incoming| {
                let finished = incoming.dropped && incoming.in_flight.load(Ordering::Acquire) == 0;
                if finished {
                    done.push(incoming.offer.id());
                }
                !finished
            });
            done
        };
        for id in done {
            self.source.conclude_drop(id);
        }
    }
}

/// A drag a tick advanced: a session of this runtime or an OS offer.
#[derive(Clone, Copy)]
enum Hovered {
    Session(usize, DragSessionId),
    Offer(DataTransferId),
}

/// Everything a [`DragView`] borrows, owned, so the view outlives no borrow.
struct ViewParts {
    data: Option<Rc<SessionData>>,
    offer: Option<DataTransferOffer>,
    key: Option<DragSessionKey>,
    scope: DragScope,
    allowed: TransferActions,
    in_flight: Option<Arc<AtomicUsize>>,
    pointer: PointerKind,
}

impl ViewParts {
    fn of_entry(state: &DragSessions, entry: &Entry) -> Self {
        Self {
            data: Some(Rc::clone(&entry.data)),
            offer: None,
            key: Some(state.key_of(entry.id)),
            scope: scope_over(entry, entry.over),
            allowed: entry.allowed.within_app,
            in_flight: None,
            pointer: pointer_kind(entry.pointer),
        }
    }

    fn of_incoming(incoming: &Incoming) -> Self {
        let allowed = match incoming.source_allowed {
            Some(within) => within & incoming.os_allowed,
            None => incoming.os_allowed,
        };
        Self {
            data: incoming.data.clone(),
            offer: Some(incoming.offer.clone()),
            key: incoming.key,
            scope: incoming.scope,
            allowed,
            in_flight: Some(Arc::clone(&incoming.in_flight)),
            pointer: PointerKind::Unknown,
        }
    }

    fn with_view<R>(
        &self,
        source: &Arc<dyn DataTransferSource>,
        run: impl FnOnce(&DragView<'_>) -> R,
    ) -> R {
        let view = DragView {
            data: self.data.as_deref(),
            offer: self.offer.as_ref(),
            key: self.key,
            scope: self.scope,
            allowed: self.allowed,
            source: self.offer.is_some().then_some(source),
            in_flight: self.in_flight.as_ref(),
            pointer: self.pointer,
        };
        run(&view)
    }
}

/// A weak handle to a runtime's drag machinery, for source code.
#[derive(Clone)]
pub struct DragHandle {
    shared: WeakShared,
}

impl std::fmt::Debug for DragHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragHandle").finish_non_exhaustive()
    }
}

impl DragHandle {
    /// Start a session.
    ///
    /// # Errors
    ///
    /// Every [`DragError`] a configuration can earn, or
    /// [`DragError::SessionClosed`] when the runtime is gone.
    pub fn start(&self, start: DragStart) -> Result<DragSession, DragError> {
        let shared = self.shared.upgrade().ok_or(DragError::SessionClosed)?;
        let DragStart {
            source,
            items,
            allowed,
            reach,
            pointer,
            window,
            hit_test,
            capture,
            observer,
        } = start;
        let reach = reach.unwrap_or_else(|| allowed.derived_reach());
        let data = Rc::new(SessionData::new(items));
        validate_session(allowed, reach, data.has_serialized())?;
        let (timing, available) = window
            .as_ref()
            .and_then(std::sync::Weak::upgrade)
            .and_then(|window| window.drag_source())
            .map_or((OsDragStart::OnLeavingWindow, false), |drag_source| {
                (drag_source.start_timing(), true)
            });
        let slot = {
            let mut state = shared.borrow_mut();
            let id = state.mint.next()?;
            let slot = state.entries.insert(Entry {
                id,
                data,
                allowed,
                reach,
                source,
                pointer,
                window,
                hit_test,
                capture,
                observer,
                tracker: DropTracker::new(),
                over: source,
                phase: DragPhase::Armed,
                timing,
                available,
                refusal_told: false,
                released: false,
            });
            (slot, id)
        };
        let (slot, id) = slot;
        // An at-start backend decides now; a failed start leaves no session.
        if matches!(timing, OsDragStart::AtStart { .. })
            && let Err(error) = escalate(&shared, slot, id, DragMoment::Start, Siblings::ByOs)
        {
            let removed = shared.borrow_mut().entries.try_remove(slot);
            drop(removed);
            return Err(error);
        }
        Ok(DragSession {
            shared: Rc::downgrade(&shared),
            slot,
            id,
        })
    }
}

/// One drag, as its source holds it.
///
/// Dropping the last handle of a drag that still runs in its window cancels it;
/// one the OS runs continues until the OS ends it.
pub struct DragSession {
    shared: WeakShared,
    slot: usize,
    id: DragSessionId,
}

impl std::fmt::Debug for DragSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragSession")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl DragSession {
    fn shared(&self) -> Result<Shared, DragError> {
        self.shared.upgrade().ok_or(DragError::SessionClosed)
    }

    /// The session's identity within its runtime.
    #[must_use]
    pub fn id(&self) -> DragSessionId {
        self.id
    }

    /// The session's identity across presentations and processes.
    ///
    /// # Errors
    ///
    /// [`DragError::SessionClosed`] when the runtime is gone.
    pub fn key(&self) -> Result<DragSessionKey, DragError> {
        Ok(self.shared()?.borrow().key_of(self.id))
    }

    /// What the session is doing.
    ///
    /// # Errors
    ///
    /// [`DragError::SessionClosed`] when the runtime is gone.
    pub fn stage(&self) -> Result<DragStage, DragError> {
        let shared = self.shared()?;
        let state = shared.borrow();
        let entry = state
            .entry(self.slot, self.id)
            .ok_or(DragError::SessionClosed)?;
        Ok(match entry.phase {
            DragPhase::Armed => DragStage::Armed,
            DragPhase::InWindow { .. } => DragStage::InWindow,
            DragPhase::EscalationPending { .. } => DragStage::EscalationPending,
            DragPhase::System { .. } => DragStage::System,
            DragPhase::Concluded { .. } => DragStage::Concluded,
        })
    }

    /// The OS session the session became, once it did: what it offers and
    /// advertises.
    #[must_use]
    pub fn os_plan(&self) -> Option<OsSessionPlan> {
        let shared = self.shared.upgrade()?;
        let state = shared.borrow();
        match state.entry(self.slot, self.id)?.phase {
            DragPhase::EscalationPending { plan } | DragPhase::System { plan } => Some(plan),
            _ => None,
        }
    }

    /// How the session ended, once it has.
    #[must_use]
    pub fn outcome(&self) -> Option<DragOutcome> {
        let shared = self.shared.upgrade()?;
        let state = shared.borrow();
        match state.entry(self.slot, self.id)?.phase {
            DragPhase::Concluded { outcome } => Some(outcome),
            _ => None,
        }
    }

    /// The pointer moved to `global` (window space) at `time`.
    ///
    /// # Errors
    ///
    /// [`DragError::SessionClosed`] once the session is over.
    pub fn pointer_moved(
        &self,
        global: Offset<f64>,
        time: Option<EventTime>,
    ) -> Result<MoveOutcome, DragError> {
        move_session(&self.shared()?, self.slot, self.id, global, time)
    }

    /// The contact lifted at `global`: the drop. `None` when the OS owns the
    /// drag and decides how it ends.
    ///
    /// # Errors
    ///
    /// [`DragError::SessionClosed`] once the session is over.
    pub fn pointer_released(
        &self,
        global: Offset<f64>,
        time: Option<EventTime>,
    ) -> Result<Option<DragOutcome>, DragError> {
        release_session(&self.shared()?, self.slot, self.id, global, time)
    }

    /// Cancel the drag (Escape, the contact was cancelled). Every receiver it
    /// entered gets its leave. A drag the OS runs is not cancelled from here.
    ///
    /// # Errors
    ///
    /// [`DragError::SessionClosed`] once the session is over.
    pub fn cancel(&self) -> Result<(), DragError> {
        let shared = self.shared()?;
        if shared.borrow().entry(self.slot, self.id).is_none() {
            return Err(DragError::SessionClosed);
        }
        if self.stage()? == DragStage::Concluded {
            return Err(DragError::SessionClosed);
        }
        cancel_session(&shared, self.slot, self.id);
        Ok(())
    }
}

impl Drop for DragSession {
    fn drop(&mut self) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let live = {
            let mut state = shared.borrow_mut();
            let Some(entry) = state.entry_mut(self.slot, self.id) else {
                return;
            };
            entry.released = true;
            match entry.phase {
                DragPhase::Armed
                | DragPhase::InWindow { .. }
                | DragPhase::EscalationPending { .. } => true,
                DragPhase::System { .. } => false,
                DragPhase::Concluded { .. } => {
                    let removed = state.entries.try_remove(self.slot);
                    drop(state);
                    drop(removed);
                    return;
                }
            }
        };
        if live {
            cancel_session(&shared, self.slot, self.id);
        }
    }
}

/// What a move needs of a session, cloned out of the table.
struct Running {
    tracker: DropTracker,
    parts: ViewParts,
    hit_test: Option<HitTestHandle>,
    kind: PointerKind,
    window: Option<std::sync::Weak<dyn PlatformWindow>>,
    observer: Option<Rc<dyn DragObserver>>,
    stage_system: bool,
    reach: DragReach,
    source: PresentationId,
    over: PresentationId,
}

fn running(shared: &Shared, slot: usize, id: DragSessionId) -> Result<Running, DragError> {
    let state = shared.borrow();
    let entry = state.entry(slot, id).ok_or(DragError::SessionClosed)?;
    if matches!(entry.phase, DragPhase::Concluded { .. }) {
        return Err(DragError::SessionClosed);
    }
    Ok(Running {
        tracker: entry.tracker.clone(),
        parts: ViewParts::of_entry(&state, entry),
        hit_test: entry.hit_test.clone(),
        kind: pointer_kind(entry.pointer),
        window: entry.window.clone(),
        observer: entry.observer.clone(),
        stage_system: matches!(entry.phase, DragPhase::System { .. }),
        reach: entry.reach,
        source: entry.source,
        over: entry.over,
    })
}

/// How a receiver in `presentation` sees `entry`'s drag.
fn scope_over(entry: &Entry, presentation: PresentationId) -> DragScope {
    if presentation == entry.source {
        DragScope::Window
    } else {
        DragScope::Runtime
    }
}

/// Whether the sessions of a runtime reach its other presentations themselves,
/// or only through an OS session.
#[derive(Clone, Copy)]
enum Siblings {
    /// The backend placed the pointer: a sibling window under it is reached on
    /// the source's capture, so none needs an OS session.
    InProcess,
    /// The backend cannot place its windows; only the OS reaches a sibling.
    ByOs,
}

/// Where a drag's pointer is, for the receivers it reaches.
enum Over {
    /// Over the content of `presentation` of the source's runtime, at
    /// `position` in that window's coordinates.
    Presentation {
        presentation: PresentationId,
        hit_test: Option<HitTestHandle>,
        position: Offset<f64>,
    },
    /// Over nothing the runtime reaches without the OS.
    Beyond(Siblings),
}

/// Where `global`, in the source window's coordinates, lies for a session.
fn locate(shared: &Shared, running: &Running, global: Offset<f64>) -> Over {
    let source = running.source;
    let hit_test = running.hit_test.clone().or_else(|| {
        let state = shared.borrow();
        state
            .presentations
            .iter()
            .find(|surface| surface.presentation == source)
            .map(|surface| surface.hit_test.clone())
    });
    let here = Over::Presentation {
        presentation: source,
        hit_test,
        position: global,
    };
    let window = running.window.as_ref().and_then(std::sync::Weak::upgrade);
    let Some(window) = window else {
        return here;
    };
    let size = window.logical_size();
    let inside =
        global.dx >= 0.0 && global.dy >= 0.0 && global.dx <= size.width && global.dy <= size.height;
    if inside {
        return here;
    }
    // A reorder never leaves its window, and so never looks for another.
    if running.reach == DragReach::Window {
        return Over::Beyond(Siblings::ByOs);
    }
    match window.window_at(flui_foundation::geometry::Point::new(global.dx, global.dy)) {
        WindowAt::Unknown => Over::Beyond(Siblings::ByOs),
        WindowAt::Window { window, position } => {
            let state = shared.borrow();
            let surface = state
                .presentations
                .iter()
                .find(|surface| surface.window == window);
            match surface {
                Some(surface) if surface.presentation == source => here,
                Some(surface) => Over::Presentation {
                    presentation: surface.presentation,
                    hit_test: Some(surface.hit_test.clone()),
                    position: Offset::new(position.x, position.y),
                },
                // Another runtime's window, or one whose presentation closed.
                None => Over::Beyond(Siblings::InProcess),
            }
        }
        _ => Over::Beyond(Siblings::InProcess),
    }
}

/// A no-op view source: a window-scope view never asks the platform.
fn no_source() -> Arc<dyn DataTransferSource> {
    Arc::new(flui_platform_api::NullDataTransferSource)
}

fn move_session(
    shared: &Shared,
    slot: usize,
    id: DragSessionId,
    global: Offset<f64>,
    time: Option<EventTime>,
) -> Result<MoveOutcome, DragError> {
    let running = running(shared, slot, id)?;
    if running.stage_system {
        return Ok(MoveOutcome::OsOwned);
    }
    {
        let mut state = shared.borrow_mut();
        if let Some(entry) = state.entry_mut(slot, id)
            && matches!(entry.phase, DragPhase::Armed)
        {
            entry.phase = DragPhase::InWindow { accepted: None };
        }
    }
    let mut outcome = None;
    let mut hits = Vec::new();
    let over = locate(shared, &running, global);
    let mut parts = running.parts;
    let mut at = global;
    match over {
        Over::Beyond(siblings) => {
            match escalate(shared, slot, id, DragMoment::LeavesWindow, siblings) {
                Ok(true) => return Ok(MoveOutcome::Escalating),
                Ok(false) => {}
                Err(error) => {
                    tell_refusal(shared, slot, id, error);
                    outcome = Some(MoveOutcome::Refused(error));
                }
            }
        }
        Over::Presentation {
            presentation,
            hit_test,
            position,
        } => {
            if let Some(hit_test) = &hit_test {
                match hit_test.hit_test_at(position, running.kind) {
                    Ok(snapshot) => {
                        hits = receivers_on_path(snapshot.path(), position).unwrap_or_default();
                    }
                    Err(error) => {
                        tracing::debug!(
                            ?error,
                            "drag hit test unavailable; keeping the last targets"
                        );
                        return Ok(MoveOutcome::Tracking(None));
                    }
                }
            }
            if presentation != running.over {
                // Crossing into another window: the last window's receivers
                // leave, seeing the drag as they saw it, before any receiver of
                // the new one enters.
                parts.with_view(&no_source(), |view| {
                    running.tracker.update(Vec::new(), view, position, time)
                });
                let mut state = shared.borrow_mut();
                if let Some(entry) = state.entry_mut(slot, id) {
                    entry.over = presentation;
                    parts.scope = scope_over(entry, presentation);
                }
            }
            at = position;
        }
    }
    let accepted = parts.with_view(&no_source(), |view| {
        running.tracker.update(hits, view, at, time)
    });
    {
        let mut state = shared.borrow_mut();
        if let Some(entry) = state.entry_mut(slot, id)
            && matches!(entry.phase, DragPhase::InWindow { .. })
        {
            entry.phase = DragPhase::InWindow { accepted };
        }
    }
    if let Some(observer) = running.observer {
        observer.moved(DragPoint {
            global,
            local: global,
            time,
        });
    }
    Ok(outcome.unwrap_or(MoveOutcome::Tracking(accepted)))
}

fn tell_refusal(shared: &Shared, slot: usize, id: DragSessionId, error: DragError) {
    let observer = {
        let mut state = shared.borrow_mut();
        let Some(entry) = state.entry_mut(slot, id) else {
            return;
        };
        if std::mem::replace(&mut entry.refusal_told, true) {
            return;
        }
        entry.observer.clone()
    };
    if let Some(observer) = observer {
        observer.escalation_refused(error);
    }
}

fn release_session(
    shared: &Shared,
    slot: usize,
    id: DragSessionId,
    global: Offset<f64>,
    time: Option<EventTime>,
) -> Result<Option<DragOutcome>, DragError> {
    match move_session(shared, slot, id, global, time)? {
        MoveOutcome::OsOwned => return Ok(None),
        MoveOutcome::Escalating => {
            // Released before the host entered the OS session: nothing to drop.
            cancel_session(shared, slot, id);
            return Ok(Some(DragOutcome::Cancelled));
        }
        MoveOutcome::Tracking(_) | MoveOutcome::Refused(_) => {}
    }
    let running = running(shared, slot, id)?;
    let accepted = {
        let state = shared.borrow();
        match state.entry(slot, id).map(|entry| &entry.phase) {
            Some(DragPhase::InWindow { accepted }) => *accepted,
            _ => None,
        }
    };
    let delivered = running
        .parts
        .with_view(&no_source(), |view| running.tracker.finish(view, accepted));
    let outcome = match (delivered, accepted) {
        (Dropped::Delivered, Some(action)) => DragOutcome::Dropped { action },
        _ => DragOutcome::Cancelled,
    };
    conclude(shared, slot, id, outcome);
    Ok(Some(outcome))
}

/// The window a session hovers is closing: its receivers leave now, while its
/// tree still stands, and the drag goes on with no target until it moves.
fn leave_hovered(shared: &Shared, slot: usize, id: DragSessionId) {
    let Ok(running) = running(shared, slot, id) else {
        return;
    };
    {
        let mut state = shared.borrow_mut();
        if let Some(entry) = state.entry_mut(slot, id) {
            entry.over = entry.source;
            if matches!(entry.phase, DragPhase::InWindow { .. }) {
                entry.phase = DragPhase::InWindow { accepted: None };
            }
        }
    }
    running.parts.with_view(&no_source(), |view| {
        running
            .tracker
            .update(Vec::new(), view, Offset::new(0.0, 0.0), None)
    });
}

fn cancel_session(shared: &Shared, slot: usize, id: DragSessionId) {
    let Ok(running) = running(shared, slot, id) else {
        return;
    };
    if running.stage_system {
        return;
    }
    {
        let mut state = shared.borrow_mut();
        state.pending.retain(|(pending, _)| *pending != id);
    }
    let mut failure = ClosePanic::new();
    failure.invoke(|| {
        running
            .parts
            .with_view(&no_source(), |view| running.tracker.finish(view, None));
    });
    conclude(shared, slot, id, DragOutcome::Cancelled);
    failure.finish();
}

/// Move a session to its last phase, release what it held and tell its observer.
fn conclude(shared: &Shared, slot: usize, id: DragSessionId, outcome: DragOutcome) {
    let (observer, capture, removed) = {
        let mut state = shared.borrow_mut();
        let Some(entry) = state.entry_mut(slot, id) else {
            return;
        };
        if matches!(entry.phase, DragPhase::Concluded { .. }) {
            return;
        }
        let _ = std::mem::replace(&mut entry.phase, DragPhase::Concluded { outcome });
        let observer = entry.observer.clone();
        let capture = entry.capture.take();
        let removed = if entry.released {
            state.entries.try_remove(slot)
        } else {
            None
        };
        (observer, capture, removed)
    };
    drop(capture);
    drop(removed);
    if let Some(observer) = observer {
        observer.ended(outcome);
    }
}

/// Decide at `moment` whether the session becomes an OS session, and if so
/// record the request for the host and leave every receiver of the window.
fn escalate(
    shared: &Shared,
    slot: usize,
    id: DragSessionId,
    moment: DragMoment,
    siblings: Siblings,
) -> Result<bool, DragError> {
    let (plan, running, request_parts) = {
        let state = shared.borrow();
        let entry = state.entry(slot, id).ok_or(DragError::SessionClosed)?;
        if matches!(
            entry.phase,
            DragPhase::EscalationPending { .. }
                | DragPhase::System { .. }
                | DragPhase::Concluded { .. }
        ) {
            return Ok(false);
        }
        // A sibling the session reaches on its own capture needs no OS session.
        let sibling = matches!(siblings, Siblings::ByOs)
            && state
                .presentations
                .iter()
                .any(|surface| surface.presentation != entry.source);
        let Some(plan) = plan_os_session(
            entry.timing,
            moment,
            entry.reach,
            entry.allowed,
            sibling,
            entry.available,
        )?
        else {
            return Ok(false);
        };
        (
            plan,
            (entry.tracker.clone(), ViewParts::of_entry(&state, entry)),
            (
                entry.source,
                state.key_of(id),
                Rc::clone(&entry.data),
                entry.pointer,
            ),
        )
    };
    let (presentation, key, data, pointer) = request_parts;
    let request = os_request(plan, key, data, pointer, shared, slot, id);
    {
        let mut state = shared.borrow_mut();
        let Some(entry) = state.entry_mut(slot, id) else {
            return Err(DragError::SessionClosed);
        };
        entry.phase = DragPhase::EscalationPending { plan };
        state.pending.push_back((
            id,
            PendingOsDrag {
                presentation,
                request,
            },
        ));
    }
    // Leaving the window leaves every receiver in it.
    let (tracker, parts) = running;
    parts.with_view(&no_source(), |view| tracker.finish(view, None));
    Ok(true)
}

fn os_request(
    plan: OsSessionPlan,
    key: DragSessionKey,
    data: Rc<SessionData>,
    pointer: Option<PointerInfo>,
    shared: &Shared,
    slot: usize,
    id: DragSessionId,
) -> OsDragRequest {
    let mut representations = match plan.offer {
        OsOffer::Full => data.descriptors(),
        OsOffer::IdentityOnly => Vec::new(),
    };
    let serialized = representations.len();
    representations.push(key.representation());
    let weak = Rc::downgrade(shared);
    OsDragRequest {
        key,
        pointer: pointer.map(|info| info.id),
        representations,
        allowed: plan.advertised,
        provider: Box::new(move |RepresentationIndex(index)| {
            let index = usize::from(index);
            match index.cmp(&serialized) {
                std::cmp::Ordering::Less => data.serialized(index),
                std::cmp::Ordering::Equal => Ok(TransferPayload::Custom {
                    mime: Mime::new(flui_platform_api::drag::IDENTITY_ESSENCE),
                    bytes: Arc::from(Vec::new()),
                }),
                std::cmp::Ordering::Greater => Err(TransferError::Cancelled),
            }
        }),
        on_end: Box::new(move |end| {
            let outcome = match end {
                OsDragEnd::Dropped(action) => DragOutcome::Dropped { action },
                OsDragEnd::Cancelled => DragOutcome::Cancelled,
            };
            if let Some(shared) = weak.upgrade() {
                conclude(&shared, slot, id, outcome);
            }
        }),
    }
}
