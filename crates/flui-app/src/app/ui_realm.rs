//! `UiRealm` — the owner-affine UI-session composition root (ADR-0027).
//!
//! One realm, one owner: the runtime is the single logical owner of a
//! window's UI state and is structurally `!Send + !Sync`. Everything that
//! crosses a thread boundary does so as a typed [`UiCommandSender`]
//! capability feeding a **bounded** inbox, whose contents the owner commits
//! **only while the scheduler phase is Idle** — at frame boundaries, never
//! inside the frame transaction (`docs/adr/ADR-0027-owner-affine-ui-realms.md`
//! §3). This is the generalization of the `RebuildHandle`/`PipelineOwnerHandle`
//! pattern: enqueue-and-wake, never touch the tree.
//!
//! # Transitional coupling (migration step 1)
//!
//! Until singleton retirement (ADR-0027 migration step 3), the runtime
//! coexists with the process-global `AppBinding`/`Scheduler` graph rather
//! than owning those subsystems. A per-window type over process-global
//! internals would be a lying API, so construction enforces **at most one
//! live runtime per process** ([`UiRealmError::AlreadyExists`]); the
//! guard retires with the singletons. Each incarnation still gets a fresh
//! generational [`RealmId`], so results stamped for a dead runtime are
//! droppable by identity, not by convention.

use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_foundation::{GenerationGate, HasInstance, RealmId, ResourceGeneration};
use flui_scheduler::{Scheduler, SchedulerPhase};

/// Default bound of the owner inbox, matching the pipeline dirty-channel
/// precedent (`DEFAULT_DIRTY_CHANNEL_CAPACITY`, ADR-0027 §4). Observable at
/// runtime via `UiCommandSender::capacity`; not part of the public API.
const DEFAULT_COMMAND_CAPACITY: usize = 256;

/// Claim flag for the at-most-one-instance transitional guard.
static REALM_CLAIMED: AtomicBool = AtomicBool::new(false);

