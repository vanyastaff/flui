//! [`AbsorbPointer`] — absorbs pointer events, stopping its subtree from being
//! hit while preventing widgets behind it from being hit too.

use flui_objects::RenderAbsorbPointer;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Absorbs pointer events: its subtree is not hit-tested, *and* (unlike
/// [`IgnorePointer`](crate::IgnorePointer)) it stops events from reaching
/// widgets visually behind it.
///
/// Flutter parity: `widgets/basic.dart` `AbsorbPointer` over
/// `RenderAbsorbPointer`. `absorbing` defaults to `true`.
#[derive(Clone, Debug)]
pub struct AbsorbPointer {
    absorbing: bool,
    child: Child,
}

impl Default for AbsorbPointer {
    fn default() -> Self {
        Self {
            absorbing: true,
            child: Child::empty(),
        }
    }
}

impl AbsorbPointer {
    /// Create an `AbsorbPointer` that absorbs pointer events (`absorbing = true`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether pointer events are absorbed.
    #[must_use]
    pub fn absorbing(mut self, absorbing: bool) -> Self {
        self.absorbing = absorbing;
        self
    }

    /// Set the child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for AbsorbPointer {
    type Protocol = BoxProtocol;
    type RenderObject = RenderAbsorbPointer;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderAbsorbPointer::new(self.absorbing)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_absorbing(self.absorbing);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(AbsorbPointer);
