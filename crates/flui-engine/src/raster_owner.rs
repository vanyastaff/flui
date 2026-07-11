//! `RasterOwner` — the raster mailbox + outcome-channel boundary (ADR-0027 §4/§5/§7).
//!
//! Compositing hands one owned [`SceneSnapshot`] per presentation frame to a
//! raster owner at exactly one seam: the **raster mailbox**. The mailbox is
//! a latest-frame-wins *slot*, not a queue — a pending, un-started frame is
//! replaced (never enqueued behind) a newer submit, and the replaced frame
//! is acknowledged [`FrameDropReason::Superseded`] immediately, synchronously,
//! at submit time.
//!
//! Two structurally separate channels carry outcomes off this module, never
//! the owner's general inbox (ADR-0027 §5: riding the inbox would deadlock
//! shutdown, since the inbox flips to drain-and-refuse before an outcome is
//! ready to send):
//!
//! - A **lossy telemetry ack** channel ([`RasterAck`]: presented, dropped,
//!   surface-outdated, device-lost) — useful for diagnostics and
//!   surface-generation feedback, never load-bearing for correctness. A full
//!   channel drops the newest ack and logs it rather than ever blocking the
//!   pump loop.
//! - A **load-bearing one-shot shutdown-completion** channel, returned
//!   separately from [`RasterOwner::new`]. Kept structurally apart from the
//!   telemetry channel so no volume of lossy acks (e.g. a frame superseded a
//!   thousand times) can ever block or displace the one signal a consumer
//!   actually needs to observe reliably (ADR-0027 §7). Nothing in this
//!   module ever performs a blocking channel send — the shutdown-completion
//!   signal is `try_send`, same as every ack.
//!
//! [`RasterOwner`] is generic over [`RasterBackend`] (defined in
//! [`crate::raster`], unchanged by this module) so the mailbox/channel
//! protocol is identical for the synchronous in-process baseline this ADR
//! ships and the threaded raster owner ADR-0028 reserves. [`RasterOwner::pump`]
//! is the baseline's per-frame call; [`RasterOwner::run_until_shutdown`] is
//! the blocking loop a dedicated raster thread (or this module's own
//! threaded test harness) drives instead.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_foundation::{FrameEpoch, SurfaceGeneration};
use flui_layer::SceneSnapshot;
use parking_lot::{Condvar, Mutex};

use crate::error::EngineError;
use crate::raster::RasterBackend;

/// Telemetry ack channel capacity.
///
/// This channel carries lossy, coalesced frame telemetry only
/// (presented/dropped/surface-outdated/device-lost) — useful for
/// diagnostics and surface-generation feedback, never load-bearing for
/// correctness. The shutdown-completion signal has its own dedicated
/// one-shot channel ([`SHUTDOWN_COMPLETE_CHANNEL_CAPACITY`]) precisely so
/// telemetry volume can never block or displace it. Sized comfortably above
/// the mailbox's structural bound (at most one pending frame in flight at a
/// time) so this channel does not fill under ordinary operation, where the
/// consumer drains after every [`RasterOwner::pump`]; a full channel drops
/// the newest ack and logs it rather than ever blocking the pump loop.
const ACK_CHANNEL_CAPACITY: usize = 16;

/// The shutdown-completion one-shot channel's capacity — always exactly 1:
/// the signal fires at most once per owner (ADR-0027 §7), and giving it a
/// dedicated channel (rather than folding it into the lossy telemetry ack
/// channel) is the point of this design — an unbounded flood of telemetry
/// acks can never block or displace it.
const SHUTDOWN_COMPLETE_CHANNEL_CAPACITY: usize = 1;

// ---------------------------------------------------------------------------
// Mailbox (private)
// ---------------------------------------------------------------------------

/// The latest-frame-wins slot plus coalesced resize plus shutdown flag,
/// guarded by one lock. Never `pub` (SP-6: no lock types in public API) —
/// [`RasterHandle`] and [`RasterOwner`] are the only ways in.
#[derive(Debug, Default)]
struct MailboxState {
    /// The one un-started frame waiting for [`RasterOwner::pump`]. A newer
    /// [`RasterHandle::submit`] replaces (never queues behind) this.
    pending_frame: Option<SceneSnapshot>,
    /// The most recent coalesced resize request, applied before the next
    /// frame render.
    pending_resize: Option<(u32, u32)>,
    /// Set by [`RasterHandle::shutdown`]; refuses further submits and tells
    /// the next pump with an empty mailbox to signal shutdown-complete
    /// (ADR-0027 §7) and stop.
    shutting_down: bool,
}

/// Shared state behind [`RasterHandle`] and [`RasterOwner`]: the mailbox
/// slot, its condvar (wakes a blocked [`RasterOwner::run_until_shutdown`]),
/// an owner-liveness flag (drop-flipped, distinguishes
/// [`RasterSubmitError::OwnerGone`] from [`RasterSubmitError::ShuttingDown`]),
/// the lossy telemetry ack sender, and the load-bearing one-shot
/// shutdown-completion sender — kept on a channel structurally separate
/// from the ack channel (ADR-0027 §7) so no volume of telemetry can ever
/// block or displace the completion signal.
struct RasterMailbox {
    state: Mutex<MailboxState>,
    condvar: Condvar,
    /// Flipped to `false` when the owning [`RasterOwner`] drops, including a
    /// drop that never went through [`RasterHandle::shutdown`]. Distinct
    /// from `shutting_down`: a shutdown owner is still alive and draining;
    /// a dropped owner never will be again.
    owner_alive: AtomicBool,
    ack_tx: Sender<RasterAck>,
    /// Fires exactly once, from [`RasterOwner::pump`], when the mailbox is
    /// observed empty and shutting down. `try_send`-only, same as
    /// [`Self::send_ack`] — no send in this module ever blocks.
    shutdown_complete_tx: Sender<()>,
}

