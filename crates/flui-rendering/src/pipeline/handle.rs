//! `PipelineOwnerHandle` -- cross-thread mark-dirty channel.
//!
//! Per docs/designs/2026-05-20-mythos-flui-rendering-redesign.md Section 7
//! and Section 8. The pipeline owner is single-owner-mutable; nothing else
//! holds `&mut PipelineOwner`. But background work (async asset loader
//! completing, scheduler timer firing, semantics platform callback) still
//! needs to say "this render object is dirty". That cross-thread request
//! goes through `PipelineOwnerHandle::request_mark_dirty`, which sends a
//! [`DirtyRequest`] over a bounded [`crossbeam_channel`] to the owner.
//!
//! The owner drains the channel into its [`DirtySets`](super::DirtySets) at a defined point
//! in each frame -- typically at the start of `run_layout` (and any other
//! `run_*` phase) where the producer-side mark-dirty signals must be
//! observed before that phase's work begins.
//!
//! ## Backpressure
//!
//! The channel is bounded. Default capacity 256 (tunable at construction).
//! When the channel is full, [`PipelineOwnerHandle::request_mark_dirty`] returns
//! `Err(SendError::ChannelFull)` -- the producer has surfaced
//! backpressure and decides what to do (wait, drop the request, log,
//! escalate). Unbounded channels would hide this in heap growth; we
//! refuse them.
//!
//! When the receiving owner drops, outstanding handles receive
//! `Err(SendError::OwnerGone)` on their next `request_mark_dirty`.

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_foundation::RenderId;
use parking_lot::RwLock;

use super::notifier::VisualUpdateNotifier;

// ---------------------------------------------------------------------------
// DirtyRequest / DirtyKind
// ---------------------------------------------------------------------------

/// The four pipeline phases that mark-dirty can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirtyKind {
    /// Mark for next-frame layout.
    Layout,
    /// Mark for next-frame compositing-bits update.
    Compositing,
    /// Mark for next-frame paint.
    Paint,
    /// Mark for next-frame semantics update.
    Semantics,
}

/// A request to mark a render object dirty for one phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRequest {
    /// The render object to mark.
    pub id: RenderId,
    /// Tree depth (or 0 if unknown -- the owner re-computes during flush).
    pub depth: usize,
    /// Which phase to mark for.
    pub kind: DirtyKind,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by [`PipelineOwnerHandle::request_mark_dirty`].
///
/// Marked `#[non_exhaustive]` so new variants can be added without a
/// breaking change for downstream matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SendError {
    /// The channel is full; the producer must back off.
    #[error("dirty channel full ({capacity} capacity); back off and retry")]
    ChannelFull {
        /// Configured channel capacity.
        capacity: usize,
    },

    /// The receiving pipeline owner has been dropped; the handle is now
    /// useless. The producer should stop sending.
    #[error("pipeline owner dropped; handle is no longer valid")]
    OwnerGone,
}

impl<T> From<TrySendError<T>> for SendError {
    fn from(err: TrySendError<T>) -> Self {
        match err {
            // Default capacity from `bounded` construction is opaque here;
            // the handle remembers its capacity and substitutes that value
            // in the public error. The 0 placeholder is replaced at the
            // call site that knows its bound.
            TrySendError::Full(_) => SendError::ChannelFull { capacity: 0 },
            TrySendError::Disconnected(_) => SendError::OwnerGone,
        }
    }
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Cross-thread handle for marking render objects dirty.
///
/// `PipelineOwnerHandle` is `Send + Sync + Clone`. Each clone gets its own
/// [`Sender`] handle on the same underlying bounded channel; sends are
/// independent. The receiver is held only by the `PipelineOwner` and
/// dropped when the owner drops -- at which point all outstanding handles
/// return [`SendError::OwnerGone`] on their next call.
#[derive(Clone)]
pub struct PipelineOwnerHandle {
    tx: Sender<DirtyRequest>,
    capacity: usize,
    /// Shared with the owner: a successful enqueue FIRES the
    /// visual-update wake, so a request landed while the event loop
    /// idles still produces the frame that observes it. Enqueue-only
    /// (the pre-wake shape) was the "GIF frozen until you scroll" bug:
    /// the channel filled and nothing ever drained it.
    notifier: Arc<RwLock<VisualUpdateNotifier>>,
}

impl std::fmt::Debug for PipelineOwnerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwnerHandle")
            .field("capacity", &self.capacity)
            .field("pending", &self.tx.len())
            .finish_non_exhaustive()
    }
}

