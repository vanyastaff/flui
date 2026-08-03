//! `ClaimSlot<T>` — a generic at-most-once request/reply primitive.
//!
//! ADR-0039 §3 needs a reply protocol strictly stronger than a buffered
//! one-shot channel: today's winit owner lane hands back a
//! `sync_channel(1)` whose send always succeeds once the receiver exists,
//! even if the receiver is later dropped without ever reading it — so a
//! requester that abandons a request *after* the owner replies leaks
//! whatever the reply carries (a created window, on the winit lane). A
//! claim slot closes that gap: the requester side and the owner side share
//! one small state machine, and every transition is linearized under the
//! slot's own private lock (SP-6: this module exposes no lock guard or
//! channel endpoint in any public signature).
//!
//! ```text
//! Pending   ──(owner delivers)──────────▶  Delivered(T)
//! Pending   ──(requester drops)─────────▶  Abandoned(None)
//! Pending   ──(owner drops)─────────────▶  OwnerGone
//! Delivered ──(requester claims)────────▶  Claimed
//! Delivered ──(requester drops)─────────▶  Abandoned(Some(T))
//! ```
//!
//! The owner side ([`ClaimSlot<T>`]) is kept alive by the caller *past*
//! `deliver` — an owner-side in-flight registry (ADR-0039 §3; the winit
//! lane's drain-and-sweep loop) holds it so it can later notice a
//! `Delivered -> Abandoned` transition (the requester dropped without
//! claiming) and reclaim the payload via [`ClaimSlot::take_abandoned`] to
//! unwind it. `deliver` is therefore `&self`, not consuming; at-most-once
//! delivery is a runtime contract on the owner (exactly one `deliver` call
//! per request — the winit lane has exactly one call site), enforced by a
//! `BUG:` panic on violation rather than by the type system. Abandonment
//! (whichever side notices it first) fires an injected wake callback
//! exactly once, so an owner parked on its own event loop learns promptly
//! that it must unwind rather than relying on a fallible reply send it
//! never observes failing.
//!
//! `ClaimSlot`'s own `Drop` additionally covers owner disconnection: if the
//! owner side is dropped while the request is still `Pending` (the owner
//! died, or unwound, without ever calling `deliver`), the slot transitions
//! to the terminal `OwnerGone` state and wakes whichever consumer-side
//! primitive is parked on it — a blocked [`ClaimHandle::wait`] and any
//! registered [`Waker`] both resolve immediately instead
//! of hanging forever on the owner's `deliver` discipline. This is on top
//! of (not a replacement for) that discipline: a well-behaved owner still
//! always calls `deliver` via a panic-safe guard; `OwnerGone` is the
//! consumer-side backstop for the case where something upstream of that
//! guard didn't hold up its end.
//!
//! This primitive is deliberately generic and carries no `flui-platform`
//! vocabulary (no `WindowId`, no `OpenWindowError`) — the ADR places its
//! tests in the winit lane, which CI does not execute; landing the tested
//! core here means the state-machine behavior is CI-verified even though
//! its winit composition is not (the same rationale `OwnerAffinity`
//! already established for the runtime-backstop half of ADR-0039).

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

use parking_lot::{Condvar, Mutex};

/// Fired exactly once, the moment a request transitions into `Abandoned` —
/// always from [`ClaimHandle`]'s `Drop`, the only place that transition
/// happens. `ClaimSlot::deliver` never fires it: when `deliver` observes an
/// already-`Abandoned` slot (the pre-delivery race), the wake already fired
/// when that abandonment happened, on the requester's side, before
/// `deliver` was even called — `deliver` only ever *reads* the outcome, via
/// its `Err` return, and correctly does not re-fire the wake for it.
type WakeOwner = Arc<dyn Fn() + Send + Sync>;

enum SlotState<T> {
    Pending,
    Delivered(T),
    Claimed,
    Abandoned(Option<T>),
    /// The owner side ([`ClaimSlot<T>`]) was dropped while this request was
    /// still `Pending` — it never called [`ClaimSlot::deliver`]. Reachable
    /// only from `Pending` (see `ClaimSlot`'s `Drop` impl); terminal.
    OwnerGone,
}

