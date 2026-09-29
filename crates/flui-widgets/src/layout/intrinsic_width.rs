//! [`IntrinsicWidth`] — sizes child to its maximum intrinsic width.

use flui_objects::RenderIntrinsicWidth;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView, impl_render_view};

/// Sizes its child to the child's maximum intrinsic width.
///
/// Useful when a widget should be exactly as wide as its natural content
/// rather than merely constrained by it.  The optional `step_width` and
/// `step_height` parameters round values up to a size grid, reducing relayout
/// churn in dynamic lists where adjacent items should snap to a common width.
///
/// Flutter parity: `widgets/basic.dart` `IntrinsicWidth` over
/// [`RenderIntrinsicWidth`].
#[derive(Clone, Debug)]
pub struct IntrinsicWidth {
    /// Optional column-width quantum; intrinsic width is rounded up to the
    /// nearest multiple.
    step_width: Option<f64>,
    /// Optional row-height quantum; the height passed to the intrinsic-width
    /// query is rounded up to the nearest multiple before querying.
    step_height: Option<f64>,
    child: Child,
}

impl IntrinsicWidth {
    /// Creates the widget with no step snapping.
    pub fn new() -> Self {
        Self {
            step_width: None,
            step_height: None,
            child: Child::empty(),
        }
    }

    /// Sets the column-width quantum (rounds intrinsic width up to a multiple).
    #[must_use]
    pub fn with_step_width(mut self, step_width: f64) -> Self {
        self.step_width = Some(step_width);
        self
    }

    /// Sets the row-height quantum (rounds height-for-width-query up to a multiple).
    #[must_use]
    pub fn with_step_height(mut self, step_height: f64) -> Self {
        self.step_height = Some(step_height);
        self
    }

    /// Sets the child view.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl Default for IntrinsicWidth {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderView for IntrinsicWidth {
    type Protocol = BoxProtocol;
    type RenderObject = RenderIntrinsicWidth;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderIntrinsicWidth::new(self.step_width, self.step_height)
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        let mut impact = flui_rendering::RenderUpdateImpact::NONE;
        impact |= render_object.set_step_width(self.step_width);
        impact |= render_object.set_step_height(self.step_height);
        impact
    }

    flui_view::single_child_view_children!();
}

impl_render_view!(IntrinsicWidth);
