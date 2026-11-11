//! The owner inbox: `UiCommand`, `UiCommandSender`, and draining it on the owner thread.

use super::UiRuntime;
use crossbeam_channel::{Sender, TrySendError};
use flui_foundation::PresentationId;
use flui_foundation::panic::FirstPanic;
use flui_scheduler::SchedulerPhase;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Errors returned by [`UiCommandSender`] sends.
///
/// Same shape as the pipeline dirty-channel errors: bounded channels surface
/// backpressure as a typed value, and a dropped owner is a typed value — the
/// producer decides what to do, nothing blocks, nothing grows unbounded.
#[derive(Debug, thiserror::Error)]
pub enum CommandSendError {
    /// The inbox is full; the producer must back off (retry next frame,
    /// drop, or escalate — its call).
    #[error("ui_runtime command inbox full ({capacity} capacity); back off and retry")]
    ChannelFull {
        /// Configured inbox capacity.
        capacity: usize,
        /// Rejected command, returned intact so the framework can retry.
        rejected: UiCommand,
    },

    /// The owning [`UiRuntime`] has been dropped; this sender is now
    /// permanently inert and the producer should stop sending.
    #[error("ui ui_runtime dropped; command sender is no longer valid")]
    OwnerGone {
        /// Rejected command, returned intact to the framework caller.
        rejected: UiCommand,
    },
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// A command enqueued for the owner thread.
///
/// Three addressing classes, by design, not oversight:
///
/// - **Presentation-scoped** commands ([`Self::Accessibility`]) carry an
///   explicit `presentation_id` stamp and are validated against the live
///   presentation at drain time, because the UI runtime's inbox outlives any one
///   presentation incarnation within it (the eventual element-forest case)
///   and a sender may have been vended for a presentation that no longer
///   owns this UI runtime's inbox.
/// - **Runtime-scoped** commands (`HotReload`)
///   carry no stamp of their own and stay bound by channel identity alone:
///   a recreated UI runtime mints new channels, and a sender into the dead UI runtime
///   already gets `OwnerGone` at send — re-stamping UI runtime identity on top of
///   that would duplicate a structural fact the channel already enforces.
/// - **Graph-addressed** commands (`SignalWrite`) carry the signal slot they
///   target and are routed by `SignalSlot::graph` to the presentation whose
///   `BuildOwner` minted it (ADR-0085 §1). They need no presentation stamp:
///   graph ids are process-unique, so two presentation incarnations never
///   share one.
pub enum UiCommand {
    /// Apply a hot-reload reassemble on the owner at the next Idle drain.
    #[cfg(feature = "hot-reload")]
    HotReload(crate::reload::ReloadTier),
    /// [`HostInput::Accessibility`](crate::HostInput::Accessibility) from the
    /// platform adapter's thread, or dispatched on the owner thread, addressed
    /// to the exact presentation that was live when the sender stamped it and
    /// unwrapped on the owner thread at the next Idle drain: every assistive
    /// request takes this path (ADR-0207, amendment).
    Accessibility {
        /// The presentation this request was stamped for.
        presentation_id: PresentationId,
        /// The request, decoded on the owner thread.
        request: accesskit::ActionRequest,
    },
    /// [`HostInput::AssistiveFocus`](crate::HostInput::AssistiveFocus) from the
    /// platform adapter's thread, addressed and drained as
    /// [`Self::Accessibility`] is, so a cursor move and the actions around it
    /// apply in the order the platform reported them.
    AssistiveFocus {
        /// The presentation this move was stamped for.
        presentation_id: PresentationId,
        /// The node the cursor is on, or `None` off the tree.
        node: Option<accesskit::NodeId>,
    },
    /// One window method of the development agent's, for the addressed presentation: run at
    /// the drain, answered through the call's reply (`agent_serve`).
    Agent {
        /// The presentation the agent's window addresses.
        presentation_id: PresentationId,
        /// The method, its parameters and its reply.
        call: flui_view::__runtime::WindowCall,
    },
    /// Register a subscription to the addressed presentation's events and answer with its
    /// stream.
    AgentSubscribe {
        /// The presentation the agent's window addresses.
        presentation_id: PresentationId,
        /// The events to stream.
        filters: Vec<flui_protocol::EventFilter>,
        /// Where the stream goes.
        reply: flui_view::__runtime::Reply<flui_view::dev_agent::AgentEvents>,
    },
    /// Start a recording of the UI runtime that hosts the addressed presentation and answer
    /// with it (`agent_record`).
    AgentRecord {
        /// The presentation the agent's window addresses.
        presentation_id: PresentationId,
        /// What to record.
        params: flui_protocol::RecordParams,
        /// Where the recording goes.
        reply: flui_view::__runtime::Reply<flui_view::dev_agent::AgentRecording>,
    },
    /// Run a write against the reactive graph that minted `target` (ADR-0074
    /// §5.8, routed per ADR-0085 §1) on the owner thread: the cross-thread
    /// way to write a signal. Readers it marks land in the owning
    /// presentation's inbox for the next frame — enqueue-and-wake, never
    /// touch the tree.
    SignalWrite {
        /// The slot the write targets; its `graph()` selects the presentation.
        target: flui_view::SignalSlot,
        /// Runs inside an `EventCx` opened by the `WriterSource` of the
        /// presentation whose graph minted `target`, and of no other. A
        /// retained `FnMut` envelope invoked at most once: after a callback
        /// panic its captures are destroyed under containment, the first
        /// failure staying first (ADR-0281 §3).
        apply: Box<dyn FnMut(&mut flui_view::EventCx<'_>) + Send>,
    },
}

impl std::fmt::Debug for UiCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "hot-reload")]
            UiCommand::HotReload(tier) => {
                f.debug_tuple("UiCommand::HotReload").field(tier).finish()
            }
            UiCommand::Accessibility {
                presentation_id,
                request,
            } => f
                .debug_struct("UiCommand::Accessibility")
                .field("presentation_id", presentation_id)
                .field("request", request)
                .finish(),
            UiCommand::AssistiveFocus {
                presentation_id,
                node,
            } => f
                .debug_struct("UiCommand::AssistiveFocus")
                .field("presentation_id", presentation_id)
                .field("node", node)
                .finish(),
            UiCommand::Agent {
                presentation_id,
                call,
            } => f
                .debug_struct("UiCommand::Agent")
                .field("presentation_id", presentation_id)
                .field("call", call)
                .finish(),
            UiCommand::AgentSubscribe {
                presentation_id,
                filters,
                ..
            } => f
                .debug_struct("UiCommand::AgentSubscribe")
                .field("presentation_id", presentation_id)
                .field("filters", filters)
                .finish_non_exhaustive(),
            UiCommand::AgentRecord {
                presentation_id,
                params,
                ..
            } => f
                .debug_struct("UiCommand::AgentRecord")
                .field("presentation_id", presentation_id)
                .field("layers", &params.layers)
                .finish_non_exhaustive(),
            UiCommand::SignalWrite { target, .. } => f
                .debug_struct("UiCommand::SignalWrite")
                .field("target", target)
                .finish_non_exhaustive(),
        }
    }
}