impl RasterMailbox {
    /// Sends `ack` to whoever drains the telemetry [`Receiver`] returned by
    /// [`RasterOwner::new`].
    ///
    /// Uniformly non-blocking `try_send`: this channel is lossy, coalesced
    /// telemetry, never load-bearing for correctness. A full channel drops
    /// the newest ack and logs it via `tracing::warn!` rather than ever
    /// blocking the pump loop. The one signal that *is* load-bearing —
    /// shutdown completion — does not ride this channel at all; see
    /// [`Self::shutdown_complete_tx`].
    fn send_ack(&self, ack: RasterAck) {
        if let Err(TrySendError::Full(dropped)) = self.ack_tx.try_send(ack) {
            tracing::warn!(?dropped, "raster ack channel full; dropping ack");
        }
    }
}

// ---------------------------------------------------------------------------
// Acks
// ---------------------------------------------------------------------------

/// One outcome of a submitted [`SceneSnapshot`], delivered on the lossy
/// telemetry channel returned by [`RasterOwner::new`] (ADR-0027 §5). Never
/// load-bearing for correctness — see the module docs for why shutdown
/// completion rides a separate, guaranteed channel instead of a variant
/// here.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterAck {
    /// The frame with this epoch rendered and presented.
    Presented {
        /// The presented frame's epoch.
        epoch: FrameEpoch,
    },
    /// The frame with this epoch was dropped without presenting.
    Dropped {
        /// The dropped frame's epoch.
        epoch: FrameEpoch,
        /// Why it was dropped.
        reason: FrameDropReason,
    },
    /// A frame was dropped because its surface generation no longer matches
    /// the currently-configured surface — either rejected proactively
    /// before ever reaching the backend (the frame's stamped generation is
    /// older than the owner's current one, ADR-0027 §6) or reported by the
    /// backend itself during render.
    SurfaceOutdated {
        /// The rejected frame's epoch.
        epoch: FrameEpoch,
        /// The generation the rejected frame was stamped with.
        stale: SurfaceGeneration,
        /// The owner's current surface generation — the value the consumer
        /// should stamp its next `SceneSnapshot` with. The consumer
        /// reconfigures against it and marks a full repaint (ADR-0027 §5).
        current: SurfaceGeneration,
    },
    /// The GPU device was lost while rendering this frame.
    ///
    /// This is the only ack for a device-loss condition — the frame did not
    /// present, but that fact is implied by `DeviceLost` itself; a separate
    /// [`RasterAck::Dropped`] for the same frame would double-report one
    /// condition (ADR-0027 §5: one ack per condition). Recovery is the
    /// consumer's job, off-thread, never inline under a lock.
    DeviceLost {
        /// The frame that was being rendered when the device was lost.
        epoch: FrameEpoch,
    },
}

/// Why a [`RasterAck::Dropped`] frame never presented.
///
/// `#[non_exhaustive]`: ADR-0028's threaded raster owner can abandon a
/// still-pending frame mid-shutdown instead of finishing it — something
/// this module's synchronous baseline never does (see [`RasterOwner::pump`],
/// which always finishes a pending frame through the ordinary render path
/// before it can observe shutdown with an empty mailbox). That reason joins
/// this enum additively when that owner lands, instead of being reserved
/// here unconstructed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDropReason {
    /// A newer [`RasterHandle::submit`] replaced this frame in the mailbox
    /// before [`RasterOwner::pump`] ever started it — the mailbox is
    /// latest-frame-wins, not a queue (ADR-0027 §4).
    Superseded,
    /// [`RasterBackend::render_scene`] returned an error that is neither
    /// device loss nor a surface-outdated condition (those get their own
    /// acks — [`RasterAck::DeviceLost`] / [`RasterAck::SurfaceOutdated`]).
    RenderFailed,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by [`RasterHandle::submit`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RasterSubmitError {
    /// [`RasterHandle::shutdown`] has already been called; no further
    /// frames are accepted. Retrying cannot succeed — the mailbox never
    /// leaves the shutting-down state.
    #[error("raster owner is shutting down; frame submit refused")]
    ShuttingDown,
    /// The owning [`RasterOwner`] has been dropped; this handle is now
    /// permanently inert.
    #[error("raster owner dropped; frame submit refused")]
    OwnerGone,
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Cross-thread capability into a [`RasterOwner`]'s mailbox.
///
/// `Clone + Send + Sync`. A handle can submit a frame, coalesce a resize, or
/// request shutdown; it never obtains a reference into the owned backend —
/// the same enqueue-and-wake shape as [`crate`]'s sibling handles
/// (`PipelineOwnerHandle`, `WindowCommandSender`).
#[derive(Clone)]
pub struct RasterHandle {
    mailbox: Arc<RasterMailbox>,
}

impl fmt::Debug for RasterHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.mailbox.state.lock();
        f.debug_struct("RasterHandle")
            .field("frame_pending", &state.pending_frame.is_some())
            .field("resize_pending", &state.pending_resize.is_some())
            .field("shutting_down", &state.shutting_down)
            .finish_non_exhaustive()
    }
}

