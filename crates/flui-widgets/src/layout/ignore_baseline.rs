//! [`IgnoreBaseline`] — hides its child's baseline from a baseline-aligning
//! parent.

use flui_objects::RenderIgnoreBaseline;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Hides `child`'s baseline from the parent, so a baseline-aligning parent
/// treats this subtree as having no baseline at all.
///
/// A [`Row`](crate::Row) with `CrossAxisAlignment::Baseline` shifts its
/// children down until their baselines meet, and grows to the tallest ascent
/// plus the deepest descent. Wrapping a child here takes it out of that
/// computation entirely: it is neither shifted nor allowed to stretch the row,
/// and sits flush at the cross start. Everything else — size, paint,
/// hit-testing, intrinsics — passes straight through.
///
/// Flutter parity: `widgets/basic.dart` `IgnoreBaseline` (tag `3.44.0`), a
/// `SingleChildRenderObjectWidget` over `RenderIgnoreBaseline` with no
/// configuration of its own.
#[derive(Clone, Debug, Default)]
pub struct IgnoreBaseline {
    child: Child,
}

impl IgnoreBaseline {
    /// Creates a childless baseline-hiding wrapper.
    #[must_use]
    pub fn new() -> Self {
        Self {
            child: Child::empty(),
        }
    }

    /// Sets the child whose baseline is hidden.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for IgnoreBaseline {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIgnoreBaseline;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIgnoreBaseline::new()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        // The render object carries no configuration — the oracle's
        // `IgnoreBaseline` has no fields either, so there is nothing to push.
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        self.child.is_some()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        if let Some(child) = self.child.as_ref() {
            visitor(child);
        }
    }
}

impl_render_view!(IgnoreBaseline);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    fn detached() -> flui_view::RenderObjectContext<'static> {
        flui_view::RenderObjectContext::detached()
    }

    #[test]
    fn childless_by_default_and_adopts_a_child() {
        assert!(!IgnoreBaseline::new().has_children());
        assert!(
            IgnoreBaseline::new()
                .child(SizedBox::new(10.0, 10.0))
                .has_children(),
            "a child must be visible to the element tree, or it is silently dropped",
        );
    }

    #[test]
    fn visits_its_single_child() {
        let widget = IgnoreBaseline::new().child(SizedBox::new(10.0, 10.0));
        let mut visited = 0;
        widget.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 1);
    }

    #[test]
    fn creates_a_render_ignore_baseline() {
        let widget = IgnoreBaseline::new();
        let render = widget.create_render_object(&detached());
        assert_eq!(
            flui_rendering::traits::RenderBox::compute_distance_to_actual_baseline(
                &render,
                flui_rendering::traits::TextBaseline::Alphabetic,
            ),
            None,
        );
    }
}