/// What one [`UiRuntime::drain_commands`] pass did, for observability
/// and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct DrainReport {
    /// Owner commands successfully applied.
    pub invoked: usize,
    /// Commands whose typed owner target is stale or no longer live.
    pub dropped_stale: usize,
}

// ---------------------------------------------------------------------------
// Sender
// ---------------------------------------------------------------------------

/// Identity tokens for coalesced platform wakes.
///
/// A boolean is insufficient because senders are concurrent: an older wake
/// may return successfully after a newer wake panics, and must not clear the
/// newer demand. Each request therefore replaces the current token; a wake
/// can acknowledge only the exact token it captured. Unlike an integer
/// generation, token identity cannot saturate on 32-bit targets.
#[derive(Default)]
pub(super) struct WakeDebt {
    current: parking_lot::Mutex<Option<Arc<AtomicBool>>>,
}

impl std::fmt::Debug for WakeDebt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WakeDebt")
            .field("pending", &self.pending_token().is_some())
            .finish()
    }
}

impl WakeDebt {
    fn request(&self) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        *self.current.lock() = Some(Arc::clone(&token));
        token
    }

    fn pending_token(&self) -> Option<Arc<AtomicBool>> {
        self.current
            .lock()
            .as_ref()
            .filter(|token| !token.load(Ordering::Acquire))
            .cloned()
    }

    fn acknowledge(token: &AtomicBool) {
        token.store(true, Ordering::Release);
    }
}

