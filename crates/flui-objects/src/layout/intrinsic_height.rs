//! RenderIntrinsicHeight - Sizes child to its intrinsic (natural) height
//!
//! Implements Flutter's IntrinsicHeight that forces a child to adopt its minimum
//! intrinsic height, ignoring parent height constraints. Useful for making content-
//! sized widgets take exactly the vertical space they need.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderIntrinsicHeight` | `RenderIntrinsicHeight` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `step_width` | `stepWidth` property (rounds to nearest multiple) |
//! | `step_height` | `stepHeight` property (rounds to nearest multiple) |
//! | `set_step_width()` | `stepWidth = value` setter |
//! | `set_step_height()` | `stepHeight = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Create infinite height constraint**
//!    - min_height = 0.0, max_height = INFINITY
//!    - Width constraints pass through unchanged
//!    - Child determines its natural height
//!
//! 2. **Layout child with infinite height**
//!    - Child laid out with unbounded height
//!    - Child returns its intrinsic (minimum natural) height
//!    - Width follows parent constraints
//!
//! 3. **Apply step rounding (if specified)**
//!    - If step_width: round width to nearest multiple
//!    - If step_height: round height to nearest multiple
//!    - Rounding uses ceiling: `(size / step).ceil() * step`
//!
//! 4. **Constrain to parent bounds**
//!    - Final size constrained within parent's original constraints
//!    - Ensures intrinsic size doesn't violate parent requirements
//!
//! # Paint Protocol
//!
//! 1. **Paint child at parent offset**
//!    - Child painted at widget offset
//!    - No transformation or clipping
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constraint modification
//! - **Paint**: O(1) - direct child paint
//! - **Memory**: 8 bytes (2 × Option<f32>)
//! - **Warning**: Can be expensive for complex children (forces extra layout pass)
//!
//! # Use Cases
//!
//! - **Row children**: Make flex children adopt intrinsic height
//! - **Uniform height**: Make row items same height as tallest
//! - **Content-sized containers**: Size to content height
//! - **Column alignment**: Align based on natural height
//! - **Scrollable content**: Size to content rather than viewport
//! - **Dynamic lists**: Calculate list item height from content
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderIntrinsicWidth**: Same concept but for width dimension
//! - **vs RenderConstrainedBox**: ConstrainedBox forces specific size, IntrinsicHeight discovers it
//! - **vs RenderFlex**: Flex distributes space, IntrinsicHeight ignores parent height
//! - **vs RenderSizedBox**: SizedBox forces exact size, IntrinsicHeight uses child's preference
//!
//! # Step Rounding
//!
//! Step properties round dimensions to nearest multiples:
//!
//! ```text
//! Without step: height = 47.8px
//! With step_height = 4.0: height = 48.0px (rounded up to nearest 4)
//! With step_height = 8.0: height = 48.0px (rounded up to nearest 8)
//! ```
//!
//! Useful for:
//! - **Baseline grids**: Typography aligned to baseline multiples (4px, 8px)
//! - **Row alignment**: Align row items to grid
//! - **Pixel snapping**: Round to whole pixels for crisp rendering
//! - **Uniform spacing**: Ensure consistent vertical rhythm
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderIntrinsicHeight;
//!
//! // Basic intrinsic height
//! let intrinsic = RenderIntrinsicHeight::new();
//!
//! // With step height (round to 4px baseline grid)
//! let stepped = RenderIntrinsicHeight::with_step_height(4.0);
//!
//! // With both step dimensions
//! let both = RenderIntrinsicHeight::with_steps(8.0, 4.0);
//! ```