/// Monotonic incarnation counter: every successfully constructed runtime
/// gets a fresh `RealmId` generation, so a recreated realm never compares
/// equal to its predecessor (ADR-0027 §6).
static NEXT_INCARNATION: AtomicU32 = AtomicU32::new(1);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors constructing a [`UiRealm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UiRealmError {
    /// A `UiRealm` is already live in this process.
    ///
    /// Transitional (ADR-0027 migration step 1): the runtime still fronts
    /// process-global binding state, so a second instance would alias it
    /// while claiming isolation. The guard retires with the singletons.
    #[error(
        "a UiRealm is already live in this process; the at-most-one guard \
         holds until singleton retirement (ADR-0027 migration step 3)"
    )]
    AlreadyExists,
}

/// Errors returned by [`UiCommandSender`] sends.
///
/// Same shape as the pipeline dirty-channel errors: bounded channels surface
/// backpressure as a typed value, and a dropped owner is a typed value — the
/// producer decides what to do, nothing blocks, nothing grows unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CommandSendError {
    /// The inbox is full; the producer must back off (retry next frame,
    /// drop, or escalate — its call).
    #[error("realm command inbox full ({capacity} capacity); back off and retry")]
    ChannelFull {
        /// Configured inbox capacity.
        capacity: usize,
    },

    /// The owning [`UiRealm`] has been dropped; this sender is now
    /// permanently inert and the producer should stop sending.
    #[error("ui realm dropped; command sender is no longer valid")]
    OwnerGone,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// A worker-result identity stamp, validated at the owner's commit point.
///
/// Captured when the job is dispatched; checked when the result is drained
/// (ADR-0027 §6): the stamp's window must be the draining realm's identity
/// and the stamped [`ResourceGeneration`] must still be current on its
/// [`GenerationGate`]. A failed check drops the result with a trace event —
/// never a panic, never a partial apply.
#[derive(Debug, Clone)]
pub struct ResultStamp {
    realm_id: RealmId,
    gate: GenerationGate,
    issued: ResourceGeneration,
}

impl ResultStamp {
    /// Stamp a job dispatched for `realm_id` against the resource state
    /// guarded by `gate`, capturing the gate's current generation.
    #[must_use]
    pub fn current(realm_id: RealmId, gate: &GenerationGate) -> Self {
        Self {
            realm_id,
            gate: gate.clone(),
            issued: gate.current(),
        }
    }

    /// `true` iff this stamp targets `owner_realm` and its generation is
    /// still current.
    #[must_use]
    pub fn is_fresh(&self, owner_realm: RealmId) -> bool {
        self.realm_id == owner_realm && self.gate.is_current(self.issued)
    }

    /// The realm this stamp was issued for.
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.realm_id
    }
}

/// A command enqueued for the owner thread.
enum UiCommand {
    /// Run on the owner thread at the next Idle drain.
    Invoke(Box<dyn FnOnce() + Send + 'static>),
    /// A versioned worker result: applied only if the stamp is fresh at
    /// drain time, dropped (traced) otherwise.
    Result {
        stamp: ResultStamp,
        apply: Box<dyn FnOnce() + Send + 'static>,
    },
}

impl std::fmt::Debug for UiCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UiCommand::Invoke(_) => f.write_str("UiCommand::Invoke"),
            UiCommand::Result { stamp, .. } => f
                .debug_struct("UiCommand::Result")
                .field("stamp", stamp)
                .finish_non_exhaustive(),
        }
    }
}

/// What one [`UiRealm::drain_commands`] pass did, for observability
/// and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct DrainReport {
    /// Plain foreground-dispatch closures run (crate-internal `invoke`).
    pub invoked: usize,
    /// Fresh worker results applied.
    pub applied: usize,
    /// Stale or foreign-window results dropped (traced, never applied).
    pub dropped_stale: usize,
}

// ---------------------------------------------------------------------------
// Sender
// ---------------------------------------------------------------------------

/// Cross-thread capability into a [`UiRealm`]'s inbox.
///
/// `Clone + Send + Sync`. A sender can enqueue a command and wake the owner;
/// it can never obtain a reference into any tree, invoke a lifecycle
/// callback, or run build/layout/paint (ADR-0027 §2). Every enqueued command
/// executes on the owner thread, at the next Idle drain.
#[derive(Clone)]
pub struct UiCommandSender {
    tx: Sender<UiCommand>,
    capacity: usize,
    redraw_pending: Arc<AtomicBool>,
    /// Fired after every successful state change so an idle event loop
    /// produces the drain that observes it — the enqueue-then-wake contract
    /// (ADR-0027 §4), same as `PipelineOwnerHandle`'s notifier.
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for UiCommandSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiCommandSender")
            .field("capacity", &self.capacity)
            .field("pending", &self.tx.len())
            .field(
                "redraw_pending",
                &self.redraw_pending.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl UiCommandSender {
    /// Enqueue `run` for the owner thread and wake it.
    ///
    /// This is the foreground-dispatch primitive: "run on the owner thread"
    /// means the next Idle drain of this realm's inbox — never
    /// `std::thread::spawn`, never inline on the caller.
    ///
    /// Crate-private by design (ADR-0027 §9: the cross-thread surface is a
    /// *closed command vocabulary* — [`Self::request_redraw`],
    /// [`Self::submit_result`]). A public "run anything on the UI thread"
    /// escape hatch would let arbitrary code bypass the typed commands; the
    /// framework's own dispatch needs (executor absorption, migration step
    /// 3) grow here as crate-internal callers.
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure (back off and
    /// retry), [`CommandSendError::OwnerGone`] once the runtime is dropped.
    // Test-only until the executor absorption lands (see the variant note).
    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) fn invoke(
        &self,
        run: impl FnOnce() + Send + 'static,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::Invoke(Box::new(run)))
    }

    /// Enqueue a stamped worker result; the owner applies it only if the
    /// stamp is still fresh at drain time (ADR-0027 §6).
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure,
    /// [`CommandSendError::OwnerGone`] once the runtime is dropped. A *stale*
    /// result is not a send error — staleness is decided at the owner's
    /// commit point, and the result is silently (traced) dropped there.
    pub fn submit_result(
        &self,
        stamp: ResultStamp,
        apply: impl FnOnce() + Send + 'static,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::Result {
            stamp,
            apply: Box::new(apply),
        })
    }

    /// Request a redraw of the realm's presentation, coalesced: any number of pending
    /// requests collapse into one flag read by the owner at the next drain
    /// (the `needs_redraw` precedent, ADR-0027 §4 "idempotent dirty marks").
    ///
    /// Infallible and idempotent by design: the flag outlives the runtime,
    /// and a request against a dropped runtime is a harmless no-op (the wake
    /// has no loop left to wake).
    pub fn request_redraw(&self) {
        // `swap` (not store) so only the first request in a burst pays the
        // wake; a pending frame absorbs repeated wakes anyway, this just
        // skips redundant platform calls.
        if !self.redraw_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }

    /// The inbox's configured capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn send(&self, command: UiCommand) -> Result<(), CommandSendError> {
        match self.tx.try_send(command) {
            Ok(()) => {
                (self.wake)();
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(CommandSendError::ChannelFull {
                capacity: self.capacity,
            }),
            Err(TrySendError::Disconnected(_)) => Err(CommandSendError::OwnerGone),
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

/// The per-window owner: receives the inbox, drains it at Idle, and is
/// structurally confined to its construction thread.
///
/// `!Send + !Sync` by construction (raw-pointer `PhantomData` marker) — the
/// compiler, not convention, keeps the owner on its thread. Cross-thread
/// access goes through [`UiCommandSender`] only.
///
/// The owner never crosses a thread:
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<flui_app::app::ui_realm::UiRealm>();
/// ```
///
/// and never shares:
///
/// ```compile_fail
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<flui_app::app::ui_realm::UiRealm>();
/// ```
pub struct UiRealm {
    realm_id: RealmId,
    rx: Receiver<UiCommand>,
    /// Prototype for [`Self::command_sender`]: crossbeam receivers cannot
    /// mint senders, so the runtime keeps one sender to clone from. Holding
    /// it here does not keep the channel alive past the runtime: `rx` drops
    /// with the runtime and every outstanding sender turns `OwnerGone`.
    sender_prototype: UiCommandSender,
    redraw_pending: Arc<AtomicBool>,
    /// `*const ()` is `!Send + !Sync`; `PhantomData` of it makes the runtime
    /// so at zero cost (ADR-0002/0027 thread-affinity marker).
    _owner_affine: PhantomData<*const ()>,
}

impl std::fmt::Debug for UiRealm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiRealm")
            .field("realm_id", &self.realm_id)
            .field("pending_commands", &self.rx.len())
            .field(
                "redraw_pending",
                &self.redraw_pending.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl UiRealm {
    /// Construct the runtime with the default inbox capacity.
    ///
    /// `wake` is the platform wake: it must deliver a wake to the owner's
    /// event loop without spawning a thread (ADR-0027 §4) — in the current
    /// desktop runner that is `AppBinding::wake_frame`.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::AlreadyExists`] while another runtime is live
    /// (transitional at-most-one guard, see module docs).
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, UiRealmError> {
        Self::with_capacity(DEFAULT_COMMAND_CAPACITY, wake)
    }

    /// [`Self::new`] with an explicit inbox capacity.
    ///
    /// # Errors
    ///
    /// [`UiRealmError::AlreadyExists`] while another runtime is live.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0` (a zero-capacity inbox could never accept
    /// a command; every sender would spuriously report backpressure).
    pub fn with_capacity(
        capacity: usize,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, UiRealmError> {
        assert!(capacity > 0, "UiRealm inbox capacity must be non-zero");
        if REALM_CLAIMED.swap(true, Ordering::AcqRel) {
            return Err(UiRealmError::AlreadyExists);
        }
        let incarnation = NEXT_INCARNATION.fetch_add(1, Ordering::Relaxed);
        let generation = NonZeroU32::new(incarnation)
            .expect("BUG: incarnation counter starts at 1 and only increments");
        // Slot 0 is the single-window slot; a real multi-window `AppRuntime`
        // registry mints slots once it exists (ADR-0027 §8 — the shape is the
        // deliverable, single-window the only instantiation).
        let realm_id = RealmId::new_gen(0, generation);
        let (tx, rx) = bounded(capacity);
        let redraw_pending = Arc::new(AtomicBool::new(false));
        Ok(Self {
            realm_id,
            rx,
            sender_prototype: UiCommandSender {
                tx,
                capacity,
                redraw_pending: Arc::clone(&redraw_pending),
                wake,
            },
            redraw_pending,
            _owner_affine: PhantomData,
        })
    }

    /// This incarnation's generational realm identity (ADR-0027 §6).
    #[must_use]
    pub fn realm_id(&self) -> RealmId {
        self.realm_id
    }

    /// A new cross-thread sender into this runtime's inbox.
    #[must_use]
    pub fn command_sender(&self) -> UiCommandSender {
        self.sender_prototype.clone()
    }

    /// Consume the coalesced redraw request, if any (ADR-0027 §4).
    ///
    /// The runner merges this into its dirty gate each frame; reading clears
    /// the flag so the next request wakes again.
    #[must_use]
    pub fn take_redraw_request(&self) -> bool {
        self.redraw_pending.swap(false, Ordering::AcqRel)
    }

    /// Drain the inbox on the owner thread: run queued closures and commit
    /// fresh worker results, in strict FIFO order.
    ///
    /// Contract (ADR-0027 §3): call only at frame boundaries — immediately
    /// before entering `drive_frame` and/or after it returns — never inside
    /// the frame transaction. Enforced in debug builds against the
    /// transitional global scheduler's phase; the thread affinity itself is
    /// structural (`UiRealm: !Send + !Sync`), not asserted.
    pub fn drain_commands(&mut self) -> DrainReport {
        debug_assert_eq!(
            Scheduler::instance().phase(),
            SchedulerPhase::Idle,
            "UiRealm::drain_commands must run at a frame boundary (Idle), \
             never inside the frame transaction (ADR-0027 §3)"
        );
        let mut report = DrainReport::default();
        // Bound the pass by the pre-read length: `try_iter` is NOT a
        // snapshot — it keeps yielding messages that arrive during
        // iteration, so an unbounded loop could be extended indefinitely by
        // a producer keeping pace (or by a drained command re-enqueueing
        // through a sender clone). Commands sent during this drain land in
        // the NEXT drain — deterministic batches, no owner starvation.
        let pending = self.rx.len();
        for _ in 0..pending {
            let Ok(command) = self.rx.try_recv() else {
                break;
            };
            match command {
                UiCommand::Invoke(run) => {
                    run();
                    report.invoked += 1;
                }
                UiCommand::Result { stamp, apply } => {
                    if stamp.is_fresh(self.realm_id) {
                        apply();
                        report.applied += 1;
                    } else {
                        tracing::trace!(
                            stamped_window = ?stamp.realm_id(),
                            owner_realm = ?self.realm_id,
                            "dropping stale worker result at commit point"
                        );
                        report.dropped_stale += 1;
                    }
                }
            }
        }
        report
    }
}

impl Drop for UiRealm {
    fn drop(&mut self) {
        REALM_CLAIMED.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::thread::ThreadId;

    use flui_foundation::GenerationGate;

    use super::*;

    /// Serializes tests that claim the process-global `REALM_CLAIMED`
    /// flag (the repo rule for tests mutating shared binding state —
    /// AGENTS.md "Testing Quirks"). nextest gives each test its own
    /// process, but `cargo test` / IDE runners share one.
    static REALM_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn noop_wake() -> Arc<dyn Fn() + Send + Sync> {
        Arc::new(|| {})
    }

    fn counting_wake() -> (Arc<dyn Fn() + Send + Sync>, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        let count_in_wake = Arc::clone(&count);
        (
            Arc::new(move || {
                count_in_wake.fetch_add(1, Ordering::Relaxed);
            }),
            count,
        )
    }

    #[test]
    fn senders_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UiCommandSender>();
        assert_send_sync::<ResultStamp>();
        assert_send_sync::<CommandSendError>();
    }

    #[test]
    fn at_most_one_runtime_second_construction_fails_typed() {
        let _claim = REALM_TEST_LOCK.lock();
        let first = UiRealm::new(noop_wake()).expect("first runtime claims");
        let second = UiRealm::new(noop_wake());
        assert!(matches!(second, Err(UiRealmError::AlreadyExists)));
        drop(first);
        let third = UiRealm::new(noop_wake()).expect("claim released on drop");
        drop(third);
    }

    #[test]
    fn recreated_runtime_gets_fresh_realm_id() {
        let _claim = REALM_TEST_LOCK.lock();
        let first = UiRealm::new(noop_wake()).expect("first runtime");
        let first_id = first.realm_id();
        drop(first);
        let second = UiRealm::new(noop_wake()).expect("second incarnation");
        assert_ne!(
            first_id,
            second.realm_id(),
            "a recreated window must never compare equal to its predecessor"
        );
    }

    #[test]
    fn cross_thread_invoke_runs_on_owner_thread_in_fifo_order() {
        let _claim = REALM_TEST_LOCK.lock();
        let mut runtime = UiRealm::new(noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        let owner_thread = std::thread::current().id();
        let (observed_tx, observed_rx) = mpsc::channel::<(usize, ThreadId)>();

        let worker = std::thread::spawn(move || {
            for sequence in 0..4 {
                let observed = observed_tx.clone();
                sender
                    .invoke(move || {
                        observed
                            .send((sequence, std::thread::current().id()))
                            .expect("test receiver alive");
                    })
                    .expect("inbox has room");
            }
        });
        worker.join().expect("sender thread panicked");

        let report = runtime.drain_commands();
        assert_eq!(report.invoked, 4);
        let executions: Vec<(usize, ThreadId)> = observed_rx.try_iter().collect();
        assert_eq!(
            executions
                .iter()
                .map(|(sequence, _)| *sequence)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3],
            "drain order must be deterministic FIFO"
        );
        assert!(
            executions.iter().all(|(_, thread)| *thread == owner_thread),
            "every command must execute on the owner thread"
        );
    }

    #[test]
    fn inbox_reports_backpressure_at_capacity() {
        let _claim = REALM_TEST_LOCK.lock();
        let mut runtime = UiRealm::with_capacity(2, noop_wake()).expect("runtime with tiny inbox");
        let sender = runtime.command_sender();
        sender.invoke(|| {}).expect("first fits");
        sender.invoke(|| {}).expect("second fits");
        let overflow = sender.invoke(|| {});
        assert_eq!(overflow, Err(CommandSendError::ChannelFull { capacity: 2 }));
        // Draining frees the inbox again.
        let _ = runtime.drain_commands();
        sender.invoke(|| {}).expect("room after drain");
    }

    #[test]
    fn dropped_runtime_yields_owner_gone() {
        let _claim = REALM_TEST_LOCK.lock();
        let runtime = UiRealm::new(noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        drop(runtime);
        assert_eq!(sender.invoke(|| {}), Err(CommandSendError::OwnerGone));
    }

    #[test]
    fn stale_generation_result_is_dropped_at_commit() {
        let _claim = REALM_TEST_LOCK.lock();
        let mut runtime = UiRealm::new(noop_wake()).expect("runtime");
        let sender = runtime.command_sender();
        let gate = GenerationGate::new();
        let applied = Arc::new(AtomicUsize::new(0));

        let fresh_stamp = ResultStamp::current(runtime.realm_id(), &gate);
        let stale_stamp = ResultStamp::current(runtime.realm_id(), &gate);
        let _invalidated = gate.bump(); // both stamps above die here
        let current_stamp = ResultStamp::current(runtime.realm_id(), &gate);

        for stamp in [fresh_stamp, stale_stamp, current_stamp] {
            let applied_in_result = Arc::clone(&applied);
            sender
                .submit_result(stamp, move || {
                    applied_in_result.fetch_add(1, Ordering::Relaxed);
                })
                .expect("inbox has room");
        }

        let report = runtime.drain_commands();
        assert_eq!(
            report.applied, 1,
            "only the current-generation result applies"
        );
        assert_eq!(report.dropped_stale, 2);
        assert_eq!(applied.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn foreign_window_result_is_dropped_at_commit() {
        let _claim = REALM_TEST_LOCK.lock();
        let gate = GenerationGate::new();
        // Stamp against the FIRST incarnation, then recreate the runtime:
        // the stamp must not apply to the successor window.
        let first = UiRealm::new(noop_wake()).expect("first incarnation");
        let dead_window_stamp = ResultStamp::current(first.realm_id(), &gate);
        drop(first);

        let mut runtime = UiRealm::new(noop_wake()).expect("second incarnation");
        let sender = runtime.command_sender();
        let applied = Arc::new(AtomicUsize::new(0));
        let applied_in_result = Arc::clone(&applied);
        sender
            .submit_result(dead_window_stamp, move || {
                applied_in_result.fetch_add(1, Ordering::Relaxed);
            })
            .expect("inbox has room");

        let report = runtime.drain_commands();
        assert_eq!(report.applied, 0);
        assert_eq!(report.dropped_stale, 1);
        assert_eq!(applied.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn redraw_requests_coalesce_to_one_flag_and_one_wake() {
        let _claim = REALM_TEST_LOCK.lock();
        let (wake, wake_count) = counting_wake();
        let runtime = UiRealm::new(wake).expect("runtime");
        let sender = runtime.command_sender();

        sender.request_redraw();
        sender.request_redraw();
        sender.request_redraw();
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            1,
            "a burst of redraw requests pays exactly one wake"
        );
        assert!(runtime.take_redraw_request(), "flag observed once");
        assert!(!runtime.take_redraw_request(), "reading clears the flag");

        sender.request_redraw();
        assert_eq!(
            wake_count.load(Ordering::Relaxed),
            2,
            "after the owner consumes the flag, the next request wakes again"
        );
    }

    #[test]
    fn every_send_wakes_the_owner() {
        let _claim = REALM_TEST_LOCK.lock();
        let (wake, wake_count) = counting_wake();
        let mut runtime = UiRealm::new(wake).expect("runtime");
        let sender = runtime.command_sender();
        sender.invoke(|| {}).expect("inbox has room");
        let gate = GenerationGate::new();
        sender
            .submit_result(ResultStamp::current(runtime.realm_id(), &gate), || {})
            .expect("inbox has room");
        assert_eq!(wake_count.load(Ordering::Relaxed), 2);
        let _ = runtime.drain_commands();
    }
}