impl PipelineOwnerHandle {
    /// Constructs the handle + receiver pair. Internal use only;
    /// `PipelineOwner::new` wires this up.
    pub(super) fn new_pair(
        capacity: usize,
        notifier: Arc<RwLock<VisualUpdateNotifier>>,
    ) -> (Self, Receiver<DirtyRequest>) {
        let (tx, rx) = bounded(capacity);
        (
            Self {
                tx,
                capacity,
                notifier,
            },
            rx,
        )
    }

    /// The channel's configured capacity.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of pending requests on the channel right now (snapshot).
    ///
    /// Pure observability accessor; the value changes between this call
    /// and any subsequent `request_mark_dirty`.
    #[inline]
    pub fn pending(&self) -> usize {
        self.tx.len()
    }

    /// Requests that the given render object be marked dirty for the
    /// given phase on the next frame.
    ///
    /// Non-blocking. Returns [`SendError::ChannelFull`] if the channel is
    /// at capacity (the producer must back off and retry) or
    /// [`SendError::OwnerGone`] if the owner has dropped.
    pub fn request_mark_dirty(
        &self,
        id: RenderId,
        depth: usize,
        kind: DirtyKind,
    ) -> Result<(), SendError> {
        let req = DirtyRequest { id, depth, kind };
        match self.tx.try_send(req) {
            Ok(()) => {
                // Wake the platform: an idle event loop must produce the
                // frame whose `drain_pending_dirty` observes this request
                // (same contract as a local dirty mark firing the
                // notifier). Idempotent — a pending frame absorbs
                // repeated wakes.
                self.notifier.read().fire_need_visual_update();
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(SendError::ChannelFull {
                capacity: self.capacity,
            }),
            Err(TrySendError::Disconnected(_)) => Err(SendError::OwnerGone),
        }
    }
}

// ---------------------------------------------------------------------------
// RepaintHandle
// ---------------------------------------------------------------------------

/// A node-bound repaint capability for background producers.
///
/// Hand one to async work whose completion changes a SPECIFIC render
/// object's pixels — a finished image decode, an arriving network
/// asset, a video frame. Calling [`mark_needs_paint`](Self::mark_needs_paint)
/// from any thread enqueues a paint request AND wakes the platform; the
/// owner replays it through the standard paint mark on the next frame.
///
/// The captured [`RenderId`] is generational: when the node is removed,
/// the id's generation dies with it and the owner's drain drops the
/// request silently — a stale handle is a no-op, never a repaint of an
/// unrelated reused slot. No explicit revocation call exists or is
/// needed.
#[derive(Debug, Clone)]
pub struct RepaintHandle {
    handle: PipelineOwnerHandle,
    id: RenderId,
    /// Depth snapshot at creation; advisory only (the owner re-reads
    /// the live node's depth on drain).
    depth: usize,
}

impl RepaintHandle {
    /// Binds a pipeline handle to one render object. Internal;
    /// `PipelineOwner::repaint_handle` is the public constructor.
    pub(super) fn new(handle: PipelineOwnerHandle, id: RenderId, depth: usize) -> Self {
        Self { handle, id, depth }
    }

    /// The render object this handle repaints.
    #[must_use]
    pub fn id(&self) -> RenderId {
        self.id
    }

