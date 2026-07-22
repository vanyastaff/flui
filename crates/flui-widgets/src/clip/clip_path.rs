//! [`ClipPath`] — clips its child to an arbitrary [`Path`] computed from the
//! child's bounds.

use std::rc::Rc;

use flui_objects::RenderClipPath;
use flui_rendering::protocol::BoxProtocol;
use flui_types::Size;
use flui_types::painting::{Clip, Path};
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// The user-supplied clip-shape function: maps the laid-out box size to the
/// [`Path`] to clip against. It is owner-local under ADR-0027; render storage
/// receives only a data-plane target token.
type PathClipper = Rc<dyn Fn(Size) -> Path>;

/// Clips its child to a custom [`Path`] derived from the child's size.
///
/// Flutter parity: `widgets/basic.dart` `ClipPath` over `RenderClipPath`, with
/// an owner-local path factory supplied as a closure `Fn(Size) -> Path`.
/// Layout is a pass-through — only painting is clipped. `clip_behavior`
/// defaults to [`Clip::AntiAlias`] (Flutter's `ClipPath` default).
#[derive(Clone)]
pub struct ClipPath {
    clipper: PathClipper,
    clip_behavior: Clip,
    child: Child,
}

impl ClipPath {
    /// Clip to the path returned by `clipper` for the laid-out size, with
    /// Flutter's default anti-aliased clip behavior.
    pub fn new(clipper: impl Fn(Size) -> Path + 'static) -> Self {
        Self {
            clipper: Rc::new(clipper),
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

    fn sync_path_clip_target(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderClipPath,
    ) {
        let clipper = Rc::clone(&self.clipper);
        match render_object.path_clip_target() {
            Some(target) => {
                if let Err(error) = ctx.replace_path_clipper(target, move |size| clipper(size)) {
                    tracing::warn!(?error, "ClipPath clipper replacement failed");
                }
            }
            None => match ctx.register_path_clipper(move |size| clipper(size)) {
                Ok(target) => render_object.set_path_clip_target(Some(target)),
                Err(error) => tracing::debug!(
                    ?error,
                    "ClipPath mounted without an active interaction lane; \
                     custom path clipper will not be resolved"
                ),
            },
        }
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

    fn create_render_object(&self, ctx: &flui_view::RenderObjectContext<'_>) -> Self::RenderObject {
        let mut render_object = RenderClipPath::new(self.clip_behavior);
        self.sync_path_clip_target(ctx, &mut render_object);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        render_object.set_clip_behavior(self.clip_behavior);
        self.sync_path_clip_target(ctx, render_object);
    }

    fn did_unmount_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
        if let Some(target) = render_object.path_clip_target() {
            if let Err(error) = ctx.unregister_path_clipper(target) {
                tracing::debug!(?error, "ClipPath clipper unregistration failed");
            }
            render_object.set_path_clip_target(None);
        }
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
    fn create_render_object_defaults_to_anti_alias() {
        let render_object =
            clip_path().create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);
        assert!(!render_object.has_custom_clipper());
    }

    #[test]
    fn create_render_object_applies_an_overridden_clip_behavior() {
        let render_object = clip_path()
            .clip_behavior(Clip::HardEdge)
            .create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
    }

    #[test]
    fn update_render_object_applies_a_changed_clip_behavior() {
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
        assert!(!render_object.has_custom_clipper());
    }

    #[test]
    fn clipper_accepts_owner_local_rc_state() {
        use std::cell::Cell;
        use std::rc::Rc;

        let total = Rc::new(Cell::new(0));
        let captured = Rc::clone(&total);

        let widget = ClipPath::new(move |_size: Size| {
            captured.set(captured.get() + 1);
            Path::new()
        });

        let _ = widget.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(
            total.get(),
            0,
            "detached render-object creation must not invoke or store the owner-local clipper"
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
