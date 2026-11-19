//! [`SliverOpacity`] — fades a sliver child within a scroll viewport.

use flui_objects::RenderSliverOpacity;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Makes a **sliver** child partially transparent inside a
/// [`Viewport`](crate::Viewport).
///
/// `opacity` is clamped to `0.0..=1.0`. At `0.0` the child's semantics are
/// left out, unless [`always_include_semantics`](Self::always_include_semantics)
/// keeps them.
#[derive(Clone, Debug)]
pub struct SliverOpacity {
    opacity: f64,
    always_include_semantics: bool,
    child: Child,
}

impl SliverOpacity {
    /// Create a `SliverOpacity` with the given opacity (clamped to `0.0..=1.0`).
    pub fn new(opacity: f64) -> Self {
        Self {
            opacity,
            always_include_semantics: false,
            child: Child::empty(),
        }
    }

    /// Publish the child's semantics at opacity `0.0` as well, so assistive
    /// technology still reaches a sliver that is invisible on screen. Off by
    /// default.
    #[must_use]
    pub fn always_include_semantics(mut self, include: bool) -> Self {
        self.always_include_semantics = include;
        self
    }

    /// Set the faded sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverOpacity {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverOpacity;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        let mut render_object = RenderSliverOpacity::new(self.opacity);
        let _ = render_object.set_always_include_semantics(self.always_include_semantics);
        render_object
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_opacity(self.opacity)
            | render_object.set_always_include_semantics(self.always_include_semantics)
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(SliverOpacity);
