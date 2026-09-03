//! [`SliverFixedExtentList`] — a sliver that lays out box children one after
//! another, each given the same fixed main-axis extent, building only the
//! ones its window needs.

use std::fmt;
use std::rc::Rc;

use flui_view::element::StaticChildren;
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BuildContext, IntoView};

/// A sliver that places its box children sequentially along the scroll axis,
/// each occupying the same `item_extent` — cheaper to lay out than measuring
/// every child, the backbone of a fixed-row-height [`ListView`](crate::ListView).
///
/// The children are handed to the element tree as a static delegate
/// (Flutter's `SliverChildListDelegate`): only the ones inside the viewport's
/// cache window are built, keyed children are found by their key when the
/// list is reordered, and everything outside the window is evicted.
///
/// Flutter parity: `widgets/sliver.dart` `SliverFixedExtentList` over
/// `RenderSliverFixedExtentList`. Lives inside a [`Viewport`](crate::Viewport).
#[derive(Clone, StatelessView)]
pub struct SliverFixedExtentList {
    item_extent: f32,
    source: Source,
}

/// Where the children come from.
#[derive(Clone)]
enum Source {
    /// A fixed list, served lazily by index (Flutter's `SliverChildListDelegate`).
    Static(Rc<StaticChildren>),
    /// Built on demand up to `item_count` (Flutter's `SliverChildBuilderDelegate`).
    Builder {
        item_count: usize,
        builder: Rc<dyn Fn(usize) -> Option<flui_view::BoxedView>>,
    },
}

impl SliverFixedExtentList {
    /// A fixed-extent sliver list: every child gets `item_extent` on the scroll
    /// axis.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    pub fn new(item_extent: f32, children: impl ViewSeq) -> Self {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and positive, got {item_extent}",
        );
        Self {
            item_extent,
            source: Source::Static(StaticChildren::new(children.into_boxed_vec())),
        }
    }

    /// The same list over an already shared delegate — two views built over
    /// one delegate compare as unchanged on update, so the resident children
    /// are not rebuilt.
    #[must_use]
    pub fn over(item_extent: f32, children: Rc<StaticChildren>) -> Self {
        Self {
            item_extent,
            source: Source::Static(children),
        }
    }

    /// A fixed-extent list of up to `item_count` children built on demand;
    /// the builder answers `None` at the end of the data (Flutter's
    /// `SliverFixedExtentList.builder`). Pass `usize::MAX` for a count the
    /// builder alone knows: the first `None` clamps it.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent` is not finite or not greater than zero.
    pub fn builder<F>(item_extent: f32, item_count: usize, builder: F) -> Self
    where
        F: Fn(usize) -> Option<flui_view::BoxedView> + 'static,
    {
        assert!(
            item_extent.is_finite() && item_extent > 0.0,
            "item_extent must be finite and positive, got {item_extent}",
        );
        Self {
            item_extent,
            source: Source::Builder {
                item_count,
                builder: Rc::new(builder),
            },
        }
    }

    /// The per-child main-axis extent.
    #[must_use]
    pub const fn item_extent(&self) -> f32 {
        self.item_extent
    }
}

impl fmt::Debug for SliverFixedExtentList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("SliverFixedExtentList");
        s.field("item_extent", &self.item_extent);
        match &self.source {
            Source::Static(children) => s.field("children", &children.len()),
            Source::Builder { item_count, .. } => s.field("item_count", item_count),
        };
        s.finish()
    }
}

impl StatelessView for SliverFixedExtentList {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        match &self.source {
            Source::Static(children) => {
                flui_view::element::SliverFixedExtentList::over(self.item_extent, children)
            }
            Source::Builder {
                item_count,
                builder,
            } => flui_view::element::SliverFixedExtentList::new(
                self.item_extent,
                *item_count,
                Rc::clone(builder),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use flui_view::ViewExt;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn debug_reports_item_extent_and_child_count() {
        let list = SliverFixedExtentList::new(
            30.0,
            vec![
                SizedBox::shrink().boxed(),
                SizedBox::shrink().boxed(),
                SizedBox::shrink().boxed(),
            ],
        );

        let debug = format!("{list:?}");
        assert!(
            debug.contains("item_extent: 30.0") && debug.contains("children: 3"),
            "Debug output must include item_extent and children count, got: {debug}",
        );
    }

    #[test]
    fn over_shares_the_delegate_between_two_views() {
        let children = StaticChildren::new(vec![SizedBox::shrink().boxed()]);
        let a = SliverFixedExtentList::over(30.0, Rc::clone(&children));
        let b = SliverFixedExtentList::over(30.0, Rc::clone(&children));
        let (Source::Static(a_children), Source::Static(b_children)) = (&a.source, &b.source)
        else {
            panic!("over builds a static source");
        };
        assert!(Rc::ptr_eq(a_children, b_children));
        assert_eq!(a.item_extent(), 30.0);
    }

    #[test]
    #[should_panic(expected = "item_extent must be finite and positive")]
    fn new_rejects_a_zero_extent() {
        let _ = SliverFixedExtentList::new(0.0, Vec::<flui_view::BoxedView>::new());
    }
}
