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
    RasterCommand, RasterExecutor, RasterSendError, RasterSender, SlotId, SlotPhase,
    sender::Shared,
};

/// The name the raster thread carries in debuggers and profilers.
const THREAD_NAME: &str = "flui-raster";

/// The owner's hold on a raster thread: it stops the thread and waits for a
/// slot's detach, each within a deadline. Dropping it stops the thread within
/// [`DETACH_DEADLINE`], or at once once the executor died.
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
                // Shared on this thread only: the executor's turns wake
                // through it, and so does a failure no turn reports.
                let wake: Rc<dyn Fn()> = Rc::new(wake);
                // The clock is the owner's code: a first read that panics
                // starts the executor at the thread's own time.
                let mut first_read = FirstPanic::new();
                let read_at = first_read
                    .contain(|| clock.now())
                    .unwrap_or_else(web_time::Instant::now);
                let owed = hand_over(first_read, &served);
                let mut caught = FirstPanic::new();
                caught.contain(|| {
                    let clock: Rc<dyn MonotonicClock> = Rc::new(SharedClock(clock));
                    let turned = Rc::clone(&wake);
                    let executor = Self::assemble(
                        backend,
                        read_at,
                        clock,
                        Box::new(move || turned()),
                        Arc::clone(&served),
                        lifecycle,
                    );
                    serve(executor, &served, &*wake, owed);
                });
                // `serve` contains every turn and the stop, and building runs
                // no code the executor does not own; a panic here left no
                // executor to answer the sends, so it is dead, not stopped.
                if let Some(failure) = turn_panicked(caught) {
                    served.die(failure);
                    wake_owner(&*wake, &served);
                }
                // The wake is the host's: its destructor runs before the stop is
                // told, so one that blocks is under the owner's deadline.
                let mut released = FirstPanic::new();
                released.contain(move || drop(wake));
                let _held = hand_over(released, &served);
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

/// Turn the executor until it is stopped, then stop it. `owed` says a failure
/// was held before the first turn, with no wake yet.
///
/// A turn's own wake, if it ran, came before its failure was held, and a turn
/// that settled nothing woke nobody: whenever a failure is newly held, the
/// owner is woken once it is there to take.
fn serve<B: RasterBackend>(
    mut executor: RasterExecutor<B>,
    shared: &Shared,
    wake: &dyn Fn(),
    mut owed: bool,
) {
    while shared.health() == ExecutorHealth::Running {
        let mut caught = FirstPanic::new();
        let _outcome = caught.contain(|| executor.turn());
        owed |= hand_over(caught, shared);
        shared.note_turn();
        let due = rebuild_due_in(&executor, shared, &mut owed);
        if std::mem::take(&mut owed) {
            wake_owner(wake, shared);
        }
        shared.wait_for_ring(due);
    }
    let mut caught = FirstPanic::new();
    caught.contain(|| executor.shut_down());
    caught.contain(|| drop(executor));
    // A failure owed from before a stop that came ahead of the first turn is
    // woken for here.
    if hand_over(caught, shared) || owed {
        wake_owner(wake, shared);
    }
    shared.note_turn();
}

/// Give a contained panic to the owner as its text; answers whether it is
/// newly held, so the owner is owed a wake.
fn hand_over(caught: FirstPanic, shared: &Shared) -> bool {
    turn_panicked(caught).is_some_and(|failure| shared.hold_failure(failure))
}

/// Wake the owner for a failure it has to take, contained. A wake that panics
/// is held behind the failure it woke for, and not woken for again.
fn wake_owner(wake: &dyn Fn(), shared: &Shared) {
    let mut caught = FirstPanic::new();
    caught.contain(wake);
    let _behind = hand_over(caught, shared);
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
///
/// The wait is the clock's distance in real time. A clock that panics paces
/// nothing, so the thread waits for a ring rather than turning on it in a
/// loop; a manual clock's owner rings by sending once it moved the clock. A
/// panic newly held sets `owed`, so the owner is woken to take it.
fn rebuild_due_in<B: RasterBackend>(
    executor: &RasterExecutor<B>,
    shared: &Shared,
    owed: &mut bool,
) -> Option<Duration> {
    let due = shared.device().next_attempt_at?;
    let mut caught = FirstPanic::new();
    let now = caught.contain(|| executor.clock.now());
    *owed |= hand_over(caught, shared);
    Some(due.saturating_duration_since(now?))
}

impl ExecutorHandle {
    /// Where the raster thread stands.
    #[must_use]
    pub fn health(&self) -> ExecutorHealth {
        self.shared.health()
    }

    /// Take `slot` down and wait until the thread did, at most `timeout`: a
    /// window's surface is given back only after its slot's detach completed
    /// (ADR-0305 §9). Sends the slot's [`RasterCommand::Detach`] unless the
    /// owner sent it already, and sends it again after a turn while the
    /// lifecycle queue is full. The owner holds no borrow of its host or a
    /// runtime while it waits.
    ///
    /// Answers at once for a slot the sender does not hold, which no surface
    /// is behind.
    ///
    /// # Errors
    /// [`ExecutorFailure::Wedged`] when the thread did not answer in time:
    /// the executor is then [`ExecutorHealth::Died`], and the owner keeps
    /// the window and its target until the process exits. The cause the
    /// executor died of, when it was dead already.
    ///
    /// [`RasterCommand::Detach`]: super::RasterCommand::Detach
    pub fn detach(
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
            // The record before the death: a slot the thread took down before
            // it died has its surface back.
            let detached = sender
                .record(slot)
                .is_none_or(|record| record.phase == SlotPhase::Detached);
            if detached {
                return Ok(());
            }
            if let Some(death) = self.shared.death() {
                return Err(death);
            }
            if !sender.detach_sent(slot) {
                // Accepted, or refused because the queue is full (the thread's
                // next turn makes room) or the executor stops (the stop takes
                // every slot down; a death is read above): wait for the record.
                // Not held under this id: no surface is behind it.
                if let Err(RasterSendError::StaleSlot(_)) =
                    sender.send(RasterCommand::Detach { slot })
                {
                    return Ok(());
                }
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
    /// down before it ends. A second call answers at once. A panic the stop
    /// contained reaches the owner as the turns' do, through
    /// [`RasterSender::take_executor_failure`].
    ///
    /// # Errors
    /// [`ExecutorFailure::Wedged`] when the thread did not end in time: the
    /// executor is then [`ExecutorHealth::Died`], and the thread is left to
    /// the process. The cause the executor died of, at once and without a
    /// wait, when it was dead already.
    pub fn shut_down(&mut self, timeout: Duration) -> Result<(), ExecutorFailure> {
        let thread = self.thread.take();
        // A dead executor is not waited for again: its thread is left to the
        // process, and a thread that is only slow still stops, since a dead
        // executor's loop ends.
        if let Some(death) = self.shared.death() {
            self.shared.shut_down();
            drop(thread);
            return Err(death);
        }
        let Some(thread) = thread else {
            return Ok(());
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

    /// Mark the executor died of a wedge, and answer the cause it died of: the
    /// first one, which a death the thread itself reached may have set.
    fn die(&self, waited: Duration) -> ExecutorFailure {
        self.shared.die(ExecutorFailure::Wedged { waited });
        self.shared
            .death()
            .expect("BUG: a died executor keeps its cause")
    }
}

impl Drop for ExecutorHandle {
    fn drop(&mut self) {
        // A death is in the shared record for any sender still held, and was
        // logged when it began.
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
