//! [`SliverList`] and [`SliverChildBuilderDelegate`] — lazy element-built sliver list.
//!
//! `SliverList` is the canonical lazy-sliver view type, defined in `flui-view`
//! so the element's identity (`view_type_id`) is `TypeId::of::<SliverList>()`
//! rather than an internal adaptor type. Re-exported here for the widgets API.

use std::fmt;
use std::rc::Rc;

use flui_view::BoxedView;

use crate::__private::SaltingChildKey as _;

// The `SliverList` type lives in `flui-view` (co-located with its element
// implementation).  Re-exporting it here keeps the widgets-crate API surface
// unchanged: users `use flui_widgets::SliverList` as before.
pub use flui_view::element::SliverList;

// ============================================================================
// SliverChildBuilderDelegate
// ============================================================================

/// A delegate's key → index callback.
pub(crate) type FindIndexByKey = Rc<dyn Fn(&dyn flui_foundation::ViewKey) -> Option<usize>>;

/// Delegate that builds sliver list items on demand.
///
/// Carries the item-builder closure and a known item count. Pass it to
/// [`ListView::builder`](crate::ListView::builder) to produce a
/// lazily-virtualized list that only builds children visible in the viewport
/// plus a configurable cache margin.
///
/// # Same-frame settling
///
/// Lazy children are built between the layout passes of the frame that
/// requested them (the layout↔build fixpoint `LayoutBuilder` also uses), so
/// a band that scrolls into view is built, laid out, and painted in that
/// frame, without a reentrant build during layout. A frame that needs more passes than the
/// lazy-band budget allows defers the remainder to the next frame; the
/// item-extent estimate adapts to the measured children so that is rare.
///
/// # Recovery
///
/// A builder that panics yields the registered error view at that index
/// only — a render-owning error box in a finite row — and every other index
/// is untouched.
#[derive(Clone)]
pub struct SliverChildBuilderDelegate {
    pub(crate) item_count: usize,
    pub(crate) builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
    /// Maps an item's key to its current index, so a keyed item whose data moved out of
    /// the resident band keeps its state. Moves within the band need no
    /// callback.
    pub(crate) find_index_by_key: Option<FindIndexByKey>,
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
        F: Fn(usize) -> Option<BoxedView> + 'static,
    {
        Self {
            item_count,
            builder: Rc::new(builder),
            find_index_by_key: None,
        }
    }
}

/// Wrap each child in a [`RepaintBoundary`](crate::paint::RepaintBoundary).
///
/// Done by default because children in a scrolling container do not need to be
/// repainted as the list scrolls. Without it the paint walk descends into every visible item every frame and there is
/// nothing for `PipelineOwner::retained_boundaries` to reuse.
#[must_use]
pub(crate) fn wrap_in_repaint_boundaries(children: Vec<BoxedView>) -> Vec<BoxedView> {
    children.into_iter().map(wrap_in_repaint_boundary).collect()
}

/// Wrap a lazy builder's output in per-item repaint boundaries.
///
/// Applied where a widget builds its sliver rather than where the delegate is
/// constructed, so `repaint_boundaries(false)` still takes effect on a widget
/// whose delegate already exists.
#[must_use]
pub(crate) fn wrap_builder_in_repaint_boundaries(
    builder: &Rc<dyn Fn(usize) -> Option<BoxedView>>,
) -> Rc<dyn Fn(usize) -> Option<BoxedView>> {
    let inner = Rc::clone(builder);
    Rc::new(move |index| inner(index).map(wrap_in_repaint_boundary))
}

/// Wrap one child, keeping its key visible to the parent.
///
/// The key matters: a lazy sliver reconciles its children by key, and an
/// unkeyed wrapper would hide the item's own key, costing element state on
/// insert, remove, and reorder. The key is carried on the boundary itself,
/// because a lazy sliver child must own a render node and a stateless keyed
/// wrapper has none — see `RepaintBoundary::salted_child_key`.
pub(crate) fn wrap_in_repaint_boundary(child: BoxedView) -> BoxedView {
    BoxedView(Box::new(
        crate::paint::RepaintBoundary::new()
            .child(child)
            .salting_child_key(),
    ))
}

impl fmt::Debug for SliverChildBuilderDelegate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverChildBuilderDelegate")
            .field("item_count", &self.item_count)
            .finish_non_exhaustive()
    }
}