impl RasterHandle {
    /// Submits `frame` to the mailbox, replacing (and immediately
    /// acknowledging as [`FrameDropReason::Superseded`]) any un-started
    /// frame already waiting there.
    ///
    /// The mailbox is structurally capacity-1: a submit can never observe
    /// the queue-is-full backpressure a bounded channel would — the newest
    /// frame always wins, and the frame it replaces is acked, never
    /// silently discarded (ADR-0027 §4).
    ///
    /// # Errors
    ///
    /// [`RasterSubmitError::ShuttingDown`] once [`Self::shutdown`] has been
    /// called; [`RasterSubmitError::OwnerGone`] once the owning
    /// [`RasterOwner`] has dropped.
    pub fn submit(&self, frame: SceneSnapshot) -> Result<(), RasterSubmitError> {
        {
            let mut state = self.mailbox.state.lock();
            if state.shutting_down {
                return Err(RasterSubmitError::ShuttingDown);
            }
            if !self.mailbox.owner_alive.load(Ordering::Acquire) {
                return Err(RasterSubmitError::OwnerGone);
            }
            if let Some(superseded) = state.pending_frame.replace(frame) {
                tracing::trace!(
                    epoch = ?superseded.epoch,
                    "raster mailbox: pending frame superseded by a newer submit"
                );
                // Sent while `state` is still held: the owner cannot
                // observe the just-inserted replacement frame until it
                // acquires this same lock, so this ack is guaranteed to
                // land on the channel before any ack the owner sends for
                // the frame that replaced it. Sending it *after* unlocking
                // (the original shape) left a window where a fast-scheduled
                // owner could take, render, and ack `Presented` for the
                // replacement before this thread got around to sending the
                // `Superseded` ack for the frame it replaced — this closes
                // that window structurally. Safe to do under the lock:
                // `send_ack`'s `try_send` never blocks (the one ack that
                // does block, `ShutdownComplete`, is never sent from here).
                self.mailbox.send_ack(RasterAck::Dropped {
                    epoch: superseded.epoch,
                    reason: FrameDropReason::Superseded,
                });
            }
        }
        self.mailbox.condvar.notify_one();
        Ok(())
    }

    /// Coalesces a resize request into the mailbox: any number of pending
    /// requests collapse into the most recent one, applied on the owner's
    /// next [`RasterOwner::pump`] before that pump renders a pending frame
    /// (ADR-0027 §4).
    ///
    /// Infallible and best-effort: a resize against a shut-down or dropped
    /// owner is a harmless no-op — there is nothing left to apply it to.
    pub fn resize(&self, width: u32, height: u32) {
        let mut state = self.mailbox.state.lock();
        state.pending_resize = Some((width, height));
        drop(state);
        self.mailbox.condvar.notify_one();
    }

    /// Begins shutdown: refuses further [`Self::submit`]s, wakes a blocked
    /// [`RasterOwner::run_until_shutdown`], and lets it finish (or drop) any
    /// already-pending frame before it signals completion on the dedicated
    /// one-shot channel [`RasterOwner::new`] returns (ADR-0027 §7).
    ///
    /// Idempotent — calling it more than once has the same effect as
    /// calling it once.
    pub fn shutdown(&self) {
        let mut state = self.mailbox.state.lock();
        state.shutting_down = true;
        drop(state);
        self.mailbox.condvar.notify_all();
    }
}

// ---------------------------------------------------------------------------
// Owner
// ---------------------------------------------------------------------------

/// What one [`RasterOwner::pump`] pass did.
#[non_exhaustive]
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpOutcome {
    /// No frame was pending. A coalesced resize may still have been applied
    /// to the backend.
    Idle,
    /// The pending frame rendered and presented.
    Presented(FrameEpoch),
    /// The pending frame was dropped without presenting.
    Dropped {
        /// The dropped frame's epoch.
        epoch: FrameEpoch,
        /// Why it was dropped.
        reason: FrameDropReason,
    },
    /// The pending frame's surface generation no longer matched the
    /// owner's current one; the frame was dropped without rendering.
    /// Mirrors [`RasterAck::SurfaceOutdated`] — see its field docs.
    SurfaceOutdated {
        /// The rejected frame's epoch.
        epoch: FrameEpoch,
        /// The generation the rejected frame was stamped with.
        stale: SurfaceGeneration,
        /// The owner's current surface generation.
        current: SurfaceGeneration,
    },
    /// The GPU device was lost while rendering this frame.
    DeviceLost(FrameEpoch),
    /// The mailbox was empty and shutdown had been requested: the
    /// shutdown-completion one-shot channel ([`RasterOwner::new`]) was
    /// signaled and no further pumps are expected to observe work.
    ShutdownComplete,
}

