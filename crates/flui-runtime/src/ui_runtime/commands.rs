//! The owner inbox: `UiCommand`, `UiCommandSender`, and draining it on the owner thread.

use super::UiRuntime;
use super::input::preserve_first_input_panic;
use crossbeam_channel::{Sender, TrySendError};
use flui_foundation::PresentationId;
use flui_scheduler::SchedulerPhase;
use flui_semantics::SemanticsActionRequest;
use flui_widgets::NavigatorCommand;
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
/// - **Presentation-scoped** commands ([`Self::SemanticsAction`]) carry an
///   explicit `presentation_id` stamp and are validated against the live
///   presentation at drain time, because the UI runtime's inbox outlives any one
///   presentation incarnation within it (the eventual element-forest case)
///   and a sender may have been vended for a presentation that no longer
///   owns this UI runtime's inbox.
/// - **Runtime-scoped** commands (`HotReload`, [`Self::Navigation`])
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
    /// Resolve and invoke an accessibility action on the owner thread,
    /// addressed to the exact presentation that was live when the sender
    /// stamped it.
    SemanticsAction {
        /// The presentation this action was stamped for.
        presentation_id: PresentationId,
        /// The stable node identity and action to resolve.
        request: SemanticsActionRequest,
    },
    /// Read the addressed presentation's semantics tree as wire nodes, for
    /// a [`SemanticsAgent`](super::SemanticsAgent), and answer on `reply`.
    SemanticsRead {
        /// The presentation the agent was vended for.
        presentation_id: PresentationId,
        /// How much of the tree to read.
        query: flui_protocol::ReadQuery,
        /// Whether the agent's reads ever reported the query's root; true
        /// when the query has none.
        issued: bool,
        /// Where the answer goes.
        reply: super::agent::ReplySender<flui_protocol::Tree>,
    },
    /// Resolve an agent's wire action against the addressed presentation's
    /// committed tree, invoke it, and answer on `reply`.
    SemanticsAgentAction {
        /// The presentation the agent was vended for.
        presentation_id: PresentationId,
        /// The element and action.
        request: flui_protocol::ActionRequest,
        /// Whether the agent's reads ever reported the element.
        issued: bool,
        /// Where the answer goes.
        reply: super::agent::ReplySender<()>,
    },
    /// Apply a typed navigator mutation on the owner thread.
    Navigation(NavigatorCommand),
    /// Run a write against the reactive graph that minted `target` (ADR-0074
    /// §5.8, routed per ADR-0085 §1) on the owner thread: the cross-thread
    /// way to write a signal. Readers it marks land in the owning
    /// presentation's inbox for the next frame — enqueue-and-wake, never
    /// touch the tree.
    SignalWrite {
        /// The slot the write targets; its `graph()` selects the presentation.
        target: flui_view::SignalSlot,
        /// Runs against the graph that minted `target`, and against no other.
        apply: Box<dyn FnMut(&flui_view::Reactive) + Send>,
    },
}