    /// Requests a repaint of the bound node on the next frame and wakes
    /// the platform. Callable from any thread.
    ///
    /// # Errors
    ///
    /// [`SendError::ChannelFull`] under backpressure (back off and
    /// retry), [`SendError::OwnerGone`] once the pipeline owner is
    /// dropped.
    pub fn mark_needs_paint(&self) -> Result<(), SendError> {
        self.handle
            .request_mark_dirty(self.id, self.depth, DirtyKind::Paint)
    }

    /// Requests a re-layout of the bound node on the next frame and wakes
    /// the platform. Callable from any thread.
    ///
    /// This is the verb an object that drives its own layout out-of-band
    /// (e.g. an owned `AnimationController` ticking a size animation)
    /// calls from a `Listenable` notification received during
    /// [`RenderObject::attach`](crate::traits::RenderObject::attach).
    ///
    /// # Errors
    ///
    /// [`SendError::ChannelFull`] under backpressure (back off and
    /// retry), [`SendError::OwnerGone`] once the pipeline owner is
    /// dropped.
    pub fn mark_needs_layout(&self) -> Result<(), SendError> {
        self.handle
            .request_mark_dirty(self.id, self.depth, DirtyKind::Layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: usize) -> RenderId {
        RenderId::new(n)
    }

    fn pair(capacity: usize) -> (PipelineOwnerHandle, Receiver<DirtyRequest>) {
        PipelineOwnerHandle::new_pair(capacity, Arc::new(RwLock::new(VisualUpdateNotifier::new())))
    }

    #[test]
    fn handle_send_recv_round_trip() {
        let (handle, rx) = pair(4);
        assert_eq!(handle.capacity(), 4);
        handle
            .request_mark_dirty(id(1), 2, DirtyKind::Layout)
            .expect("first send must succeed");
        let req = rx.try_recv().expect("receiver should observe the request");
        assert_eq!(req.id, id(1));
        assert_eq!(req.depth, 2);
        assert_eq!(req.kind, DirtyKind::Layout);
    }

    #[test]
    fn repaint_handle_mark_needs_layout_round_trips_as_layout_kind() {
        let (pipeline_handle, rx) = pair(4);
        let repaint_handle = RepaintHandle::new(pipeline_handle, id(7), 3);

        repaint_handle
            .mark_needs_layout()
            .expect("first send must succeed");

        let req = rx.try_recv().expect("receiver should observe the request");
        assert_eq!(req.id, id(7));
        assert_eq!(req.depth, 3);
        assert_eq!(req.kind, DirtyKind::Layout);
    }

    #[test]
    fn handle_returns_channel_full_at_capacity() {
        let (handle, _rx) = pair(2);
        handle
            .request_mark_dirty(id(1), 0, DirtyKind::Paint)
            .unwrap();
        handle
            .request_mark_dirty(id(2), 0, DirtyKind::Paint)
            .unwrap();
        let err = handle
            .request_mark_dirty(id(3), 0, DirtyKind::Paint)
            .unwrap_err();
        assert_eq!(err, SendError::ChannelFull { capacity: 2 });
    }

    #[test]
    fn handle_returns_owner_gone_after_receiver_drop() {
        let (handle, rx) = pair(4);
        drop(rx);
        let err = handle
            .request_mark_dirty(id(1), 0, DirtyKind::Layout)
            .unwrap_err();
        assert_eq!(err, SendError::OwnerGone);
    }

    #[test]
    fn handle_is_clone_and_each_clone_sends_independently() {
        let (handle_a, rx) = pair(4);
        let handle_b = handle_a.clone();
        handle_a
            .request_mark_dirty(id(1), 0, DirtyKind::Layout)
            .unwrap();
        handle_b
            .request_mark_dirty(id(2), 0, DirtyKind::Paint)
            .unwrap();
        assert_eq!(rx.len(), 2);
    }

    #[test]
    fn handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PipelineOwnerHandle>();
    }
}