/// The raster owner: solely owns a [`RasterBackend`] and drains the mailbox
/// (ADR-0027 §5). Never `Sync` — `Sync`-ness is inherited from `B`, and this
/// type adds no `unsafe impl` of its own; a backend that is itself `!Sync`
/// (the wgpu `Renderer`) keeps `RasterOwner<Renderer>` `!Sync` for free.
///
/// No `Debug` impl: `B` (e.g. the wgpu `Renderer`) is not required to
/// implement it either — this crate already suppresses
/// `missing_debug_implementations` crate-wide for the same reason (wgpu
/// resource handles do not implement `Debug`).
pub struct RasterOwner<B: RasterBackend> {
    backend: B,
    mailbox: Arc<RasterMailbox>,
    /// The surface generation this owner currently considers valid
    /// (ADR-0027 §6). Bumped whenever this owner applies a resize or the
    /// backend itself reports the surface as outdated — both are
    /// surface-(re)configure events. A pending frame stamped with any other
    /// generation is rejected before [`RasterBackend::render_scene`] is
    /// ever called, so a torn-down swapchain is never presented into.
    current_surface_generation: SurfaceGeneration,
    /// Latches once the shutdown-completion signal has been sent, so a
    /// later [`Self::pump`] call on an already-shut-down, empty mailbox
    /// reports [`PumpOutcome::ShutdownComplete`] again without re-sending
    /// on the one-shot channel (ADR-0027 §7: it fires exactly once).
    has_signaled_shutdown_complete: bool,
}

impl<B: RasterBackend> RasterOwner<B> {
    /// Builds the owner alongside its [`RasterHandle`] and two outcome
    /// channels (ADR-0027 §5/§7):
    ///
    /// - A lossy telemetry ack [`Receiver`] — draining it is optional
    ///   (diagnostics / surface-generation feedback), never required for
    ///   correctness.
    /// - A one-shot shutdown-completion [`Receiver`] — fires exactly once:
    ///   the load-bearing signal that [`Self::run_until_shutdown`] has
    ///   finished (or, for the synchronous baseline, that the caller should
    ///   stop pumping). Kept on its own channel, structurally separate from
    ///   the telemetry channel, so no volume of telemetry can ever block or
    ///   displace it.
    ///
    /// Neither channel rides the mailbox or any other inbox.
    #[must_use]
    pub fn new(backend: B) -> (Self, RasterHandle, Receiver<RasterAck>, Receiver<()>) {
        let (ack_tx, ack_rx) = bounded(ACK_CHANNEL_CAPACITY);
        let (shutdown_complete_tx, shutdown_complete_rx) =
            bounded(SHUTDOWN_COMPLETE_CHANNEL_CAPACITY);
        let mailbox = Arc::new(RasterMailbox {
            state: Mutex::new(MailboxState::default()),
            condvar: Condvar::new(),
            owner_alive: AtomicBool::new(true),
            ack_tx,
            shutdown_complete_tx,
        });
        let owner = Self {
            backend,
            mailbox: Arc::clone(&mailbox),
            current_surface_generation: SurfaceGeneration::ZERO,
            has_signaled_shutdown_complete: false,
        };
        (
            owner,
            RasterHandle { mailbox },
            ack_rx,
            shutdown_complete_rx,
        )
    }

    /// Scoped access to the wrapped backend, for backend-specific recovery
    /// (e.g. device-loss recreation) that [`RasterBackend`] does not itself
    /// expose. No guard escapes this call — `f` runs, its result returns,
    /// nothing else does.
    pub fn with_backend<R>(&mut self, f: impl FnOnce(&mut B) -> R) -> R {
        f(&mut self.backend)
    }

    /// One synchronous pump: applies the latest coalesced resize (if any),
    /// renders the pending frame (if any), then acks the outcome.
    ///
    /// This is the in-process baseline's per-frame call (ADR-0027 §5): the
    /// owner-thread caller invokes it once per frame; no thread is spawned
    /// and no work happens off this call.
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn pump(&mut self) -> PumpOutcome {
        let (resize, frame, shutting_down) = {
            let mut state = self.mailbox.state.lock();
            let resize = state.pending_resize.take();
            let frame = state.pending_frame.take();
            (resize, frame, state.shutting_down)
        };

        if let Some((width, height)) = resize {
            self.backend.resize(width, height);
            // A resize reconfigures the surface — bump the generation this
            // owner considers valid (ADR-0027 §6) so a frame stamped
            // against the pre-resize surface is rejected below rather than
            // rendered into a torn-down swapchain.
            self.current_surface_generation = self.current_surface_generation.next();
            tracing::debug!(
                surface_generation = ?self.current_surface_generation,
                "raster owner: surface reconfigured by resize"
            );
        }

        let Some(frame) = frame else {
            if shutting_down {
                if !self.has_signaled_shutdown_complete {
                    self.has_signaled_shutdown_complete = true;
                    tracing::debug!("raster owner: mailbox drained, signaling shutdown complete");
                    // `try_send`, never `send`: no send in this module ever
                    // blocks (see the module docs). A `Full` result is
                    // structurally unreachable (capacity 1, sent at most
                    // once, guarded above); `Disconnected` just means no
                    // consumer is listening — either way this is a
                    // best-effort fire-and-forget signal, logged, not
                    // escalated.
                    if let Err(error) = self.mailbox.shutdown_complete_tx.try_send(()) {
                        tracing::warn!(?error, "raster owner: failed to signal shutdown complete");
                    }
                }
                return PumpOutcome::ShutdownComplete;
            }
            return PumpOutcome::Idle;
        };

        if frame.surface_generation != self.current_surface_generation {
            let stale = frame.surface_generation;
            let current = self.current_surface_generation;
            tracing::warn!(
                epoch = ?frame.epoch,
                ?stale,
                ?current,
                "raster owner: frame stamped with a stale surface generation, \
                 rejecting before render"
            );
            self.mailbox.send_ack(RasterAck::SurfaceOutdated {
                epoch: frame.epoch,
                stale,
                current,
            });
            return PumpOutcome::SurfaceOutdated {
                epoch: frame.epoch,
                stale,
                current,
            };
        }

        // `DamageRegion::Full` is the only variant that exists today
        // (flui-layer's own doc: fine-grained damage is an additive,
        // `#[non_exhaustive]`-guarded follow-up, ADR-0027 §5), so
        // `frame.damage` is not yet inspected here — there is exactly one
        // correct action regardless of its value. Revisit this call once a
        // `Partial` variant lands and `RasterBackend` gains a
        // partial-repaint path (`mark_dirty` already exists for it).
        self.backend.mark_full_repaint();

        match self.backend.render_scene(&frame.scene) {
            Ok(()) => {
                self.mailbox
                    .send_ack(RasterAck::Presented { epoch: frame.epoch });
                PumpOutcome::Presented(frame.epoch)
            }
            Err(error) => self.handle_render_failure(frame.epoch, frame.surface_generation, error),
        }
    }

