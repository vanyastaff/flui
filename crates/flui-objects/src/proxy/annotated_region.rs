//! `RenderAnnotatedRegion` — attaches a value to its box in the composited
//! scene, for readers such as the presentation's overlay-style probe.

use std::{any::Any, fmt, sync::Arc};

use flui_foundation::Single;
use flui_rendering::{
    context::PaintCx, layer::AnnotationValue, parent_data::BoxParentData, traits::RenderBox,
};

/// A pass-through proxy whose box carries a type-erased annotation.
///
/// It paints an annotated region around its child, sized to its own box;
/// [`LayerTree::find_annotations`](flui_rendering::layer::LayerTree::find_annotations)
/// finds the value at points inside it. Layout and hit testing are the
/// child's. Without a child it annotates nothing.
#[derive(Clone)]
pub struct RenderAnnotatedRegion {
    value: AnnotationValue,
    has_child: bool,
}

impl RenderAnnotatedRegion {
    /// Annotates this box with `value`.
    pub fn new<T: Any + Send + Sync>(value: Arc<T>) -> Self {
        Self {
            value,
            has_child: false,
        }
    }

    /// The annotation, type-erased.
    #[inline]
    pub fn value(&self) -> &AnnotationValue {
        &self.value
    }

    /// Replaces the annotation. A value shared with the current one (the same
    /// `Arc`) is no change; anything else repaints the region.
    pub fn set_value<T: Any + Send + Sync>(
        &mut self,
        value: Arc<T>,
    ) -> flui_rendering::RenderUpdateImpact {
        let value: AnnotationValue = value;
        if Arc::ptr_eq(&self.value, &value) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.value = value;
        flui_rendering::RenderUpdateImpact::PAINT
    }
}

impl fmt::Debug for RenderAnnotatedRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderAnnotatedRegion")
            .field("value_type", &self.value.as_ref().type_id())
            .field("has_child", &self.has_child)
            .finish()
    }
}

impl flui_foundation::Diagnosticable for RenderAnnotatedRegion {}

impl RenderBox for RenderAnnotatedRegion {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    flui_rendering::forward_single_child_box_hit_test!();

    // The region is a layer, so it needs compositing whenever it has a child
    // to annotate.
    fn always_needs_compositing(&self) -> bool {
        self.has_child
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if ctx.child_count() == 0 {
            return;
        }
        ctx.with_annotated_region(Arc::clone(&self.value), |ctx| {
            ctx.paint_child();
        });
    }
}