/// Cross-thread capability into a [`UiRuntime`]'s inbox.
///
/// `Clone + Send + Sync`. A sender can enqueue a command and wake the owner;
/// it can never obtain a reference into any tree, invoke a lifecycle
/// callback, or run build/layout/paint. Every enqueued command
/// executes on the owner thread, at the next Idle drain.
#[derive(Clone)]
pub struct UiCommandSender {
    pub(super) tx: Sender<UiCommand>,
    pub(super) capacity: usize,
    pub(super) redraw_pending: Arc<AtomicBool>,
    /// A platform wake accepts each command/redraw token only once that exact
    /// token is acknowledged. A panicking wake leaves debt so the
    /// next command ingress or completed owner-inbox drain retries delivery
    /// instead of mistaking durable queue/dirty flags for event-loop progress.
    pub(super) wake_debt: Arc<WakeDebt>,
    /// The presentation this sender stamps onto every presentation-scoped
    /// command it sends (set once, at [`UiRuntime::construct`]). For the
    /// eventual element forest, senders become vended per-presentation with
    /// different stamps into the same UI runtime inbox — this field is already
    /// the right shape; only the vending point changes.
    pub(super) presentation_id: PresentationId,
    /// Fired after every successful state change so an idle event loop
    /// produces the drain that observes it — the enqueue-then-wake contract,
    /// same as `RenderInvalidationHandle`'s notifier.
    pub(super) wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for UiCommandSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiCommandSender")
            .field("capacity", &self.capacity)
            .field("presentation_id", &self.presentation_id)
            .field("pending", &self.tx.len())
            .field(
                "redraw_pending",
                &self.redraw_pending.load(Ordering::Relaxed),
            )
            .field("wake_debt", &self.wake_debt.pending_token().is_some())
            .finish_non_exhaustive()
    }
}

