//! Shared external build queue and its panic-safe frame-wake delivery state.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use flui_foundation::panic::FirstPanic;
use flui_foundation::{ElementId, RebuildReasons};
use parking_lot::Mutex;

#[derive(Default)]
struct WakeState {
    active_threads: HashSet<std::thread::ThreadId>,
    reentrant_threads: HashSet<std::thread::ThreadId>,
}

/// One frame request a wake answers. Tickets are compared under the ledger's
/// lock, so a wake that began before a newer request never acknowledges it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WakeTicket(u64);

/// The frame request still owed, if any.
#[derive(Debug, Default)]
struct WakeLedger {
    issued: u64,
    owed: Option<WakeTicket>,
    /// Set once `issued` cannot advance: tickets would repeat, so from then on
    /// no wake acknowledges a request and each later one retries it.
    exhausted: bool,
}

impl WakeLedger {
    fn request(&mut self) -> WakeTicket {
        match self.issued.checked_add(1) {
            Some(next) => self.issued = next,
            None => self.exhausted = true,
        }
        let ticket = WakeTicket(self.issued);
        self.owed = Some(ticket);
        ticket
    }

    fn acknowledge(&mut self, ticket: WakeTicket) {
        if !self.exhausted && self.owed == Some(ticket) {
            self.owed = None;
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum BuildStamp {
    Tracked(u64),
    Untrackable,
}

impl BuildStamp {
    fn advance(&mut self) {
        *self = match *self {
            Self::Tracked(value) => value
                .checked_add(1)
                .map_or(Self::Untrackable, Self::Tracked),
            Self::Untrackable => Self::Untrackable,
        };
    }

    fn compatible(self, saved: Self) -> bool {
        matches!((self, saved), (Self::Tracked(now), Self::Tracked(previous)) if now == previous)
    }
}

/// Opaque accepted build-input premise for a suspended presentation segment.
/// Local and external admissions share one inbox identity. Exhausted stamps
/// never match, even themselves; this token intentionally has no equality API.
#[derive(Clone, Debug)]
#[must_use]
pub struct BuildPremise {
    inbox: Weak<ExternalBuildInbox>,
    stamp: BuildStamp,
}

/// Rebuild requests waiting for a drain, kept in the order each id was first
/// queued.
///
/// A drain pushes them onto the dirty heap in this order, which orders
/// same-depth elements among themselves; a hash map's own order changes from
/// map to map.
#[derive(Debug, Default)]
pub(crate) struct PendingBuilds {
    order: Vec<ElementId>,
    reasons: HashMap<ElementId, RebuildReasons>,
}

impl PendingBuilds {
    /// Queue `reasons` for `id`, behind every id already queued; an id
    /// already queued keeps its place and merges the causes. Returns whether
    /// `id` was newly queued.
    pub(crate) fn merge(&mut self, id: ElementId, reasons: RebuildReasons) -> bool {
        match self.reasons.entry(id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(reasons);
                self.order.push(id);
                true
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().merge(reasons);
                false
            }
        }
    }

    pub(crate) fn get(&self, id: ElementId) -> Option<RebuildReasons> {
        self.reasons.get(&id).copied()
    }

    /// The queued ids, first queued first.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> &[ElementId] {
        &self.order
    }

    pub(crate) fn len(&self) -> usize {
        self.order.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.order.clear();
        self.reasons.clear();
    }

    /// Remove every request, first queued first.
    pub(crate) fn take(&mut self) -> Vec<(ElementId, RebuildReasons)> {
        let reasons = &mut self.reasons;
        let taken = self
            .order
            .drain(..)
            .map(|id| {
                let queued = reasons
                    .remove(&id)
                    .expect("BUG: every queued id has its reasons");
                (id, queued)
            })
            .collect();
        debug_assert!(reasons.is_empty(), "BUG: every id with reasons is queued");
        taken
    }
}

/// Queue contents and accepted-input identity committed under one guard.
struct PendingState {
    builds: PendingBuilds,
    premise: BuildStamp,
}

impl Default for PendingState {
    fn default() -> Self {
        Self {
            builds: PendingBuilds::default(),
            premise: BuildStamp::Tracked(0),
        }
    }
}

/// Shared external work and wake-retry state for one build owner.
#[derive(Default)]
pub(crate) struct ExternalBuildInbox {
    closed: AtomicBool,
    pending: Mutex<PendingState>,
    current_wake: Mutex<WakeLedger>,
    wake_state: Mutex<WakeState>,
}

impl ExternalBuildInbox {
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        let mut pending = self.pending.lock();
        self.closed.store(true, Ordering::Release);
        pending.builds.clear();
        drop(pending);
        self.current_wake.lock().owed = None;
    }

