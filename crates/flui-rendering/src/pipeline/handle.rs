//! Node-bound cross-thread invalidation.
//!
//! Per docs/designs/2026-05-20-mythos-flui-rendering-redesign.md Section 7
//! and Section 8. The pipeline owner is single-owner-mutable; nothing else
//! holds `&mut PipelineOwner`. But background work (async asset loader
//! completing, scheduler timer firing, semantics platform callback) still
//! needs to say "this render object is dirty". That request crosses the
//! owner boundary only through [`RenderInvalidationHandle`], which is stamped for one
//! exact render-node attachment interval.
//!
//! The owner drains the channel into its [`DirtySets`](super::DirtySets) at a defined point
//! in each frame -- typically at the start of `run_layout` (and any other
//! `run_*` phase) where the producer-side mark-dirty signals must be
//! observed before that phase's work begins.
//!
//! ## Backpressure
//!
//! The channel is bounded. Default capacity 256 (tunable at construction).
//! When the channel is full, a [`RenderInvalidationHandle`] method returns
//! `Err(SendError::ChannelFull)` -- the producer has surfaced
//! backpressure and decides what to do (wait, drop the request, log,
//! escalate). Unbounded channels would hide this in heap growth; we
//! refuse them.
//!
//! When the receiving owner drops, outstanding handles receive
//! `Err(SendError::OwnerGone)` on their next `request_mark_dirty`.

use std::{num::NonZeroU64, sync::Arc};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_foundation::RenderId;
use parking_lot::RwLock;

use super::notifier::VisualUpdateNotifier;

// ---------------------------------------------------------------------------
// Private channel protocol
// ---------------------------------------------------------------------------

/// The four pipeline phases that mark-dirty can target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum DirtyKind {
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
pub(super) struct DirtyRequest {
    pub(super) id: RenderId,
    pub(super) attachment_epoch: AttachmentEpoch,
    pub(super) kind: DirtyKind,
}

/// Identity of one attached interval for a stable render-tree slot.
///
/// Private by design: only `PipelineOwner` may mint epochs, and only a
/// node-bound [`RenderInvalidationHandle`] may carry one across threads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AttachmentEpoch(NonZeroU64);

impl AttachmentEpoch {
    pub(crate) const FIRST: Self = Self(NonZeroU64::MIN);

    pub(crate) fn next(self) -> Self {
        Self(
            NonZeroU64::new(
                self.0.get().checked_add(1).expect(
                    "BUG: a render node cannot exhaust every non-zero u64 attachment epoch",
                ),
            )
            .expect("BUG: incrementing a non-zero attachment epoch must remain non-zero"),
        )
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by node-bound cross-thread invalidation.
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
#[derive(Clone)]
pub(super) struct DirtySender {
    tx: Sender<DirtyRequest>,
    capacity: usize,
    /// Shared with the owner: a successful enqueue FIRES the
    /// visual-update wake, so a request landed while the event loop
    /// idles still produces the frame that observes it. Enqueue-only
    /// (the pre-wake shape) was the "GIF frozen until you scroll" bug:
    /// the channel filled and nothing ever drained it.
    notifier: Arc<RwLock<VisualUpdateNotifier>>,
}

impl std::fmt::Debug for DirtySender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirtySender")
            .field("capacity", &self.capacity)
            .field("pending", &self.tx.len())
            .finish_non_exhaustive()
    }
}

impl DirtySender {
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