struct Inner<T> {
    state: Mutex<SlotState<T>>,
    delivered: Condvar,
    wake: WakeOwner,
    // Belt-and-braces, not currently load-bearing: `notify_abandoned` has
    // exactly one call site (`ClaimHandle`'s `Drop`), and `Drop::drop` runs
    // at most once per value by Rust's own ownership rules, so this guard
    // is redundant against today's single caller. Kept so a future call
    // site added to this type could not silently double-fire the wake.
    wake_fired: AtomicBool,
    /// The waker of whichever task last polled this handle via the `Future`
    /// impl (or registered through it), if any. Separate lock from `state`
    /// so waking a task never happens while `state` is held (the same
    /// outside-the-lock discipline `notify_abandoned` already follows for
    /// `wake`) — the woken task may re-enter this module synchronously.
    waker: Mutex<Option<Waker>>,
}

impl<T> Inner<T> {
    /// Fires the wake callback outside the state lock (never call this
    /// while `state` is held — the callback is arbitrary owner code and
    /// must not be able to deadlock by re-entering the slot) and only on
    /// the first abandonment transition this slot ever sees.
    fn notify_abandoned(&self) {
        if !self.wake_fired.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }

    /// Wakes whichever task last polled this handle via `Future::poll`, if
    /// any — called outside `state`'s lock (same discipline as
    /// `notify_abandoned`) from every transition that changes what a poll
    /// would observe: delivery, requester abandonment, and owner
    /// disconnection.
    fn wake_task(&self) {
        if let Some(waker) = self.waker.lock().take() {
            waker.wake();
        }
    }
}

/// Owner-side handle: produces the request's result and, after delivery,
/// can be polled for a late abandonment to unwind.
pub struct ClaimSlot<T> {
    inner: Arc<Inner<T>>,
}

impl<T> fmt::Debug for ClaimSlot<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = match &*self.inner.state.lock() {
            SlotState::Pending => "Pending",
            SlotState::Delivered(_) => "Delivered",
            SlotState::Claimed => "Claimed",
            SlotState::Abandoned(_) => "Abandoned",
            SlotState::OwnerGone => "OwnerGone",
        };
        f.debug_struct("ClaimSlot").field("state", &state).finish()
    }
}

impl<T> ClaimSlot<T> {
    /// True once the slot is `Abandoned`, regardless of whether it still
    /// carries a reclaimable payload. Before delivery this is the "should I
    /// even bother producing `T`?" check (the owner lane skips creation
    /// entirely); after delivery it is the "does the sweep need to reclaim
    /// this?" check (paired with [`take_abandoned`](Self::take_abandoned)).
    ///
    /// Does not report `OwnerGone` — that transition is this slot's own
    /// `Drop`, so by the time any code could observe it through this method
    /// the `ClaimSlot` value is already gone.
    #[must_use]
    pub fn is_abandoned(&self) -> bool {
        matches!(*self.inner.state.lock(), SlotState::Abandoned(_))
    }

    /// True once the request is fully resolved — `Claimed`, or `Abandoned`
    /// with its payload already reclaimed via
    /// [`take_abandoned`](Self::take_abandoned). An owner-side in-flight
    /// registry sweeps entries where this is `true`.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        matches!(
            *self.inner.state.lock(),
            SlotState::Claimed | SlotState::Abandoned(None)
        )
    }

    /// Delivers the completed value, `Pending -> Delivered`.
    ///
    /// If the requester abandoned the slot before this call, the value is
    /// handed back via `Err` so the owner can unwind whatever it just
    /// produced immediately. If the requester abandons *after* a
    /// successful delivery, the transition happens on the requester's side
    /// (`Delivered -> Abandoned(Some(value))`) and the owner reclaims it
    /// later via [`take_abandoned`](Self::take_abandoned) — `deliver`
    /// itself only ever observes the pre-delivery race.
    ///
    /// # Errors
    /// Returns `Err(value)` when the requester already abandoned the slot
    /// before this call — the caller must unwind `value` instead of handing
    /// it to anyone.
    ///
    /// # Panics
    /// Calling `deliver` a second time on the same slot is an owner-side
    /// contract violation (this primitive supports exactly one delivery
    /// per request); it panics rather than silently double-delivering.
    /// `OwnerGone` is likewise unreachable here — that transition is this
    /// same `ClaimSlot` value's own `Drop`, so no further method call on it
    /// can observe it.
    pub fn deliver(&self, value: T) -> Result<(), T> {
        let mut state = self.inner.state.lock();
        match &*state {
            SlotState::Pending => {
                *state = SlotState::Delivered(value);
                drop(state);
                self.inner.delivered.notify_one();
                self.inner.wake_task();
                Ok(())
            }
            SlotState::Abandoned(None) => {
                // No write-back needed: the state is already exactly
                // `Abandoned(None)` (confirmed by the match arm above), so
                // there is nothing to update before handing `value` back.
                drop(state);
                Err(value)
            }
            SlotState::Delivered(_)
            | SlotState::Claimed
            | SlotState::Abandoned(Some(_))
            | SlotState::OwnerGone => {
                unreachable!(
                    "BUG: ClaimSlot::deliver called a second time on the same \
                     request — exactly one deliver call per request is the \
                     owner-side contract this primitive assumes"
                )
            }
        }
    }

    /// Reclaims the payload of a `Delivered -> Abandoned` transition (the
    /// requester claimed delivery, then dropped its handle without ever
    /// calling `try_take`/`wait`). `None` if the slot never delivered, was
    /// abandoned before delivery (already returned via `deliver`'s `Err`),
    /// or was already reclaimed. Idempotent past the first successful call.
    #[must_use]
    pub fn take_abandoned(&self) -> Option<T> {
        let mut state = self.inner.state.lock();
        match &mut *state {
            SlotState::Abandoned(payload) => payload.take(),
            SlotState::Pending
            | SlotState::Delivered(_)
            | SlotState::Claimed
            | SlotState::OwnerGone => None,
        }
    }
}

