//! RenderIntrinsicWidth - Sizes child to its intrinsic (natural) width
//!
//! Implements Flutter's IntrinsicWidth that forces a child to adopt its minimum
//! intrinsic width, ignoring parent width constraints. Useful for making content-
//! sized widgets like text or images take exactly the space they need.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderIntrinsicWidth` | `RenderIntrinsicWidth` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `step_width` | `stepWidth` property (rounds to nearest multiple) |
//! | `step_height` | `stepHeight` property (rounds to nearest multiple) |
//! | `set_step_width()` | `stepWidth = value` setter |
//! | `set_step_height()` | `stepHeight = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Create infinite width constraint**
//!    - min_width = 0.0, max_width = INFINITY
//!    - Height constraints pass through unchanged
//!    - Child determines its natural width
//!
//! 2. **Layout child with infinite width**
//!    - Child laid out with unbounded width
//!    - Child returns its intrinsic (minimum natural) width
//!    - Height follows parent constraints
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
//! - **Text sizing**: Make text take only needed width (no wrapping)
//! - **Image sizing**: Size images to their natural dimensions
//! - **Button sizing**: Size buttons to content width
//! - **Column children**: Make flex children adopt intrinsic width
//! - **Grid cells**: Size cells to content rather than grid fraction
//! - **Alignment**: Center/align based on natural width
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderIntrinsicHeight**: Same concept but for height dimension
//! - **vs RenderConstrainedBox**: ConstrainedBox forces specific size, IntrinsicWidth discovers it
//! - **vs RenderFlex**: Flex distributes space, IntrinsicWidth ignores parent width
//! - **vs RenderSizedBox**: SizedBox forces exact size, IntrinsicWidth uses child's preference
//!
//! # Step Rounding
//!
//! Step properties round dimensions to nearest multiples:
//!
//! ```text
//! Without step: width = 123.45px
//! With step_width = 10.0: width = 130.0px (rounded up to nearest 10)
//! With step_width = 8.0: width = 128.0px (rounded up to nearest 8)
//! ```
//!
//! Useful for:
//! - **Grid alignment**: Round to grid cell size (e.g., 8px grid)
//! - **Baseline grids**: Typography aligned to baseline multiples
//! - **Pixel snapping**: Round to whole pixels for crisp rendering
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderIntrinsicWidth;
//!
//! // Basic intrinsic width
//! let intrinsic = RenderIntrinsicWidth::new();
//!
//! // With step width (round to 8px grid)
//! let stepped = RenderIntrinsicWidth::with_step_width(8.0);
//!
//! // With both step dimensions
//! let both = RenderIntrinsicWidth::with_steps(8.0, 4.0);
//! ```

use crate::core::{
    RenderBox, Single, {BoxLayoutCtx, BoxPaintCtx},
};
use crate::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that sizes child to its intrinsic (natural) width.
///
/// Forces child to adopt its minimum intrinsic width by laying it out with
/// infinite width constraints. Useful for content-sized widgets like text
/// that should take only the space they need rather than filling available width.
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
/// **Constraint Modifier** - Modifies constraints (infinite width) before child layout.
///
/// # Use Cases
///
/// - **Text widgets**: Size text to content width without wrapping
/// - **Images**: Size to natural dimensions ignoring parent width
/// - **Buttons**: Size to content rather than filling width
/// - **Column children**: Make flex children adopt intrinsic width
/// - **Grid cells**: Size to content rather than grid fraction
/// - **Centered content**: Align based on natural width
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderIntrinsicWidth behavior:
/// - Lays out child with infinite width (min=0, max=INFINITY)
/// - Height constraints pass through unchanged
/// - Optional step rounding for width and height
/// - Final size constrained to parent bounds
/// - Extends RenderProxyBox base class
///
/// # Step Rounding
///
/// Optional step properties round dimensions to nearest multiples:
/// - `step_width`: Rounds width to nearest multiple (e.g., 8px grid)
/// - `step_height`: Rounds height to nearest multiple (e.g., 4px baseline)
/// - Rounding uses ceiling: `(dimension / step).ceil() * step`
///
/// # Performance Warning
///
/// IntrinsicWidth can be expensive for complex children as it forces an
/// additional layout pass with modified constraints. Use sparingly in
/// performance-critical paths.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderIntrinsicWidth;
///
/// // Basic intrinsic width
/// let intrinsic = RenderIntrinsicWidth::new();
///
/// // With 8px grid alignment
/// let grid_aligned = RenderIntrinsicWidth::with_step_width(8.0);
///
/// // With both width and height stepping
/// let both = RenderIntrinsicWidth::with_steps(8.0, 4.0);
/// ```
#[derive(Debug)]
pub struct RenderIntrinsicWidth {
    /// Step width (rounds intrinsic width to nearest multiple)
    pub step_width: Option<f32>,
    /// Step height (rounds intrinsic height to nearest multiple)
    pub step_height: Option<f32>,
}

impl RenderIntrinsicWidth {
    /// Create new RenderIntrinsicWidth
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

impl Default for RenderIntrinsicWidth {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderIntrinsicWidth {}

impl RenderBox<Single> for RenderIntrinsicWidth {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Layout child with infinite width to get intrinsic width
        // Height constraints pass through unchanged
        let intrinsic_constraints = BoxConstraints::new(
            0.0,
            f32::INFINITY,
            ctx.constraints.min_height,
            ctx.constraints.max_height,
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
    fn test_render_intrinsic_width_new() {
        let intrinsic = RenderIntrinsicWidth::new();
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_width_with_step_width() {
        let intrinsic = RenderIntrinsicWidth::with_step_width(10.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, None);
    }

    #[test]
    fn test_render_intrinsic_width_with_step_height() {
        let intrinsic = RenderIntrinsicWidth::with_step_height(5.0);
        assert_eq!(intrinsic.step_width, None);
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_width_with_steps() {
        let intrinsic = RenderIntrinsicWidth::with_steps(10.0, 5.0);
        assert_eq!(intrinsic.step_width, Some(10.0));
        assert_eq!(intrinsic.step_height, Some(5.0));
    }

    #[test]
    fn test_render_intrinsic_width_set_step_width() {
        let mut intrinsic = RenderIntrinsicWidth::new();
        intrinsic.set_step_width(Some(8.0));
        assert_eq!(intrinsic.step_width, Some(8.0));
    }

    #[test]
    fn test_render_intrinsic_width_set_step_height() {
        let mut intrinsic = RenderIntrinsicWidth::new();
        intrinsic.set_step_height(Some(4.0));
        assert_eq!(intrinsic.step_height, Some(4.0));
    }
}
