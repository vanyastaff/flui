//! [`FittedBox`] — scales and positions its child within itself per a [`BoxFit`].

use flui_objects::RenderFittedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Alignment;
use flui_types::layout::BoxFit;
use flui_types::painting::Clip;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Scales and positions its child within itself according to a [`BoxFit`].
///
/// Flutter parity: `widgets/basic.dart` `FittedBox` over `RenderFittedBox`.
/// Defaults match Flutter: `BoxFit::Contain`, `Alignment::CENTER`, `Clip::None`.
#[derive(Clone, Debug)]
pub struct FittedBox {
    fit: BoxFit,
    alignment: Alignment,
    clip: Clip,
    child: Child,
}

impl Default for FittedBox {
    fn default() -> Self {
        Self {
            fit: BoxFit::Contain,
            alignment: Alignment::CENTER,
            clip: Clip::None,
            child: Child::empty(),
        }
    }
}

impl FittedBox {
    /// A `FittedBox` with Flutter's defaults (`Contain` / centered / no clip).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set how the child is inscribed into the available space.
    #[must_use]
    pub fn fit(mut self, fit: BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// Set how the scaled child is aligned within the box.
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set the clip behavior applied when the child overflows.
    #[must_use]
    pub fn clip(mut self, clip: Clip) -> Self {
        self.clip = clip;
        self
    }

    /// Set the fitted child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for FittedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderFittedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderFittedBox::new(self.fit, self.alignment, self.clip)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_fit(self.fit);
        render_object.set_alignment(self.alignment);
        render_object.set_clip_behavior(self.clip);
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

impl_render_view!(FittedBox);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn create_render_object_defaults_match_flutter() {
        let render_object =
            FittedBox::new().create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.fit(), BoxFit::Contain);
        assert_eq!(render_object.alignment(), Alignment::CENTER);
        assert_eq!(render_object.clip_behavior(), Clip::None);
    }

    #[test]
    fn create_render_object_uses_the_given_fit_alignment_and_clip() {
        let fitted_box = FittedBox::new()
            .fit(BoxFit::Cover)
            .alignment(Alignment::TOP_LEFT)
            .clip(Clip::AntiAlias);
        let render_object =
            fitted_box.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.fit(), BoxFit::Cover);
        assert_eq!(render_object.alignment(), Alignment::TOP_LEFT);
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn update_render_object_applies_a_changed_fit_alignment_and_clip() {
        let mut render_object =
            FittedBox::new().create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.fit(), BoxFit::Contain);
        assert_eq!(render_object.alignment(), Alignment::CENTER);
        assert_eq!(render_object.clip_behavior(), Clip::None);

        let updated = FittedBox::new()
            .fit(BoxFit::Fill)
            .alignment(Alignment::BOTTOM_RIGHT)
            .clip(Clip::HardEdge);
        updated.update_render_object(
            &flui_view::RenderObjectContext::detached(),
            &mut render_object,
        );

        assert_eq!(render_object.fit(), BoxFit::Fill);
        assert_eq!(render_object.alignment(), Alignment::BOTTOM_RIGHT);
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        let empty = FittedBox::new();
        assert!(!empty.has_children());

        let with_child = FittedBox::new().child(SizedBox::shrink());
        assert!(with_child.has_children());
    }

    #[test]
    fn debug_reports_fit_and_child_presence() {
        let fitted_box = FittedBox::new().fit(BoxFit::Cover);
        let debug = format!("{fitted_box:?}");
        assert!(
            debug.contains("fit: Cover") && debug.contains("has_child: false"),
            "Debug output must report fit and child presence, got: {debug}",
        );
    }

    #[test]
    fn visit_child_views_invokes_the_visitor_once_with_a_child_set() {
        let fitted_box = FittedBox::new().child(SizedBox::shrink());
        let mut visited = 0;
        fitted_box.visit_child_views(&mut |_| visited += 1);
        assert_eq!(
            visited, 1,
            "visitor must run exactly once for a single child"
        );
    }

    #[test]
    fn visit_child_views_does_not_invoke_the_visitor_without_a_child() {
        let fitted_box = FittedBox::new();
        let mut visited = 0;
        fitted_box.visit_child_views(&mut |_| visited += 1);
        assert_eq!(visited, 0, "no child -> visitor must not run");
    }
}
