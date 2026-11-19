//! [`Opacity`] — makes its child partially transparent.

use flui_objects::RenderOpacity;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, CreationSite, IntoView, RenderView, impl_render_view};

/// Makes its child partially transparent.
///
/// `opacity` is clamped to `0.0..=1.0`; `0.0` paints nothing (but the child is
/// still laid out and interactive unless wrapped in `IgnorePointer`). At `0.0`
/// the child's semantics are left out too, unless
/// [`always_include_semantics`](Self::always_include_semantics) keeps them.
#[derive(Clone)]
pub struct Opacity {
    alpha: f64,
    always_include_semantics: bool,
    child: Child,
    site: CreationSite,
}

impl std::fmt::Debug for Opacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opacity")
            .field("opacity", &self.alpha)
            .field("always_include_semantics", &self.always_include_semantics)
            .field("child", &self.child)
            .finish_non_exhaustive()
    }
}

impl Opacity {
    /// Create an `Opacity` with the given opacity (clamped to `0.0..=1.0`).
    #[track_caller]
    pub fn new(opacity: f64) -> Self {
        Self {
            alpha: opacity,
            always_include_semantics: false,
            child: Child::empty(),
            site: CreationSite::here(),
        }
    }

    /// Publish the child's semantics at opacity `0.0` as well, so assistive
    /// technology still reaches a child that is invisible on screen (a
    /// control faded out that stays operable). Off by default.
    #[must_use]
    pub fn always_include_semantics(mut self, include: bool) -> Self {
        self.always_include_semantics = include;
        self
    }

    /// Set the child to fade.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for Opacity {
    type Protocol = BoxProtocol;
    type RenderObject = RenderOpacity;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut render_object = RenderOpacity::new(self.alpha);
        let _ = render_object.set_always_include_semantics(self.always_include_semantics);
        render_object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_opacity(self.alpha);
        impact |= render_object.set_always_include_semantics(self.always_include_semantics);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(Opacity, site = site);
