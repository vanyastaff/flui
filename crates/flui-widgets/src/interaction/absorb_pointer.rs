//! [`AbsorbPointer`] — absorbs pointer events, stopping its subtree from being
//! hit while preventing widgets behind it from being hit too.

use flui_objects::RenderAbsorbPointer;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

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

    fn create_render_object(&self) -> Self::RenderObject {
        RenderAbsorbPointer::new(self.absorbing)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_absorbing(self.absorbing);
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

impl_render_view!(AbsorbPointer);
