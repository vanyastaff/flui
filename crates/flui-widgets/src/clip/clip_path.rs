//! [`ClipPath`] — clips its child to an arbitrary [`Path`] computed from the
//! child's bounds.

use std::rc::Rc;

use flui_foundation::geometry::Size;
use flui_objects::{ClipSourceToken, RenderClipPath};
use flui_painting::paint::{Clip, Path};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// The user-supplied clip-shape function: maps the laid-out box size to the
/// [`Path`] to clip against. It is owner-local under ADR-0027; render storage
/// receives only a data-plane target token.
type PathClipper = Rc<dyn Fn(Size) -> Path>;

/// Clips its child to a custom [`Path`] derived from the child's size.
///
/// The path factory is a closure `Fn(Size) -> Path`.
/// Layout is a pass-through — only painting is clipped. `clip_behavior`
/// defaults to [`Clip::AntiAlias`].
#[derive(Clone)]
pub struct ClipPath {
    clipper: PathClipper,
    clip_source_token: ClipSourceToken,
    clip_behavior: Clip,
    child: Child,
}

impl ClipPath {
    /// Clip to the path returned by `clipper` for the laid-out size, with
    /// the default anti-aliased clip behavior.
    ///
    /// # Clip identity
    ///
    /// **Each call mints a NEW clip identity, and that costs an invalidation.**
    /// Rust cannot compare two closures, so the render object is told the clip
    /// changed whenever the identity does — and under the ordinary pattern of
    /// building a view fresh on every rebuild, that is every frame the
    /// surrounding tree rebuilds. Under a retained repaint boundary the cost
    /// is a composited-layer update: the clip layer is rebuilt in place and
    /// the clipped subtree is not repainted. Without a retained capture to
    /// patch (the first frame, or a boundary that is repainting anyway) it is
    /// a full repaint of the subtree.
    ///
    /// Two ways to avoid even that, in order of preference:
    ///
    /// * Build the `ClipPath` once and `clone()` it. A clone shares the
    ///   identity, so an update reports no impact.
    /// * Hold a [`ClipSourceToken`] alongside your own state and pass it to
    ///   [`with_source`](Self::with_source), which reuses it. Use this when
    ///   the closure must be rebuilt (it captures changing state) but the clip
    ///   it produces has not actually changed.
    ///
    /// The caller has to decide this, so the decision is made visible at the
    /// call site.
    pub fn new(clipper: impl Fn(Size) -> Path + 'static) -> Self {
        Self::with_source(ClipSourceToken::fresh(), clipper)
    }

    /// Like [`new`](Self::new), but reuses an existing clip identity.
    ///
    /// Supplying the same token across rebuilds tells the render object the
    /// clip is unchanged, so nothing is invalidated. Supplying a fresh one
    /// says it changed. See [`new`](Self::new)'s *Clip identity* section.
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
    /// clip is considered different. A rebuild that reuses a token to avoid an
    /// invalidation — the documented way to avoid the per-rebuild cost — still
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
