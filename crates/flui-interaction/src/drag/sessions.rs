//! The session table of one UI runtime (ADR-0211 §6).
//!
//! [`DragSessions`] is shared by `Rc<RefCell<..>>` and borrowed only for short
//! reads and writes; every call into a receiver, an observer, the platform or a
//! provider runs on clones taken out of the borrow. A phase is advanced by
//! `mem::replace`, so a callback that reenters finds the new phase, never a
//! half-moved one.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use flui_foundation::{DataTransferId, DragSessionId, IdMint, PresentationId, UiRuntimeId};
use flui_platform_api::data_transfer::{DataTransferOffer, TransferActions};
use flui_platform_api::drag::{
    AllowedActions, DragReach, DragSessionKey, OsDragRequest, OsDragStart, ProcessNonce,
};
use flui_platform_api::{PlatformWindow, WindowId};
use slab::Slab;

use super::cursor::DragCursor;
use super::item::SessionData;
use super::owner::{DragObserver, DragOutcome};
use super::tracker::DropTracker;
use super::view::DragScope;
use crate::events::PointerKind;
use crate::pointer::{HitTestHandle, PointerCapture};

/// Where a session is in its life.
pub(crate) enum DragPhase {
    /// Started; the pointer has not moved yet.
    Armed,
    /// Running in its window on pointer capture. `accepted` is the action the
    /// last move was accepted with, if a receiver accepted it.
    InWindow { accepted: Option<TransferActions> },
    /// An OS session was decided on; the host has not entered it yet.
    EscalationPending,
    /// The OS runs the session, or the host is about to start it.
    System { start: OsStart },
    /// Over, with the outcome observers were told.
    Concluded { outcome: DragOutcome },
}

/// Whether the host's start of a session's OS session is running.
#[derive(Clone, Copy)]
pub(crate) enum OsStart {
    /// Not running: the start has not been called yet, or it returned. An end
    /// hook dropped now ends the session cancelled.
    Idle,
    /// The backend's `start` is running. An end hook it drops is a refusal
    /// when the start fails, and an end with no drop when it succeeds; which,
    /// is known once it returns.
    Running { end_dropped: bool },
}

pub(crate) struct Entry {
    pub(crate) id: DragSessionId,
    pub(crate) data: Rc<SessionData>,
    pub(crate) allowed: AllowedActions,
    pub(crate) reach: DragReach,
    pub(crate) source: PresentationId,
    pub(crate) pointer: Option<flui_platform_api::pointer::PointerInfo>,
    pub(crate) window: Option<std::sync::Weak<dyn PlatformWindow>>,
    pub(crate) hit_test: Option<HitTestHandle>,
    pub(crate) capture: Option<PointerCapture>,
    pub(crate) observer: Option<Rc<dyn DragObserver>>,
    /// The cursor the session shows while it runs in its window.
    pub(crate) cursor: Option<Rc<DragCursor>>,
    /// The action a keyboard or assistive transfer holds the session for,
    /// until a paste or a cancel ends it.
    pub(crate) held: Option<TransferActions>,
    /// Where the pointer last moved to, in the window's space.
    pub(crate) last_position: Option<flui_foundation::geometry::Offset<f64>>,
    pub(crate) tracker: DropTracker,
    /// The presentation whose receivers the tracker last entered: the source's
    /// own, or a sibling's the drag moved over on the source's capture.
    pub(crate) over: PresentationId,
    pub(crate) phase: DragPhase,
    pub(crate) timing: OsDragStart,
    pub(crate) available: bool,
    /// The refusal of an escalation has been told to the observer.
    pub(crate) refusal_told: bool,
    /// Nothing holds the session's handle any more: the entry goes with the
    /// session's last phase.
    pub(crate) released: bool,
}

/// An OS session decided on and waiting for the host to enter it with
/// [`start`](Self::start).
///
/// Its session ends however the OS session goes: the backend calls the end
/// hook, or drops it uncalled and the session ends cancelled. Dropping this
/// without starting it ends the session cancelled too.
#[must_use = "a pending OS drag ends cancelled unless it is started"]
pub struct PendingOsDrag {
    /// The presentation whose window starts the session.
    pub presentation: PresentationId,
    pub(crate) request: OsDragRequest,
    pub(crate) session: SessionRef,
}

impl std::fmt::Debug for PendingOsDrag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingOsDrag")
            .field("presentation", &self.presentation)
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

/// One session of a table, named by its slot and identity.
#[derive(Clone)]
pub(crate) struct SessionRef {
    pub(crate) shared: WeakShared,
    pub(crate) slot: usize,
    pub(crate) id: DragSessionId,
}

