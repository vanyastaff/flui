//! [`SliverPadding`] — insets a sliver child within a scroll viewport.

use flui_geometry::{EdgeInsets, px};
use flui_objects::RenderSliverPadding;
use flui_rendering::protocol::SliverProtocol;
use flui_view::{Child, IntoView, RenderView, View, impl_render_view};

/// Applies padding around a **sliver** child inside a
/// [`Viewport`](crate::Viewport).
///
/// Flutter parity: `widgets/sliver.dart` `SliverPadding` over
/// `RenderSliverPadding`. Its child is itself a sliver (e.g. a
/// [`SliverFixedExtentList`](crate::SliverFixedExtentList)).
#[derive(Clone, Debug)]
pub struct SliverPadding {
    padding: EdgeInsets,
    child: Child,
}

impl SliverPadding {
    /// Pad a sliver child by explicit [`EdgeInsets`].
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            child: Child::empty(),
        }
    }

    /// Uniform padding on all four sides.
    pub fn all(value: f32) -> Self {
        Self::new(EdgeInsets::all(px(value)))
    }

    /// Set the padded sliver child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl RenderView for SliverPadding {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverPadding;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSliverPadding::new(self.padding)
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        render_object.set_padding(self.padding);
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

impl_render_view!(SliverPadding);
