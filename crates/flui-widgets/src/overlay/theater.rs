//! [`Theater`] — the [`Overlay`](super::Overlay)'s private render view.
//!
//! Flutter's `_Theater` (`overlay.dart:979-1006`), likewise private to the
//! overlay library. It exists only so [`OverlayState::build`](super::OverlayState)
//! can hand `skip_count` to [`RenderTheater`], which drops the leading offstage
//! children from layout, paint and hit-test.

use flui_objects::RenderTheater;
use flui_rendering::protocol::BoxProtocol;
use flui_view::element::ElementKind;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, View};

/// A `StackFit::Expand` stack whose first `skip_count` children are offstage.
#[derive(Clone, Debug)]
pub(super) struct Theater {
    skip_count: usize,
    children: Vec<BoxedView>,
}

impl Theater {
    /// Flutter asserts `skipCount >= 0 && children.length >= skipCount`
    /// (`overlay.dart:989-990`). The first is unrepresentable here; the second is
    /// an invariant of `OverlayState::build`, which computes `skip_count` from
    /// `children.len()`, so it is a `debug_assert`.
    pub(super) fn new(children: Vec<BoxedView>, skip_count: usize) -> Self {
        debug_assert!(
            skip_count <= children.len(),
            "BUG: Theater skip_count {skip_count} exceeds child count {}",
            children.len()
        );
        Self {
            skip_count,
            children,
        }
    }
}

impl View for Theater {
    fn create_element(&self) -> ElementKind {
        ElementKind::render_variable(self)
    }
}

impl flui_view::RenderView for Theater {
    type Protocol = BoxProtocol;
    type RenderObject = RenderTheater;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderTheater::new().with_skip_count(self.skip_count)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_skip_count(self.skip_count);
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}