impl<T> Drop for ClaimSlot<T> {
    /// Owner-disconnect transition (ADR-0039 §3/slice-2 amendment):
    /// `Pending -> OwnerGone` if the owner drops this handle without ever
    /// calling [`deliver`](Self::deliver) — the owner died mid-request, or
    /// unwound before reaching its `deliver` guard. Wakes both consumer-side
    /// waiting primitives: a blocked [`ClaimHandle::wait`] (via the
    /// condvar) and a parked `Future` poll (via the registered
    /// [`Waker`]), so neither hangs forever on discipline the owner itself
    /// failed to uphold. A no-op past `Pending` — the request already has a
    /// resolution (delivered, or already abandoned by the requester) that
    /// this transition must not clobber.
    fn drop(&mut self) {
        let became_owner_gone = {
            let mut state = self.inner.state.lock();
            if matches!(*state, SlotState::Pending) {
                *state = SlotState::OwnerGone;
                true
            } else {
                false
            }
        }; // lock released here — notify/wake must never run under it.
        if became_owner_gone {
            self.inner.delivered.notify_all();
            self.inner.wake_task();
        }
    }
}

/// Requester-side handle: claims the delivered value, or abandons the
/// request on drop.
///
/// Dropping a `ClaimHandle` before claiming its value is the deliberate
/// cancellation path (a dying realm, a finished worker) — it is not an
/// error, but it does transition the slot to `Abandoned` and wake the
/// owner so the owner can unwind promptly rather than leak.
#[must_use = "dropping a ClaimHandle before claiming abandons the request; \
              see `ClaimSlot` module docs for the unwind contract"]
pub struct ClaimHandle<T> {
    inner: Arc<Inner<T>>,
}

impl<T> fmt::Debug for ClaimHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClaimHandle").finish_non_exhaustive()
    }
}

/// Outcome of resolving a [`ClaimHandle`] — returned by
/// [`ClaimHandle::wait`] and, wrapped in [`Poll`], by its `Future` impl.
/// `Delivered`/`AlreadyClaimed`/`OwnerGone` mirror three of the state
/// diagram's terminal destinations; the fourth, `Abandoned`, is never
/// reachable through this type (see the `Abandoned` note below).
#[derive(Debug, PartialEq, Eq)]
pub enum ClaimOutcome<T> {
    /// The owner delivered a value; this call/poll claims it.
    Delivered(T),
    /// Already claimed by an earlier [`try_take`](ClaimHandle::try_take) (or
    /// a previous resolution of the same `Future`) on this same handle —
    /// nothing is left to hand back. A caller-sequencing fact, not a
    /// slot-invariant violation.
    AlreadyClaimed,
    /// The owner side ([`ClaimSlot`]) was dropped before ever delivering.
    OwnerGone,
}

impl<T> ClaimHandle<T> {
    /// Registers `waker` to be woken exactly once, the next time this
    /// request's outcome changes (delivery, or the owner disconnecting).
    /// Replaces any previously registered waker — the most recent `poll`
    /// wins, per the `Future` contract. Called before re-checking `state`
    /// (not after) so a transition landing between an unguarded check and
    /// registration can never go unobserved: either the transition already
    /// happened and the immediate re-check below sees it, or it happens
    /// later and wakes the now-registered waker.
    fn register_waker(&self, waker: &Waker) {
        let mut slot = self.inner.waker.lock();
        match &*slot {
            Some(existing) if existing.will_wake(waker) => {}
            _ => *slot = Some(waker.clone()),
        }
    }

