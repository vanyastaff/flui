//! [`SliverList`] and [`SliverChildBuilderDelegate`] — lazy element-built sliver list.
//!
//! `SliverList` is the canonical lazy-sliver view type, defined in `flui-view`
//! so the element's identity (`view_type_id`) is `TypeId::of::<SliverList>()`
//! rather than an internal adaptor type. Re-exported here for the widgets API.

use std::fmt;
use std::rc::Rc;

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
    pub(crate) builder: Rc<dyn Fn(usize) -> Option<BoxedView>>,
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
        Self::with_options(item_count, true, builder)
    }

    /// Like [`Self::new`], but lets the caller decline the per-item repaint
    /// boundary.
    ///
    /// Flutter parity: `SliverChildBuilderDelegate.addRepaintBoundaries`,
    /// which defaults to `true` for the reason its own doc gives — children in
    /// a scrolling container "do not need to be repainted as the list
    /// scrolls". Without the boundary the paint walk descends into every
    /// visible item on every frame, and nothing the retention path
    /// (`PipelineOwner::retained_boundaries`) can reuse ever exists.
    ///
    /// Pass `false` when the items are cheaper to repaint than to composite —
    /// solid colour blocks, a short line of text — which is the same guidance
    /// Flutter gives.
    ///
    /// `pub(crate)` on purpose: no scroll widget accepts a prebuilt delegate,
    /// so a public form would be unreachable API. Flutter exposes the flag on
    /// `ListView.builder` / `GridView.count` themselves; giving the FLUI
    /// widgets the same knob is the follow-up, and this stays internal until
    /// there is a caller.
    #[must_use]
    pub(crate) fn with_options<F>(
        item_count: usize,
        add_repaint_boundaries: bool,
        builder: F,
    ) -> Self
    where
        F: Fn(usize) -> Option<BoxedView> + 'static,
    {
        let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = if add_repaint_boundaries {
            Rc::new(move |index| builder(index).map(wrap_in_repaint_boundary))
        } else {
            Rc::new(builder)
        };
        Self {
            item_count,
            builder,
        }
    }
}

/// Wrap each child in a [`RepaintBoundary`](crate::paint::RepaintBoundary).
///
/// Flutter parity: the delegates in `widgets/scroll_delegate.dart` do this by
/// default (`addRepaintBoundaries = true`) so that "children in a scrolling
/// container ... do not need to be repainted as the list scrolls". Without it
/// the paint walk descends into every visible item every frame and there is
/// nothing for `PipelineOwner::retained_boundaries` to reuse.
#[must_use]
pub(crate) fn wrap_in_repaint_boundaries(children: Vec<BoxedView>) -> Vec<BoxedView> {
    children.into_iter().map(wrap_in_repaint_boundary).collect()
}

/// Wrap one child, keeping its key visible to the parent.
///
/// The key matters: a lazy sliver reconciles its children by key, and an
/// unkeyed wrapper would hide the item's own key, costing element state on
/// insert, remove, and reorder. Flutter restores the key outside the boundary
/// with `KeyedSubtree`; FLUI carries it on the boundary itself, because a lazy
/// sliver child must own a render node and a stateless `KeyedSubtree`
/// equivalent has none — see `RepaintBoundary::forwards_child_key`.
fn wrap_in_repaint_boundary(child: BoxedView) -> BoxedView {
    BoxedView(Box::new(
        crate::paint::RepaintBoundary::new()
            .child(child)
            .forwarding_child_key(),
    ))
}

impl fmt::Debug for SliverChildBuilderDelegate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverChildBuilderDelegate")
            .field("item_count", &self.item_count)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use flui_view::ViewExt;

    use super::*;

    /// The repaint-boundary wrap keeps the item's own key visible.
    ///
    /// A lazy sliver reconciles its children by key. An unkeyed wrapper hides
    /// the item's key from the parent, which then treats every insert, remove,
    /// or reorder as a fresh child and drops its element state. Flutter avoids
    /// this by restoring the key OUTSIDE the boundary with `KeyedSubtree`
    /// (`widgets/scroll_delegate.dart:559`, `:572`); FLUI carries it on the
    /// boundary, which is the outermost element the parent reconciles either
    /// way.
    ///
    /// Reverted (drop `forwarding_child_key`) the wrapper reports `None`.
    #[test]
    fn the_repaint_boundary_wrap_keeps_the_items_key() {
        use flui_foundation::ValueKey;
        use flui_view::{BuildContext, IntoView, StatelessView, View};

        #[derive(Clone)]
        struct KeyedItem(ValueKey<u32>);

        impl View for KeyedItem {
            fn create_element(&self) -> flui_view::element::ElementKind {
                flui_view::element::ElementKind::stateless(self)
            }

            fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
                Some(&self.0)
            }
        }

        impl StatelessView for KeyedItem {
            fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
                self.clone()
            }
        }

        let wrapped =
            wrap_in_repaint_boundary(BoxedView(Box::new(KeyedItem(ValueKey::new(7_u32)))));
        let expected = ValueKey::new(7_u32);

        let actual = wrapped
            .key()
            .expect("the wrapper must expose the item's key, not swallow it");
        assert!(
            actual.key_eq(&expected),
            "the wrapper's key must BE the item's key, not merely present"
        );
    }

    #[test]
    fn new_stores_the_item_count_and_invokes_the_builder_by_index() {
        let delegate = SliverChildBuilderDelegate::new(3, |i| {
            if i < 3 {
                Some(crate::SizedBox::new(10.0, 10.0).boxed())
            } else {
                None
            }
        });

        assert_eq!(delegate.item_count, 3);
        assert!((delegate.builder)(0).is_some());
        assert!((delegate.builder)(2).is_some());
        assert!(
            (delegate.builder)(3).is_none(),
            "the builder must return None past the declared item_count",
        );
    }

    #[test]
    fn debug_reports_the_item_count() {
        let delegate = SliverChildBuilderDelegate::new(7, |_| None);
        let debug = format!("{delegate:?}");
        assert!(
            debug.contains("item_count: 7"),
            "Debug output must include the item_count, got: {debug}",
        );
    }
}
