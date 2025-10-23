//! RenderBaseline - aligns child based on baseline

use flui_types::{Offset, Size, constraints::BoxConstraints};
use flui_core::DynRenderObject;
use crate::core::{SingleRenderBox, RenderBoxMixin};

/// Baseline type for text alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBaseline {
    /// Alphabetic baseline (most common for Latin scripts)
    Alphabetic,
    /// Ideographic baseline (used for CJK scripts)
    Ideographic,
}

/// Data for RenderBaseline
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaselineData {
    /// Distance from top to baseline
    pub baseline: f32,
    /// Type of baseline
    pub baseline_type: TextBaseline,
}

impl BaselineData {
    /// Create new baseline data
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
}

/// RenderObject that positions child based on baseline
///
/// This is used for aligning text and other widgets along a common baseline.
/// The baseline is measured from the top of the widget.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{SingleRenderBox, objects::layout::{BaselineData, TextBaseline}};
///
/// // Align child with alphabetic baseline at 20 pixels from top
/// let mut baseline = SingleRenderBox::new(BaselineData::alphabetic(20.0));
/// ```
pub type RenderBaseline = SingleRenderBox<BaselineData>;

// ===== Public API =====

impl RenderBaseline {
    /// Get the baseline distance
    pub fn baseline(&self) -> f32 {
        self.data().baseline
    }

    /// Get the baseline type
    pub fn baseline_type(&self) -> TextBaseline {
        self.data().baseline_type
    }

    /// Set new baseline
    pub fn set_baseline(&mut self, baseline: f32) {
        if self.data().baseline != baseline {
            self.data_mut().baseline = baseline;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }

    /// Set new baseline type
    pub fn set_baseline_type(&mut self, baseline_type: TextBaseline) {
        if self.data().baseline_type != baseline_type {
            self.data_mut().baseline_type = baseline_type;
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderBaseline {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let baseline = self.data().baseline;

        // Layout child with same constraints
        let size = if let Some(child) = self.child_mut() {
            let child_size = child.layout(constraints);

            // Our height includes space above baseline and child height
            // For simplicity, we use child height + baseline offset
            Size::new(
                child_size.width,
                (child_size.height + baseline).max(child_size.height),
            )
        } else {
            // No child - use smallest size
            constraints.smallest()
        };

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        // Paint child offset by baseline
        if let Some(child) = self.child() {
            let baseline = self.data().baseline;

            // Offset child by baseline distance
            let paint_offset = Offset::new(offset.dx, offset.dy + baseline);

            child.paint(painter, paint_offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_baseline_types() {
        assert_ne!(TextBaseline::Alphabetic, TextBaseline::Ideographic);
    }

    #[test]
    fn test_baseline_data_new() {
        let data = BaselineData::new(20.0, TextBaseline::Alphabetic);
        assert_eq!(data.baseline, 20.0);
        assert_eq!(data.baseline_type, TextBaseline::Alphabetic);
    }

    #[test]
    fn test_baseline_data_alphabetic() {
        let data = BaselineData::alphabetic(15.0);
        assert_eq!(data.baseline, 15.0);
        assert_eq!(data.baseline_type, TextBaseline::Alphabetic);
    }

    #[test]
    fn test_baseline_data_ideographic() {
        let data = BaselineData::ideographic(25.0);
        assert_eq!(data.baseline, 25.0);
        assert_eq!(data.baseline_type, TextBaseline::Ideographic);
    }

    #[test]
    fn test_render_baseline_new() {
        let baseline = SingleRenderBox::new(BaselineData::alphabetic(20.0));
        assert_eq!(baseline.baseline(), 20.0);
        assert_eq!(baseline.baseline_type(), TextBaseline::Alphabetic);
    }

    #[test]
    fn test_render_baseline_set_baseline() {
        let mut baseline = SingleRenderBox::new(BaselineData::alphabetic(20.0));

        baseline.set_baseline(30.0);
        assert_eq!(baseline.baseline(), 30.0);
        assert!(RenderBoxMixin::needs_layout(&baseline));
    }

    #[test]
    fn test_render_baseline_set_baseline_type() {
        let mut baseline = SingleRenderBox::new(BaselineData::alphabetic(20.0));

        baseline.set_baseline_type(TextBaseline::Ideographic);
        assert_eq!(baseline.baseline_type(), TextBaseline::Ideographic);
        assert!(RenderBoxMixin::needs_layout(&baseline));
    }

    #[test]
    fn test_render_baseline_layout() {
        let mut baseline = SingleRenderBox::new(BaselineData::alphabetic(20.0));
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = baseline.layout(constraints);

        // Should use smallest size (no child)
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
