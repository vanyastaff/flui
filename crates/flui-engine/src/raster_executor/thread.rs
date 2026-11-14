//! The threaded placement: one raster thread turns the executor, and the
//! owner holds an [`ExecutorHandle`] (ADR-0305 §9, §10).
//!
//! The thread builds the executor itself, so the owner-local parts (the
//! clock's `Rc`, the boxed wake) never cross a thread; only the backend, the
//! clock's `Arc`, the wake and the shared state do. It turns once per ring of
//! the doorbell, which every accepted send and every drain that frees a ring's
//! place rings, and once when a lost device's rebuild is due.
//!
//! A turn's panic outside a slot's command cannot be resumed on the owner's
//! thread: the thread contains it, keeps serving, and hands its text to the
//! owner as [`ExecutorFailure::TurnPanicked`]. A thread that does not answer a
//! detach or a stop within [`DETACH_DEADLINE`] is marked
//! [`ExecutorHealth::Died`] and left running: it holds the context, and the
//! owner neither joins it nor builds a second context beside it.

use std::{
    rc::Rc,
    sync::Arc,
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, bounded};
use flui_foundation::{
    clock::MonotonicClock,
    panic::{FirstPanic, discard_payload, payload_text},
};

use super::{
    DETACH_DEADLINE, ExecutorFailure, ExecutorHealth, LIFECYCLE_CAPACITY, RasterBackend,
    RasterExecutor, RasterSender, SlotId, SlotPhase, sender::Shared,
};

/// The name the raster thread carries in debuggers and profilers.
const THREAD_NAME: &str = "flui-raster";

/// The owner's hold on a raster thread: it stops the thread and waits for a
/// slot's detach, each within a deadline. Dropping it stops the thread within
/// [`DETACH_DEADLINE`].
pub struct ExecutorHandle {
    shared: Arc<Shared>,
    /// Answers once the thread finished stopping.
    stopped: Receiver<()>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ExecutorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutorHandle")
            .field("health", &self.shared.health())
            .finish_non_exhaustive()
    }
}

impl<B: RasterBackend + Send + 'static> RasterExecutor<B> {
    /// Start an executor over `backend` on its own raster thread, and answer
    /// the owner's handle and sender for it (ADR-0305 §10).
    ///
    /// `clock` paces the rebuilds of a lost device; `wake` is the host's,
    /// called on the raster thread once a turn wrote a settlement or moved
    /// the device's health, with no lock held and contained. A slot holds
    /// two settlements undrained, so a frame presents while the owner builds
    /// the next.
    ///
    /// # Errors
    /// The operating system's refusal to start the thread; the backend is
    /// dropped with it.
    pub fn spawn(
        backend: B,
        clock: Arc<dyn MonotonicClock + Send + Sync>,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<(ExecutorHandle, RasterSender)> {
        let (lifecycle_tx, lifecycle) = bounded(LIFECYCLE_CAPACITY);
        let shared = Arc::new(Shared::threaded(lifecycle_tx));
        let sender = RasterSender::new(Arc::clone(&shared));
        let (stopped_tx, stopped) = bounded(1);
        let served = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || {
                let mut caught = FirstPanic::new();
                caught.contain(|| {
                    let clock: Rc<dyn MonotonicClock> = Rc::new(SharedClock(clock));
                    let executor = Self::assemble(
                        backend,
                        clock,
                        Box::new(wake),
                        Arc::clone(&served),
                        lifecycle,
                    );
                    serve(executor, &served);
                });
                // Only building the executor escapes `serve`'s containment (its
                // first clock read): no executor is left to answer the sends,
                // so it is dead, not stopped.
                if let Some(failure) = turn_panicked(caught) {
                    served.die(failure);
                }
                // The owner may have stopped waiting; nobody to tell then.
                let _told = stopped_tx.send(());
            })?;
        Ok((
            ExecutorHandle {
                shared,
                stopped,
                thread: Some(thread),
            },
            sender,
        ))
    }
}

/// Turn the executor until it is stopped, then stop it.
fn serve<B: RasterBackend>(mut executor: RasterExecutor<B>, shared: &Shared) {
    while shared.health() == ExecutorHealth::Running {
        let mut caught = FirstPanic::new();
        let _outcome = caught.contain(|| executor.turn());
        hand_over(caught, shared);
        // After the panic is held, so an owner woken by the turn finds it.
        shared.note_turn();
        let due = rebuild_due_in(&executor, shared);
        shared.wait_for_ring(due);
    }
    let mut caught = FirstPanic::new();
    caught.contain(|| {
        executor.shut_down();
        drop(executor);
    });
    hand_over(caught, shared);
    shared.note_turn();
}