use flui_rendering::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that sizes child to its intrinsic (natural) height.
///
/// Forces child to adopt its minimum intrinsic height by laying it out with
/// infinite height constraints. Useful for content-sized widgets that should
/// take only the vertical space they need rather than filling available height.
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
/// **Constraint Modifier** - Modifies constraints (infinite height) before child layout.
///
/// # Use Cases
///
/// - **Row children**: Make flex children adopt intrinsic height (uniform height)
/// - **Content-sized**: Size containers to content height
/// - **Column alignment**: Align based on natural height
/// - **Scrollable content**: Calculate total content height
/// - **Dynamic lists**: Size list items to content
/// - **Baseline alignment**: Align to typography baseline grid
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderIntrinsicHeight behavior:
/// - Lays out child with infinite height (min=0, max=INFINITY)
/// - Width constraints pass through unchanged
/// - Optional step rounding for width and height
/// - Final size constrained to parent bounds
/// - Extends RenderProxyBox base class
///
/// # Step Rounding
///
/// Optional step properties round dimensions to nearest multiples:
/// - `step_width`: Rounds width to nearest multiple
/// - `step_height`: Rounds height to nearest multiple (useful for baseline grids)
/// - Rounding uses ceiling: `(dimension / step).ceil() * step`
///
/// # Performance Warning
///
/// IntrinsicHeight can be expensive for complex children as it forces an
/// additional layout pass with modified constraints. Use sparingly in
/// performance-critical paths.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderIntrinsicHeight;
///
/// // Basic intrinsic height
/// let intrinsic = RenderIntrinsicHeight::new();
///
/// // With 4px baseline grid alignment
/// let baseline_aligned = RenderIntrinsicHeight::with_step_height(4.0);
///
/// // With both width and height stepping
/// let both = RenderIntrinsicHeight::with_steps(8.0, 4.0);
/// ```
#[derive(Debug)]
pub struct RenderIntrinsicHeight {
    /// Step width (rounds intrinsic width to nearest multiple)
    pub step_width: Option<f32>,
    /// Step height (rounds intrinsic height to nearest multiple)
    pub step_height: Option<f32>,
}

impl RenderIntrinsicHeight {
    /// Create new RenderIntrinsicHeight
    pub fn new() -> Self {
        Self {
            step_width: None,
            step_height: None,
        }
    }

    /// Create with step width
    pub fn with_step_width(step_width: f32) -> Self {
        Self {
            step_width: Some(step_width),
            step_height: None,
        }
    }

    /// Create with step height
    pub fn with_step_height(step_height: f32) -> Self {
        Self {
            step_width: None,
            step_height: Some(step_height),
        }
    }

    /// Create with both step dimensions
    pub fn with_steps(step_width: f32, step_height: f32) -> Self {
        Self {
            step_width: Some(step_width),
            step_height: Some(step_height),
        }
    }

    /// Set step width
    pub fn set_step_width(&mut self, step_width: Option<f32>) {
        self.step_width = step_width;
    }

    /// Set step height
    pub fn set_step_height(&mut self, step_height: Option<f32>) {
        self.step_height = step_height;
    }
}

impl Default for RenderIntrinsicHeight {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderIntrinsicHeight {}

impl RenderBox<Single> for RenderIntrinsicHeight {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Layout child with infinite height to get intrinsic height
        // Width constraints pass through unchanged
        let intrinsic_constraints = BoxConstraints::new(
            ctx.constraints.min_width,
            ctx.constraints.max_width,
            0.0,
            f32::INFINITY,
        );

        let child_size = ctx.layout_child(child_id, intrinsic_constraints)?;

        // Apply step width/height if specified
        // Rounding uses ceiling to ensure child fits
        let width = if let Some(step) = self.step_width {
            (child_size.width / step).ceil() * step
        } else {
            child_size.width
        };

        let height = if let Some(step) = self.step_height {
            (child_size.height / step).ceil() * step
        } else {
            child_size.height
        };

        // Constrain to parent constraints
        // Ensures intrinsic size doesn't violate parent requirements
        Ok(ctx.constraints.constrain(Size::new(width, height)))
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Paint child at parent offset (no transformation)
        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_intrinsic_height_new() {
        let intrinsic = RenderIntrinsicHeight::new();
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_height_with_step_width() {
        let intrinsic = RenderIntrinsicHeight::with_step_width(10.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_height_with_step_height() {
        let intrinsic = RenderIntrinsicHeight::with_step_height(5.0);
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_height_with_steps() {
        let intrinsic = RenderIntrinsicHeight::with_steps(10.0, 5.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_height_set_step_width() {
        let mut intrinsic = RenderIntrinsicHeight::new();
        intrinsic.set_step_width(Some(8.0));
        assert_eq!(intrinsic.step_width, Some(8.0));
    }

    #[test]
    fn test_render_intrinsic_height_set_step_height() {
        let mut intrinsic = RenderIntrinsicHeight::new();
        intrinsic.set_step_height(Some(4.0));
        assert_eq!(intrinsic.step_height, Some(4.0));
    }
}
