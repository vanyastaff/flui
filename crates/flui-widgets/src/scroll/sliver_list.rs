//! [`SliverList`] and [`SliverChildBuilderDelegate`] — lazy element-built sliver list.
//!
//! `SliverList` is the canonical lazy-sliver view type, defined in `flui-view`
//! so the element's identity (`view_type_id`) is `TypeId::of::<SliverList>()`
//! rather than an internal adaptor type. Re-exported here for the widgets API.

use std::fmt;
use std::sync::Arc;

use flui_view::BoxedView;

// The `SliverList` type lives in `flui-view` (co-located with its element
// implementation).  Re-exporting it here keeps the widgets-crate API surface
// unchanged: users `use flui_widgets::SliverList` as before.
pub use flui_view::element::SliverList;

// ============================================================================
// SliverChildBuilderDelegate
// ============================================================================

/// Delegate that builds sliver list items on demand.
///
/// Carries the item-builder closure and a known item count. Pass it to
/// [`ListView::builder`](crate::ListView::builder) to produce a
/// lazily-virtualized list that only builds children visible in the viewport
/// plus a configurable cache margin.
///
/// # First-frame settling (Flutter divergence)
///
/// Lazy children are built **after** the frame's paint completes, not during
/// layout as Flutter does. This means the very first frame a viewport band
/// becomes visible, the children paint blank; content lands on the *next*
/// frame (~16 ms at 60 fps). The settling frame is automatically scheduled
/// because layout marks the sliver dirty (`PipelineOwner::has_dirty_nodes`
/// returns `true`), which keeps the runner's `has_pending_work()` gate open.
///
/// This is a deliberate, recorded divergence from Flutter — Flutter builds
/// lazy children during the same-frame layout pass so no blank frame is
/// visible. FLUI defers to the post-paint service step for architectural
/// simplicity; prefetch-hidden items are not affected.
#[derive(Clone)]
pub struct SliverChildBuilderDelegate {
    pub(crate) item_count: usize,
    pub(crate) builder: Arc<dyn Fn(usize) -> Option<BoxedView> + Send + Sync>,
}

impl SliverChildBuilderDelegate {
    /// Create a delegate that builds `item_count` items with `builder`.
    ///
    /// `builder(i)` returns the view for logical index `i`, or `None` when
    /// `i` is at or past the end of the data source. Both `item_count` and a
    /// `None` return are checked by the element manager; the stricter bound
    /// wins.
    #[must_use]
    pub fn new<F>(item_count: usize, builder: F) -> Self
    where
        F: Fn(usize) -> Option<BoxedView> + Send + Sync + 'static,
    {
        Self {
            item_count,
            builder: Arc::new(builder),
        }
    }
}

impl fmt::Debug for SliverChildBuilderDelegate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverChildBuilderDelegate")
            .field("item_count", &self.item_count)
            .finish_non_exhaustive()
    }
}