    /// The shared non-blocking check behind [`try_take`](Self::try_take) and
    /// the `Future` impl. `waker` is `Some` only from the `Future` path —
    /// `try_take` itself never parks anything.
    fn poll_claim(&mut self, waker: Option<&Waker>) -> Poll<ClaimOutcome<T>> {
        if let Some(waker) = waker {
            self.register_waker(waker);
        }
        let mut state = self.inner.state.lock();
        match std::mem::replace(&mut *state, SlotState::Claimed) {
            SlotState::Delivered(value) => Poll::Ready(ClaimOutcome::Delivered(value)),
            SlotState::Pending => {
                *state = SlotState::Pending;
                Poll::Pending
            }
            SlotState::Claimed => {
                *state = SlotState::Claimed;
                Poll::Ready(ClaimOutcome::AlreadyClaimed)
            }
            SlotState::OwnerGone => {
                *state = SlotState::OwnerGone;
                Poll::Ready(ClaimOutcome::OwnerGone)
            }
            SlotState::Abandoned(_) => unreachable!(
                "BUG: ClaimHandle observed Abandoned while still holding the \
                 sole handle — only this handle's own Drop can abandon it, \
                 and Drop cannot have run while `self` is alive here"
            ),
        }
    }

    /// Non-blocking poll. Returns `Some(T)` exactly once — the first call
    /// observing `Delivered` claims it (`Delivered -> Claimed`) and every
    /// later call (on this handle, or after the request is otherwise
    /// resolved, including `OwnerGone`) returns `None`. Safe on any thread,
    /// including the owner. Callers that need to distinguish "nothing yet"
    /// from "the owner is gone" should poll the `Future` impl instead (or
    /// use [`wait`](Self::wait), off the owner thread).
    #[must_use = "discarding Some(value) strands whatever the owner delivered"]
    pub fn try_take(&mut self) -> Option<T> {
        match self.poll_claim(None) {
            Poll::Ready(ClaimOutcome::Delivered(value)) => Some(value),
            Poll::Ready(ClaimOutcome::AlreadyClaimed | ClaimOutcome::OwnerGone) | Poll::Pending => {
                None
            }
        }
    }

    /// Blocks the calling thread until the request resolves — delivery, or
    /// the owner disconnecting — then claims the outcome.
    ///
    /// Returns [`ClaimOutcome::AlreadyClaimed`] if this handle was already
    /// claimed by an earlier [`try_take`](Self::try_take) call — `try_take`
    /// takes `&mut self`, so nothing in the type system stops a caller from
    /// following a successful `try_take` with a `wait` on the same
    /// still-alive handle; that is a caller-sequencing fact (there is
    /// nothing left to wait for), not a slot-invariant violation. Callers
    /// that hand this outcome to a typed error (e.g. `flui-platform`'s
    /// `PendingWindow::wait`) map each `ClaimOutcome` variant onto their own
    /// `#[non_exhaustive]` error enum.
    ///
    /// This primitive does not itself refuse the owner thread — callers
    /// that must not block their own owner (e.g. `flui-platform`'s
    /// `PendingWindow::wait`, which drains the very lane this would block
    /// on) are responsible for checking thread identity *before* calling
    /// `wait` and taking the non-blocking `try_take` path (or the `Future`
    /// impl, which is always non-blocking) instead.
    ///
    /// # Owner obligation
    /// The owner must still guarantee it always eventually calls `deliver`,
    /// typically via a panic-safe guard that calls it unconditionally on
    /// drop if not already called explicitly (`flui-platform`'s
    /// `OpenWindowReplyGuard` is exactly this guard for the winit lane).
    /// `ClaimSlot`'s `Drop` is the backstop for when that discipline is
    /// broken upstream of the guard (the owner died or unwound before
    /// `deliver` even had a guard protecting it): a blocked `wait` now
    /// resolves to [`ClaimOutcome::OwnerGone`] instead of hanging forever —
    /// the guard is still the primary mechanism, not something this
    /// backstop makes optional.
    #[must_use = "discarding a Delivered outcome strands whatever the owner delivered"]
    pub fn wait(self) -> ClaimOutcome<T> {
        let mut state = self.inner.state.lock();
        loop {
            match &*state {
                SlotState::Delivered(_) => {
                    let SlotState::Delivered(value) =
                        std::mem::replace(&mut *state, SlotState::Claimed)
                    else {
                        unreachable!("BUG: match on &*state just confirmed Delivered")
                    };
                    return ClaimOutcome::Delivered(value);
                }
                SlotState::Pending => {
                    self.inner.delivered.wait(&mut state);
                }
                // Already resolved by an earlier `try_take` on this same
                // handle -- a caller-sequencing fact, not a bug: nothing
                // left to wait for.
                SlotState::Claimed => return ClaimOutcome::AlreadyClaimed,
                // The owner side dropped without ever delivering (its own
                // `Drop` set this and woke this condvar) -- resolve rather
                // than loop forever waiting for a `deliver` that will now
                // never come.
                SlotState::OwnerGone => return ClaimOutcome::OwnerGone,
                SlotState::Abandoned(_) => unreachable!(
                    "BUG: ClaimHandle::wait observed Abandoned while still \
                     holding the sole handle — only this handle's own Drop \
                     can abandon it, and Drop cannot have run while `self` \
                     is alive here"
                ),
            }
        }
    }
}

