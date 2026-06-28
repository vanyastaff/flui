//! [`SliverList`] and [`SliverChildBuilderDelegate`] — lazy element-built sliver list.

use std::fmt;
use std::sync::Arc;

use flui_view::element::SliverListAdaptorView;
use flui_view::{BoxedView, ElementBase, View};

// ============================================================================
// SliverChildBuilderDelegate
// ============================================================================

/// Delegate that builds sliver list items on demand.
///
/// Carries the item-builder closure and a known item count. Pass it to
/// [`SliverList::builder`] or [`crate::ListView::builder`] to produce a
/// lazily-virtualized list that only builds children visible in the viewport
/// plus a configurable cache margin.
///
/// # Headless-only (U4.3)
///
/// Lazy lists are wired into [`flui_binding::HeadlessBinding::pump_frame`];
/// production-window support (a `widgets-binding` that owns a `BuildOwner`) is
/// a deferred unit. The API is forward-compatible — no delegate-shape changes
/// will be needed.
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

// ============================================================================
// SliverList
// ============================================================================

/// A lazily-built sliver list widget for use inside a [`crate::Viewport`].
///
/// Wraps [`SliverListAdaptorView`] — the element-backed, request-strategy list.
/// Children are built on demand from a [`SliverChildBuilderDelegate`] and
/// disposed when they scroll out of the cache window. The cache band is
/// determined by the underlying [`RenderSliverList`] after each layout pass.
///
/// # Usage
///
/// ```ignore
/// use flui_widgets::prelude::*;
/// use flui_widgets::{SliverChildBuilderDelegate, SliverList, Viewport};
///
/// let delegate = SliverChildBuilderDelegate::new(1000, |i| {
///     Some(Text::new(format!("Item {i}")).boxed())
/// });
/// let list = SliverList::builder(delegate, 48.0);
/// let viewport = Viewport::new((list,)).offset(scroll_offset);
/// ```
///
/// # Flutter parity
///
/// Corresponds to Flutter's `SliverList` over a `SliverChildBuilderDelegate`
/// (request strategy, not build strategy). The variable-height virtualizer
/// (`Virtualizer` / `RenderSliverList`) updates item extents from real
/// measurements after each layout pass, converging to a fixed point.
///
/// [`RenderSliverList`]: flui_objects::RenderSliverList
#[derive(Clone)]
pub struct SliverList {
    delegate: SliverChildBuilderDelegate,
    item_extent_estimate: f32,
}

impl SliverList {
    /// Create a lazily-built sliver list from `delegate` with the given
    /// per-item `item_extent_estimate` (logical pixels).
    ///
    /// `item_extent_estimate` seeds the virtualizer until real measurements
    /// arrive from laid-out children. Must be finite and positive.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent_estimate` is not finite and positive.
    #[must_use]
    pub fn builder(delegate: SliverChildBuilderDelegate, item_extent_estimate: f32) -> Self {
        assert!(
            item_extent_estimate.is_finite() && item_extent_estimate > 0.0,
            "item_extent_estimate must be finite and positive, got {item_extent_estimate}",
        );
        Self {
            delegate,
            item_extent_estimate,
        }
    }
}

impl fmt::Debug for SliverList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverList")
            .field("item_count", &self.delegate.item_count)
            .field("item_extent_estimate", &self.item_extent_estimate)
            .finish_non_exhaustive()
    }
}

impl View for SliverList {
    fn create_element(&self) -> Box<dyn ElementBase> {
        // Delegate to SliverListAdaptorView, which creates the
        // SliverListAdaptorElement with its registered ChildManager (F8).
        SliverListAdaptorView::new(
            self.delegate.item_count,
            self.item_extent_estimate,
            Arc::clone(&self.delegate.builder),
        )
        .create_element()
    }
}