    /// Blocking loop for a dedicated raster thread (or this module's own
    /// threaded test double): parks on the mailbox's condvar until a frame,
    /// a resize, or shutdown is pending, pumps once, and repeats until a
    /// pump observes [`PumpOutcome::ShutdownComplete`] (ADR-0027 §5/§7).
    pub fn run_until_shutdown(mut self) {
        loop {
            {
                let mut state = self.mailbox.state.lock();
                while state.pending_frame.is_none()
                    && state.pending_resize.is_none()
                    && !state.shutting_down
                {
                    self.mailbox.condvar.wait(&mut state);
                }
            }
            if matches!(self.pump(), PumpOutcome::ShutdownComplete) {
                return;
            }
        }
    }

    /// Classifies a [`RasterBackend::render_scene`] failure into its ack and
    /// [`PumpOutcome`] (ADR-0027 §5): device loss and surface-outdated
    /// conditions each get their own dedicated ack; everything else is a
    /// generic [`FrameDropReason::RenderFailed`], logged at `error` level
    /// since it is the catch-all bucket an operator needs to see.
    fn handle_render_failure(
        &mut self,
        epoch: FrameEpoch,
        stale: SurfaceGeneration,
        error: EngineError,
    ) -> PumpOutcome {
        match error {
            EngineError::DeviceLost => {
                tracing::warn!(?epoch, "raster owner: GPU device lost");
                self.mailbox.send_ack(RasterAck::DeviceLost { epoch });
                PumpOutcome::DeviceLost(epoch)
            }
            EngineError::SurfaceLost | EngineError::SurfaceValidation => {
                // The backend itself detected the surface is gone — a
                // surface-(re)configure event just like a resize (ADR-0027
                // §6), so it bumps the same generation counter a resize
                // does. Every frame the consumer has already stamped
                // against the old generation (including any submitted
                // before it observes this ack) is rejected by the
                // proactive check in `pump` until the consumer catches up.
                self.current_surface_generation = self.current_surface_generation.next();
                let current = self.current_surface_generation;
                tracing::warn!(
                    ?epoch,
                    ?stale,
                    ?current,
                    "raster owner: surface outdated, dropping frame"
                );
                self.mailbox.send_ack(RasterAck::SurfaceOutdated {
                    epoch,
                    stale,
                    current,
                });
                PumpOutcome::SurfaceOutdated {
                    epoch,
                    stale,
                    current,
                }
            }
            other => {
                tracing::error!(?epoch, error = %other, "raster owner: frame render failed");
                self.mailbox.send_ack(RasterAck::Dropped {
                    epoch,
                    reason: FrameDropReason::RenderFailed,
                });
                PumpOutcome::Dropped {
                    epoch,
                    reason: FrameDropReason::RenderFailed,
                }
            }
        }
    }
}

