//! [`ClipPath`] — clips its child to an arbitrary [`Path`] computed from the
//! child's bounds.

use std::sync::Arc;

use flui_objects::RenderClipPath;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Size;
use flui_types::painting::{Clip, Path};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// The user-supplied clip-shape function: maps the laid-out box size to the
/// [`Path`] to clip against. `Arc` so [`ClipPath`] stays `Clone` (a view is
/// re-cloned on every rebuild) and `Send + Sync` for the view bounds.
type PathClipper = Arc<dyn Fn(Size) -> Path + Send + Sync>;

/// Clips its child to a custom [`Path`] derived from the child's size.
///
/// Flutter parity: `widgets/basic.dart` `ClipPath` over `RenderClipPath`, with a
/// `CustomClipper<Path>` supplied as a closure `Fn(Size) -> Path`. Layout is a
/// pass-through — only painting is clipped. `clip_behavior` defaults to
/// [`Clip::AntiAlias`] (Flutter's `ClipPath` default).
#[derive(Clone)]
pub struct ClipPath {
    clipper: PathClipper,
    clip_behavior: Clip,
    child: Child,
}

impl ClipPath {
    /// Clip to the path returned by `clipper` for the laid-out size, with
    /// Flutter's default anti-aliased clip behavior.
    pub fn new(clipper: impl Fn(Size) -> Path + Send + Sync + 'static) -> Self {
        Self {
            clipper: Arc::new(clipper),
            clip_behavior: Clip::AntiAlias,
            child: Child::empty(),
        }
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

impl std::fmt::Debug for ClipPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipPath")
            .field("clip_behavior", &self.clip_behavior)
            .finish_non_exhaustive()
    }
}

impl RenderView for ClipPath {
    type Protocol = BoxProtocol;
    type RenderObject = RenderClipPath;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let clipper = Arc::clone(&self.clipper);
        RenderClipPath::new(self.clip_behavior).with_clipper(move |size| clipper(size))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_clip_behavior(self.clip_behavior);
        // The clipper is a closure with no identity to diff (Flutter compares via
        // `CustomClipper.shouldReclip`; the closure-based render clipper cannot),
        // so the latest closure is always reinstalled — the next paint reads it.
        let clipper = Arc::clone(&self.clipper);
        render_object.set_clipper(Some(move |size| clipper(size)));
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

impl_render_view!(ClipPath);

#[cfg(test)]
mod tests {
    use flui_view::RenderView;

    use super::*;
    use crate::SizedBox;

    fn clip_path() -> ClipPath {
        ClipPath::new(|_size: Size| Path::new())
    }

    #[test]
    fn create_render_object_defaults_to_anti_alias_and_installs_the_clipper() {
        let render_object =
            clip_path().create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
        assert!(
            render_object.has_custom_clipper(),
            "the clipper closure must always be installed on create",
        );
    }

    #[test]
    fn create_render_object_applies_an_overridden_clip_behavior() {
        let render_object = clip_path()
            .clip_behavior(Clip::HardEdge)
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn update_render_object_applies_a_changed_clip_behavior_and_reinstalls_the_clipper() {
        let mut render_object =
            clip_path().create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);

        clip_path()
            .clip_behavior(Clip::HardEdge)
            .update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            );

        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
        assert!(
            render_object.has_custom_clipper(),
            "update must reinstall the (identity-less) clipper closure",
        );
    }

    #[test]
    fn debug_reports_clip_behavior() {
        let debug = format!("{:?}", clip_path().clip_behavior(Clip::None));
        assert!(
            debug.contains("clip_behavior: None"),
            "Debug output must include clip_behavior, got: {debug}",
        );
    }

    #[test]
    fn has_children_reflects_whether_a_child_was_set() {
        assert!(!clip_path().has_children());
        assert!(clip_path().child(SizedBox::shrink()).has_children());
    }
}