impl UiCommandSender {
    /// Enqueue a hot-reload request for the UI runtime owner.
    ///
    /// Unlike direct platform dispatch, this capability is safe to call from
    /// any thread: delivery occurs at the owner's next Idle drain and the
    /// normal enqueue-and-wake contract pumps that drain.
    ///
    /// # Errors
    ///
    /// [`CommandSendError`] when the inbox is full or the UI runtime is gone.
    #[cfg(feature = "hot-reload")]
    pub fn request_hot_reload(
        &self,
        tier: crate::reload::ReloadTier,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::HotReload(tier))
    }

    /// Enqueue an assistive-technology request as host input for owner-local
    /// semantics resolution.
    ///
    /// The sender itself selects the target ui_runtime/presentation; the request
    /// carries only the stable node identity exported by that presentation's
    /// snapshot. Delivery is bounded, FIFO, and committed only at the next
    /// Idle drain, where it is decoded.
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure,
    /// [`CommandSendError::OwnerGone`] once the runtime is dropped.
    pub(crate) fn send_accessibility(
        &self,
        request: accesskit::ActionRequest,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::Accessibility {
            presentation_id: self.presentation_id,
            request,
        })
    }

    /// Enqueue a move of the assistive technology's cursor as host input, in
    /// the same FIFO as [`Self::send_accessibility`].
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure,
    /// [`CommandSendError::OwnerGone`] once the runtime is dropped.
    pub(crate) fn send_assistive_focus(
        &self,
        node: Option<accesskit::NodeId>,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::AssistiveFocus {
            presentation_id: self.presentation_id,
            node,
        })
    }

    /// Enqueue a signal write for the owner thread (ADR-0074 §5.8). At the
    /// next Idle drain `apply` receives `target` re-attached and an `EventCx`
    /// opened by the writer source of the presentation whose graph minted it
    /// (ADR-0085 §1), wherever in this UI runtime that presentation lives; a
    /// write it performs marks readers for the following frame. If no
    /// presentation of this UI runtime owns that graph any more, `apply` is
    /// dropped without running and the drain counts the command as stale.
    ///
    /// The routing key comes from `target` itself, so a caller cannot
    /// address a write to one graph and perform it against another.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "cross-thread signal write sender is wired before public runtime vending"
        )
    )]
    pub(crate) fn send_signal_write<T: 'static>(
        &self,
        target: flui_view::SignalSender<T>,
        mut apply: impl FnMut(flui_view::Signal<T>, &mut flui_view::EventCx<'_>) + Send + 'static,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::SignalWrite {
            target: target.slot(),
            apply: Box::new(move |cx| apply(target.attach(), cx)),
        })
    }

    /// Request a redraw of the UI runtime's presentation, coalesced: any number of pending
    /// requests collapse into one flag read by the owner at the next drain
    /// (the `needs_redraw` precedent — idempotent dirty marks).
    ///
    /// Infallible and idempotent by design: the flag outlives the runtime,
    /// and a request against a dropped runtime is a harmless no-op (the wake
    /// has no loop left to wake).
    // Test-only: the typed redraw capability is not yet vended externally.
    #[cfg(any(test, feature = "testing"))]
    pub fn request_redraw(&self) {
        // `swap` (not store) so only the first request in a burst pays the
        // wake; a pending frame absorbs repeated wakes anyway, this just
        // skips redundant platform calls.
        if !self.redraw_pending.swap(true, Ordering::AcqRel)
            || self.wake_debt.pending_token().is_some()
        {
            self.wake_owner();
        }
    }

    /// The inbox's configured capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub(super) fn send(&self, command: UiCommand) -> Result<(), CommandSendError> {
        self.deliver_enqueued(self.enqueue(command))
    }

    /// Serialize only durable admission; delivery runs after any caller's fence.
    pub(super) fn enqueue(&self, command: UiCommand) -> Result<(), CommandSendError> {
        match self.tx.try_send(command) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(rejected)) => Err(CommandSendError::ChannelFull {
                capacity: self.capacity,
                rejected,
            }),
            Err(TrySendError::Disconnected(rejected)) => {
                Err(CommandSendError::OwnerGone { rejected })
            }
        }
    }

    /// Deliver accepted demand, or retry prior delivery debt under backpressure.
    pub(super) fn deliver_enqueued(
        &self,
        result: Result<(), CommandSendError>,
    ) -> Result<(), CommandSendError> {
        match result {
            Ok(()) => {
                self.wake_owner();
                Ok(())
            }
            Err(CommandSendError::ChannelFull { capacity, rejected }) => {
                // A failed wake can leave an accepted command in a full inbox.
                // Backpressure must retry its debt after admission locks retire.
                #[expect(
                    clippy::disallowed_methods,
                    reason = "ADR-0287: the platform wake is user code; a panic must not lose the debt"
                )]
                let wake = catch_unwind(AssertUnwindSafe(|| self.retry_wake_debt()));
                if let Err(wake_payload) = wake {
                    // The rejected envelope is destroyed under containment; a
                    // destructor that panics does not replace the wake failure
                    // (ADR-0281 §3).
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "ADR-0287: a rejected envelope's drop must not replace the wake failure"
                    )]
                    if let Err(dropped) = catch_unwind(AssertUnwindSafe(|| drop(rejected))) {
                        flui_foundation::panic::discard_payload(dropped);
                    }
                    resume_unwind(wake_payload);
                }
                Err(CommandSendError::ChannelFull { capacity, rejected })
            }
            Err(error @ CommandSendError::OwnerGone { .. }) => Err(error),
        }
    }

    /// Deliver one platform wake while retaining failed-delivery debt.
    ///
    /// The demand itself is already durable in the inbox/redraw flags. This
    /// identity token records that the host has not yet accepted the owner
    /// turn needed to observe that demand. Only a normally returning wake
    /// acknowledges its captured token; unwinding therefore leaves
    /// the next ingress or owner boundary responsible for retrying it.
    pub(super) fn wake_owner(&self) {
        let token = self.wake_debt.request();
        self.deliver_wake(token);
    }

    fn deliver_wake(&self, token: Arc<AtomicBool>) {
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: the platform wake is user code; a panic clears the pending token"
        )]
        let outcome = catch_unwind(AssertUnwindSafe(|| (self.wake)()));
        match outcome {
            Ok(()) => WakeDebt::acknowledge(&token),
            Err(payload) => resume_unwind(payload),
        }
    }

    /// Retry a wake that a prior owner-turn rearm failed to deliver.
    fn retry_wake_debt(&self) {
        if let Some(token) = self.wake_debt.pending_token() {
            self.deliver_wake(token);
        }
    }
}