impl<B: RasterBackend> Drop for RasterOwner<B> {
    fn drop(&mut self) {
        self.mailbox.owner_alive.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// Tests — the ADR-0027 §5 mandated threaded protocol harness
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Barrier;
    use std::thread;

    use flui_foundation::RealmId;
    use flui_layer::{CanvasLayer, DamageRegion, Layer, Scene};
    use flui_types::Size;
    use flui_types::geometry::{Pixels, Rect};

    use super::*;

    // -----------------------------------------------------------------------
    // FakeBackend — records invocations, returns pre-programmed outcomes.
    // -----------------------------------------------------------------------

    #[derive(Default)]
    struct FakeBackend {
        render_calls: usize,
        resize_calls: Vec<(u32, u32)>,
        full_repaint_calls: usize,
        planned_results: VecDeque<Result<(), EngineError>>,
        size: (u32, u32),
    }

    impl FakeBackend {
        fn with_planned(results: impl IntoIterator<Item = Result<(), EngineError>>) -> Self {
            Self {
                planned_results: results.into_iter().collect(),
                ..Self::default()
            }
        }
    }

    impl RasterBackend for FakeBackend {
        fn render_scene(&mut self, _scene: &Scene) -> Result<(), EngineError> {
            self.render_calls += 1;
            self.planned_results.pop_front().unwrap_or(Ok(()))
        }

        fn resize(&mut self, width: u32, height: u32) {
            self.resize_calls.push((width, height));
            self.size = (width, height);
        }

        fn is_device_lost(&self) -> bool {
            false
        }

        fn mark_dirty(&mut self, _rect: Rect<Pixels>) {}

        fn mark_full_repaint(&mut self) {
            self.full_repaint_calls += 1;
        }

        fn has_damage(&self) -> bool {
            true
        }

        fn size(&self) -> (u32, u32) {
            self.size
        }

        fn reconfigure_surface(&mut self) -> Result<(), EngineError> {
            Ok(())
        }
    }

    fn test_frame(epoch: FrameEpoch, surface_generation: SurfaceGeneration) -> SceneSnapshot {
        SceneSnapshot::new(
            RealmId::new(1),
            epoch,
            surface_generation,
            DamageRegion::Full,
            Scene::from_layer(Size::ZERO, Layer::from(CanvasLayer::new()), 0),
        )
    }

    // -----------------------------------------------------------------------
    // Compile-time contract assertions
    // -----------------------------------------------------------------------

    #[test]
    fn contracts_are_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send::<RasterOwner<FakeBackend>>();
        assert_send_sync::<RasterHandle>();
        assert_send::<RasterAck>();
    }

    // -----------------------------------------------------------------------
    // 1. latest-frame-wins supersedes a pending, un-started frame
    // -----------------------------------------------------------------------

