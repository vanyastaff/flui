//! Cross-axis sliver limits through the normal sliver protocol.

use flui_objects::{InvalidSliverCrossAxisExtent, RenderSliverConstrainedCrossAxis};
use flui_rendering::protocol::SliverProtocol;
use flui_view::{BoxedView, IntoView, View, ViewExt};

/// Limits cross-axis size while retaining the child's scrolling protocol.
#[derive(Clone, Debug)]
pub struct SliverConstrainedCrossAxis {
    configuration: RenderSliverConstrainedCrossAxis,
    child: BoxedView,
}

impl SliverConstrainedCrossAxis {
    /// Creates a limit without shifting the child's local origin.
    ///
    /// # Errors
    /// Refuses negative or nonfinite limits.
    pub fn new(
        max_extent: f64,
        child: impl IntoView,
    ) -> Result<Self, InvalidSliverCrossAxisExtent> {
        Ok(Self {
            configuration: RenderSliverConstrainedCrossAxis::new(max_extent)?,
            child: child.into_view().boxed(),
        })
    }
}

impl flui_view::RenderView for SliverConstrainedCrossAxis {
    type Protocol = SliverProtocol;
    type RenderObject = RenderSliverConstrainedCrossAxis;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.configuration.clone()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render
            .set_max_extent(self.configuration.max_extent())
            .expect("BUG: widget configuration contains a validated cross-axis limit")
    }

    fn has_children(&self) -> bool {
        true
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        visitor(&self.child);
    }
}

flui_view::impl_render_view!(SliverConstrainedCrossAxis);
