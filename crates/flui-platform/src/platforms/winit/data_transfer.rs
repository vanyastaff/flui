//! Winit data-transfer source: system file drops as ADR-0038 offers.
//!
//! Owns the platform instance's **one** [`OfferTable`] — the single minting
//! authority for every [`DataTransferId`] this backend can emit. The event
//! pump reports winit's `HoveredFile` / `DroppedFile` / `HoveredFileCancelled`
//! window events into the session state machine here and dispatches the
//! [`DragDropEvent`]s it returns; all methods return events instead of
//! dispatching so no handler ever runs under this source's lock (ADR-0038 §5
//! lock discipline).
//!
//! # Approximation ceiling (documented, not hidden)
//!
//! winit exposes file drops only, with no negotiation channel and no
//! end-of-burst marker, so this backend approximates the transport:
//!
//! - offers carry a single `text/uri-list` representation — files only;
//! - `Dropped.action` is stamped [`TransferActions::COPY`], the file-manager
//!   convention, because winit cannot report the real effect;
//! - `update_drop_feedback` is a no-op — winit has no API to answer the
//!   OS's hover queries; `conclude_drop` has no protocol resources to
//!   release but does retire the offer locally, so a concluded id goes
//!   stale as the trait contract promises;
//! - the drop boundary is heuristic: winit delivers one `DroppedFile` per
//!   file with no terminator, so the burst is frozen at the first
//!   `about_to_wait` after at least one `DroppedFile`. A straggler
//!   `DroppedFile` after that freeze mints a **new** defensive session, so a
//!   pathological split burst surfaces as two complete drops, never one
//!   truncated one;
//! - a LOST `HoveredFileCancelled` (known flaky on some X11 setups) leaves
//!   the unfrozen session live, so the next drag's `HoveredFile` paths
//!   accumulate into it — two distinct drags can merge into one offer with
//!   no second `Entered`. Bounded per window; the session resolves at the
//!   next freeze or teardown either way.
//!
//! Native backends (Win32 `IDropTarget`, AppKit `NSDraggingDestination`,
//! Wayland `wl_data_device`) remove every one of these ceilings — the trait
//! and event surface already carry what they need (ADR-0038 §5).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use flui_foundation::DataTransferId;
use flui_types::geometry::{Pixels, Point};
use parking_lot::Mutex;

use crate::data_transfer::{
    DataTransferOffer, DataTransferSource, DropFeedback, OfferRecord, OfferTable,
    RepresentationDescriptor, RepresentationIndex, TransferActions, TransferCompleter,
    TransferError, TransferFormat, TransferLimits, TransferPayload, TransferRequest, TransferUri,
};
use crate::traits::{DragDropEvent, WindowId};

/// One drag session over one window (at most one live per window).
#[derive(Debug)]
struct DragSession {
    /// The offer id minted for this session.
    id: DataTransferId,
    /// Paths accumulated from `HoveredFile` / `DroppedFile` events, drained
    /// into the offer's `UriList` payload at the freeze.
    paths: Vec<PathBuf>,
    /// At least one `DroppedFile` arrived — the next `about_to_wait` freezes
    /// the burst.
    dropped: bool,
    /// The burst was frozen: the payload is snapshot in the offer record and
    /// the session no longer accepts accumulation.
    frozen: bool,
    /// Deliveries requested before the freeze; completed at the freeze or
    /// dropped (→ `SourceGone`) when the session is torn down.
    pending: Vec<(TransferCompleter, TransferLimits)>,
}

/// Interior state: the one offer table plus per-window sessions.
#[derive(Debug, Default)]
struct DragDropState {
    table: OfferTable,
    sessions: HashMap<WindowId, DragSession>,
}

/// The winit backend's single [`DataTransferSource`] (ADR-0038).
///
/// Constructed once at platform init, held in the backend's shared state,
/// and returned by `Platform::data_transfer()` as clones of the same `Arc` —
/// never reconstructed per call. DnD-capable within the winit approximation
/// ceiling (see the module docs); `clipboard_offer` honestly returns `None`
/// until the clipboard transport lands (ADR-0038 §6).
#[derive(Debug, Default)]
pub struct WinitDataTransfer {
    /// Private lock; no guard escapes any signature (SP-6).
    state: Mutex<DragDropState>,
}

