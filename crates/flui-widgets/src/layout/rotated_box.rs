//! [`RotatedBox`] — rotates its child by a whole number of quarter turns.

use flui_objects::RenderRotatedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Rotates its child by a whole number of clockwise 90° turns.
///
/// Unlike [`Transform`](super::Transform), `RotatedBox` accounts for the
/// rotation during layout: odd turn counts (1, 3, −1, …) swap the width and
/// height axes so the child is laid out in a rotated coordinate frame.  The
/// parent sees the swapped dimensions.
///
/// Flutter parity: `widgets/basic.dart` `RotatedBox` over
/// [`RenderRotatedBox`].
#[derive(Clone, Debug)]
pub struct RotatedBox {
    /// Number of clockwise 90° rotations.  Negative values rotate
    /// counter-clockwise.  Any integer is accepted; the angle is reduced modulo
    /// 4 when constructing the paint matrix.
    quarter_turns: i32,
    child: Child,
}

impl RotatedBox {
    /// Creates the widget with the given clockwise quarter-turn count.
    pub fn new(quarter_turns: i32) -> Self {
        Self {
            quarter_turns,
            child: Child::empty(),
        }
    }

    /// Sets the child view.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for RotatedBox {
    type Protocol = BoxProtocol;
    type RenderObject = RenderRotatedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderRotatedBox::new(self.quarter_turns)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_quarter_turns(self.quarter_turns);
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

impl_render_view!(RotatedBox);
