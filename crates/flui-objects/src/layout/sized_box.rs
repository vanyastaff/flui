//! RenderSizedBox - Enforces exact size constraints or acts as spacer
//!
//! Implements Flutter's SizedBox container that forces specific dimensions
//! on its child or acts as a spacer when childless. Essential building block
//! for fixed-size layouts and spacing.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSizedBox` | `RenderConstrainedBox` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `width` | `BoxConstraints.tightFor(width: ...)` |
//! | `height` | `BoxConstraints.tightFor(height: ...)` |
//! | `set_width()` | `width = value` |
//! | `set_height()` | `height = value` |
//!
//! # Layout Protocol
//!
//! 1. **Check for child**
//!    - If no child: return specified size (act as spacer)
//!    - Spacer reserves space without rendering anything
//!
//! 2. **Determine layout strategy**
//!    - Both dimensions specified: tight constraints (exact size)
//!    - One or both unspecified: loose constraints for child's natural size
//!
//! 3. **Layout child**
//!    - Tight: force child to exact width × height
//!    - Loose: let child choose size, use for unspecified dimensions
//!
//! 4. **Return final size**
//!    - Use specified dimensions or child's size for unspecified dimensions
//!    - Constrained to parent's bounds
//!
//! # Paint Protocol
//!
//! 1. **Paint child if present**
//!    - Child painted at parent offset
//!    - No transformation or clipping
//!
//! 2. **No child case (spacer)**
//!    - Nothing painted (empty space reserved)
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constant-time constraint calculation
//! - **Paint**: O(1) - direct child paint at offset (no transformation)
//! - **Memory**: 16 bytes (2 × Option<f32>)
//!
//! # Use Cases
//!
//! - **Fixed sizing**: Force exact dimensions on child (100×100 box)
//! - **Partial sizing**: Set one dimension, let other be flexible
//! - **Spacers**: Create empty space with specified dimensions
//! - **Layout gaps**: Horizontal/vertical spacing between widgets
//! - **Aspect ratio helpers**: Size one dimension, other follows aspect ratio
//! - **Button sizing**: Force consistent button dimensions
//!
//! # Sizing Behavior
//!
//! ```text
//! Both dimensions specified:
//!   SizedBox(width=100, height=50) → Tight 100×50 (exact size)
//!
//! Width only:
//!   SizedBox(width=100, height=None) → Width=100, height from child
//!
//! Height only:
//!   SizedBox(width=None, height=50) → Height=50, width from child
//!
//! No dimensions (unusual):
//!   SizedBox(width=None, height=None) → Child's natural size
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderConstrainedBox**: ConstrainedBox adds constraints, SizedBox forces exact size
//! - **vs RenderAlign**: Align positions child, SizedBox sizes child
//! - **vs RenderFractionallySizedBox**: FractionallySizedBox uses percentages, SizedBox uses pixels
//! - **vs RenderPadding**: Padding adds space around, SizedBox forces dimensions
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSizedBox;
//!
//! // Force child to be exactly 100×100
//! let square = RenderSizedBox::exact(100.0, 100.0);
//!
//! // Set width only, height flexible
//! let wide = RenderSizedBox::width(200.0);
//!
//! // Set height only, width flexible
//! let tall = RenderSizedBox::height(150.0);
//!
//! // Spacer (no child): 50px horizontal gap
//! let spacer = RenderSizedBox::width(50.0);
//!
//! // Spacer: 20px vertical gap
//! let gap = RenderSizedBox::height(20.0);
//! ```

use flui_rendering::{RenderObject, RenderResult};

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx};
use flui_rendering::{Optional, RenderBox};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that enforces exact size constraints.
///
/// Forces its child to have specific width and/or height, or acts as
/// a spacer when no child is present.
///
/// # Arity
///
/// `Optional` - Can have 0 or 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Constraint Modifier with Exact Sizing** - Forces child to exact dimensions
/// or acts as spacer reserving specified space.
///
/// # Use Cases
///
/// - **Fixed sizing**: Force exact dimensions on child (100×100 button)
/// - **Partial sizing**: Set one dimension, let other be flexible
/// - **Spacers**: Create empty space with specified dimensions
/// - **Layout gaps**: Horizontal/vertical spacing between widgets
/// - **Aspect ratio helpers**: Size one dimension explicitly
/// - **Button sizing**: Force consistent button dimensions across UI
///
/// # Flutter Compliance
///
/// Matches Flutter's SizedBox (RenderConstrainedBox with tight constraints):
/// - When both dimensions specified: uses tight constraints (exact size)
/// - When dimensions unspecified: uses loose constraints for natural size
/// - Acts as spacer when no child present
/// - Extends RenderConstrainedBox base class
///
/// # Sizing Strategy
///
/// - **Both specified**: Tight constraints (child must be exactly that size)
/// - **One specified**: Tight for that dimension, loose for other
/// - **None specified**: Fully loose (child chooses size)
///
/// Common pattern: Use for spacers with one dimension specified.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSizedBox;
///
/// // Fixed 100×100 square
/// let square = RenderSizedBox::exact(100.0, 100.0);
///
/// // Width=200, height flexible
/// let wide = RenderSizedBox::width(200.0);
///
/// // Height=150, width flexible
/// let tall = RenderSizedBox::height(150.0);
///
/// // Spacer: 50px horizontal gap (no child)
/// let h_spacer = RenderSizedBox::width(50.0);
///
/// // Spacer: 20px vertical gap (no child)
/// let v_spacer = RenderSizedBox::height(20.0);
/// ```
#[derive(Debug)]
pub struct RenderSizedBox {
    /// Explicit width (None = unconstrained)
    pub width: Option<f32>,
    /// Explicit height (None = unconstrained)
    pub height: Option<f32>,
}