    /// Existing-work transport, without admitting a new build premise.
    pub(crate) fn lock(&self) -> parking_lot::MappedMutexGuard<'_, PendingBuilds> {
        parking_lot::MutexGuard::map(self.pending.lock(), |state| &mut state.builds)
    }

    pub(crate) fn try_len(&self) -> Option<usize> {
        self.pending.try_lock().map(|pending| pending.builds.len())
    }

    pub(crate) fn premise(self: &Arc<Self>) -> BuildPremise {
        let stamp = self.pending.lock().premise;
        BuildPremise {
            inbox: Arc::downgrade(self),
            stamp,
        }
    }

    pub(crate) fn accepts_premise(self: &Arc<Self>, saved: &BuildPremise) -> bool {
        if !Weak::ptr_eq(&Arc::downgrade(self), &saved.inbox) {
            return false;
        }
        let pending = self.pending.lock();
        !self.is_closed() && pending.premise.compatible(saved.stamp)
    }

    /// Commit accepted local state independently of dirty-heap deduplication.
    pub(crate) fn note_local_admission(&self) {
        let mut pending = self.pending.lock();
        if !self.is_closed() {
            pending.premise.advance();
        }
    }

    /// Admit one complete batch before waking. A repeated queued element still
    /// changes the authored premise; queue transport uses `lock` instead.
    pub(crate) fn admit_batch(&self, ids: &[ElementId], reasons: RebuildReasons) -> bool {
        let mut pending = self.pending.lock();
        if self.is_closed() || ids.is_empty() {
            return false;
        }
        pending.premise.advance();
        let mut newly_queued = false;
        for &id in ids {
            newly_queued |= pending.builds.merge(id, reasons);
        }
        newly_queued
    }

    fn request_ticket(&self) -> WakeTicket {
        self.current_wake.lock().request()
    }

    /// Owe a frame for fresh work without waking now: the next
    /// [`request_frame_if_needed`](Self::request_frame_if_needed) delivers
    /// it, as it retries a wake that panicked.
    pub(crate) fn owe_frame(&self, has_fresh_work: bool) {
        if has_fresh_work && !self.is_closed() {
            let _owed = self.request_ticket();
        }
    }

    fn pending_ticket(&self) -> Option<WakeTicket> {
        self.current_wake.lock().owed
    }

    /// [`request_frame_if_needed`](Self::request_frame_if_needed) for a
    /// request made on behalf of user code (a listener, a callback, an event
    /// handler) that has accepted work and goes on after it: a failing wake
    /// never unwinds into that caller, which would lose what it does next.
    /// The work stays queued, the wake stays owed for the next request to
    /// retry, and the failure is reported, as the runtime reports a contained
    /// application callback.
    pub(crate) fn request_frame_contained(
        &self,
        has_fresh_work: bool,
        request_frame: Option<&(dyn Fn() + Send + Sync)>,
    ) {
        let mut wake = FirstPanic::new();
        wake.contain(|| self.request_frame_if_needed(has_fresh_work, request_frame));
        if let Some(payload) = wake.held() {
            // A panicking log is contained too, so the report never unwinds
            // into the caller either. Both payloads are destroyed under
            // containment when their collectors drop.
            FirstPanic::new().contain(|| {
                tracing::error!(
                    panic_message =
                        flui_foundation::panic::payload_text(payload).unwrap_or("<opaque>"),
                    "a frame request failed; the work stays queued and the wake owed"
                );
            });
        }
    }

    /// Request a frame for fresh work or retry a wake that previously
    /// panicked after work was committed.
    ///
    /// Calls never wait behind an arbitrary external wake hook. Concurrent
    /// callers may race delivery, and each successful hook acknowledges only
    /// the work ticket captured before that hook began. A same-thread
    /// reentrant caller cannot invoke the hook recursively, so its outer call
    /// makes at most one compensating attempt before returning or resuming the
    /// first panic.
    pub(crate) fn request_frame_if_needed(
        &self,
        has_fresh_work: bool,
        request_frame: Option<&(dyn Fn() + Send + Sync)>,
    ) {
        if self.is_closed() {
            return;
        }
        let Some(request_frame) = request_frame else {
            return;
        };
        let requested = has_fresh_work.then(|| self.request_ticket());
        let Some(mut ticket) = requested.or_else(|| self.pending_ticket()) else {
            return;
        };

        let current_thread = std::thread::current().id();
        {
            let mut state = self.wake_state.lock();
            if !state.active_threads.insert(current_thread) {
                state.reentrant_threads.insert(current_thread);
                return;
            }
        }

        let mut first_panic = FirstPanic::new();
        let mut may_compensate = true;
        loop {
            // The hook runs contained so the wake state settles.
            let woke = first_panic.contain(request_frame).is_some();
            let reentered = {
                let mut state = self.wake_state.lock();
                state.reentrant_threads.remove(&current_thread)
            };

            if woke {
                self.current_wake.lock().acknowledge(ticket);
            }

            let compensation = (reentered && may_compensate)
                .then(|| self.pending_ticket())
                .flatten();
            if let Some(pending_ticket) = compensation {
                may_compensate = false;
                ticket = pending_ticket;
                continue;
            }

            self.wake_state
                .lock()
                .active_threads
                .remove(&current_thread);
            first_panic.resume();
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildStamp, ExternalBuildInbox, WakeLedger};
    use flui_foundation::{ElementId, RebuildReason, RebuildReasons};
    use std::sync::Arc;

    // A terminal stamp cannot be reached with public scheduling in a finite
    // test. Seed only the initial counter, then exercise actual admission,
    // occupied-queue dedup, transport and local scheduling.
    #[test]
    fn exhausted_build_premise_keeps_work_deliverable() {
        let inbox = Arc::new(ExternalBuildInbox::default());
        inbox.pending.lock().premise = BuildStamp::Tracked(u64::MAX - 1);
        let first = ElementId::new(1);
        let second = ElementId::new(2);
        let reasons = RebuildReasons::from_reason(RebuildReason::StateChange);
        assert!(inbox.admit_batch(&[first], reasons));
        let last_tracked = inbox.premise();
        assert!(inbox.accepts_premise(&last_tracked));
        assert!(!inbox.admit_batch(&[first], reasons));
        let exhausted = inbox.premise();
        assert!(!inbox.accepts_premise(&exhausted));
        assert!(!inbox.accepts_premise(&last_tracked));
        let delivered = inbox.lock().take();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].0, first);
        inbox.lock().merge(first, reasons);
        inbox.note_local_admission();
        assert!(!inbox.accepts_premise(&exhausted));
        assert!(inbox.admit_batch(&[second], reasons));
        let delivered = inbox.lock().take();
        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0].0, first);
        assert_eq!(delivered[1].0, second);
        assert!(!inbox.accepts_premise(&inbox.premise()));
    }

    #[test]
    fn a_wake_acknowledges_only_the_request_it_answered() {
        let mut ledger = WakeLedger::default();
        let older = ledger.request();
        let newer = ledger.request();
        ledger.acknowledge(older);
        assert_eq!(
            ledger.owed,
            Some(newer),
            "an older wake cleared a newer request"
        );
        ledger.acknowledge(newer);
        assert_eq!(ledger.owed, None);
    }

    #[test]
    fn an_exhausted_ledger_keeps_every_request_owed() {
        let mut ledger = WakeLedger {
            issued: u64::MAX - 1,
            ..WakeLedger::default()
        };
        let last = ledger.request();
        let repeated = ledger.request();
        assert_eq!(last, repeated, "the ticket counter stops at its end");
        ledger.acknowledge(last);
        assert_eq!(
            ledger.owed,
            Some(repeated),
            "a repeated ticket must not let an older wake acknowledge a newer request"
        );
    }
}
