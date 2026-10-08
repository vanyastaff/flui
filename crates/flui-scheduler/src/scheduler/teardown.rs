//! Last-owner completion delivery and contained waker retirement.

use super::{SchedulerClosed, SchedulerInner};

/// Resolves every still-pending [`end_of_frame`](super::UpdateScheduler::end_of_frame)
/// waiter with `Err(`[`SchedulerClosed`]`)` when the last strong
/// [`super::UpdateScheduler`] handle is dropped, so a caller awaiting one never
/// hangs forever with no frame left to run and resolve it.
///
/// # Why `get_mut`, not `lock()`, is sound with no runtime check
///
/// `Drop::drop` hands this `&mut SchedulerInner`, and the reason that is
/// sound is `Arc`, not the borrow: this runs only once the strong count has
/// reached zero, and `Arc`'s own release/acquire ordering means this
/// destructor observes every prior mutation through any dropped clone — no
/// other thread can be mid-registration or mid-notification against this
/// same registry at this point. So teardown takes no lock at all.
///
/// # Why the `Some(Err(SchedulerClosed))` write below needs no `is_none()` guard
///
/// `drain()` performs `mem::take`, removing every entry it returns from the
/// registry — so every entry this loop reaches is, by construction, one
/// `notify_frame_completion` never reached first (a delivered completion
/// already took its entry out of the registry, via that same `drain`, long
/// before this ran). `completed` is therefore always `None` here; a runtime
/// check would be dead code testing a fact the type already proves.
///
/// # May run on a foreign thread
///
/// A [`super::FrameWaker::request_frame`] can itself be the call that drops its own
/// `upgrade()`d temporary strong reference — making THIS the final release,
/// on whichever thread that wake happened to run on. Everything this touches
/// (`FrameCompletionRegistry`, `FrameCompletionState`, `Waker`) is
/// `Send + Sync`, so that is sound, but it means this must never assume it
/// runs on the scheduler's "home" thread.
///
/// # Never `resume_unwind`
///
/// A panic raised from a destructor while the thread is already unwinding
/// aborts the process with no diagnostic. Wakers are borrowed for invocation,
/// retaining their owning envelopes on failure or existing unwind. Ordinary
/// retirement and telemetry have separate catches; caught opaque payloads
/// are retained, and this delivery loop never resumes a caught failure.
///
/// # What this cannot reach
///
/// This runs only once every strong [`super::UpdateScheduler`] reference is gone —
/// the ordinary `Arc` rule, nothing special to this type. A task holding its
/// OWN strong clone (captured into an `async` block spawned on the UI runtime's
/// [`AsyncDriver`](crate::AsyncDriver), say) defers this for as long as that
/// task is still pending, and a live strong clone anywhere else — an embedder
/// holding one, another thread's handle — does the same. The scheduler's
/// wake capability never causes that: a [`super::FrameWaker`] (the hook an
/// [`OwnerFrame`](crate::OwnerFrame) installs for its task wakes) holds only a
/// `Weak<SchedulerInner>`, precisely so a pending task cannot keep the
/// scheduler it belongs to alive through this destructor. See this crate's `ARCHITECTURE.md` "The teardown lifetime
/// guarantee remains partial" paragraph in the #1162 mapping entry for the full
/// argument.
impl Drop for SchedulerInner {
    fn drop(&mut self) {
        let waiters = self.frame.completion_waiters.get_mut().drain();
        let mut delivery = crate::completion_wake::WakeBatch::new("scheduler teardown", false);

        for notifier in waiters {
            // A failed upgrade means the future was already dropped
            // (cancelled) before teardown reached it — an ordinary outcome,
            // not an error, exactly as in `notify_frame_completion`.
            let Some(state) = notifier.state.upgrade() else {
                continue;
            };

            let waker = {
                let mut state = state.lock();
                state.completed = Some(Err(SchedulerClosed));
                state.waker.take()
            };
            let Some(waker) = waker else { continue };

            delivery.wake(waker);
        }
        delivery.finish(false);
    }
}
