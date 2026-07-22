//! [`IntrinsicWidth`] — sizes child to its maximum intrinsic width.

use flui_objects::RenderIntrinsicWidth;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Sizes its child to the child's maximum intrinsic width.
///
/// Useful when a widget should be exactly as wide as its natural content
/// rather than merely constrained by it.  The optional `step_width` and
/// `step_height` parameters round values up to a size grid, reducing relayout
/// churn in dynamic lists where adjacent items should snap to a common width.
///
/// Flutter parity: `widgets/basic.dart` `IntrinsicWidth` over
/// [`RenderIntrinsicWidth`].
#[derive(Clone, Debug)]
pub struct IntrinsicWidth {
    /// Optional column-width quantum; intrinsic width is rounded up to the
    /// nearest multiple.
    step_width: Option<f32>,
    /// Optional row-height quantum; the height passed to the intrinsic-width
    /// query is rounded up to the nearest multiple before querying.
    step_height: Option<f32>,
    child: Child,
}

impl IntrinsicWidth {
    /// Creates the widget with no step snapping.
    pub fn new() -> Self {
        Self {
            step_width: None,
            step_height: None,
            child: Child::empty(),
        }
    }

    /// Sets the column-width quantum (rounds intrinsic width up to a multiple).
    #[must_use]
    pub fn with_step_width(mut self, step_width: f32) -> Self {
        self.step_width = Some(step_width);
        self
    }

    /// Sets the row-height quantum (rounds height-for-width-query up to a multiple).
    #[must_use]
    pub fn with_step_height(mut self, step_height: f32) -> Self {
        self.step_height = Some(step_height);
        self
    }

    /// Sets the child view.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl Default for IntrinsicWidth {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderView for IntrinsicWidth {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIntrinsicWidth;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIntrinsicWidth::new(self.step_width, self.step_height)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_step_width(self.step_width);
        render_object.set_step_height(self.step_height);
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

impl_render_view!(IntrinsicWidth);

#[cfg(test)]
#[allow(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats
mod tests {
    use super::*;
    use crate::SizedBox;

    #[test]
    fn new_defaults_to_no_step_and_no_child() {
        let widget = IntrinsicWidth::new();
        assert_eq!(widget.step_width, None);
        assert_eq!(widget.step_height, None);
        assert!(!widget.has_children());
    }

    #[test]
    fn default_matches_new() {
        let widget = IntrinsicWidth::default();
        assert_eq!(widget.step_width, None);
        assert_eq!(widget.step_height, None);
        assert!(!widget.has_children());
    }

    #[test]
    fn with_step_width_overrides_the_default() {
        let widget = IntrinsicWidth::new().with_step_width(40.0);
        assert_eq!(widget.step_width, Some(40.0));
        assert_eq!(widget.step_height, None);
    }

    #[test]
    fn with_step_height_overrides_the_default() {
        let widget = IntrinsicWidth::new().with_step_height(25.0);
        assert_eq!(widget.step_height, Some(25.0));
        assert_eq!(widget.step_width, None);
    }

    #[test]
    fn child_sets_a_child_and_has_children_reports_true() {
        let widget = IntrinsicWidth::new().child(SizedBox::shrink());
        assert!(widget.has_children());
    }

    #[test]
    fn has_children_reports_false_without_a_child() {
        let widget = IntrinsicWidth::new();
        assert!(!widget.has_children());
    }

    #[test]
    fn create_render_object_wires_step_width_and_step_height() {
        let widget = IntrinsicWidth::new()
            .with_step_width(40.0)
            .with_step_height(25.0);
        let render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.step_width(), Some(40.0));
        assert_eq!(render_object.step_height(), Some(25.0));
    }

    #[test]
    fn create_render_object_defaults_to_no_step() {
        let widget = IntrinsicWidth::new();
        let render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.step_width(), None);
        assert_eq!(render_object.step_height(), None);
    }

    #[test]
    fn update_render_object_applies_changed_steps() {
        let widget = IntrinsicWidth::new();
        let mut render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.step_width(), None);
        assert_eq!(render_object.step_height(), None);

        let updated = IntrinsicWidth::new()
            .with_step_width(10.0)
            .with_step_height(15.0);
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert_eq!(render_object.step_width(), Some(10.0));
        assert_eq!(render_object.step_height(), Some(15.0));
    }

    #[test]
    fn update_render_object_clears_steps_back_to_none() {
        let widget = IntrinsicWidth::new()
            .with_step_width(10.0)
            .with_step_height(15.0);
        let mut render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.step_width(), Some(10.0));
        assert_eq!(render_object.step_height(), Some(15.0));

        let updated = IntrinsicWidth::new();
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert_eq!(render_object.step_width(), None);
        assert_eq!(render_object.step_height(), None);
    }

    #[test]
    fn visit_child_views_invokes_the_visitor_once_with_a_child() {
        let widget = IntrinsicWidth::new().child(SizedBox::shrink());
        let mut visited = 0;
        widget.visit_child_views(&mut |_| visited += 1);
        assert_eq!(
            visited, 1,
            "visitor must run exactly once for the one child"
        );
    }

    #[test]
    fn visit_child_views_does_not_invoke_the_visitor_without_a_child() {
        let widget = IntrinsicWidth::new();
        let mut visited = 0;
        widget.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 0, "no child -> visitor must not run");
    }

    #[test]
    fn debug_reports_defaults_and_no_child() {
        let widget = IntrinsicWidth::new();
        let debug = format!("{widget:?}");
        assert!(
            debug.contains("step_width: None")
                && debug.contains("step_height: None")
                && debug.contains("has_child: false"),
            "Debug output must report step_width, step_height and child presence, got: {debug}",
        );
    }

    #[test]
    fn debug_reports_overridden_steps_and_a_present_child() {
        let widget = IntrinsicWidth::new()
            .with_step_width(40.0)
            .with_step_height(25.0)
            .child(SizedBox::shrink());
        let debug = format!("{widget:?}");
        assert!(
            debug.contains("step_width: Some(40.0)")
                && debug.contains("step_height: Some(25.0)")
                && debug.contains("has_child: true"),
            "Debug output must report the overridden steps and child presence, got: {debug}",
        );
    }
}
