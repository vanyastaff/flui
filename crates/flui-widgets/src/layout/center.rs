//! [`Center`] — centers its child within itself.

use flui_objects::RenderCenter;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Centers its child within itself.
///
/// Flutter parity: `widgets/basic.dart` `Center extends Align` with
/// `Alignment.center`. Optionally sizes itself to a multiple of the child's
/// dimensions via `width_factor`/`height_factor`.
#[derive(Clone, Debug, Default)]
pub struct Center {
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    child: Child,
}

impl Center {
    /// Create a `Center` with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Size this box to `factor` × the child's width (must be `>= 0`).
    #[must_use]
    pub fn width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size this box to `factor` × the child's height (must be `>= 0`).
    #[must_use]
    pub fn height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Set the centered child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn build_render_object(&self) -> RenderCenter {
        let mut render_object = RenderCenter::new();
        if let Some(factor) = self.width_factor {
            render_object = render_object.with_width_factor(factor);
        }
        if let Some(factor) = self.height_factor {
            render_object = render_object.with_height_factor(factor);
        }
        render_object
    }
}

impl RenderView for Center {
    type Protocol = BoxProtocol;
    type RenderObject = RenderCenter;

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        *render_object = self.build_render_object();
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

impl_render_view!(Center);

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats

    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn create_render_object_defaults_match_flutter() {
        let render_object = Center::new().create_render_object();
        assert_eq!(render_object.width_factor(), None);
        assert_eq!(render_object.height_factor(), None);
    }

    #[test]
    fn create_render_object_uses_the_given_width_and_height_factor() {
        let center = Center::new().width_factor(0.5).height_factor(2.0);
        let render_object = center.create_render_object();
        assert_eq!(render_object.width_factor(), Some(0.5));
        assert_eq!(render_object.height_factor(), Some(2.0));
    }

    #[test]
    fn update_render_object_applies_a_changed_width_and_height_factor() {
        let mut render_object = Center::new().create_render_object();
        assert_eq!(render_object.width_factor(), None);
        assert_eq!(render_object.height_factor(), None);

        let updated = Center::new().width_factor(1.5).height_factor(3.0);
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.width_factor(), Some(1.5));
        assert_eq!(render_object.height_factor(), Some(3.0));
    }

    #[test]
    fn update_render_object_clears_a_previously_set_factor() {
        let mut render_object = Center::new()
            .width_factor(0.5)
            .height_factor(0.5)
            .create_render_object();
        assert_eq!(render_object.width_factor(), Some(0.5));
        assert_eq!(render_object.height_factor(), Some(0.5));

        let updated = Center::new();
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.width_factor(), None);
        assert_eq!(render_object.height_factor(), None);
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        let empty = Center::new();
        assert!(!empty.has_children());

        let with_child = Center::new().child(SizedBox::shrink());
        assert!(with_child.has_children());
    }

    #[test]
    fn debug_reports_factors_and_child_presence() {
        let center = Center::new().width_factor(0.5);
        let debug = format!("{center:?}");
        assert!(
            debug.contains("width_factor: Some(0.5)") && debug.contains("has_child: false"),
            "Debug output must report factors and child presence, got: {debug}",
        );
    }

    #[test]
    fn visit_child_views_invokes_the_visitor_once_with_a_child_set() {
        let center = Center::new().child(SizedBox::shrink());
        let mut visited = 0;
        center.visit_child_views(&mut |_| visited += 1);
        assert_eq!(
            visited, 1,
            "visitor must run exactly once for a single child"
        );
    }

    #[test]
    fn visit_child_views_does_not_invoke_the_visitor_without_a_child() {
        let center = Center::new();
        let mut visited = 0;
        center.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 0, "no child -> visitor must not run");
    }
}
