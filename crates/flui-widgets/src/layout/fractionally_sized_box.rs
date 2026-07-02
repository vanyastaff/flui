//! [`FractionallySizedBox`] — sizes its child to a fraction of the available
//! space.

use flui_objects::{FractionFactor, RenderFractionallySizedBox};
use flui_rendering::protocol::BoxProtocol;
use flui_types::Alignment;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Sizes its child to a fraction of the available space along each axis.
///
/// Flutter parity: `widgets/basic.dart` `FractionallySizedBox` over
/// `RenderFractionallySizedBox`. A `None` factor leaves that axis at the
/// incoming constraint; factors must be finite and `>= 0`. Defaults to
/// `Alignment::CENTER`.
#[derive(Clone, Debug, Default)]
pub struct FractionallySizedBox {
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    alignment: Option<Alignment>,
    child: Child,
}

impl FractionallySizedBox {
    /// An empty `FractionallySizedBox`; set factors with the builders below.
    pub fn new() -> Self {
        Self::default()
    }

    /// Size the child's width to `factor` × the available width.
    #[must_use]
    pub fn width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Size the child's height to `factor` × the available height.
    #[must_use]
    pub fn height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Set how the sized child is aligned within the box.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Set the fractionally-sized child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    fn factor(value: Option<f32>) -> Option<FractionFactor> {
        value.map(FractionFactor::new_unchecked)
    }

    fn build_render_object(&self) -> RenderFractionallySizedBox {
        let mut render_object = RenderFractionallySizedBox::new();
        if let Some(alignment) = self.alignment {
            render_object = render_object.with_alignment(alignment);
        }
        if let Some(factor) = Self::factor(self.width_factor) {
            render_object = render_object.with_width_factor(factor);
        }
        if let Some(factor) = Self::factor(self.height_factor) {
            render_object = render_object.with_height_factor(factor);
        }
        render_object
    }
}

impl RenderView for FractionallySizedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFractionallySizedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_width_factor(Self::factor(self.width_factor));
        render_object.set_height_factor(Self::factor(self.height_factor));
        render_object.set_alignment(self.alignment.unwrap_or(Alignment::CENTER));
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

impl_render_view!(FractionallySizedBox);

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats

    use flui_objects::FractionFactor;
    use flui_types::Alignment;
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn create_render_object_defaults_match_flutter() {
        let render_object = FractionallySizedBox::new().create_render_object();
        assert_eq!(render_object.width_factor(), None);
        assert_eq!(render_object.height_factor(), None);
        assert_eq!(render_object.alignment(), Alignment::CENTER);
    }

    #[test]
    fn create_render_object_uses_the_given_width_height_factor_and_alignment() {
        let widget = FractionallySizedBox::new()
            .width_factor(0.5)
            .height_factor(0.75)
            .alignment(Alignment::TOP_LEFT);
        let render_object = widget.create_render_object();
        assert_eq!(render_object.width_factor(), FractionFactor::new(0.5));
        assert_eq!(render_object.height_factor(), FractionFactor::new(0.75));
        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);
    }

    #[test]
    fn update_render_object_applies_a_changed_width_and_height_factor() {
        let mut render_object = FractionallySizedBox::new().create_render_object();
        assert_eq!(render_object.width_factor(), None);
        assert_eq!(render_object.height_factor(), None);

        let updated = FractionallySizedBox::new()
            .width_factor(0.25)
            .height_factor(0.6);
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.width_factor(), FractionFactor::new(0.25));
        assert_eq!(render_object.height_factor(), FractionFactor::new(0.6));
    }

    #[test]
    fn update_render_object_clears_a_previously_set_factor() {
        let mut render_object = FractionallySizedBox::new()
            .width_factor(0.5)
            .height_factor(0.5)
            .create_render_object();
        assert_eq!(render_object.width_factor(), FractionFactor::new(0.5));
        assert_eq!(render_object.height_factor(), FractionFactor::new(0.5));

        let updated = FractionallySizedBox::new(); // no factors set -> None
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.width_factor(), None);
        assert_eq!(render_object.height_factor(), None);
    }

    #[test]
    fn update_render_object_applies_a_changed_alignment() {
        let mut render_object = FractionallySizedBox::new().create_render_object();
        assert_eq!(render_object.alignment(), Alignment::CENTER);

        let updated = FractionallySizedBox::new().alignment(Alignment::BOTTOM_RIGHT);
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.alignment(), Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn update_render_object_resets_alignment_to_center_when_the_widget_omits_it() {
        // A widget rebuilt without an explicit `.alignment(..)` call
        // (`self.alignment == None`, meaning "use the default") must reset
        // a previously applied alignment back to `Alignment::CENTER` --
        // matching `width_factor`/`height_factor`'s always-overwrite
        // behavior, and what `create_render_object` on the same widget
        // value would produce fresh. Regression test for a bug where
        // `update_render_object` only called `set_alignment` when
        // `self.alignment` was `Some(..)`, silently leaving a stale
        // alignment in place on an otherwise-default rebuild.
        let mut render_object = FractionallySizedBox::new()
            .alignment(Alignment::TOP_LEFT)
            .create_render_object();
        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);

        let updated = FractionallySizedBox::new(); // no explicit alignment -> None
        updated.update_render_object(&mut render_object);

        assert_eq!(render_object.alignment(), Alignment::CENTER);
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        let empty = FractionallySizedBox::new();
        assert!(!empty.has_children());

        let with_child = FractionallySizedBox::new().child(SizedBox::shrink());
        assert!(with_child.has_children());
    }

    #[test]
    fn debug_reports_factors_alignment_and_child_presence() {
        let widget = FractionallySizedBox::new()
            .width_factor(0.5)
            .alignment(Alignment::TOP_LEFT);
        let debug = format!("{widget:?}");
        assert!(
            debug.contains("width_factor: Some(0.5)")
                && debug.contains("alignment: Some(Alignment")
                && debug.contains("has_child: false"),
            "Debug output must report factors, alignment and child presence, got: {debug}",
        );
    }

    #[test]
    fn visit_child_views_invokes_the_visitor_once_with_a_child_set() {
        let widget = FractionallySizedBox::new().child(SizedBox::shrink());
        let mut visited = 0;
        widget.visit_child_views(&mut |_| visited += 1);
        assert_eq!(
            visited, 1,
            "visitor must run exactly once for a single child"
        );
    }

    #[test]
    fn visit_child_views_does_not_invoke_the_visitor_without_a_child() {
        let widget = FractionallySizedBox::new();
        let mut visited = 0;
        widget.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 0, "no child -> visitor must not run");
    }
}
