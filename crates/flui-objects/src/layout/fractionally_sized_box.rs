//! RenderFractionallySizedBox - Sizes child as fraction of available space
//!
//! Implements Flutter's FractionallySizedBox that sizes its child to a fraction
//! of the available space (parent's max constraints). Useful for responsive
//! layouts where child should take percentage of parent size.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderFractionallySizedBox` | `RenderFractionallySizedBox` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `width_factor` | `widthFactor` property (0.0-1.0 or null) |
//! | `height_factor` | `heightFactor` property (0.0-1.0 or null) |
//!
//! # Layout Protocol
//!
//! 1. **Calculate target size**
//!    - If width_factor is Some: target_width = max_width × width_factor
//!    - If width_factor is None: width unconstrained (child chooses)
//!    - Same logic for height_factor
//!
//! 2. **Tighten constraints**
//!    - Create tight constraints for specified dimensions
//!    - Leave unspecified dimensions loose
//!
//! 3. **Layout child**
//!    - Child laid out with tightened constraints
//!    - Child forced to fractional size if factor specified
//!
//! 4. **Return child size**
//!    - Container size = child size (always)
//!
//! # Paint Protocol
//!
//! 1. **Paint child normally**
//!    - Child painted at parent offset
//!    - No transformation or clipping
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constraint calculation
//! - **Paint**: O(1) - direct child paint
//! - **Memory**: 8 bytes (2 × Option<f32>)
//!
//! # Use Cases
//!
//! - **Responsive sizing**: Make child 50% of parent width
//! - **Aspect ratio containers**: Size one dimension, let other flex
//! - **Percentage-based layouts**: CSS-like percentage sizing
//! - **Grid cells**: Size cells as fractions of grid
//! - **Banner sizing**: Make banner 100% width, 20% height
//! - **Progress indicators**: Width represents progress (0%-100%)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderFractionallySizedBox;
//!
//! // Child takes 50% of parent width and height
//! let half_size = RenderFractionallySizedBox::both(0.5);
//!
//! // Child takes 100% width, height unconstrained
//! let full_width = RenderFractionallySizedBox::width(1.0);
//!
//! // Child takes 75% height, width unconstrained
//! let tall = RenderFractionallySizedBox::height(0.75);
//!
//! // Custom fractions
//! let custom = RenderFractionallySizedBox::new(Some(0.6), Some(0.4));
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::Size;

/// RenderObject that sizes child as a fraction of available space.
///
/// Forces child to be a specific fraction of parent's max constraints.
/// Factors are in range 0.0-1.0 (0% to 100%). None means unconstrained.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Responsive sizing**: Child takes percentage of parent (50%, 75%, 100%)
/// - **Percentage layouts**: CSS-like percentage-based sizing
/// - **Grid cells**: Size cells as fractions of available space
/// - **Banner sizing**: Full width (1.0), partial height (0.2)
/// - **Progress bars**: Width represents progress percentage
/// - **Aspect ratio helpers**: Size one dimension fractionally
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderFractionallySizedBox behavior:
/// - Calculates target size as max_constraints × factor
/// - Tightens constraints for specified dimensions
/// - Leaves unspecified dimensions loose (child chooses)
/// - Returns child size (child always determines final size)
/// - Factors must be in range 0.0-1.0 (validated)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderFractionallySizedBox;
///
/// // 50% of both dimensions
/// let half = RenderFractionallySizedBox::both(0.5);
///
/// // Full width, 25% height
/// let banner = RenderFractionallySizedBox::new(Some(1.0), Some(0.25));
///
/// // 75% width, height flexible
/// let wide = RenderFractionallySizedBox::width(0.75);
/// ```
#[derive(Debug)]
pub struct RenderFractionallySizedBox {
    /// Width factor (0.0 - 1.0), None means unconstrained
    pub width_factor: Option<f32>,
    /// Height factor (0.0 - 1.0), None means unconstrained
    pub height_factor: Option<f32>,
}