/// An offer the OS delivered to one of this runtime's windows.
pub(crate) struct Incoming {
    pub(crate) offer: DataTransferOffer,
    pub(crate) presentation: PresentationId,
    pub(crate) scope: DragScope,
    pub(crate) key: Option<DragSessionKey>,
    /// The source's data, when this runtime made the drag and it is still live.
    pub(crate) data: Option<Rc<SessionData>>,
    /// What the source allows its own runtime, when this runtime made it.
    pub(crate) source_allowed: Option<TransferActions>,
    /// The latest OS-permitted set.
    pub(crate) os_allowed: TransferActions,
    pub(crate) tracker: DropTracker,
    /// Requests made from the offer that have not resolved, shared by its
    /// hovers over every window.
    pub(crate) in_flight: Arc<AtomicUsize>,
    /// The target's callbacks are done; the OS is told once nothing is in flight.
    pub(crate) dropped: bool,
    pub(crate) position: Option<flui_foundation::geometry::Offset<f64>>,
    /// The kind of pointer that drags, when this runtime started the drag;
    /// `Unknown` for another application's.
    pub(crate) pointer: PointerKind,
}

impl Incoming {
    /// Whether this is the hover of offer `id` over `presentation`'s window.
    pub(crate) fn is(&self, presentation: PresentationId, id: DataTransferId) -> bool {
        self.presentation == presentation && self.offer.id() == id
    }
}

/// An open presentation of the runtime, as its drags reach it.
pub(crate) struct Surface {
    pub(crate) presentation: PresentationId,
    pub(crate) window: WindowId,
    pub(crate) hit_test: HitTestHandle,
}

pub(crate) struct DragSessions {
    pub(crate) entries: Slab<Entry>,
    pub(crate) mint: IdMint<DragSessionId>,
    pub(crate) nonce: ProcessNonce,
    pub(crate) runtime: UiRuntimeId,
    pub(crate) presentations: Vec<Surface>,
    pub(crate) pending: VecDeque<(DragSessionId, PendingOsDrag)>,
    pub(crate) incoming: Vec<Incoming>,
    /// The session a keyboard or assistive cut or copy holds for the next
    /// paste: at most one per runtime, as a clipboard holds one content.
    pub(crate) held: Option<(usize, DragSessionId)>,
}

impl DragSessions {
    pub(crate) fn new(nonce: ProcessNonce, runtime: UiRuntimeId) -> Self {
        Self {
            entries: Slab::new(),
            mint: IdMint::new(),
            nonce,
            runtime,
            presentations: Vec::new(),
            pending: VecDeque::new(),
            incoming: Vec::new(),
            held: None,
        }
    }

    pub(crate) fn key_of(&self, id: DragSessionId) -> DragSessionKey {
        DragSessionKey::new(self.nonce, self.runtime, id)
    }

    pub(crate) fn entry_mut(&mut self, slot: usize, id: DragSessionId) -> Option<&mut Entry> {
        self.entries.get_mut(slot).filter(|entry| entry.id == id)
    }

    pub(crate) fn entry(&self, slot: usize, id: DragSessionId) -> Option<&Entry> {
        self.entries.get(slot).filter(|entry| entry.id == id)
    }

    /// The live session an identity names, if it names one of this runtime's.
    pub(crate) fn find_live(&self, key: DragSessionKey) -> Option<&Entry> {
        if key.nonce() != self.nonce || key.runtime() != self.runtime {
            return None;
        }
        self.entries
            .iter()
            .map(|(_, entry)| entry)
            .find(|entry| entry.id == key.session())
            .filter(|entry| !matches!(entry.phase, DragPhase::Concluded { .. }))
    }

    /// The offer `id` over `presentation`'s window. The same offer over two
    /// windows is two hovers, each with its own receivers.
    pub(crate) fn incoming_mut(
        &mut self,
        presentation: PresentationId,
        id: DataTransferId,
    ) -> Option<&mut Incoming> {
        self.incoming
            .iter_mut()
            .find(|incoming| incoming.is(presentation, id))
    }
}

pub(crate) type Shared = Rc<RefCell<DragSessions>>;
pub(crate) type WeakShared = Weak<RefCell<DragSessions>>;

/// The pointer kind hit tests ask with when a session knows none.
pub(crate) fn pointer_kind(
    pointer: Option<flui_platform_api::pointer::PointerInfo>,
) -> PointerKind {
    pointer.map_or(PointerKind::Unknown, |info| info.kind)
}
