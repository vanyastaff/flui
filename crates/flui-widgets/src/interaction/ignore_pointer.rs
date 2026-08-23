//! [`IgnorePointer`] — makes its subtree invisible to hit-testing.

use flui_objects::RenderIgnorePointer;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// During hit-testing, makes its child (and subtree) invisible to pointer
/// events while still laying it out and painting it.
///
/// Flutter parity: `widgets/basic.dart` `IgnorePointer` over
/// `RenderIgnorePointer`. `ignoring` defaults to `true`.
#[derive(Clone, Debug)]
pub struct IgnorePointer {
    ignoring: bool,
    child: Child,
}

impl Default for IgnorePointer {
    fn default() -> Self {
        Self {
            ignoring: true,
            child: Child::empty(),
        }
    }
}

impl IgnorePointer {
    /// Create an `IgnorePointer` that ignores pointer events (`ignoring = true`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether pointer events are ignored.
    #[must_use]
    pub fn ignoring(mut self, ignoring: bool) -> Self {
        self.ignoring = ignoring;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for IgnorePointer {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIgnorePointer;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIgnorePointer::new(self.ignoring)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_ignoring(self.ignoring);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(IgnorePointer);