impl RenderFractionallySizedBox {
    /// Create new RenderFractionallySizedBox
    pub fn new(width_factor: Option<f32>, height_factor: Option<f32>) -> Self {
        if let Some(w) = width_factor {
            assert!(
                (0.0..=1.0).contains(&w),
                "Width factor must be between 0.0 and 1.0"
            );
        }
        if let Some(h) = height_factor {
            assert!(
                (0.0..=1.0).contains(&h),
                "Height factor must be between 0.0 and 1.0"
            );
        }
        Self {
            width_factor,
            height_factor,
        }
    }

    /// Create with both width and height factors
    pub fn both(factor: f32) -> Self {
        Self::new(Some(factor), Some(factor))
    }

    /// Create with only width factor
    pub fn width(factor: f32) -> Self {
        Self::new(Some(factor), None)
    }

    /// Create with only height factor
    pub fn height(factor: f32) -> Self {
        Self::new(None, Some(factor))
    }

    /// Set new width factor
    pub fn set_width_factor(&mut self, factor: Option<f32>) {
        if let Some(w) = factor {
            assert!(
                (0.0..=1.0).contains(&w),
                "Width factor must be between 0.0 and 1.0"
            );
        }
        self.width_factor = factor;
    }

    /// Set new height factor
    pub fn set_height_factor(&mut self, factor: Option<f32>) {
        if let Some(h) = factor {
            assert!(
                (0.0..=1.0).contains(&h),
                "Height factor must be between 0.0 and 1.0"
            );
        }
        self.height_factor = factor;
    }
}

impl Default for RenderFractionallySizedBox {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl RenderObject for RenderFractionallySizedBox {}

impl RenderBox<Single> for RenderFractionallySizedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Calculate target size based on factors
        // If factor is Some: target = max_constraint × factor
        // If factor is None: dimension unconstrained (child chooses)
        let target_width = self.width_factor.map(|f| ctx.constraints.max_width * f);
        let target_height = self.height_factor.map(|f| ctx.constraints.max_height * f);

        // Tighten constraints for specified dimensions
        // Tight constraint forces child to exact size
        let child_constraints = ctx.constraints.tighten(target_width, target_height);

        // Layout child with fractional constraints
        Ok(ctx.layout_child(child_id, child_constraints, true)?)
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
    fn test_render_fractionally_sized_box_new() {
        let fractional = RenderFractionallySizedBox::new(Some(0.5), Some(0.75));
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, Some(0.75));
    }

    #[test]
    fn test_render_fractionally_sized_box_both() {
        let fractional = RenderFractionallySizedBox::both(0.5);
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, Some(0.5));
    }

    #[test]
    fn test_render_fractionally_sized_box_width() {
        let fractional = RenderFractionallySizedBox::width(0.5);
        assert_eq!(fractional.width_factor, Some(0.5));
        assert_eq!(fractional.height_factor, None);
    }

    #[test]
    fn test_render_fractionally_sized_box_height() {
        let fractional = RenderFractionallySizedBox::height(0.75);
        assert_eq!(fractional.width_factor, None);
        assert_eq!(fractional.height_factor, Some(0.75));
    }

    #[test]
    #[should_panic(expected = "Width factor must be between 0.0 and 1.0")]
    fn test_render_fractionally_sized_box_invalid_width() {
        RenderFractionallySizedBox::new(Some(1.5), None);
    }

    #[test]
    #[should_panic(expected = "Height factor must be between 0.0 and 1.0")]
    fn test_render_fractionally_sized_box_invalid_height() {
        RenderFractionallySizedBox::new(None, Some(-0.1));
    }

    #[test]
    fn test_render_fractionally_sized_box_set_factors() {
        let mut fractional = RenderFractionallySizedBox::both(0.5);
        fractional.set_width_factor(Some(0.75));
        assert_eq!(fractional.width_factor, Some(0.75));
    }
}