/// Give a contained turn's panic to the owner as its text.
fn hand_over(caught: FirstPanic, shared: &Shared) {
    if let Some(failure) = turn_panicked(caught) {
        shared.hold_failure(failure);
    }
}

/// The failure a contained panic is reported as; its payload is discarded
/// here, since no one resumes it on another thread.
fn turn_panicked(caught: FirstPanic) -> Option<ExecutorFailure> {
    let payload = caught.into_payload()?;
    let message = payload_text(&*payload)
        .unwrap_or("a panic without a message")
        .to_owned();
    discard_payload(payload);
    Some(ExecutorFailure::TurnPanicked { message })
}

/// How long until the lost device's next rebuild may run; `None` while
/// nothing is due, so the thread waits for a ring.
fn rebuild_due_in<B: RasterBackend>(
    executor: &RasterExecutor<B>,
    shared: &Shared,
) -> Option<Duration> {
    let due = shared.device().next_attempt_at?;
    let mut caught = FirstPanic::new();
    let now = caught.contain(|| executor.clock.now()).unwrap_or(due);
    hand_over(caught, shared);
    Some(due.saturating_duration_since(now))
}

impl ExecutorHandle {
    /// Where the raster thread stands.
    #[must_use]
    pub fn health(&self) -> ExecutorHealth {
        self.shared.health()
    }

    /// Wait until the thread took `slot` down, at most `timeout`: a window's
    /// surface is given back only after its slot's detach completed
    /// (ADR-0305 §9). The owner holds no borrow of its host or a runtime
    /// while it waits.
    ///
    /// # Errors
    /// [`ExecutorFailure::Wedged`] when the thread did not answer in time:
    /// the executor is then [`ExecutorHealth::Died`], and the owner keeps
    /// the window and its target until the process exits.
    pub fn wait_detached(
        &self,
        sender: &RasterSender,
        slot: SlotId,
        timeout: Duration,
    ) -> Result<(), ExecutorFailure> {
        let started = std::time::Instant::now();
        loop {
            // Read before the record, so a turn that ends between the check
            // and the wait is not missed.
            let seen = self.shared.turns();
            let detached = sender
                .record(slot)
                .is_none_or(|record| record.phase == SlotPhase::Detached);
            if detached {
                return Ok(());
            }
            if self.shared.health() == ExecutorHealth::Died {
                return Err(ExecutorFailure::Wedged { waited: timeout });
            }
            let waited = started.elapsed();
            if waited >= timeout {
                return Err(self.die(timeout));
            }
            self.shared
                .wait_for_turn(seen, timeout.saturating_sub(waited));
        }
    }

    /// Stop the thread and wait for it, at most `timeout`. Every send answers
    /// [`RasterSendError::Gone`](super::RasterSendError::Gone) from the
    /// call on; the thread settles every pending frame and takes every slot
    /// down before it ends.
    ///
    /// # Errors
    /// [`ExecutorFailure::Wedged`] when the thread did not end in time: the
    /// executor is then [`ExecutorHealth::Died`], and the thread is left to
    /// the process.
    pub fn shut_down(&mut self, timeout: Duration) -> Result<(), ExecutorFailure> {
        let Some(thread) = self.thread.take() else {
            return match self.shared.health() {
                ExecutorHealth::Died => Err(ExecutorFailure::Wedged { waited: timeout }),
                _ => Ok(()),
            };
        };
        self.shared.shut_down();
        match self.stopped.recv_timeout(timeout) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                // The thread ran its last line or unwound past it; either way
                // it is ending, and its panic was contained.
                let _ended = thread.join();
                Ok(())
            }
            Err(RecvTimeoutError::Timeout) => {
                // Dropping the join handle leaves the thread to the process.
                drop(thread);
                Err(self.die(timeout))
            }
        }
    }

    fn die(&self, waited: Duration) -> ExecutorFailure {
        let failure = ExecutorFailure::Wedged { waited };
        self.shared.die(failure.clone());
        tracing::error!(
            ?waited,
            "the raster thread did not answer; rendering stopped for every window"
        );
        failure
    }
}

impl Drop for ExecutorHandle {
    fn drop(&mut self) {
        // A wedge is in the shared record for any sender still held, and was
        // logged.
        let _stopped = self.shut_down(DETACH_DEADLINE);
    }
}

/// The owner's clock as the executor reads it on the raster thread.
#[derive(Debug)]
struct SharedClock(Arc<dyn MonotonicClock + Send + Sync>);

impl MonotonicClock for SharedClock {
    fn now(&self) -> web_time::Instant {
        self.0.now()
    }

    fn epoch(&self) -> web_time::Instant {
        self.0.epoch()
    }
}
