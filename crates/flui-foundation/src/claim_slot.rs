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
//! This primitive is deliberately generic and carries no `flui-platform`
//! vocabulary (no `WindowId`, no `OpenWindowError`) — the ADR places its
//! tests in the winit lane, which CI does not execute; landing the tested
//! core here means the state-machine behavior is CI-verified even though
//! its winit composition is not (the same rationale `OwnerAffinity`
//! already established for the runtime-backstop half of ADR-0039).

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
    pub fn deliver(&self, value: T) -> Result<(), T> {
        let mut state = self.inner.state.lock();
        match &*state {
            SlotState::Pending => {
                *state = SlotState::Delivered(value);
                drop(state);
                self.inner.delivered.notify_one();
                Ok(())
            }
            SlotState::Abandoned(None) => {
                // No write-back needed: the state is already exactly
                // `Abandoned(None)` (confirmed by the match arm above), so
                // there is nothing to update before handing `value` back.
                drop(state);
                Err(value)
            }
            SlotState::Delivered(_) | SlotState::Claimed | SlotState::Abandoned(Some(_)) => {
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
            SlotState::Pending | SlotState::Delivered(_) | SlotState::Claimed => None,
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

impl<T> ClaimHandle<T> {
    /// Non-blocking poll. Returns `Some(T)` exactly once — the first call
    /// observing `Delivered` claims it (`Delivered -> Claimed`) and every
    /// later call (on this handle, or after the request is otherwise
    /// resolved) returns `None`. Safe on any thread, including the owner.
    #[must_use = "discarding Some(value) strands whatever the owner delivered"]
    pub fn try_take(&mut self) -> Option<T> {
        let mut state = self.inner.state.lock();
        let current = std::mem::replace(&mut *state, SlotState::Claimed);
        match current {
            SlotState::Delivered(value) => Some(value),
            other @ (SlotState::Pending | SlotState::Claimed | SlotState::Abandoned(_)) => {
                *state = other;
                None
            }
        }
    }

    /// Blocks the calling thread until the owner delivers, then claims the
    /// value.
    ///
    /// Returns `None` if this handle was already claimed by an earlier
    /// [`try_take`](Self::try_take) call — `try_take` takes `&mut self`, so
    /// nothing in the type system stops a caller from following a
    /// successful `try_take` with a `wait` on the same still-alive handle;
    /// that is a caller-sequencing fact (there is nothing left to wait
    /// for), not a slot-invariant violation, so it is reported through the
    /// return value rather than a panic. Callers that hand this outcome
    /// to a typed error (e.g. `flui-platform`'s `PendingWindow::wait`) map
    /// `None` onto their own `#[non_exhaustive]` error enum.
    ///
    /// This primitive does not itself refuse the owner thread — callers
    /// that must not block their own owner (e.g. `flui-platform`'s
    /// `PendingWindow::wait`, which drains the very lane this would block
    /// on) are responsible for checking thread identity *before* calling
    /// `wait` and taking the non-blocking `try_take` path instead.
    ///
    /// # Owner obligation
    /// `wait` has no "owner disconnected" transition to wake it early —
    /// unlike a channel's `Receiver`, dropping the owner-side [`ClaimSlot`]
    /// without ever calling [`deliver`](ClaimSlot::deliver) sends no signal
    /// here, so a caller blocked in `wait` would block forever. The owner
    /// must guarantee it always eventually calls `deliver`, typically via a
    /// panic-safe guard that calls it unconditionally on drop if not
    /// already called explicitly (`flui-platform`'s `OpenWindowReplyGuard`
    /// is exactly this guard for the winit lane) — never a bare, fallible
    /// call site with no fallback.
    #[must_use = "discarding Some(value) strands whatever the owner delivered"]
    pub fn wait(self) -> Option<T> {
        let mut state = self.inner.state.lock();
        loop {
            match &*state {
                SlotState::Delivered(_) => {
                    let SlotState::Delivered(value) =
                        std::mem::replace(&mut *state, SlotState::Claimed)
                    else {
                        unreachable!("BUG: match on &*state just confirmed Delivered")
                    };
                    return Some(value);
                }
                SlotState::Pending => {
                    self.inner.delivered.wait(&mut state);
                }
                // Already resolved by an earlier `try_take` on this same
                // handle -- a caller-sequencing fact, not a bug: nothing
                // left to wait for.
                SlotState::Claimed => return None,
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
                already_resolved @ (SlotState::Claimed | SlotState::Abandoned(_)) => {
                    *state = already_resolved;
                    false
                }
            }
        }; // lock released here — notify_abandoned must never run under it.
        if became_abandoned {
            self.inner.notify_abandoned();
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use super::claim_slot;

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
            Some(123)
        );
    }

    #[test]
    fn wait_after_a_successful_try_take_on_the_same_handle_returns_none_not_a_panic() {
        // Regression test (spec-compliance review MAJOR-1): `try_take` takes
        // `&mut self`, so nothing in the type system stops a caller from
        // following a successful `try_take` with a `wait` on the same
        // still-alive handle. That used to hit an `unreachable!("BUG: ...")`
        // panic through entirely safe public API; `wait` must instead
        // report "nothing left to wait for" via `None`.
        let (wake, wake_count) = counting_wake();
        let (slot, mut handle) = claim_slot::<u32>(wake);

        slot.deliver(7).expect("slot is still Pending");
        assert_eq!(handle.try_take(), Some(7), "first claim succeeds normally");

        assert_eq!(
            handle.wait(),
            None,
            "wait on an already-claimed handle must report None, not panic"
        );
        assert_eq!(
            wake_count.load(Ordering::Acquire),
            0,
            "a claimed handle's wait/drop must not abandon or wake"
        );
    }

    #[test]
    fn panic_during_owner_work_before_delivery_leaves_the_slot_claimable_or_abandoned() {
        // Simulates the owner panicking while producing `T`, *before* ever
        // calling `deliver` — the hazard this test rules out is a wedged
        // Mutex from a poisoned lock. `parking_lot::Mutex` does not
        // poison, so a panicking owner thread that never touches the slot
        // again still leaves it in a state the requester can act on.
        let (wake, wake_count) = counting_wake();
        let (slot, handle) = claim_slot::<u32>(wake);

        let owner = thread::spawn(move || {
            let _slot_kept_alive_until_panic = &slot;
            panic!("owner failed to produce the value");
        });
        assert!(owner.join().is_err(), "owner thread panics as designed");

        // The slot was dropped when the panicking thread unwound, so the
        // requester's abandonment path — not the owner's — resolves this
        // request the moment the handle itself is dropped.
        drop(handle);
        assert_eq!(
            wake_count.load(Ordering::Acquire),
            1,
            "slot is not wedged: abandonment still completes and wakes exactly once"
        );
    }
}