    #[test]
    fn latest_frame_wins_supersedes_pending() {
        let (owner, handle, ack_rx, shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        let start = Barrier::new(2);
        let epoch1 = FrameEpoch::ZERO.next();
        let epoch2 = epoch1.next();

        thread::scope(|scope| {
            let owner_thread = scope.spawn(|| {
                start.wait();
                owner.run_until_shutdown();
            });

            handle
                .submit(test_frame(epoch1, SurfaceGeneration::ZERO))
                .expect("first submit");
            handle
                .submit(test_frame(epoch2, SurfaceGeneration::ZERO))
                .expect("second submit supersedes the first");
            start.wait();
            handle.shutdown();
            owner_thread.join().expect("owner thread must not panic");
        });

        let acks: Vec<RasterAck> = ack_rx.try_iter().collect();
        assert_eq!(
            acks,
            vec![
                RasterAck::Dropped {
                    epoch: epoch1,
                    reason: FrameDropReason::Superseded,
                },
                RasterAck::Presented { epoch: epoch2 },
            ]
        );
        assert_eq!(
            shutdown_complete_rx.try_recv(),
            Ok(()),
            "shutdown must have completed by the time the owner thread joins"
        );
    }

    // -----------------------------------------------------------------------
    // 1b. regression: the Superseded ack must never be observed after the
    //     Presented ack for the frame that superseded it, under a real race
    //     (not the Barrier-separated determinism of test 1 above).
    // -----------------------------------------------------------------------

    #[test]
    fn superseded_ack_never_observed_after_presented_ack_for_its_supersessor() {
        // Regression coverage for an ack-ordering bug: `submit` used to
        // send the `Superseded` ack for a replaced frame *after* releasing
        // the mailbox lock and notifying the owner, leaving a window where
        // a fast-scheduled owner could pump, render, and ack `Presented`
        // for the *newer* frame before the losing submit ever got around
        // to sending its own ack. `submit` now sends that ack from inside
        // the same critical section that performs the replace, so the
        // owner cannot even observe the replacement frame until that ack
        // is already on the channel — a structural guarantee, not a
        // scheduling accident.
        //
        // Unlike `latest_frame_wins_supersedes_pending` above (which parks
        // the owner on a `Barrier` until *both* submits are done — full
        // separation, so it cannot distinguish the old ordering from the
        // new one), this test starts the owner with zero synchronization
        // against the two submits, so it genuinely races them on real
        // threads. No sleeps: `join()` after `shutdown()` is the only
        // pacing, and it is required for correctness (draining the owner)
        // rather than a race workaround. Repeats the race many times to
        // give a regression a real chance of being scheduled into the
        // interesting interleaving.
        for _ in 0..200 {
            let (owner, handle, ack_rx, shutdown_complete_rx) =
                RasterOwner::new(FakeBackend::default());
            let epoch1 = FrameEpoch::ZERO.next();
            let epoch2 = epoch1.next();

            let owner_thread = thread::spawn(move || owner.run_until_shutdown());

            handle
                .submit(test_frame(epoch1, SurfaceGeneration::ZERO))
                .expect("first submit");
            handle
                .submit(test_frame(epoch2, SurfaceGeneration::ZERO))
                .expect("second submit races the owner thread");
            handle.shutdown();
            owner_thread.join().expect("owner thread must not panic");

            assert_eq!(
                shutdown_complete_rx.try_recv(),
                Ok(()),
                "shutdown must have completed by the time the owner thread joins"
            );

            let acks: Vec<RasterAck> = ack_rx.try_iter().collect();
            let superseded_index = acks.iter().position(|ack| {
                matches!(
                    ack,
                    RasterAck::Dropped {
                        epoch,
                        reason: FrameDropReason::Superseded,
                    } if *epoch == epoch1
                )
            });
            let presented_epoch2_index = acks
                .iter()
                .position(|ack| matches!(ack, RasterAck::Presented { epoch } if *epoch == epoch2));

            // Two legal outcomes depending on who won the race for epoch1
            // itself: either the owner already took and presented epoch1
            // before the second submit ever arrived (no supersede at all),
            // or the second submit won and superseded it. Only the latter
            // produces a `Superseded` ack, and when it does, that ack must
            // precede the `Presented` ack for the frame that replaced it.
            if let (Some(superseded_index), Some(presented_epoch2_index)) =
                (superseded_index, presented_epoch2_index)
            {
                assert!(
                    superseded_index < presented_epoch2_index,
                    "Superseded ack for epoch1 must precede the Presented ack \
                     for the epoch2 frame that replaced it: {acks:?}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // 2. presented acks arrive in submission order (pump-per-frame flavor)
    // -----------------------------------------------------------------------

    #[test]
    fn presented_acks_arrive_in_submission_order() {
        let (mut owner, handle, ack_rx, _shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        let epoch1 = FrameEpoch::ZERO.next();
        let epoch2 = epoch1.next();
        let epoch3 = epoch2.next();

        for epoch in [epoch1, epoch2, epoch3] {
            handle
                .submit(test_frame(epoch, SurfaceGeneration::ZERO))
                .expect("submit");
            assert_eq!(owner.pump(), PumpOutcome::Presented(epoch));
        }

        let acks: Vec<RasterAck> = ack_rx.try_iter().collect();
        assert_eq!(
            acks,
            vec![
                RasterAck::Presented { epoch: epoch1 },
                RasterAck::Presented { epoch: epoch2 },
                RasterAck::Presented { epoch: epoch3 },
            ]
        );
    }

    // -----------------------------------------------------------------------
    // 3. shutdown handshake completes; the owner thread joins; the ack is last
    // -----------------------------------------------------------------------

    #[test]
    fn shutdown_handshake_completes_and_thread_joins() {
        let (owner, handle, _ack_rx, shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        let start = Barrier::new(2);

        thread::scope(|scope| {
            let owner_thread = scope.spawn(|| {
                start.wait();
                owner.run_until_shutdown();
            });
            start.wait();
            handle.shutdown();
            // The dedicated one-shot channel is the load-bearing completion
            // signal (ADR-0027 §7) — block on it directly, the way a real
            // consumer would, rather than only inferring completion from
            // the thread join.
            shutdown_complete_rx
                .recv()
                .expect("shutdown-complete signal must arrive, not be dropped");
            owner_thread
                .join()
                .expect("owner thread must join without hanging or panicking");
        });
    }

    // -----------------------------------------------------------------------
    // 4. submit after shutdown / after owner drop fails typed
    // -----------------------------------------------------------------------

    #[test]
    fn submit_after_shutdown_fails_typed() {
        let (_owner, handle, _ack_rx, _shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        handle.shutdown();
        let err = handle
            .submit(test_frame(FrameEpoch::ZERO.next(), SurfaceGeneration::ZERO))
            .unwrap_err();
        assert_eq!(err, RasterSubmitError::ShuttingDown);
    }

    #[test]
    fn submit_after_owner_dropped_fails_typed() {
        let (owner, handle, _ack_rx, _shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        drop(owner);
        let err = handle
            .submit(test_frame(FrameEpoch::ZERO.next(), SurfaceGeneration::ZERO))
            .unwrap_err();
        assert_eq!(err, RasterSubmitError::OwnerGone);
    }

    // -----------------------------------------------------------------------
    // 5. render failure acks Dropped { RenderFailed }
    // -----------------------------------------------------------------------

    #[test]
    fn render_failure_acks_dropped_render_failed() {
        let backend = FakeBackend::with_planned([Err(EngineError::NotInitialized)]);
        let (mut owner, handle, ack_rx, _shutdown_complete_rx) = RasterOwner::new(backend);
        let epoch = FrameEpoch::ZERO.next();
        handle
            .submit(test_frame(epoch, SurfaceGeneration::ZERO))
            .expect("submit");

        let outcome = owner.pump();
        assert_eq!(
            outcome,
            PumpOutcome::Dropped {
                epoch,
                reason: FrameDropReason::RenderFailed,
            }
        );
        assert_eq!(
            ack_rx.try_recv().unwrap(),
            RasterAck::Dropped {
                epoch,
                reason: FrameDropReason::RenderFailed,
            }
        );
    }

    // -----------------------------------------------------------------------
    // 6. resize requests coalesce to the latest before the next render
    // -----------------------------------------------------------------------

    #[test]
    fn resize_coalesces_latest_wins() {
        let (mut owner, handle, _ack_rx, _shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        handle.resize(100, 100);
        handle.resize(200, 150);
        handle.resize(320, 240);

        // Stamped against the generation the single coalesced resize will
        // produce (ZERO -> ONE) inside this same pump — a real compositor
        // stamps a frame with the generation it observed most recently.
        let epoch = FrameEpoch::ZERO.next();
        handle
            .submit(test_frame(epoch, SurfaceGeneration::ZERO.next()))
            .expect("submit");
        let outcome = owner.pump();

        assert_eq!(outcome, PumpOutcome::Presented(epoch));
        owner.with_backend(|backend| {
            assert_eq!(
                backend.resize_calls,
                vec![(320, 240)],
                "only the last coalesced resize must reach the backend"
            );
            assert_eq!(backend.render_calls, 1, "the frame renders exactly once");
        });
    }

    // -----------------------------------------------------------------------
    // 7. device lost maps to a DeviceLost ack (no accompanying Dropped ack)
    // -----------------------------------------------------------------------

    #[test]
    fn device_lost_maps_to_device_lost_ack() {
        let backend = FakeBackend::with_planned([Err(EngineError::DeviceLost)]);
        let (mut owner, handle, ack_rx, _shutdown_complete_rx) = RasterOwner::new(backend);
        let epoch = FrameEpoch::ZERO.next();
        handle
            .submit(test_frame(epoch, SurfaceGeneration::ZERO))
            .expect("submit");

        let outcome = owner.pump();
        assert_eq!(outcome, PumpOutcome::DeviceLost(epoch));
        assert_eq!(ack_rx.try_recv().unwrap(), RasterAck::DeviceLost { epoch });
        // Exactly one ack: DeviceLost does not also emit a Dropped ack for
        // the same frame (ADR-0027 §5: one ack per condition).
        assert!(ack_rx.try_recv().is_err());
    }

    // -----------------------------------------------------------------------
    // Bonus: surface-outdated mapping + empty-mailbox idle pump
    // -----------------------------------------------------------------------

    #[test]
    fn surface_validation_error_maps_to_surface_outdated_ack() {
        let backend = FakeBackend::with_planned([Err(EngineError::SurfaceValidation)]);
        let (mut owner, handle, ack_rx, _shutdown_complete_rx) = RasterOwner::new(backend);
        let epoch = FrameEpoch::ZERO.next();
        handle
            .submit(test_frame(epoch, SurfaceGeneration::ZERO))
            .expect("submit");

        let outcome = owner.pump();
        // A backend-reported surface-outdated condition bumps the owner's
        // tracked generation the same way a resize does (ADR-0027 §6), so
        // `current` is ZERO.next() — the frame's own stamp (`stale`) stays
        // ZERO, distinct from it.
        let stale = SurfaceGeneration::ZERO;
        let current = SurfaceGeneration::ZERO.next();
        assert_eq!(
            outcome,
            PumpOutcome::SurfaceOutdated {
                epoch,
                stale,
                current,
            }
        );
        assert_eq!(
            ack_rx.try_recv().unwrap(),
            RasterAck::SurfaceOutdated {
                epoch,
                stale,
                current,
            }
        );
    }

    #[test]
    fn pump_with_empty_mailbox_returns_idle() {
        let (mut owner, _handle, _ack_rx, _shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        assert_eq!(owner.pump(), PumpOutcome::Idle);
    }

    // -----------------------------------------------------------------------
    // 8. stale surface generation is rejected before render (ADR-0027 §6)
    // -----------------------------------------------------------------------

    #[test]
    fn stale_surface_generation_is_rejected_before_render() {
        let (mut owner, handle, ack_rx, _shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        handle.resize(800, 600);
        // Stamped against the pre-resize generation (ZERO): the coalesced
        // resize applied inside the same pump bumps the owner's current
        // generation to ONE before this frame is ever considered for
        // render.
        let epoch = FrameEpoch::ZERO.next();
        handle
            .submit(test_frame(epoch, SurfaceGeneration::ZERO))
            .expect("submit");

        let outcome = owner.pump();
        let stale = SurfaceGeneration::ZERO;
        let current = SurfaceGeneration::ZERO.next();
        assert_eq!(
            outcome,
            PumpOutcome::SurfaceOutdated {
                epoch,
                stale,
                current,
            }
        );
        assert_eq!(
            ack_rx.try_recv().unwrap(),
            RasterAck::SurfaceOutdated {
                epoch,
                stale,
                current,
            }
        );
        owner.with_backend(|backend| {
            assert_eq!(
                backend.render_calls, 0,
                "a stale-generation frame must never reach render_scene"
            );
            assert_eq!(
                backend.resize_calls,
                vec![(800, 600)],
                "the resize itself still applies even though the frame is rejected"
            );
            assert_eq!(
                backend.full_repaint_calls, 0,
                "mark_full_repaint is only called on the render path, never \
                 for a proactively-rejected frame"
            );
        });
    }

    // -----------------------------------------------------------------------
    // 9. pump after shutdown-complete does not re-signal (ADR-0027 §7: the
    //    one-shot fires exactly once)
    // -----------------------------------------------------------------------

    #[test]
    fn pump_after_shutdown_complete_does_not_resignal() {
        let (mut owner, handle, ack_rx, shutdown_complete_rx) =
            RasterOwner::new(FakeBackend::default());
        handle.shutdown();
        assert_eq!(owner.pump(), PumpOutcome::ShutdownComplete);
        assert_eq!(owner.pump(), PumpOutcome::ShutdownComplete);
        assert_eq!(
            shutdown_complete_rx.try_recv(),
            Ok(()),
            "the first pump must have signaled shutdown complete"
        );
        assert!(
            shutdown_complete_rx.try_recv().is_err(),
            "a second pump on an already-shut-down, empty mailbox must not \
             re-signal shutdown complete"
        );
        assert!(
            ack_rx.try_iter().next().is_none(),
            "no telemetry ack is produced by the shutdown-complete path"
        );
    }
}
