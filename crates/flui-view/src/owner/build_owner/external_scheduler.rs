//! The owned handle that lets an out-of-frame listener enqueue a rebuild on the owner.

use super::super::external_build_inbox::ExternalBuildInbox;
use super::BuildOwner;
use crate::owner::RebuildReason;
use flui_foundation::{ElementId, RebuildReasons};
use std::sync::Arc;

/// A cloneable, owned handle that lets a listener callback — an animation tick
/// fired *outside* any frame, with no `&mut BuildOwner` in scope — enqueue an
/// element for the next [`BuildOwner::build_scope`] drain and request a frame.
///
/// It carries a mark-dirty request from an element to the owner's build queue
/// and the scheduler's frame request: an
/// `AnimatedView`'s mark-dirty callback captures one of these at mount (via
/// [`ElementOwner::external_scheduler`](super::super::ElementOwner::external_scheduler))
/// and calls [`schedule`](Self::schedule) when the listenable changes. The
/// pending ids accumulate in a shared inbox that `build_scope` absorbs onto
/// its dirty heap at the top of every heap pop — not just once at frame
/// start — so a schedule landing mid-drain (a build calling another
/// element's [`RebuildHandle`](super::super::RebuildHandle) synchronously) joins
/// the SAME drain instead of waiting for the next frame (issue #1180). The
/// listener still never needs to touch the owner.
///
/// The inbox carries the element id and every cause accumulated since it was
/// last absorbed. The dirty-heap ordering key (tree depth) is read from the
/// node at absorb time, not captured here: a GlobalKey move between the
/// schedule and the absorb can change it.
#[derive(Clone)]
pub(crate) struct ExternalBuildScheduler {
    /// Shared inbox drained by `build_scope`; one accumulated cause set per
    /// element.
    inbox: Arc<ExternalBuildInbox>,
    /// Frame-request hook (the binding's `on_build_scheduled`), so a tick
    /// between frames asks the platform for a new frame. `None` in headless
    /// tests, which drive `build_scope` directly.
    request_frame: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl ExternalBuildScheduler {
    pub(crate) fn is_active(&self) -> bool {
        !self.inbox.is_closed()
    }
    /// Enqueue `id` for the next `build_scope` drain and request a frame.
    ///
    /// Deduplicating: a repeat tick for an id already queued consumes no extra
    /// inbox slot. With healthy delivery a burst still costs one frame request;
    /// while that delivery is in flight or owed, a concurrent caller may race
    /// or retry it so a failed wake cannot strand the batch. Thread-safe: the
    /// inbox lock is held only for the insert and released before
    /// `request_frame` runs (no lock across the platform wake).
    pub(crate) fn schedule(&self, id: ElementId, reason: RebuildReason) {
        self.schedule_many(std::iter::once(id), reason);
    }

    /// Enqueue a related set of ids as one durable batch, then request one
    /// frame after every id is visible to the drain. A panicking wake can
    /// therefore delay the batch, but cannot expose only a prefix of it.
    pub(crate) fn schedule_many(
        &self,
        ids: impl IntoIterator<Item = ElementId>,
        reason: RebuildReason,
    ) {
        // An iterator can run user code; collect before acquiring the inbox.
        let ids: smallvec::SmallVec<[ElementId; 4]> = ids.into_iter().collect();
        let any_newly_queued = self
            .inbox
            .admit_batch(&ids, RebuildReasons::from_reason(reason));
        self.inbox
            .request_frame_contained(any_newly_queued, self.request_frame.as_deref());
    }

    /// Build a scheduler from the shared inbox + frame-request handle. Used by
    /// [`ElementOwner::external_scheduler`](super::super::ElementOwner::external_scheduler).
    pub(crate) fn from_parts(
        inbox: Arc<ExternalBuildInbox>,
        request_frame: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        Self {
            inbox,
            request_frame,
        }
    }
}

impl std::fmt::Debug for ExternalBuildScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `try_lock`, not `lock`: `parking_lot::Mutex` is non-reentrant, so a
        // `{:?}` while the inbox is already held (e.g. instrumenting the drain)
        // would otherwise deadlock silently.
        f.debug_struct("ExternalBuildScheduler")
            .field("pending", &self.inbox.try_len())
            .field("has_request_frame", &self.request_frame.is_some())
            .finish()
    }
}

impl BuildOwner {
    /// Build an [`ExternalBuildScheduler`] over this owner's shared inbox and
    /// frame-request hook.
    ///
    /// The single construction point for the out-of-frame rebuild channel:
    /// [`ElementOwner::external_scheduler`](super::super::ElementOwner::external_scheduler)
    /// and `testing::ElementBuildContext::rebuild_handle` both route through it,
    /// so there is exactly one inbox and one frame-request path.
    pub(crate) fn external_scheduler(&self) -> ExternalBuildScheduler {
        ExternalBuildScheduler::from_parts(
            Arc::clone(&self.external_inbox),
            self.on_build_scheduled.clone(),
        )
    }
}
