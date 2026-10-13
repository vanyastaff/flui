//! [`Theater`] — the [`Overlay`](super::Overlay)'s private render view.
//!
//! Private to the overlay module. It exists only so [`OverlayState::build`](super::OverlayState)
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
    /// Leading children shown but hidden from semantics, under a modal entry.
    semantics_blocked: usize,
    children: Vec<BoxedView>,
}

impl Theater {
    /// `skip_count` must not exceed the child count. A negative count is
    /// unrepresentable; the bound is an invariant of `OverlayState::build`, which computes `skip_count` from
    /// `children.len()`, so it is a `debug_assert`.
    pub(super) fn new(children: Vec<BoxedView>, skip_count: usize) -> Self {
        debug_assert!(
            skip_count <= children.len(),
            "BUG: Theater skip_count {skip_count} exceeds child count {}",
            children.len()
        );
        Self {
            skip_count,
            semantics_blocked: 0,
            children,
        }
    }

    /// Hides the first `count` children from semantics while they stay onstage.
    pub(super) fn semantics_blocked(mut self, count: usize) -> Self {
        debug_assert!(
            count <= self.children.len(),
            "BUG: Theater semantics_blocked {count} exceeds child count {}",
            self.children.len()
        );
        self.semantics_blocked = count;
        self
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
        RenderTheater::new()
            .with_skip_count(self.skip_count)
            .with_semantics_blocked_count(self.semantics_blocked)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_skip_count(self.skip_count);
        impact |= render_object.set_semantics_blocked_count(self.semantics_blocked);
        impact
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        self.children.for_each(|_index, child| visitor(child));
    }
}