impl WinitDataTransfer {
    /// An empty source with no live sessions.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A `HoveredFile` event: accumulate into the live accepting session, or
    /// mint a new session (retiring any predecessor) and return the
    /// `Entered` event to dispatch.
    pub(crate) fn note_hovered_file(
        &self,
        window: WindowId,
        path: PathBuf,
        position: Option<Point<Pixels>>,
    ) -> Option<DragDropEvent> {
        self.accumulate_or_begin(window, path, false, position)
    }

    /// A `DroppedFile` event: accumulate into the live accepting session and
    /// arm the freeze, or — defensive minting — start a new session when
    /// none is accepting (no prior `HoveredFile`, or a straggler after the
    /// freeze), returning its `Entered` event.
    pub(crate) fn note_dropped_file(
        &self,
        window: WindowId,
        path: PathBuf,
        position: Option<Point<Pixels>>,
    ) -> Option<DragDropEvent> {
        self.accumulate_or_begin(window, path, true, position)
    }

    /// A `HoveredFileCancelled` event: retire the accepting session (bumping
    /// its generation, so the id goes stale) and return the `Exited` event.
    /// A cancel after the freeze is ignored — the drop already completed and
    /// its offer stays redeemable.
    pub(crate) fn note_hover_cancelled(&self, window: WindowId) -> Option<DragDropEvent> {
        let (event, stale_pending) = {
            let mut state = self.state.lock();
            match state.sessions.get(&window) {
                None => return None,
                Some(session) if session.frozen => {
                    tracing::trace!(?window, "ignoring hover-cancel after the drop burst froze");
                    return None;
                }
                Some(_) => {}
            }
            let session = state
                .sessions
                .remove(&window)
                .expect("BUG: drag session vanished while the state lock was held");
            state.table.retire(session.id);
            (DragDropEvent::Exited { id: session.id }, session.pending)
        };
        // Parked completers resolve SourceGone from their Drop — outside the
        // lock, so a waker can never re-enter this source under it.
        drop(stale_pending);
        Some(event)
    }

    /// Windows whose drop burst is complete-so-far and awaiting the freeze.
    /// Cheap; empty in the steady state, so `about_to_wait` stays a no-op.
    pub(crate) fn windows_awaiting_freeze(&self) -> Vec<WindowId> {
        self.state
            .lock()
            .sessions
            .iter()
            .filter(|(_, session)| session.dropped && !session.frozen)
            .map(|(window, _)| *window)
            .collect()
    }

    /// Freeze every completed drop burst: snapshot the accumulated paths as
    /// the offer's `UriList` payload, complete parked deliveries, and return
    /// the `Dropped` events to dispatch. The action is stamped
    /// [`TransferActions::COPY`] — winit cannot report the real effect
    /// (documented approximation).
    pub(crate) fn freeze_completed(
        &self,
        positions: &HashMap<WindowId, Point<Pixels>>,
    ) -> Vec<(WindowId, DragDropEvent)> {
        let mut events = Vec::new();
        let mut completions: Vec<(TransferCompleter, Result<TransferPayload, TransferError>)> =
            Vec::new();
        {
            let mut state = self.state.lock();
            let DragDropState { table, sessions } = &mut *state;
            for (window, session) in sessions.iter_mut() {
                if !session.dropped || session.frozen {
                    continue;
                }
                let uris: Vec<TransferUri> =
                    session.paths.drain(..).map(TransferUri::Path).collect();
                let payload = TransferPayload::UriList(uris);
                let actual = payload.byte_len();
                if let Some(record) = table.get_mut(session.id) {
                    record.set_payload(payload.clone());
                }
                session.frozen = true;
                for (completer, limits) in session.pending.drain(..) {
                    let result = if actual > limits.max_bytes {
                        Err(TransferError::TooLarge {
                            actual,
                            limit: limits.max_bytes,
                        })
                    } else {
                        Ok(payload.clone())
                    };
                    completions.push((completer, result));
                }
                events.push((
                    *window,
                    DragDropEvent::Dropped {
                        id: session.id,
                        action: TransferActions::COPY,
                        position: positions.get(window).copied(),
                    },
                ));
            }
        }
        // Completions wake consumer wakers — strictly outside the lock.
        for (completer, result) in completions {
            completer.complete(result);
        }
        events
    }