impl<T> Future for ClaimHandle<T> {
    type Output = ClaimOutcome<T>;

    /// Non-blocking poll; safe on any thread, including the owner (unlike
    /// [`wait`](Self::wait), which refuses there because it would block on
    /// the very lane the owner itself drains). Resolves on delivery, on
    /// owner disconnection ([`ClaimOutcome::OwnerGone`], woken by
    /// `ClaimSlot`'s `Drop`), or immediately if this handle was already
    /// claimed by an earlier [`try_take`](Self::try_take).
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.poll_claim(Some(cx.waker()))
    }
}

impl<T> Drop for ClaimHandle<T> {
    fn drop(&mut self) {
        let became_abandoned = {
            let mut state = self.inner.state.lock();
            match std::mem::replace(&mut *state, SlotState::Claimed) {
                SlotState::Pending => {
                    *state = SlotState::Abandoned(None);
                    true
                }
                SlotState::Delivered(value) => {
                    *state = SlotState::Abandoned(Some(value));
                    true
                }
                already_resolved @ (SlotState::Claimed
                | SlotState::Abandoned(_)
                | SlotState::OwnerGone) => {
                    *state = already_resolved;
                    false
                }
            }
        }; // lock released here — notify_abandoned must never run under it.
        if became_abandoned {
            self.inner.notify_abandoned();
            // No task can still be parked on this same handle's `Future`
            // once the handle itself is being dropped, so this is a no-op
            // in practice — fired anyway for symmetry with `deliver` and
            // `ClaimSlot`'s `Drop`, which both wake unconditionally on
            // their own state-changing transitions.
            self.inner.wake_task();
        }
    }
}

