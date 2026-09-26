//! The owner inbox: `UiCommand`, `UiCommandSender`, and draining it on the owner thread.

use super::UiRealm;
use crossbeam_channel::{Sender, TrySendError};
use flui_foundation::PresentationId;
use flui_scheduler::SchedulerPhase;
use flui_semantics::SemanticsActionRequest;
use flui_widgets::NavigatorCommand;
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
    #[error("realm command inbox full ({capacity} capacity); back off and retry")]
    ChannelFull {
        /// Configured inbox capacity.
        capacity: usize,
        /// Rejected command, returned intact so the framework can retry.
        rejected: UiCommand,
    },

    /// The owning [`UiRealm`] has been dropped; this sender is now
    /// permanently inert and the producer should stop sending.
    #[error("ui realm dropped; command sender is no longer valid")]
    OwnerGone {
        /// Rejected command, returned intact to the framework caller.
        rejected: UiCommand,
    },
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "payload recovery is exercised by the protocol tests"
    )
)]
impl CommandSendError {
    pub(super) fn into_rejected(self) -> UiCommand {
        match self {
            Self::ChannelFull { rejected, .. } | Self::OwnerGone { rejected } => rejected,
        }
    }
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
///   presentation at drain time, because the realm's inbox outlives any one
///   presentation incarnation within it (the eventual element-forest case)
///   and a sender may have been vended for a presentation that no longer
///   owns this realm's inbox.
/// - **Realm-scoped** commands (`HotReload`, [`Self::Navigation`])
///   carry no stamp of their own and stay bound by channel identity alone:
///   a recreated realm mints new channels, and a sender into the dead realm
///   already gets `OwnerGone` at send — re-stamping realm identity on top of
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
        apply: Box<dyn FnOnce(&flui_view::Reactive) + Send>,
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

/// What one [`UiRealm::drain_commands`] pass did, for observability
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

/// Cross-thread capability into a [`UiRealm`]'s inbox.
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
    /// The presentation this sender stamps onto every presentation-scoped
    /// command it sends (set once, at [`UiRealm::construct`]). For the
    /// eventual element forest, senders become vended per-presentation with
    /// different stamps into the same realm inbox — this field is already
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
            .finish_non_exhaustive()
    }
}

impl UiCommandSender {
    /// Enqueue a hot-reload request for the realm owner.
    ///
    /// Unlike direct platform dispatch, this capability is safe to call from
    /// any thread: delivery occurs at the owner's next Idle drain and the
    /// normal enqueue-and-wake contract pumps that drain.
    ///
    /// # Errors
    ///
    /// [`CommandSendError`] when the inbox is full or the realm is gone.
    #[cfg(feature = "hot-reload")]
    pub fn request_hot_reload(
        &self,
        tier: crate::reload::ReloadTier,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::HotReload(tier))
    }

    /// Enqueue an accessibility action for owner-local semantics resolution.
    ///
    /// The sender itself selects the target realm/presentation; the request
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
    /// that minted it (ADR-0085 §1), wherever in this realm that graph lives;
    /// a write it performs marks readers for the following frame. If no
    /// presentation of this realm owns that graph any more, `apply` is
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
        apply: impl FnOnce(flui_view::Signal<T>, &flui_view::Reactive) + Send + 'static,
    ) -> Result<(), CommandSendError> {
        self.send(UiCommand::SignalWrite {
            target: target.slot(),
            apply: Box::new(move |reactive| apply(target.attach(), reactive)),
        })
    }

    /// Request a redraw of the realm's presentation, coalesced: any number of pending
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
        if !self.redraw_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }

    /// The inbox's configured capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub(super) fn send(&self, command: UiCommand) -> Result<(), CommandSendError> {
        match self.tx.try_send(command) {
            Ok(()) => {
                (self.wake)();
                Ok(())
            }
            Err(TrySendError::Full(rejected)) => Err(CommandSendError::ChannelFull {
                capacity: self.capacity,
                rejected,
            }),
            Err(TrySendError::Disconnected(rejected)) => {
                Err(CommandSendError::OwnerGone { rejected })
            }
        }
    }
}

impl UiRealm {
    /// A new cross-thread sender into this runtime's inbox.
    #[must_use]
    pub fn command_sender(&self) -> UiCommandSender {
        self.sender_prototype.clone()
    }

    /// Consume the coalesced redraw request, if any.
    ///
    /// The runner merges this into its dirty gate each frame; reading clears
    /// the flag so the next request wakes again.
    #[must_use]
    pub fn take_redraw_request(&self) -> bool {
        self.redraw_pending.swap(false, Ordering::AcqRel)
    }

    /// Drain the closed command inbox on the owner thread in strict FIFO
    /// order.
    ///
    /// Call only at frame boundaries — immediately before entering
    /// `drive_frame` and/or after it returns — never inside the frame
    /// transaction. Enforced in debug builds against the transitional global
    /// scheduler's phase; the thread affinity itself is structural
    /// (`UiRealm: !Send + !Sync`), not asserted.
    pub fn drain_commands(&self) -> DrainReport {
        debug_assert_eq!(
            self.scheduler.phase(),
            SchedulerPhase::Idle,
            "UiRealm::drain_commands must run at a frame boundary (Idle), \
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
                    // Reuse the SAME fan-out `Self::apply_hot_reload` the
                    // direct `perform_hot_reload_entered` path calls --
                    // calling `self.presentations.primary().apply_hot_reload`
                    // here instead (as this arm once did) reassembles only
                    // the primary and silently skips every other
                    // presentation, exactly the class of bug
                    // `reassemble_fans_out_to_all_presentations_in_mount_
                    // order` exists to catch on the direct path; this
                    // inbox arm is the untested twin of that same call, and
                    // the desktop worker's own hot-reload trigger goes
                    // through THIS arm, not the direct one.
                    if self.apply_hot_reload(tier) {
                        self.redraw_pending.store(true, Ordering::Release);
                    }
                    report.invoked += 1;
                }
                UiCommand::SemanticsAction {
                    presentation_id,
                    request,
                } => {
                    // Forest-membership check (issue #555's addressed-routing slice), not
                    // primary-only equality: a presentation generation this
                    // realm no longer hosts -- closed since the action was
                    // stamped, or a genuinely different (non-primary)
                    // presentation among several -- is a traced drop either
                    // way, never delivered to a sibling.
                    let Some(presentation) = self.presentations.get(presentation_id) else {
                        tracing::trace!(
                            stamped_presentation_id = presentation_id.as_u64(),
                            "dropping semantics action stamped for a presentation this realm no \
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
                            // Flutter deliberately ignores actions for stale
                            // views/nodes because screen readers may lag behind
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
                UiCommand::SignalWrite { target, apply } => {
                    let Some((presentation, reactive)) = self.signal_graph_for(target) else {
                        tracing::warn!(
                            target: "flui::signals",
                            graph = target.graph(),
                            slot = ?target,
                            "dropping a cross-thread signal write: no presentation of this \
                             realm owns the slot's graph (its presentation closed, or the \
                             handle belongs to another realm)"
                        );
                        report.dropped_stale += 1;
                        continue;
                    };
                    apply(&reactive);
                    // A secondary presentation's BuildOwner has no wake hook,
                    // so the owning presentation's frame is requested here.
                    presentation.mark_redraw_pending();
                    self.redraw_pending.store(true, Ordering::Release);
                    report.invoked += 1;
                }
            }
        }
        report
    }
}
