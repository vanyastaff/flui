//! RenderBaseline - aligns child based on baseline
//!
//! Flutter equivalent: `RenderBaseline`
//! Source: https://api.flutter.dev/flutter/rendering/RenderBaseline-class.html

use crate::core::{BoxProtocol, LayoutContext, PaintContext};
use crate::core::{RenderBox, Single};
use flui_types::{layout::TextBaseline, Offset, Size};

/// RenderObject that positions child based on baseline
///
/// Shifts the child down such that the child's baseline (or the bottom of the
/// child, if the child has no baseline) is `baseline` logical pixels below the
/// top of this box.
///
/// This is used for aligning text and other widgets along a common baseline.
///
/// # Layout Algorithm (Flutter-compatible)
///
/// 1. Layout child with incoming constraints
/// 2. Query child's baseline (or use child's height if no baseline)
/// 3. Calculate vertical offset: `baseline - child_baseline`
/// 4. If offset is negative, child may overflow top (not hittable)
/// 5. Final height = max(baseline, child_baseline) + (child_height - child_baseline)
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
    /// Distance from top of this box to the baseline
    pub baseline: f32,
    /// Type of baseline to align to
    pub baseline_type: TextBaseline,

    /// Cached child offset for paint phase
    child_offset: Offset,
}

// ===== Public API =====

impl RenderBaseline {
    /// Create new RenderBaseline
    pub fn new(baseline: f32, baseline_type: TextBaseline) -> Self {
        Self {
            baseline,
            baseline_type,
            child_offset: Offset::ZERO,
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

impl RenderBox<Single> for RenderBaseline {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;

        // Layout child with same constraints
        let child_size = ctx.layout_child(child_id, constraints);

        // Query child's baseline via LayoutTree trait
        // Falls back to estimated baseline if child doesn't have one
        let child_baseline = ctx
            .tree()
            .get_baseline(child_id, self.baseline_type)
            .unwrap_or({
                // Fallback: estimate baseline based on type
                // Typical values for text without explicit baseline
                match self.baseline_type {
                    TextBaseline::Alphabetic => child_size.height * 0.75,
                    TextBaseline::Ideographic => child_size.height * 0.85,
                }
            });

        // Calculate how much to shift child down so its baseline aligns with our baseline
        // child_top = our_baseline - child_baseline
        let child_top = self.baseline - child_baseline;

        // Cache offset for paint (can be negative if baseline is too small)
        self.child_offset = Offset::new(0.0, child_top);

        // Calculate our height:
        // - Top boundary: 0 (even if child overflows top)
        // - Bottom boundary: child's bottom position
        // Height = child_top + child_height, but at least child_height
        let height = if child_top >= 0.0 {
            child_top + child_size.height
        } else {
            // Child overflows top, but we still need to contain its bottom
            // Our height is from 0 to child's bottom
            // child's bottom = child_top + child_size.height
            (child_top + child_size.height).max(0.0)
        };

        // Constrain to parent constraints
        let size = constraints.constrain(Size::new(child_size.width, height));

        tracing::trace!(
            baseline = self.baseline,
            child_baseline = child_baseline,
            child_top = child_top,
            child_size = ?child_size,
            final_size = ?size,
            "RenderBaseline::layout"
        );

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        // Paint child at calculated offset (may be negative for overflow)
        ctx.paint_child(child_id, offset + self.child_offset);
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
        assert_eq!(baseline.child_offset, Offset::ZERO);
    }

    #[test]
    fn test_render_baseline_ideographic() {
        let baseline = RenderBaseline::ideographic(30.0);
        assert_eq!(baseline.baseline, 30.0);
        assert_eq!(baseline.baseline_type, TextBaseline::Ideographic);
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