impl RenderSizedBox {
    /// Create new RenderSizedBox with optional width and height
    pub fn new(width: Option<f32>, height: Option<f32>) -> Self {
        Self { width, height }
    }

    /// Create with specific width and height
    pub fn exact(width: f32, height: f32) -> Self {
        Self {
            width: Some(width),
            height: Some(height),
        }
    }

    /// Create with only width specified
    pub fn width(width: f32) -> Self {
        Self {
            width: Some(width),
            height: None,
        }
    }

    /// Create with only height specified
    pub fn height(height: f32) -> Self {
        Self {
            width: None,
            height: Some(height),
        }
    }

    /// Set width
    pub fn set_width(&mut self, width: Option<f32>) {
        self.width = width;
    }

    /// Set height
    pub fn set_height(&mut self, height: Option<f32>) {
        self.height = height;
    }
}

impl Default for RenderSizedBox {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl RenderObject for RenderSizedBox {}

impl RenderBox<Optional> for RenderSizedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Optional>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        if let Some(&child_id) = ctx.children.get() {
            // Layout child first if we need its size
            let child_size = if self.width.is_none() || self.height.is_none() {
                // Need child's intrinsic size for unspecified dimensions
                // Give child loose constraints for dimensions we don't control
                let child_constraints = BoxConstraints::new(
                    0.0,
                    self.width.unwrap_or(constraints.max_width),
                    0.0,
                    self.height.unwrap_or(constraints.max_height),
                );
                ctx.layout_child(child_id, child_constraints, true)?
            } else {
                // Both dimensions specified, we don't need child size yet
                Size::ZERO
            };

            // Calculate final size
            let width = self.width.unwrap_or(child_size.width);
            let height = self.height.unwrap_or(child_size.height);
            let size = Size::new(width, height);

            // If we already laid out child with correct constraints, we're done
            // Otherwise, force child to match our size
            if self.width.is_some() && self.height.is_some() {
                let child_constraints = BoxConstraints::tight(size);
                ctx.layout_child(child_id, child_constraints, true)?;
            }

            Ok(size)
        } else {
            // No child - act as spacer with specified dimensions
            let width = self.width.unwrap_or(constraints.max_width);
            let height = self.height.unwrap_or(constraints.max_height);
            Ok(Size::new(width, height))
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Optional>) {
        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        if let Some(&child_id) = ctx.children.get() {
            // Paint child at parent offset (no transformation)
            ctx.paint_child(child_id, ctx.offset);
        }
        // If no child, nothing to paint (spacer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_sized_box_new() {
        let sized = RenderSizedBox::new(Some(100.0), Some(50.0));
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, Some(50.0));
    }

    #[test]
    fn test_render_sized_box_exact() {
        let sized = RenderSizedBox::exact(100.0, 100.0);
        assert_eq!(sized.width, Some(100.0));
        assert_eq!(sized.height, Some(100.0));
    }

    #[test]
    fn test_render_sized_box_width() {
        let sized = RenderSizedBox::width(50.0);
        assert_eq!(sized.width, Some(50.0));
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_render_sized_box_height() {
        let sized = RenderSizedBox::height(75.0);
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, Some(75.0));
    }

    #[test]
    fn test_render_sized_box_default() {
        let sized = RenderSizedBox::default();
        assert_eq!(sized.width, None);
        assert_eq!(sized.height, None);
    }

    #[test]
    fn test_render_sized_box_set_width() {
        let mut sized = RenderSizedBox::width(50.0);
        sized.set_width(Some(100.0));
        assert_eq!(sized.width, Some(100.0));
    }

    #[test]
    fn test_render_sized_box_set_height() {
        let mut sized = RenderSizedBox::height(50.0);
        sized.set_height(Some(100.0));
        assert_eq!(sized.height, Some(100.0));
    }
}
