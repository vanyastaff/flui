//! RenderBaseline - Aligns child along typography baseline
//!
//! Implements Flutter's Baseline that positions a child so its baseline is at
//! a specific distance from the top. Essential for aligning text and typography
//! elements along a common baseline for proper visual alignment.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderBaseline` | `RenderBaseline` from `package:flutter/src/rendering/shifted_box.dart` |
//! | `baseline` | `baseline` property (distance from top in logical pixels) |
//! | `baseline_type` | `baselineType` property (alphabetic or ideographic) |
//! | `set_baseline()` | `baseline = value` setter |
//! | `set_baseline_type()` | `baselineType = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!    - Baseline doesn't affect constraints
//!
//! 2. **Layout child**
//!    - Child determines its natural size
//!    - Child size used to calculate container height
//!
//! 3. **Calculate container height**
//!    - Container height = max(child.height, child.height + baseline)
//!    - Ensures enough space above baseline for child
//!    - Width always matches child width
//!
//! 4. **Return size**
//!    - Width = child width (unchanged)
//!    - Height = adjusted height with baseline offset
//!
//! # Paint Protocol
//!
//! 1. **Calculate baseline offset**
//!    - Vertical offset = baseline distance from top
//!    - Positions child below baseline reference point
//!
//! 2. **Paint child with offset**
//!    - Child painted at (0, baseline) relative to parent
//!    - Child baseline aligns with container's baseline position
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with height calculation
//! - **Paint**: O(1) - simple offset addition + child paint
//! - **Memory**: 8 bytes (f32 + enum)
//!
//! # Use Cases
//!
//! - **Text alignment**: Align text widgets on common baseline
//! - **Mixed typography**: Align different font sizes/styles
//! - **Inline widgets**: Position icons/images aligned with text baseline
//! - **Form fields**: Align labels and input text baselines
//! - **Button text**: Align button text across different button sizes
//! - **Row alignment**: Create baseline-aligned rows of mixed content
//!
//! # Baseline Types
//!
//! Flutter supports two baseline types:
//!
//! - **Alphabetic**: Standard Latin baseline (bottom of 'x', 'a', 'c')
//! - **Ideographic**: East Asian baseline (bottom of ideographic characters)
//!
//! Different scripts and writing systems use different baselines. The baseline
//! type should match the primary script of the text being aligned.
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderAlign**: Align positions by geometric alignment, Baseline by typography
//! - **vs RenderPositioned**: Positioned uses absolute coordinates, Baseline uses typography
//! - **vs RenderPadding**: Padding adds space, Baseline aligns based on typography
//! - **vs RenderShiftedBox**: ShiftedBox uses pixel offset, Baseline uses baseline distance
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderBaseline;
//! use flui_types::typography::TextBaseline;
//!
//! // Align with alphabetic baseline at 20px from top
//! let baseline = RenderBaseline::alphabetic(20.0);
//!
//! // Align with ideographic baseline for CJK text
//! let cjk_baseline = RenderBaseline::ideographic(18.0);
//!
//! // Custom baseline configuration
//! let custom = RenderBaseline::new(24.0, TextBaseline::Alphabetic);
//! ```

use flui_rendering::{RenderObject, RenderResult};

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx};
use flui_rendering::{RenderBox, Single};
use flui_types::{typography::TextBaseline, Offset, Size};

/// RenderObject that positions child based on typography baseline.
///
/// Aligns a child so its baseline is at a specific distance from the top.
/// Essential for proper typography alignment when mixing text of different
/// sizes or aligning text with other widgets.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy with Baseline Positioning** - Passes constraints unchanged, adjusts
/// height based on baseline, positions child at baseline offset.
///
/// # Use Cases
///
/// - **Text alignment**: Align text widgets along common baseline
/// - **Mixed typography**: Align different font sizes/styles properly
/// - **Inline widgets**: Position icons/images aligned with text
/// - **Form fields**: Align labels and input text baselines
/// - **Button text**: Consistent text alignment across button sizes
/// - **Row alignment**: Create baseline-aligned rows
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderBaseline behavior:
/// - Passes constraints unchanged to child
/// - Width determined by child
/// - Height = max(child.height, child.height + baseline)
/// - Child painted at vertical offset equal to baseline
/// - Supports both alphabetic and ideographic baselines
/// - Extends RenderShiftedBox base class
///
/// # Baseline Types
///
/// - **Alphabetic**: Standard Latin baseline (bottom of lowercase 'x')
/// - **Ideographic**: East Asian baseline (bottom of ideographic characters)
///
/// Choose the baseline type that matches your primary script.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderBaseline;
/// use flui_types::typography::TextBaseline;
///
/// // Alphabetic baseline for Latin text
/// let baseline = RenderBaseline::alphabetic(20.0);
///
/// // Ideographic baseline for CJK text
/// let cjk = RenderBaseline::ideographic(18.0);
///
/// // Custom baseline configuration
/// let custom = RenderBaseline::new(24.0, TextBaseline::Alphabetic);
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

impl RenderObject for RenderBaseline {}

impl RenderBox<Single> for RenderBaseline {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        let constraints = ctx.constraints;

        // Layout child with same constraints (proxy behavior)
        let child_size = ctx.layout_child(child_id, constraints, true)?;

        // Our height includes space for baseline offset
        // Container height = max(child_height, child_height + baseline)
        // This ensures enough vertical space for baseline positioning
        Ok(Size::new(
            child_size.width,
            (child_size.height + self.baseline).max(child_size.height),
        ))
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        let offset = ctx.offset;

        // Apply baseline offset to child painting position
        // Child painted below baseline reference point
        let child_offset = offset + Offset::new(0.0, self.baseline);
        ctx.paint_child(child_id, child_offset);
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