    pub(super) fn request_mark_dirty(
        &self,
        id: RenderId,
        attachment_epoch: AttachmentEpoch,
        kind: DirtyKind,
    ) -> Result<(), SendError> {
        let request = DirtyRequest {
            id,
            attachment_epoch,
            kind,
        };
        match self.tx.try_send(request) {
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
// RenderInvalidationHandle
// ---------------------------------------------------------------------------

/// A node-bound, attachment-interval-scoped render invalidation capability.
///
/// Its four verbs enqueue layout, compositing-bits, paint, or semantics work
/// for one specific render node and wake the platform. The owner replays each
/// accepted request through the corresponding canonical dirty-mark path.
///
/// Both the [`RenderId`] generation and private attachment epoch must match at
/// drain time. Removal, detach, or same-id detach/reattach therefore makes an
/// old handle inert; it can never invalidate an unrelated reused slot or a new
/// attachment interval.
#[derive(Debug, Clone)]
pub struct RenderInvalidationHandle {
    sender: DirtySender,
    id: RenderId,
    attachment_epoch: AttachmentEpoch,
}

impl RenderInvalidationHandle {
    /// Binds a pipeline handle to one render object. Internal;
    /// `PipelineOwner::render_invalidation_handle` is the public constructor.
    pub(super) fn new(
        sender: DirtySender,
        id: RenderId,
        attachment_epoch: AttachmentEpoch,
    ) -> Self {
        Self {
            sender,
            id,
            attachment_epoch,
        }
    }

    /// The render object this handle may invalidate during its attachment interval.
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
        self.sender
            .request_mark_dirty(self.id, self.attachment_epoch, DirtyKind::Paint)
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
        self.sender
            .request_mark_dirty(self.id, self.attachment_epoch, DirtyKind::Layout)
    }

    /// Requests a compositing-bits update of the bound node on the next
    /// frame and wakes the platform. Callable from any thread.
    ///
    /// This is the verb an object whose compositing requirement depends on
    /// out-of-band state (e.g. an animated alpha crossing the
    /// layered/unlayered threshold) calls when that threshold crosses, so
    /// the owner's compositing-bits walk revisits this node on the next
    /// frame.
    ///
    /// # Errors
    ///
    /// [`SendError::ChannelFull`] under backpressure (back off and
    /// retry), [`SendError::OwnerGone`] once the pipeline owner is
    /// dropped.
    pub fn mark_needs_compositing_bits_update(&self) -> Result<(), SendError> {
        self.sender
            .request_mark_dirty(self.id, self.attachment_epoch, DirtyKind::Compositing)
    }

    /// Requests a semantics update of the bound node on the next frame.
    pub fn mark_needs_semantics(&self) -> Result<(), SendError> {
        self.sender
            .request_mark_dirty(self.id, self.attachment_epoch, DirtyKind::Semantics)
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(RenderInvalidationHandle: Send, Sync);

    fn id(n: usize) -> RenderId {
        RenderId::new(n)
    }

    fn pair(capacity: usize) -> (DirtySender, Receiver<DirtyRequest>) {
        DirtySender::new_pair(capacity, Arc::new(RwLock::new(VisualUpdateNotifier::new())))
    }

    #[test]
    fn render_invalidation_handle_round_trips_identity_epoch_and_kind() {
        let (sender, rx) = pair(4);
        let render_invalidation_handle =
            RenderInvalidationHandle::new(sender, id(7), AttachmentEpoch::FIRST);

        render_invalidation_handle
            .mark_needs_layout()
            .expect("first send must succeed");

        let request = rx.try_recv().expect("receiver should observe the request");
        assert_eq!(request.id, id(7));
        assert_eq!(request.attachment_epoch, AttachmentEpoch::FIRST);
        assert_eq!(request.kind, DirtyKind::Layout);
    }

    #[test]
    fn handle_returns_channel_full_at_capacity() {
        let (sender, _rx) = pair(2);
        sender
            .request_mark_dirty(id(1), AttachmentEpoch::FIRST, DirtyKind::Paint)
            .unwrap();
        sender
            .request_mark_dirty(id(2), AttachmentEpoch::FIRST, DirtyKind::Paint)
            .unwrap();
        let err = sender
            .request_mark_dirty(id(3), AttachmentEpoch::FIRST, DirtyKind::Paint)
            .unwrap_err();
        assert_eq!(err, SendError::ChannelFull { capacity: 2 });
    }

    #[test]
    fn handle_returns_owner_gone_after_receiver_drop() {
        let (sender, rx) = pair(4);
        drop(rx);
        let err = sender
            .request_mark_dirty(id(1), AttachmentEpoch::FIRST, DirtyKind::Layout)
            .unwrap_err();
        assert_eq!(err, SendError::OwnerGone);
    }

    #[test]
    #[should_panic(expected = "cannot exhaust every non-zero u64 attachment epoch")]
    fn attachment_epoch_overflow_fails_closed() {
        AttachmentEpoch(NonZeroU64::MAX).next();
    }

    #[test]
    fn queued_invalidation_envelope_stays_compact() {
        assert!(size_of::<AttachmentEpoch>() <= size_of::<u64>());
        assert!(size_of::<DirtyRequest>() <= 3 * size_of::<u64>());
    }
}
