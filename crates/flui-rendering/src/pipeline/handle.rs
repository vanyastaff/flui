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
//! The owner drains the channel into its [`DirtySets`] at a defined point
//! in each frame (today: at the start of `flush_layout` / `flush_paint`;
//! eventually under Mythos Step 7, at phase transitions).
//!
//! ## Backpressure
//!
//! The channel is bounded. Default capacity 256 (tunable at construction).
//! When the channel is full, [`request_mark_dirty`] returns
//! `Err(SendError::ChannelFull)` -- the producer has surfaced
//! backpressure and decides what to do (wait, drop the request, log,
//! escalate). Unbounded channels would hide this in heap growth; we
//! refuse them.
//!
//! When the receiving owner drops, outstanding handles receive
//! `Err(SendError::OwnerGone)` on their next `request_mark_dirty`.

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use flui_foundation::RenderId;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
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
#[derive(Debug, Clone)]
pub struct PipelineOwnerHandle {
    tx: Sender<DirtyRequest>,
    capacity: usize,
}

impl PipelineOwnerHandle {
    /// Constructs the handle + receiver pair. Internal use only;
    /// `PipelineOwner::new` wires this up.
    pub(super) fn new_pair(capacity: usize) -> (Self, Receiver<DirtyRequest>) {
        let (tx, rx) = bounded(capacity);
        (Self { tx, capacity }, rx)
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
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(SendError::ChannelFull {
                capacity: self.capacity,
            }),
            Err(TrySendError::Disconnected(_)) => Err(SendError::OwnerGone),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: usize) -> RenderId {
        RenderId::new(n)
    }

    #[test]
    fn handle_send_recv_round_trip() {
        let (handle, rx) = PipelineOwnerHandle::new_pair(4);
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
    fn handle_returns_channel_full_at_capacity() {
        let (handle, _rx) = PipelineOwnerHandle::new_pair(2);
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
        let (handle, rx) = PipelineOwnerHandle::new_pair(4);
        drop(rx);
        let err = handle
            .request_mark_dirty(id(1), 0, DirtyKind::Layout)
            .unwrap_err();
        assert_eq!(err, SendError::OwnerGone);
    }

    #[test]
    fn handle_is_clone_and_each_clone_sends_independently() {
        let (handle_a, rx) = PipelineOwnerHandle::new_pair(4);
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