/// Creates a linked owner/requester pair for one request.
///
/// `wake` fires at most once, exactly when the request transitions into
/// `Abandoned` (see the module docs' state diagram) — never on delivery or
/// claim. On the winit lane this is the coalesced owner wake that already
/// exists for enqueue (`control.rs`'s `wake_owner`); a generic caller with
/// no wake-worthy event loop may pass `Arc::new(|| {})`.
#[must_use = "dropping both halves immediately abandons the request for no reason"]
pub fn claim_slot<T>(wake: WakeOwner) -> (ClaimSlot<T>, ClaimHandle<T>) {
    let inner = Arc::new(Inner {
        state: Mutex::new(SlotState::Pending),
        delivered: Condvar::new(),
        wake,
        wake_fired: AtomicBool::new(false),
        waker: Mutex::new(None),
    });
    (
        ClaimSlot {
            inner: Arc::clone(&inner),
        },
        ClaimHandle { inner },
    )
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};
    use std::thread;
    use std::time::Duration;

    use super::{ClaimOutcome, claim_slot};

    fn counting_wake() -> (Arc<dyn Fn() + Send + Sync>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let count_for_wake = Arc::clone(&count);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            count_for_wake.fetch_add(1, Ordering::AcqRel);
        });
        (wake, count)
    }

    #[test]
    fn dropped_before_delivery_abandons_with_no_payload() {
        let (wake, wake_count) = counting_wake();
        let (slot, handle) = claim_slot::<u32>(wake);
        drop(handle);

        assert!(slot.is_abandoned(), "requester dropped before delivery");
        assert_eq!(wake_count.load(Ordering::Acquire), 1);

        let delivered = slot.deliver(7);
        assert_eq!(
            delivered,
            Err(7),
            "owner must get the value back to unwind it"
        );
        assert!(
            slot.is_settled(),
            "after the owner reclaims via deliver's Err, nothing is left to sweep"
        );
    }

    #[test]
    fn dropped_after_delivery_abandons_with_recoverable_payload() {
        let (wake, wake_count) = counting_wake();
        let (slot, handle) = claim_slot::<String>(wake);

        slot.deliver("payload".to_string())
            .expect("slot is still Pending");
        assert_eq!(
            wake_count.load(Ordering::Acquire),
            0,
            "delivery alone must not fire the abandonment wake"
        );

        // Requester claims delivery happened (so a real `PendingWindow`
        // would now be holding the value) is NOT what we're testing here —
        // this test is the "claimed delivery but then dropped without
        // reading it" path, so drop the handle without calling try_take.
        assert!(!slot.is_settled(), "not yet abandoned or claimed");
        drop(handle);

        assert_eq!(
            wake_count.load(Ordering::Acquire),
            1,
            "abandonment after delivery must still wake the owner"
        );
        assert!(slot.is_abandoned());
        assert_eq!(
            slot.take_abandoned(),
            Some("payload".to_string()),
            "the owner must be able to reclaim the payload to unwind it"
        );
        assert_eq!(
            slot.take_abandoned(),
            None,
            "reclaiming is idempotent — a second sweep must not double-unwind"
        );
        assert!(slot.is_settled());
    }

    #[test]
    fn claimed_then_dropped_keeps_the_value_with_the_requester() {
        let (wake, wake_count) = counting_wake();
        let (slot, mut handle) = claim_slot::<u32>(wake);

        slot.deliver(42).expect("slot is still Pending");
        let claimed = handle.try_take();
        assert_eq!(claimed, Some(42));

        drop(handle);
        assert_eq!(
            wake_count.load(Ordering::Acquire),
            0,
            "a claimed-then-dropped handle must not abandon or wake"
        );
        assert!(slot.is_settled());
        assert_eq!(slot.take_abandoned(), None);
    }

    #[test]
    fn double_take_after_claim_returns_none() {
        let (wake, _wake_count) = counting_wake();
        let (slot, mut handle) = claim_slot::<u32>(wake);
        slot.deliver(1).expect("slot is still Pending");

        assert_eq!(handle.try_take(), Some(1));
        assert_eq!(
            handle.try_take(),
            None,
            "a second claim on an already-Claimed slot must not repeat the value"
        );
    }

    #[test]
    fn try_take_before_delivery_is_none_and_does_not_abandon() {
        let (wake, wake_count) = counting_wake();
        let (_slot, mut handle) = claim_slot::<u32>(wake);
        assert_eq!(handle.try_take(), None);
        assert_eq!(wake_count.load(Ordering::Acquire), 0);
    }

    #[test]
    fn abandonment_wake_fires_exactly_once() {
        let (wake, wake_count) = counting_wake();
        let (_slot, handle) = claim_slot::<u32>(wake);
        drop(handle);
        assert_eq!(wake_count.load(Ordering::Acquire), 1);
    }

    #[test]
    fn concurrent_abandon_and_deliver_race_wakes_exactly_once() {
        // Stress the race the module doc calls out: the owner may call
        // `deliver` concurrently with the requester dropping its handle.
        // Whichever side observes the transition first, the wake must
        // still fire exactly once and the owner must get its payload back
        // whenever abandonment preceded (or tied with) delivery.
        for _ in 0..64 {
            let (wake, wake_count) = counting_wake();
            let (slot, handle) = claim_slot::<u32>(wake);

            let deliverer = thread::spawn(move || slot.deliver(99));
            drop(handle);
            let result = deliverer.join().expect("owner thread does not panic");

            assert_eq!(wake_count.load(Ordering::Acquire), 1);
            if let Err(rejected) = result {
                assert_eq!(rejected, 99);
            }
        }
    }

    #[test]
    fn wait_blocks_until_delivery_then_returns_the_value() {
        let (wake, _wake_count) = counting_wake();
        let (slot, handle) = claim_slot::<u32>(wake);

        let waiter = thread::spawn(move || handle.wait());
        // Give the waiter a chance to reach the condvar; not required for
        // correctness (deliver would still be observed), only to exercise
        // the blocking path rather than the immediate one.
        thread::sleep(Duration::from_millis(20));
        slot.deliver(123).expect("slot is still Pending");

        assert_eq!(
            waiter.join().expect("waiter thread does not panic"),
            ClaimOutcome::Delivered(123)
        );
    }

    #[test]
    fn wait_after_a_successful_try_take_on_the_same_handle_returns_already_claimed_not_a_panic() {
        // Regression test (foundation claim-slot compliance review): `try_take`
        // takes `&mut self`, so nothing in the type system stops a caller from
        // following a successful `try_take` with a `wait` on the same
        // still-alive handle. That used to hit an `unreachable!("BUG: ...")`
        // panic through entirely safe public API; `wait` must instead
        // report "nothing left to wait for" via `AlreadyClaimed`.
        let (wake, wake_count) = counting_wake();
        let (slot, mut handle) = claim_slot::<u32>(wake);

        slot.deliver(7).expect("slot is still Pending");
        assert_eq!(handle.try_take(), Some(7), "first claim succeeds normally");

        assert_eq!(
            handle.wait(),
            ClaimOutcome::AlreadyClaimed,
            "wait on an already-claimed handle must report AlreadyClaimed, not panic"
        );
        assert_eq!(
            wake_count.load(Ordering::Acquire),
            0,
            "a claimed handle's wait/drop must not abandon or wake"
        );
    }

    /// Real second thread, bounded via `recv_timeout`: a `wait()` blocked on
    /// a request the owner never delivers must not hang forever once the
    /// owner side (`ClaimSlot`) is dropped — it must unblock with
    /// `ClaimOutcome::OwnerGone` (ADR-0039 §3/slice-2 amendment). A test
    /// that used a bare `.join()` would itself hang the test suite if this
    /// regressed; `recv_timeout` turns that failure mode into a normal
    /// assertion failure instead.
    #[test]
    fn blocked_waiter_unblocks_on_owner_drop() {
        let (wake, _wake_count) = counting_wake();
        let (slot, handle) = claim_slot::<u32>(wake);
        let (result_tx, result_rx) = std::sync::mpsc::channel();

        let waiter = thread::spawn(move || {
            let outcome = handle.wait();
            // A join-based test could hang forever if `wait` regressed to
            // blocking indefinitely; sending through a channel lets the
            // main thread bound how long it waits instead.
            let _ = result_tx.send(outcome);
        });

        // Give the waiter a chance to reach the condvar before the owner
        // disconnects; not required for correctness (dropping `slot` first
        // would still resolve `wait` correctly), only to exercise the
        // blocking path rather than the immediate one.
        thread::sleep(Duration::from_millis(20));
        drop(slot);

        let outcome = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("wait() must unblock promptly once the owner side drops");
        assert_eq!(outcome, ClaimOutcome::OwnerGone);
        waiter.join().expect("waiter thread does not panic");
    }

    #[test]
    fn future_resolves_on_deliver() {
        let (wake, _wake_count) = counting_wake();
        let (slot, mut handle) = claim_slot::<u32>(wake);

        let (waker, _wake_count_task) = test_waker();
        let mut cx = Context::from_waker(&waker);
        assert_eq!(
            Pin::new(&mut handle).poll(&mut cx),
            Poll::Pending,
            "nothing delivered yet"
        );

        slot.deliver(42).expect("slot is still Pending");

        assert_eq!(
            Pin::new(&mut handle).poll(&mut cx),
            Poll::Ready(ClaimOutcome::Delivered(42)),
            "a re-poll after delivery must resolve, not just the woken task"
        );
    }

    /// Distinguishes "the value eventually resolves" from "the registered
    /// `Waker` actually fires" — a `Future` that only ever resolved on the
    /// next unconditional re-poll (never truly parking) would still pass a
    /// resolves-on-deliver test but stall forever under a real executor
    /// that only re-polls after a wake.
    #[test]
    fn future_wakes_not_just_resolves_after_waker_registration() {
        let (wake, _wake_count) = counting_wake();
        let (slot, mut handle) = claim_slot::<u32>(wake);

        let (waker, wake_count) = test_waker();
        let mut cx = Context::from_waker(&waker);
        assert_eq!(Pin::new(&mut handle).poll(&mut cx), Poll::Pending);
        assert_eq!(
            wake_count.count(),
            0,
            "registering must not itself count as a wake"
        );

        slot.deliver(7).expect("slot is still Pending");

        assert_eq!(
            wake_count.count(),
            1,
            "delivery must wake the registered waker directly, not merely \
             become observable on a hypothetical future poll"
        );
        assert_eq!(
            Pin::new(&mut handle).poll(&mut cx),
            Poll::Ready(ClaimOutcome::Delivered(7))
        );
    }

    #[test]
    fn future_wakes_on_owner_drop() {
        let (wake, _wake_count) = counting_wake();
        let (slot, mut handle) = claim_slot::<u32>(wake);

        let (waker, wake_count) = test_waker();
        let mut cx = Context::from_waker(&waker);
        assert_eq!(Pin::new(&mut handle).poll(&mut cx), Poll::Pending);

        drop(slot);

        assert_eq!(
            wake_count.count(),
            1,
            "owner disconnection must wake a parked poll, not just resolve a later one"
        );
        assert_eq!(
            Pin::new(&mut handle).poll(&mut cx),
            Poll::Ready(ClaimOutcome::OwnerGone)
        );
    }

    /// The owner thread is also a legal thread to poll from — unlike
    /// `wait()`, which must refuse there (callers building on top, e.g.
    /// `flui-platform`'s `PendingWindow`, check thread identity themselves;
    /// this primitive's `Future` impl has no such refusal because polling
    /// never blocks in the first place).
    #[test]
    fn owner_thread_poll_never_blocks() {
        let (wake, _wake_count) = counting_wake();
        let (_slot, mut handle) = claim_slot::<u32>(wake);

        let (waker, _wake_count_task) = test_waker();
        let mut cx = Context::from_waker(&waker);
        // Same thread as the (still-live) owner side `_slot` -- a blocking
        // `wait()` here would be the exact hazard `PendingWindow::wait`
        // refuses; `poll` must return immediately regardless.
        assert_eq!(Pin::new(&mut handle).poll(&mut cx), Poll::Pending);
    }

    /// Counting `Wake` implementation for tests: no executor, `futures`
    /// dependency, or hand-rolled `RawWaker`/`unsafe` needed just to prove a
    /// wake fired — `std::task::Wake` builds a real `Waker` from a safe
    /// `Arc<impl Wake>` (stable since 1.51).
    struct CountingWake(AtomicUsize);

    impl CountingWake {
        fn count(&self) -> usize {
            self.0.load(Ordering::Acquire)
        }
    }

    impl std::task::Wake for CountingWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn test_waker() -> (Waker, Arc<CountingWake>) {
        let inner = Arc::new(CountingWake(AtomicUsize::new(0)));
        (Waker::from(Arc::clone(&inner)), inner)
    }

    #[test]
    fn panic_during_owner_work_before_delivery_leaves_the_slot_claimable_or_abandoned() {
        // Simulates the owner panicking while producing `T`, *before* ever
        // calling `deliver` — the hazard this test rules out is a wedged
        // `Mutex` from a poisoned lock (`parking_lot::Mutex` does not
        // poison) AND, since `ClaimSlot`'s own `Drop` now covers owner
        // disconnection (ADR-0039 §3 slice-2 amendment), a handle left
        // waiting forever for a `deliver` call that will now never come.
        //
        // The panicking thread's `slot` unwinds through `ClaimSlot::drop`
        // right there, on the owner thread — it observes `Pending` and
        // transitions straight to `OwnerGone` before this test ever touches
        // `handle`, so the injected `WakeOwner` callback (`counting_wake`)
        // correctly never fires here: that callback wakes the *owner*'s
        // event loop when the *requester* abandons, which is not what
        // happened in this scenario (the owner is what disconnected).
        let (wake, wake_count) = counting_wake();
        let (slot, handle) = claim_slot::<u32>(wake);

        let owner = thread::spawn(move || {
            let _slot_kept_alive_until_panic = &slot;
            panic!("owner failed to produce the value");
        });
        assert!(owner.join().is_err(), "owner thread panics as designed");

        assert_eq!(
            handle.wait(),
            ClaimOutcome::OwnerGone,
            "slot is not wedged: the handle resolves to OwnerGone instead of \
             blocking forever on a deliver call that will never come"
        );
        assert_eq!(
            wake_count.load(Ordering::Acquire),
            0,
            "the requester never abandoned anything here -- the owner did -- \
             so the WakeOwner callback (which wakes the owner on requester \
             abandonment) must not fire"
        );
    }
}