impl UiRuntime {
    /// A new cross-thread sender into this runtime's inbox.
    #[must_use]
    pub fn command_sender(&self) -> UiCommandSender {
        self.sender_prototype.clone()
    }

    /// Tell the UI runtime that a face was registered on the app's font
    /// collection: every presentation draws its next frame, and that frame's
    /// pipeline lays out again the text measured before the face existed.
    ///
    /// Only wakes: the marking happens when each pipeline drains before its
    /// next frame (`PipelineOwner::apply_font_change`), which compares the
    /// collection's generation with the one it last applied, so a notice for
    /// a change a pipeline already applied lays nothing out. Owner thread,
    /// at a frame boundary; the app's registration sends it to every UI runtime.
    pub fn fonts_changed(&self) {
        for presentation in self.presentations.iter() {
            self.request_redraw_for(presentation);
        }
    }

    /// Consume the coalesced redraw request, if any.
    ///
    /// The runner merges this into its dirty gate each frame; reading clears
    /// the flag so the next request wakes again.
    #[must_use]
    pub fn take_redraw_request(&self) -> bool {
        self.redraw_pending.swap(false, Ordering::AcqRel)
    }

    /// An agent job's user code panicked with `payload`. The reply gets `answer`
    /// first, so the agent learns of it whatever happens next; then a queued
    /// tail gets a future owner turn, since this one is unwinding; then the
    /// original panic resumes, never replaced by a later one.
    pub(super) fn fail_agent_job<T>(
        &self,
        reply: &super::agent::ReplySender<T>,
        element: Option<flui_protocol::ElementId>,
        answer: Result<T, super::AgentError>,
        payload: Box<dyn std::any::Any + Send>,
    ) -> ! {
        // An agent reply's receiver runs contained before the panic resumes.
        let mut first_panic = FirstPanic::new();
        first_panic.record(payload);
        first_panic.contain(|| {
            super::agent::send_reply(reply, element, answer);
        });
        if self.has_queued_work() {
            // The owner wake runs contained before the reply panic resumes.
            first_panic.contain(|| {
                self.sender_prototype.wake_owner();
            });
        }
        resume_unwind(
            first_panic
                .into_payload()
                .expect("BUG: the semantics agent job panic must be preserved"),
        );
    }

    /// Whether a command or an owner job waits for a later turn: what a
    /// panicking drain must leave a wake for.
    fn has_queued_work(&self) -> bool {
        !self.rx.is_empty() || self.owner_jobs.has_queued()
    }

    fn rearm_after_command_panic(&self, payload: Box<dyn std::any::Any + Send>) -> ! {
        let mut first_panic = FirstPanic::new();
        first_panic.record(payload);
        // The owner wake runs contained before the command panic resumes.
        first_panic.contain(|| {
            self.sender_prototype.wake_owner();
        });
        let payload = first_panic
            .into_payload()
            .expect("BUG: the original command panic must be preserved");
        resume_unwind(payload);
    }

    /// Drain the closed command inbox on the owner thread in strict FIFO
    /// order.
    ///
    /// Call only at frame boundaries — immediately before entering
    /// `drive_frame` and/or after it returns — never inside the frame
    /// transaction. Enforced in debug builds against the transitional global
    /// scheduler's phase; the thread affinity itself is structural
    /// (`UiRuntime: !Send + !Sync`), not asserted.
    ///
    /// The drain runs inside the UI runtime's entry ([`Self::enter`]), whether or
    /// not the caller already entered it (entry nests): a semantics action's
    /// handler lives in the UI runtime's interaction lane, so an action drained
    /// outside it would be counted as invoked while its handler never ran.
    pub fn drain_commands(&self) -> DrainReport {
        self.enter(Self::drain_commands_entered)
    }