impl std::fmt::Debug for UiCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(feature = "hot-reload")]
            UiCommand::HotReload(tier) => {
                f.debug_tuple("UiCommand::HotReload").field(tier).finish()
            }
            UiCommand::SemanticsAction {
                presentation_id,
                request,
            } => f
                .debug_struct("UiCommand::SemanticsAction")
                .field("presentation_id", presentation_id)
                .field("request", request)
                .finish(),
            UiCommand::SemanticsRead {
                presentation_id,
                query,
                ..
            } => f
                .debug_struct("UiCommand::SemanticsRead")
                .field("presentation_id", presentation_id)
                .field("query", query)
                .finish_non_exhaustive(),
            // The request's value stays out: it may be what a user typed.
            UiCommand::SemanticsAgentAction {
                presentation_id,
                request,
                ..
            } => f
                .debug_struct("UiCommand::SemanticsAgentAction")
                .field("presentation_id", presentation_id)
                .field("element", &request.element)
                .field("action", &request.action)
                .finish_non_exhaustive(),
            UiCommand::Navigation(command) => f
                .debug_tuple("UiCommand::Navigation")
                .field(command)
                .finish(),
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

    /// Enqueue an accessibility action for owner-local semantics resolution.
    ///
    /// The sender itself selects the target ui_runtime/presentation; the request
    /// carries only the stable node identity exported by that presentation's
    /// snapshot. Delivery is bounded, FIFO, and committed only at the next
    /// Idle drain.
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure,
    /// [`CommandSendError::OwnerGone`] once the runtime is dropped.
    pub(crate) fn send_semantics_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::SemanticsAction {
            presentation_id: self.presentation_id,
            request,
        })
    }

    /// Enqueue a typed navigation command for owner-thread application.
    ///
    /// This is the ADR-0027 cross-thread navigation ingress. The sender only
    /// accepts the closed [`NavigatorCommand`] vocabulary; it does not expose a
    /// generic "run this closure on the UI thread" API.
    ///
    /// # Errors
    ///
    /// [`CommandSendError::ChannelFull`] under backpressure,
    /// [`CommandSendError::OwnerGone`] once the runtime is dropped.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "typed navigation command sender is wired before public runtime vending"
        )
    )]
    pub(crate) fn send_navigation(
        &self,
        command: NavigatorCommand,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::Navigation(command))
    }

    /// Enqueue a signal write for the owner thread (ADR-0074 §5.8). At the
    /// next Idle drain `apply` receives `target` re-attached and the graph
    /// that minted it (ADR-0085 §1), wherever in this UI runtime that graph lives;
    /// a write it performs marks readers for the following frame. If no
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
        mut apply: impl FnMut(flui_view::Signal<T>, &flui_view::Reactive) + Send + 'static,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::SignalWrite {
            target: target.slot(),
            apply: Box::new(move |reactive| apply(target.attach(), reactive)),
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
    #[cfg(any(test, feature = "test-support"))]
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
                let wake = catch_unwind(AssertUnwindSafe(|| self.retry_wake_debt()));
                if let Err(wake_payload) = wake {
                    // Retain the opaque rejected envelope once delivery failed;
                    // competing capture destructors cannot replace that failure.
                    std::mem::forget(rejected);
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

    fn rearm_after_command_panic(&self, payload: Box<dyn std::any::Any + Send>) -> ! {
        let wake_panic = catch_unwind(AssertUnwindSafe(|| {
            self.sender_prototype.wake_owner();
        }))
        .err();
        let mut first_panic = Some(payload);
        preserve_first_input_panic(&mut first_panic, wake_panic, "command redraw wake");
        let payload = first_panic.expect("BUG: the original command panic must be preserved");
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
                UiCommand::SemanticsAction {
                    presentation_id,
                    request,
                } => {
                    // Forest-membership check (issue #555's addressed-routing slice), not
                    // primary-only equality: a presentation generation this
                    // ui_runtime no longer hosts -- closed since the action was
                    // stamped, or a genuinely different (non-primary)
                    // presentation among several -- is a traced drop either
                    // way, never delivered to a sibling.
                    let Some(presentation) = self.presentations.get(presentation_id) else {
                        tracing::trace!(
                            stamped_presentation_id = presentation_id.as_u64(),
                            "dropping semantics action stamped for a presentation this ui_runtime no \
                             longer hosts"
                        );
                        report.dropped_stale += 1;
                        continue;
                    };
                    match presentation.dispatch_semantics_action(request) {
                        Ok(()) => {
                            report.invoked += 1;
                        }
                        Err(error) => {
                            // Actions for stale views/nodes are deliberately
                            // ignored because screen readers may lag behind
                            // the latest semantics update.
                            tracing::trace!(
                                { flui_foundation::diagnostics::PRESENTATION_ID } =
                                    presentation_id.as_u64(),
                                ?error,
                                "dropping semantics action against a stale snapshot"
                            );
                            report.dropped_stale += 1;
                        }
                    }
                }
                UiCommand::SemanticsRead {
                    presentation_id,
                    query,
                    issued,
                    reply,
                } => {
                    let result = self.serve_semantics_read(presentation_id, &query, issued);
                    if result == Err(super::AgentError::PresentationGone) {
                        report.dropped_stale += 1;
                    } else {
                        report.invoked += 1;
                    }
                    super::agent::send_reply(&reply, query.root, result);
                }
                UiCommand::SemanticsAgentAction {
                    presentation_id,
                    request,
                    issued,
                    reply,
                } => {
                    let element = request.element;
                    let reached_handler = std::cell::Cell::new(false);
                    let served = catch_unwind(AssertUnwindSafe(|| {
                        self.serve_semantics_act(
                            presentation_id,
                            &request,
                            issued,
                            &reached_handler,
                        )
                    }));
                    match served {
                        Ok(result) => {
                            if result == Err(super::AgentError::PresentationGone) {
                                report.dropped_stale += 1;
                            } else {
                                report.invoked += 1;
                            }
                            super::agent::send_reply(&reply, Some(element), result);
                        }
                        Err(payload) => {
                            // The handler, or the owner before reaching it,
                            // panicked. The reply fails first, so the agent
                            // learns of it whatever happens next; then a
                            // queued tail gets a future owner turn, since this
                            // one is unwinding; then the original panic
                            // resumes, never replaced by a later one.
                            let failure = if reached_handler.get() {
                                super::AgentError::HandlerPanicked { element }
                            } else {
                                super::AgentError::ResolvePanicked { element }
                            };
                            let answered = catch_unwind(AssertUnwindSafe(|| {
                                super::agent::send_reply(&reply, Some(element), Err(failure));
                            }))
                            .err();
                            let mut first_panic = Some(payload);
                            preserve_first_input_panic(
                                &mut first_panic,
                                answered,
                                "semantics agent reply",
                            );
                            if !self.rx.is_empty() {
                                let wake_panic = catch_unwind(AssertUnwindSafe(|| {
                                    self.sender_prototype.wake_owner();
                                }))
                                .err();
                                preserve_first_input_panic(
                                    &mut first_panic,
                                    wake_panic,
                                    "semantics agent action wake",
                                );
                            }
                            resume_unwind(first_panic.expect(
                                "BUG: the semantics agent handler panic must be preserved",
                            ));
                        }
                    }
                }
                UiCommand::Navigation(command) => match command.apply_on_owner() {
                    Ok(_) => {
                        report.invoked += 1;
                    }
                    Err(error) => {
                        tracing::trace!(
                            ?error,
                            "dropping navigation command that no longer reaches its owner"
                        );
                        report.dropped_stale += 1;
                    }
                },
                UiCommand::SignalWrite { target, mut apply } => {
                    let Some((presentation, reactive)) = self.signal_graph_for(target) else {
                        let diagnosed = catch_unwind(AssertUnwindSafe(|| {
                            tracing::warn!(
                                target: "flui::signals",
                                graph = target.graph(),
                                slot = ?target,
                                "dropping a cross-thread signal write: no presentation of this \
                                 ui_runtime owns the slot's graph (its presentation closed, or the \
                                 handle belongs to another ui_runtime)"
                            );
                        }));
                        report.dropped_stale += 1;
                        if let Err(payload) = diagnosed {
                            // Diagnostics failed before the opaque envelope
                            // could be retired. Retain it so aggregate drop
                            // glue cannot replace that panic.
                            std::mem::forget(apply);
                            self.rearm_after_command_panic(payload);
                        }

                        // The stale envelope can contain several hostile
                        // captured destructors. Ensure an already-queued FIFO
                        // tail has a future owner turn before its opaque drop
                        // glue runs; two field panics cannot be contained by
                        // an outer catch in Rust.
                        if !self.rx.is_empty()
                            && let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                                self.sender_prototype.wake_owner();
                            }))
                        {
                            std::mem::forget(apply);
                            self.rearm_after_command_panic(payload);
                        }
                        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(apply))) {
                            // A producer can enqueue after the empty-inbox
                            // snapshot while opaque destruction is blocked.
                            // If that ingress's wake failed, pay its shared
                            // debt before resuming the destructor panic.
                            let wake_panic = catch_unwind(AssertUnwindSafe(|| {
                                self.sender_prototype.retry_wake_debt();
                            }))
                            .err();
                            let mut first_panic = Some(payload);
                            preserve_first_input_panic(
                                &mut first_panic,
                                wake_panic,
                                "stale signal-write teardown wake",
                            );
                            resume_unwind(first_panic.expect(
                                "BUG: the stale command destructor panic must be preserved",
                            ));
                        }
                        continue;
                    };
                    let outcome = catch_unwind(AssertUnwindSafe(|| apply(&reactive)));
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
                    let command_panic = match outcome {
                        Err(payload) => {
                            // Once the callback has panicked, destroying an
                            // opaque aggregate capture bundle can abort inside
                            // generated drop glue before any outer catch
                            // regains control.
                            std::mem::forget(apply);
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
        // A prior rearm wake may have panicked after leaving queue/redraw
        // demand durable. This completed owner boundary has made progress;
        // retry the delivery now so any work enqueued during this turn still
        // receives a future opportunity. A second wake panic deliberately
        // escapes with the debt still armed for a later ingress/boundary.
        self.sender_prototype.retry_wake_debt();
        report
    }
}
