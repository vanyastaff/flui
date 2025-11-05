//! RenderBaseline - aligns child based on baseline

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::SingleRender;
use flui_engine::{BoxedLayer, TransformLayer};
use flui_types::{Offset, Size, constraints::BoxConstraints, typography::TextBaseline};

/// RenderObject that positions child based on baseline
///
/// This is used for aligning text and other widgets along a common baseline.
/// The baseline is measured from the top of the widget.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderBaseline;
///
/// // Align child with alphabetic baseline at 20 pixels from top
/// let mut baseline = RenderBaseline::alphabetic(20.0);
/// ```
#[derive(Debug)]
pub struct RenderBaseline {
    /// Distance from top to baseline
    pub baseline: f32,
    /// Type of baseline
    pub baseline_type: TextBaseline,
}

// ===== Public API =====

impl RenderBaseline {
    /// Create new RenderBaseline
    pub fn new(baseline: f32, baseline_type: TextBaseline) -> Self {
        Self {
            baseline,
            baseline_type,
        }
    }

    /// Create with alphabetic baseline
    pub fn alphabetic(baseline: f32) -> Self {
        Self::new(baseline, TextBaseline::Alphabetic)
    }

    /// Create with ideographic baseline
    pub fn ideographic(baseline: f32) -> Self {
        Self::new(baseline, TextBaseline::Ideographic)
    }

    /// Set new baseline
    pub fn set_baseline(&mut self, baseline: f32) {
        self.baseline = baseline;
    }

    /// Set new baseline type
    pub fn set_baseline_type(&mut self, baseline_type: TextBaseline) {
        self.baseline_type = baseline_type;
    }
}

// ===== RenderObject Implementation =====

impl SingleRender for RenderBaseline {
    /// No metadata needed
    type Metadata = ();

    fn layout(
        &mut self,
        tree: &ElementTree,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        // Layout child with same constraints
        let child_size = tree.layout_child(child_id, constraints);

        // Our height includes space above baseline and child height
        // For simplicity, we use child height + baseline offset
        Size::new(
            child_size.width,
            (child_size.height + self.baseline).max(child_size.height),
        )
    }

    fn paint(&self, tree: &ElementTree, child_id: ElementId, offset: Offset) -> BoxedLayer {
        // Capture child layer with baseline offset
        let child_layer = tree.paint_child(child_id, offset);

        // Apply baseline offset using TransformLayer
        let offset = Offset::new(0.0, self.baseline);
        Box::new(TransformLayer::translate(child_layer, offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_baseline_types() {
        assert_ne!(TextBaseline::Alphabetic, TextBaseline::Ideographic);
    }

    #[test]
    fn test_render_baseline_new() {
        let baseline = RenderBaseline::alphabetic(20.0);
        assert_eq!(baseline.baseline, 20.0);
        assert_eq!(baseline.baseline_type, TextBaseline::Alphabetic);
    }

    #[test]
    fn test_render_baseline_set_baseline() {
        let mut baseline = RenderBaseline::alphabetic(20.0);

        baseline.set_baseline(30.0);
        assert_eq!(baseline.baseline, 30.0);
    }

    #[test]
    fn test_render_baseline_set_baseline_type() {
        let mut baseline = RenderBaseline::alphabetic(20.0);

        baseline.set_baseline_type(TextBaseline::Ideographic);
        assert_eq!(baseline.baseline_type, TextBaseline::Ideographic);
    }
}