    /// [`Self::drain_commands`]'s body, run inside the UI runtime's entry.
    fn drain_commands_entered(&self) -> DrainReport {
        debug_assert_eq!(
            self.scheduler.phase(),
            SchedulerPhase::Idle,
            "UiRuntime::drain_commands must run at a frame boundary (Idle), \
             never inside the frame transaction"
        );
        let mut report = DrainReport::default();
        // Counted before the commands are: a lease let go is released after the calls asked
        // through it, which were enqueued before it was let go.
        let released = self.count_released_leases();
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
                #[cfg(feature = "hot-reload")]
                UiCommand::HotReload(tier) => {
                    // A failed reassemble may already have changed a tree. Keep
                    // redraw demand and the accepted FIFO tail live before resuming.
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "ADR-0287: a failed hot reload keeps redraw demand and the tail live"
                    )]
                    let outcome = catch_unwind(AssertUnwindSafe(|| self.apply_hot_reload(tier)));
                    match outcome {
                        Ok(changed) => {
                            if changed {
                                self.redraw_pending.store(true, Ordering::Release);
                            }
                        }
                        Err(payload) => {
                            self.redraw_pending.store(true, Ordering::Release);
                            self.rearm_after_command_panic(payload);
                        }
                    }
                    report.invoked += 1;
                }
                UiCommand::Accessibility {
                    presentation_id,
                    request,
                } => {
                    if self.presentations.get(presentation_id).is_none() {
                        // Accepted earlier, but the presentation closed first:
                        // the request was never applied.
                        tracing::warn!(
                            { flui_foundation::diagnostics::PRESENTATION_ID } =
                                presentation_id.as_u64(),
                            action = ?request.action,
                            "dropping an accepted accessibility request: its presentation closed before the drain"
                        );
                        report.dropped_stale += 1;
                        continue;
                    }
                    // A panicking handler keeps the accepted tail live, as a
                    // failed reassemble does, before resuming.
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "ADR-0287: an accessibility handler's panic keeps the tail live"
                    )]
                    match catch_unwind(AssertUnwindSafe(|| {
                        self.dispatch_accessibility(presentation_id, &request)
                    })) {
                        Ok(true) => report.invoked += 1,
                        Ok(false) => report.dropped_stale += 1,
                        Err(payload) => self.rearm_after_command_panic(payload),
                    }
                }
                UiCommand::AssistiveFocus {
                    presentation_id,
                    node,
                } => {
                    // A panicking handler keeps the accepted tail live, as an
                    // action's does, before resuming.
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "ADR-0287: an assistive focus handler's panic keeps the tail live"
                    )]
                    match catch_unwind(AssertUnwindSafe(|| {
                        self.dispatch_assistive_focus(presentation_id, node)
                    })) {
                        Ok(true) => report.invoked += 1,
                        Ok(false) => report.dropped_stale += 1,
                        Err(payload) => self.rearm_after_command_panic(payload),
                    }
                }
                UiCommand::Agent {
                    presentation_id,
                    call,
                } => self.serve_window_call(presentation_id, call, &mut report),
                UiCommand::AgentSubscribe {
                    presentation_id,
                    filters,
                    reply,
                } => {
                    let window = super::agent::window_of(presentation_id);
                    let Some(state) = self.presentations.get(presentation_id) else {
                        reply.send(Err(super::AgentError::PresentationGone.into_error(window)));
                        report.dropped_stale += 1;
                        continue;
                    };
                    let reply = super::agent::ReplySender::new(
                        reply,
                        window,
                        super::agent::issued_of(state),
                        |_, _| {},
                    );
                    self.subscribe_agent_events(state, filters, &reply);
                    report.invoked += 1;
                }
                UiCommand::AgentRecord {
                    presentation_id,
                    params,
                    reply,
                } => {
                    self.start_agent_recording(presentation_id, &params, &reply);
                    report.invoked += 1;
                }
                UiCommand::SignalWrite { target, mut apply } => {
                    let Some((presentation, source)) = self.signal_writer_for(target) else {
                        #[expect(
                            clippy::disallowed_methods,
                            reason = "ADR-0287: a tracing subscriber's panic must not lose the queued tail"
                        )]
                        let diagnosed = catch_unwind(AssertUnwindSafe(|| {
                            tracing::warn!(
                                target: "flui.signals",
                                graph = target.graph(),
                                slot = ?target,
                                "dropping a cross-thread signal write: no presentation of this \
                                 ui_runtime owns the slot's graph (its presentation closed, or the \
                                 handle belongs to another ui_runtime)"
                            );
                        }));
                        report.dropped_stale += 1;
                        if let Err(payload) = diagnosed {
                            // Diagnostics failed: the envelope is destroyed
                            // under containment before the failure resumes
                            // (ADR-0281 §3).
                            flui_foundation::panic::drop_contained(apply);
                            self.rearm_after_command_panic(payload);
                        }

                        // The stale envelope can contain several hostile
                        // captured destructors. Ensure an already-queued FIFO
                        // tail has a future owner turn before its opaque drop
                        // glue runs.
                        #[expect(
                            clippy::disallowed_methods,
                            reason = "ADR-0287: the owner wake runs contained before an opaque drop"
                        )]
                        if self.has_queued_work()
                            && let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                                self.sender_prototype.wake_owner();
                            }))
                        {
                            flui_foundation::panic::drop_contained(apply);
                            self.rearm_after_command_panic(payload);
                        }
                        // A command envelope's drop runs contained so the tail stays live.
                        let mut destroyed = FirstPanic::new();
                        destroyed.contain(|| drop(apply));
                        if destroyed.is_held() {
                            // A producer can enqueue after the empty-inbox
                            // snapshot while opaque destruction is blocked.
                            // If that ingress's wake failed, pay its shared
                            // debt before resuming the destructor panic.
                            destroyed.contain(|| {
                                self.sender_prototype.retry_wake_debt();
                            });
                            resume_unwind(destroyed.into_payload().expect(
                                "BUG: the stale command destructor panic must be preserved",
                            ));
                        }
                        continue;
                    };
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "ADR-0287: a signal write callback runs contained so the tail stays live"
                    )]
                    let outcome = catch_unwind(AssertUnwindSafe(|| source.write(|cx| apply(cx))));
                    // Keep the command envelope outside the caught invocation.
                    // Consuming a boxed `FnOnce` there would destroy captures
                    // during a callback unwind, where a panicking destructor
                    // would abort before redraw and wake recovery can run.
                    // A secondary presentation's BuildOwner has no wake hook,
                    // so the owning presentation's frame is requested here.
                    // Do it before resuming a command panic: a signal update
                    // may have partially committed and already invalidated
                    // this presentation's readers.
                    presentation.mark_redraw_pending();
                    self.redraw_pending.store(true, Ordering::Release);
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "ADR-0287: a replaced command envelope's drop runs contained after the write"
                    )]
                    let command_panic = match outcome {
                        Err(payload) => {
                            // The envelope is destroyed under containment
                            // after the callback failed too (ADR-0281 §3).
                            flui_foundation::panic::drop_contained(apply);
                            Some(payload)
                        }
                        Ok(()) => catch_unwind(AssertUnwindSafe(|| drop(apply))).err(),
                    };
                    // The send wake is the owner turn currently unwinding.
                    // Re-arm the platform so the remaining FIFO tail and a
                    // partial commit cannot starve in an otherwise idle host.
                    // An external wake panic must not replace the command's
                    // original payload.
                    if let Some(payload) = command_panic {
                        self.rearm_after_command_panic(payload);
                    }
                    report.invoked += 1;
                }
            }
        }
        // The input of the leases let go is released, every window's, and then the recordings
        // their readers stopped or let go end, before a panic continues: a recording stopped
        // here holds the release.
        let mut first = self.release_counted_leases(released);
        first.absorb(self.sweep_agent_recordings());
        if let Some(payload) = first.into_payload() {
            self.rearm_after_command_panic(payload);
        }
        // Then what providers posted back: settled completions run their
        // waiting closures here, at the owner turn, never on the thread that
        // answered. A panicking closure keeps the queued tail live, as a
        // command's does, before resuming.
        report.invoked += self.owner_jobs.drain(|job, scope| {
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: an owner job's panic keeps the queued tail live before resuming"
            )]
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| job.run(scope))) {
                self.rearm_after_command_panic(payload);
            }
        });
        // A prior rearm wake may have panicked after leaving queue/redraw
        // demand durable. This completed owner boundary has made progress;
        // retry the delivery now so any work enqueued during this turn still
        // receives a future opportunity. A second wake panic deliberately
        // escapes with the debt still armed for a later ingress/boundary.
        self.sender_prototype.retry_wake_debt();
        report
    }
}
