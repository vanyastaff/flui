//! [`ClipOval`] — clips its child to the oval inscribed in its bounds.

use flui_objects::RenderClipOval;
use flui_rendering::protocol::BoxProtocol;
use flui_types::painting::Clip;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Clips its child to the axis-aligned oval inscribed in this widget's bounds
/// (a circle when the bounds are square — the common avatar case).
///
/// Flutter parity: `widgets/basic.dart` `ClipOval` over `RenderClipOval`.
/// Layout is a pass-through; only painting is clipped. `clip_behavior` defaults
/// to [`Clip::AntiAlias`] (Flutter's `ClipOval` default — smooth edges).
#[derive(Clone, Debug)]
pub struct ClipOval {
    clip_behavior: Clip,
    child: Child,
}

impl Default for ClipOval {
    fn default() -> Self {
        Self {
            clip_behavior: Clip::AntiAlias,
            child: Child::empty(),
        }
    }
}

impl ClipOval {
    /// Create an oval clip with Flutter's default `AntiAlias` behavior.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the clip behavior (anti-aliasing / save-layer policy).
    #[must_use]
    pub fn clip_behavior(mut self, clip_behavior: Clip) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Set the clipped child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for ClipOval {
    type Protocol = BoxProtocol;
    type RenderObject = RenderClipOval;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderClipOval::new(self.clip_behavior)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_clip_behavior(self.clip_behavior);
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

impl_render_view!(ClipOval);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    #[test]
    fn create_render_object_defaults_to_anti_alias() {
        let render_object = ClipOval::new().create_render_object();
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
    }

    #[test]
    fn create_render_object_applies_an_overridden_clip_behavior() {
        let render_object = ClipOval::new()
            .clip_behavior(Clip::HardEdge)
            .create_render_object();
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn update_render_object_applies_a_changed_clip_behavior() {
        let mut render_object = ClipOval::new().create_render_object();
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);

        ClipOval::new()
            .clip_behavior(Clip::HardEdge)
            .update_render_object(&mut render_object);

        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn update_render_object_is_idempotent_for_an_unchanged_clip_behavior() {
        let mut render_object = ClipOval::new()
            .clip_behavior(Clip::HardEdge)
            .create_render_object();

        ClipOval::new()
            .clip_behavior(Clip::HardEdge)
            .update_render_object(&mut render_object);

        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!ClipOval::new().has_children());
        assert!(ClipOval::new().child(SizedBox::shrink()).has_children());
    }

    #[test]
    fn default_matches_new() {
        let defaulted = ClipOval::default();
        let constructed = ClipOval::new();
        assert_eq!(defaulted.clip_behavior, constructed.clip_behavior);
        assert!(!defaulted.has_children());
        assert!(!constructed.has_children());
    }

    #[test]
    fn debug_reports_clip_behavior_and_type_name() {
        let debug = format!("{:?}", ClipOval::new().clip_behavior(Clip::None));
        assert!(
            debug.contains("ClipOval"),
            "Debug output must name the type, got: {debug}",
        );
        assert!(
            debug.contains("clip_behavior: None"),
            "Debug output must include clip_behavior, got: {debug}",
        );
    }

    #[test]
    fn debug_reports_has_child_flag() {
        let without_child = format!("{:?}", ClipOval::new());
        let with_child = format!("{:?}", ClipOval::new().child(SizedBox::shrink()));
        assert!(
            without_child.contains("has_child: false"),
            "got: {without_child}",
        );
        assert!(with_child.contains("has_child: true"), "got: {with_child}");
    }
}