    /// Tear down any session addressed at a closed window: retire the offer
    /// and let parked deliveries resolve `SourceGone`.
    pub(crate) fn forget_window(&self, window: WindowId) {
        let stale_pending = {
            let mut state = self.state.lock();
            state.sessions.remove(&window).map(|session| {
                state.table.retire(session.id);
                session.pending
            })
        };
        drop(stale_pending);
    }

    /// Shared body of the two path-bearing arms: accumulate into a live
    /// accepting session, or begin a new one and return its `Entered` event.
    fn accumulate_or_begin(
        &self,
        window: WindowId,
        path: PathBuf,
        dropped: bool,
        position: Option<Point<Pixels>>,
    ) -> Option<DragDropEvent> {
        let (event, stale_pending) = {
            let mut state = self.state.lock();
            if let Some(session) = state.sessions.get_mut(&window)
                && !session.frozen
            {
                // winit re-reports every hovered path through the
                // `DroppedFile` arm at the actual drop, so each file of one
                // drag arrives twice. The session tracks the drag's file
                // SET, not the event log — an already-known path only arms
                // the drop flag. (Linear scan: a drag's file count is
                // small; worst case O(n²) over the burst.)
                if !session.paths.contains(&path) {
                    session.paths.push(path);
                }
                session.dropped |= dropped;
                return None;
            }

            // Mint a new session. One live session slot per window
            // (ADR-0038 §2): the predecessor — frozen or not — is retired,
            // so its id goes stale and its slot's generation bumps.
            let mut stale_pending = Vec::new();
            if let Some(previous) = state.sessions.remove(&window) {
                state.table.retire(previous.id);
                stale_pending = previous.pending;
            }
            let representations: Arc<[RepresentationDescriptor]> =
                Arc::from([RepresentationDescriptor {
                    format: TransferFormat::UriList,
                    declared_len: None,
                }]);
            let id = state
                .table
                .mint(OfferRecord::new(Arc::clone(&representations)));
            state.sessions.insert(
                window,
                DragSession {
                    id,
                    paths: vec![path],
                    dropped,
                    frozen: false,
                    pending: Vec::new(),
                },
            );
            (
                DragDropEvent::Entered {
                    offer: DataTransferOffer::new(id, representations),
                    // winit reports no source-permitted set; COPY is the
                    // file-manager convention (documented approximation).
                    allowed: TransferActions::COPY,
                    position,
                },
                stale_pending,
            )
        };
        drop(stale_pending);
        Some(event)
    }
}

impl DataTransferSource for WinitDataTransfer {
    fn clipboard_offer(&self) -> Option<DataTransferOffer> {
        // The clipboard half of this source arrives with the clipboard
        // transport (ADR-0038 §6). Until then there is honestly no offer —
        // not a fabricated one over the sync `Clipboard` trait.
        None
    }

    fn request(
        &self,
        id: DataTransferId,
        representation: RepresentationIndex,
        limits: TransferLimits,
    ) -> TransferRequest {
        let mut state = self.state.lock();
        let DragDropState { table, sessions } = &mut *state;
        let Some(record) = table.get(id) else {
            return TransferRequest::ready(Err(TransferError::StaleOffer(id)));
        };
        if usize::from(representation.0) >= record.representations().len() {
            return TransferRequest::ready(Err(TransferError::UnknownRepresentation {
                id,
                index: representation,
            }));
        }
        if let Some(payload) = record.payload() {
            // Frozen file offer: the path list is genuinely in memory, so
            // the delivery resolves synchronously — not fake async.
            let actual = payload.byte_len();
            if actual > limits.max_bytes {
                return TransferRequest::ready(Err(TransferError::TooLarge {
                    actual,
                    limit: limits.max_bytes,
                }));
            }
            return TransferRequest::ready(Ok(payload.clone()));
        }
        // Live pre-drop session: park the completer; the drop-burst freeze
        // completes it, and session teardown drops it (→ `SourceGone`).
        if let Some(session) = sessions.values_mut().find(|session| session.id == id) {
            let (request, completer) = TransferRequest::channel();
            session.pending.push((completer, limits));
            return request;
        }
        // A record without payload always belongs to a live session in this
        // source; a miss means the producer side is gone.
        TransferRequest::ready(Err(TransferError::SourceGone))
    }

    fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
        // Documented no-op: winit exposes no channel through which the OS's
        // synchronous hover queries could be answered, so the drop can be
        // neither rejected nor renegotiated here (ADR-0038 approximation
        // ceiling; native backends implement the real feedback loop).
        tracing::trace!(%id, ?feedback, "winit exposes no drop-feedback channel; ignoring");
    }

    fn conclude_drop(&self, id: DataTransferId) {
        // winit holds no PROTOCOL resources to release — but table
        // retirement is purely local, and the trait contract promises the
        // offer goes stale after conclusion. Honor it here so downstream
        // consumers never see a source that silently keeps concluded
        // offers redeemable until the next mint.
        let stale_pending = {
            let mut state = self.state.lock();
            state.table.retire(id);
            // Drop any session still tracking this offer (a frozen session
            // whose drop the consumer has now concluded).
            let window = state
                .sessions
                .iter()
                .find(|(_, session)| session.id == id)
                .map(|(window, _)| *window);
            window.and_then(|window| state.sessions.remove(&window))
        };
        // Parked completers (if any survived the freeze) resolve from their
        // Drop — outside the lock, so a waker cannot re-enter this source.
        drop(stale_pending);
        tracing::trace!(%id, "drop concluded; offer retired");
    }
}

// These are the winit arm-conversion tests referenced by ADR-0038. They are
// valuable locally (`just test`), but they are explicitly NOT the CI gate
// evidence for the transport: flui-platform's tests are excluded from the CI
// test job, so the gate-visible transport tests live in
// `crates/flui-app/tests/data_transfer_transport.rs`.
#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use super::*;

    const WINDOW: WindowId = WindowId(1);
    const OTHER_WINDOW: WindowId = WindowId(2);

    fn poll_now(request: TransferRequest) -> Poll<Result<TransferPayload, TransferError>> {
        let mut request = pin!(request);
        let waker = Waker::noop();
        request.as_mut().poll(&mut Context::from_waker(waker))
    }

    fn entered_offer(event: DragDropEvent) -> DataTransferOffer {
        match event {
            DragDropEvent::Entered { offer, .. } => offer,
            other => panic!("expected Entered, got {other:?}"),
        }
    }

    #[test]
    fn hovered_file_mints_a_session_and_emits_entered_once() {
        let source = WinitDataTransfer::new();

        let entered = source
            .note_hovered_file(WINDOW, PathBuf::from("/tmp/a.txt"), None)
            .expect("first path mints the session");
        let DragDropEvent::Entered {
            offer,
            allowed,
            position,
        } = entered
        else {
            panic!("first hovered path must emit Entered");
        };
        assert_eq!(allowed, TransferActions::COPY);
        assert_eq!(position, None);
        assert_eq!(offer.representations().len(), 1);
        assert_eq!(
            offer.find(&TransferFormat::UriList),
            Some(RepresentationIndex(0))
        );

        // Subsequent paths accumulate silently into the same session.
        assert!(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/b.txt"), None)
                .is_none()
        );
    }

    #[test]
    fn hover_cancel_emits_exited_and_makes_the_offer_stale() {
        let source = WinitDataTransfer::new();
        let offer = entered_offer(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/a.txt"), None)
                .expect("mint"),
        );

        let exited = source
            .note_hover_cancelled(WINDOW)
            .expect("live session cancels");
        assert!(matches!(exited, DragDropEvent::Exited { id } if id == offer.id()));

        // The retired id fails the generation check.
        let outcome = poll_now(source.request(
            offer.id(),
            RepresentationIndex(0),
            TransferLimits::default(),
        ));
        assert!(matches!(
            outcome,
            Poll::Ready(Err(TransferError::StaleOffer(id))) if id == offer.id()
        ));

        // Cancelling again is a no-op.
        assert!(source.note_hover_cancelled(WINDOW).is_none());
    }

    #[test]
    fn drop_burst_freezes_at_about_to_wait_and_serves_the_uri_list() {
        let source = WinitDataTransfer::new();
        let offer = entered_offer(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/a.txt"), None)
                .expect("mint"),
        );
        assert!(
            source
                .note_dropped_file(WINDOW, PathBuf::from("/tmp/b.txt"), None)
                .is_none(),
            "drop into a live session accumulates silently"
        );

        assert_eq!(source.windows_awaiting_freeze(), vec![WINDOW]);
        let drops = source.freeze_completed(&HashMap::new());
        assert_eq!(drops.len(), 1);
        let (window, DragDropEvent::Dropped { id, action, .. }) = drops[0].clone() else {
            panic!("freeze must emit Dropped");
        };
        assert_eq!(window, WINDOW);
        assert_eq!(id, offer.id());
        assert_eq!(action, TransferActions::COPY, "documented COPY stamp");
        assert!(source.windows_awaiting_freeze().is_empty());

        // A frozen offer resolves synchronously from memory.
        let outcome = poll_now(source.request(
            offer.id(),
            RepresentationIndex(0),
            TransferLimits::default(),
        ));
        let Poll::Ready(Ok(TransferPayload::UriList(uris))) = outcome else {
            panic!("frozen offer must deliver its UriList, got {outcome:?}");
        };
        assert_eq!(
            uris,
            vec![
                TransferUri::Path(PathBuf::from("/tmp/a.txt")),
                TransferUri::Path(PathBuf::from("/tmp/b.txt")),
            ]
        );
    }

    #[test]
    fn hover_then_drop_of_the_same_files_yields_each_path_once() {
        let source = WinitDataTransfer::new();
        let offer = entered_offer(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/a.txt"), None)
                .expect("mint"),
        );
        assert!(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/b.txt"), None)
                .is_none()
        );
        // The actual drop re-reports both hovered paths (winit's contract):
        // they must not double up in the frozen payload.
        assert!(
            source
                .note_dropped_file(WINDOW, PathBuf::from("/tmp/a.txt"), None)
                .is_none()
        );
        assert!(
            source
                .note_dropped_file(WINDOW, PathBuf::from("/tmp/b.txt"), None)
                .is_none()
        );

        source.freeze_completed(&HashMap::new());
        let outcome = poll_now(source.request(
            offer.id(),
            RepresentationIndex(0),
            TransferLimits::default(),
        ));
        let Poll::Ready(Ok(TransferPayload::UriList(uris))) = outcome else {
            panic!("frozen offer must deliver its UriList, got {outcome:?}");
        };
        assert_eq!(
            uris,
            vec![
                TransferUri::Path(PathBuf::from("/tmp/a.txt")),
                TransferUri::Path(PathBuf::from("/tmp/b.txt")),
            ],
            "each dragged file appears exactly once despite the hover+drop double report",
        );
    }

    #[test]
    fn dropped_file_without_prior_hover_mints_defensively() {
        let source = WinitDataTransfer::new();

        let entered = source
            .note_dropped_file(WINDOW, PathBuf::from("/tmp/only.txt"), None)
            .expect("a drop with no prior hover mints its own session");
        let offer = entered_offer(entered);

        let drops = source.freeze_completed(&HashMap::new());
        assert_eq!(drops.len(), 1);
        assert!(matches!(&drops[0].1, DragDropEvent::Dropped { id, .. } if *id == offer.id()));
    }

    #[test]
    fn straggler_after_freeze_becomes_a_second_complete_drop() {
        let source = WinitDataTransfer::new();
        let first = entered_offer(
            source
                .note_dropped_file(WINDOW, PathBuf::from("/tmp/first.txt"), None)
                .expect("mint"),
        );
        assert_eq!(source.freeze_completed(&HashMap::new()).len(), 1);

        // The straggler mints a NEW defensive session (two complete drops,
        // never one truncated one) — and, per the one-live-session rule,
        // retires the frozen predecessor.
        let second = entered_offer(
            source
                .note_dropped_file(WINDOW, PathBuf::from("/tmp/straggler.txt"), None)
                .expect("straggler mints a fresh session"),
        );
        assert_ne!(first.id(), second.id());

        let drops = source.freeze_completed(&HashMap::new());
        assert_eq!(drops.len(), 1);
        assert!(matches!(&drops[0].1, DragDropEvent::Dropped { id, .. } if *id == second.id()));

        let outcome = poll_now(source.request(
            second.id(),
            RepresentationIndex(0),
            TransferLimits::default(),
        ));
        let Poll::Ready(Ok(TransferPayload::UriList(uris))) = outcome else {
            panic!("second drop must be complete, got {outcome:?}");
        };
        assert_eq!(
            uris,
            vec![TransferUri::Path(PathBuf::from("/tmp/straggler.txt"))]
        );

        let stale = poll_now(source.request(
            first.id(),
            RepresentationIndex(0),
            TransferLimits::default(),
        ));
        assert!(matches!(
            stale,
            Poll::Ready(Err(TransferError::StaleOffer(id))) if id == first.id()
        ));
    }

    #[test]
    fn pre_freeze_request_is_parked_and_completed_at_the_freeze() {
        let source = WinitDataTransfer::new();
        let offer = entered_offer(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/early.txt"), None)
                .expect("mint"),
        );

        let request = source.request(
            offer.id(),
            RepresentationIndex(0),
            TransferLimits::default(),
        );
        let mut request = pin!(request);
        let waker = Waker::noop();
        assert!(
            request
                .as_mut()
                .poll(&mut Context::from_waker(waker))
                .is_pending(),
            "no payload exists before the drop burst freezes"
        );

        source.note_dropped_file(WINDOW, PathBuf::from("/tmp/late.txt"), None);
        source.freeze_completed(&HashMap::new());

        let Poll::Ready(Ok(TransferPayload::UriList(uris))) =
            request.as_mut().poll(&mut Context::from_waker(waker))
        else {
            panic!("the parked delivery must resolve at the freeze");
        };
        assert_eq!(uris.len(), 2);
    }

    #[test]
    fn window_teardown_resolves_parked_deliveries_as_source_gone() {
        let source = WinitDataTransfer::new();
        let offer = entered_offer(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/gone.txt"), None)
                .expect("mint"),
        );
        let request = source.request(
            offer.id(),
            RepresentationIndex(0),
            TransferLimits::default(),
        );

        source.forget_window(WINDOW);

        assert!(matches!(
            poll_now(request),
            Poll::Ready(Err(TransferError::SourceGone))
        ));
    }

    #[test]
    fn unknown_representation_and_limits_are_enforced() {
        let source = WinitDataTransfer::new();
        let offer = entered_offer(
            source
                .note_dropped_file(WINDOW, PathBuf::from("/tmp/limited.txt"), None)
                .expect("mint"),
        );
        source.freeze_completed(&HashMap::new());

        assert!(matches!(
            poll_now(source.request(
                offer.id(),
                RepresentationIndex(7),
                TransferLimits::default(),
            )),
            Poll::Ready(Err(TransferError::UnknownRepresentation { index, .. }))
                if index == RepresentationIndex(7)
        ));

        assert!(matches!(
            poll_now(source.request(
                offer.id(),
                RepresentationIndex(0),
                TransferLimits::default().with_max_bytes(1),
            )),
            Poll::Ready(Err(TransferError::TooLarge { limit: 1, .. }))
        ));
    }

    #[test]
    fn sessions_are_independent_per_window() {
        let source = WinitDataTransfer::new();
        let first = entered_offer(
            source
                .note_hovered_file(WINDOW, PathBuf::from("/tmp/one.txt"), None)
                .expect("mint"),
        );
        let second = entered_offer(
            source
                .note_hovered_file(OTHER_WINDOW, PathBuf::from("/tmp/two.txt"), None)
                .expect("independent window mints its own session"),
        );
        assert_ne!(first.id(), second.id());

        // Cancelling one window's drag leaves the other live.
        source.note_hover_cancelled(WINDOW);
        assert!(
            source
                .note_dropped_file(OTHER_WINDOW, PathBuf::from("/tmp/more.txt"), None)
                .is_none(),
            "the other window's session still accumulates"
        );
    }
}
