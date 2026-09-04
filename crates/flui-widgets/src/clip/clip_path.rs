//! [`ClipPath`] — clips its child to an arbitrary [`Path`] computed from the
//! child's bounds.

use std::rc::Rc;

use flui_objects::{ClipSourceToken, RenderClipPath};
use flui_rendering::protocol::BoxProtocol;
use flui_types::Size;
use flui_types::painting::{Clip, Path};
use flui_view::{Child, IntoView, RenderView, impl_render_view};

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
    clip_source_token: ClipSourceToken,
    clip_behavior: Clip,
    child: Child,
}

impl ClipPath {
    /// Clip to the path returned by `clipper` for the laid-out size, with
    /// Flutter's default anti-aliased clip behavior.
    ///
    /// # Repaint identity
    ///
    /// **Each call mints a NEW clip identity, and that costs a repaint.**
    /// Rust cannot compare two closures, so the render object is told the clip
    /// changed whenever the identity does — and under the ordinary pattern of
    /// building a view fresh on every rebuild, that is every frame the
    /// surrounding tree rebuilds. `RenderClip` has no cache of its own, so the
    /// cost is a full repaint of the clipped subtree, not a cheap
    /// invalidation.
    ///
    /// Two ways to avoid it, in order of preference:
    ///
    /// * Build the `ClipPath` once and `clone()` it. A clone shares the
    ///   identity, so an update reports no impact.
    /// * Hold a [`ClipSourceToken`] alongside your own state and pass it to
    ///   [`with_source`](Self::with_source), which reuses it. Use this when
    ///   the closure must be rebuilt (it captures changing state) but the clip
    ///   it produces has not actually changed.
    ///
    /// Flutter has the same problem and forces the answer: `CustomClipper`'s
    /// `shouldReclip` is abstract, so every clipper author must decide. This
    /// is the same decision, made visible at the call site instead.
    pub fn new(clipper: impl Fn(Size) -> Path + 'static) -> Self {
        Self::with_source(ClipSourceToken::fresh(), clipper)
    }

    /// Like [`new`](Self::new), but reuses an existing clip identity.
    ///
    /// Supplying the same token across rebuilds tells the render object the
    /// clip is unchanged, so it does not repaint. Supplying a fresh one says
    /// it changed. See [`new`](Self::new)'s *Repaint identity* section.
    pub fn with_source(source: ClipSourceToken, clipper: impl Fn(Size) -> Path + 'static) -> Self {
        Self {
            clipper: Rc::new(clipper),
            clip_source_token: source,
            clip_behavior: Clip::AntiAlias,
            child: Child::empty(),
        }
    }

    /// This clip's source identity, for reuse across a rebuild.
    #[must_use]
    pub fn source(&self) -> ClipSourceToken {
        self.clip_source_token.clone()
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

    /// Installs this widget's closure in the owner lane, and reports what the
    /// render object must invalidate.
    ///
    /// The two are INDEPENDENT, and conflating them was a bug: the closure is
    /// always the live one, while `identity_changed` says only whether the
    /// clip is considered different. A rebuild that reuses a token to avoid a
    /// repaint — the documented way to avoid the per-rebuild cost — still
    /// carries a NEW closure allocation, which may capture different state.
    /// Skipping the install there left the render object invoking the previous
    /// widget's closure for every later paint and hit test.
    fn sync_path_clip_target(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut RenderClipPath,
        identity_changed: bool,
    ) -> flui_rendering::RenderUpdateImpact {
        let clipper = Rc::clone(&self.clipper);
        match render_object.path_clip_target() {
            Some(target) => {
                if let Err(error) = ctx.replace_path_clipper(target, move |size| clipper(size)) {
                    tracing::warn!(?error, "ClipPath clipper replacement failed");
                }
                if identity_changed {
                    render_object.set_path_clip_target(Some(target))
                } else {
                    // Same clip, new closure: nothing to invalidate. The
                    // install above is what makes reusing an identity safe
                    // rather than merely cheap.
                    flui_rendering::RenderUpdateImpact::NONE
                }
            }
            None => match ctx.register_path_clipper(move |size| clipper(size)) {
                Ok(target) => render_object.set_path_clip_target(Some(target)),
                Err(error) => {
                    tracing::debug!(
                        ?error,
                        "ClipPath mounted without an active interaction lane; \
                         custom path clipper will not be resolved"
                    );
                    flui_rendering::RenderUpdateImpact::NONE
                }
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
        let mut render_object = RenderClipPath::new(self.clip_behavior)
            .with_path_clip_source_token(self.clip_source_token.clone());
        // Creation is not a mounted update; the initial target is already
        // reflected in the new render object's first frame.
        let _initial_target_impact = self.sync_path_clip_target(ctx, &mut render_object, false);
        render_object
    }

    fn update_render_object(
        &self,
        ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_clip_behavior(self.clip_behavior);
        let source_impact = render_object.set_path_clip_source_token(&self.clip_source_token);
        impact |= source_impact;
        impact |= self.sync_path_clip_target(ctx, render_object, !source_impact.is_none());
        impact
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
            // Unmount has no owner-side update application; unregistering
            // removes the target before the render node is disposed.
            let _unmount_target_impact = render_object.set_path_clip_target(None);
        }
    }

    flui_view::single_child_view_children!();
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
        let widget = clip_path();
        let mut render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(render_object.clip_behavior(), Clip::AntiAlias);

        let impact = widget
            .clone()
            .clip_behavior(Clip::HardEdge)
            .update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            );
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::PAINT);

        assert_eq!(render_object.clip_behavior(), Clip::HardEdge);
        assert!(!render_object.has_custom_clipper());
    }

    #[test]
    fn clipper_source_token_distinguishes_cloned_and_separate_updates() {
        let widget = clip_path();
        let mut render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());
        assert_eq!(
            widget.clone().update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        assert_eq!(
            clip_path().update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::PAINT
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
        );
    }

    /// A rebuilt `ClipPath` that reuses its source reports NO impact, even
    /// though its closure is a different allocation.
    ///
    /// This is the escape hatch from the cost the sibling test above
    /// demonstrates: `ClipPath::new` mints a fresh identity, so under the
    /// ordinary pattern of building a view fresh each rebuild it repaints the
    /// clipped subtree every frame. `with_source` says "same clip, new
    /// closure" — the answer Rust cannot derive because closures do not
    /// compare, and the one Flutter forces every `CustomClipper` author to
    /// give through an abstract `shouldReclip`.
    #[test]
    fn a_reused_source_reports_no_impact_across_a_rebuild() {
        let widget = clip_path();
        let source = widget.source();
        let mut render_object =
            widget.create_render_object(&flui_view::RenderObjectContext::detached());

        // A different closure allocation, the same declared identity.
        let rebuilt = ClipPath::with_source(source, |size| {
            let mut path = Path::new();
            path.add_rect(flui_types::geometry::Rect::from_origin_size(
                flui_types::Point::ZERO,
                size,
            ));
            path
        });

        assert_eq!(
            rebuilt.update_render_object(
                &flui_view::RenderObjectContext::detached(),
                &mut render_object,
            ),
            flui_rendering::RenderUpdateImpact::NONE,
            "reusing the source must not repaint — that is the whole point of \
             being able to supply one",
        );
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
