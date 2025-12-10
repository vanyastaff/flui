//! RenderAlign - Aligns child within available space with optional size factors
//!
//! Implements Flutter's alignment container for positioning a child within
//! available space with optional size factors for controlling container dimensions.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderAlign` | `RenderPositionedBox` from `package:flutter/src/rendering/shifted_box.dart` |
//! | `alignment` | `alignment` property (AlignmentGeometry) |
//! | `width_factor` | `widthFactor` property (multiplier for child width) |
//! | `height_factor` | `heightFactor` property (multiplier for child height) |
//! | `set_alignment()` | `alignment = value` setter |
//! | `set_width_factor()` | `widthFactor = value` setter |
//! | `set_height_factor()` | `heightFactor = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Layout child with loose constraints**
//!    - Child receives loosened constraints (min=0, same max)
//!    - Child determines its natural size
//!
//! 2. **Calculate container size**
//!    - If `width_factor` is Some: `width = child_width × width_factor` (clamped)
//!    - If `width_factor` is None: `width = constraints.max_width` (expand to fill)
//!    - Same logic for height
//!
//! 3. **Calculate alignment offset**
//!    - Use `Alignment::calculate_offset(child_size, container_size)`
//!    - Offset positions child within container according to alignment
//!    - Cache offset for paint phase
//!
//! 4. **Handle no child case**
//!    - Return max constraints size (or min if max is infinite)
//!    - Reserves space even without child
//!
//! # Paint Protocol
//!
//! 1. **Paint child at aligned offset**
//!    - Child painted at parent offset + cached alignment offset
//!    - No clipping applied (child positioned within bounds by layout)
//!
//! 2. **No child case**
//!    - Nothing painted (empty space reserved)
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with simple size calculation
//! - **Paint**: O(1) - direct child paint with cached offset
//! - **Memory**: 40 bytes (Alignment + 2 × Option<f32> + Offset cache)
//!
//! # Use Cases
//!
//! - **Centering**: Position child at center of available space (most common)
//! - **Corner alignment**: Align to edges (top-left, bottom-right, etc.)
//! - **Sized containers**: Control container size relative to child size
//! - **Flexible spacing**: Create containers that expand or shrink with content
//! - **Modal dialogs**: Center dialog with size based on content
//! - **Tooltips**: Position tooltips with alignment relative to anchor
//!
//! # Size Factor Behavior
//!
//! Size factors control container dimensions relative to child size:
//!
//! ```text
//! width_factor = None:
//!   Container width = max_width (expand to fill)
//!
//! width_factor = Some(1.0):
//!   Container width = child_width (tight around child)
//!
//! width_factor = Some(2.0):
//!   Container width = child_width × 2.0 (double child width)
//!
//! width_factor = Some(0.5):
//!   Container width = child_width × 0.5 (half child width, may clip)
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderCenter**: Center is Align with alignment=CENTER and no size factors
//! - **vs RenderPositioned**: Positioned uses absolute coordinates, Align uses alignment
//! - **vs RenderFlex**: Flex distributes space among children, Align positions single child
//! - **vs RenderPadding**: Padding adds space around child, Align positions within space
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderAlign;
//! use flui_types::Alignment;
//!
//! // Center align with natural sizing (expand to fill)
//! let center = RenderAlign::new(Alignment::CENTER);
//!
//! // Top-left align
//! let top_left = RenderAlign::new(Alignment::TOP_LEFT);
//!
//! // Center with size factors (tight around child × factor)
//! let sized = RenderAlign::with_factors(
//!     Alignment::CENTER,
//!     Some(2.0),   // Width = child_width × 2.0
//!     Some(1.5),   // Height = child_height × 1.5
//! );
//!
//! // Right align with no vertical expansion
//! let right = RenderAlign::with_factors(
//!     Alignment::CENTER_RIGHT,
//!     None,        // Expand to fill width
//!     Some(1.0),   // Height matches child
//! );
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, Optional, RenderBox};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::{Alignment, Offset, Size};

/// RenderObject that aligns its child within available space.
///
/// Positions child according to alignment parameter with optional size factors
/// for constraining container dimensions.
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
/// **Alignment Container with Optional Sizing Factors** - Positions child using
/// alignment, optionally scales container size based on child size.
///
/// # Use Cases
///
/// - **Centering**: Position child at center (most common use case)
/// - **Corner alignment**: Align to edges (top-left, bottom-right, etc.)
/// - **Sized containers**: Control size relative to child (tight fit, doubled, etc.)
/// - **Flexible spacing**: Expand to fill or shrink to child
/// - **Modal dialogs**: Center with content-based sizing
/// - **Tooltips**: Position with alignment relative to anchor
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderPositionedBox behavior:
/// - Layouts child with loose constraints
/// - Respects width_factor and height_factor for sizing
/// - Uses Alignment to calculate child offset
/// - Expands to fill space when factors are None
/// - Extends RenderAligningShiftedBox base class
///
/// # Size Factor Behavior
///
/// - **None**: Expand to fill available space (max constraints)
/// - **Some(1.0)**: Tight around child (container = child size)
/// - **Some(2.0)**: Container = child size × 2.0
/// - **Some(0.5)**: Container = child size × 0.5 (may clip child)
///
/// Result is clamped to parent constraints.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAlign;
/// use flui_types::Alignment;
///
/// // Center align (expand to fill)
/// let center = RenderAlign::new(Alignment::CENTER);
///
/// // Center with tight fit around child
/// let tight = RenderAlign::with_factors(Alignment::CENTER, Some(1.0), Some(1.0));
///
/// // Top-left with double width
/// let wide = RenderAlign::with_factors(Alignment::TOP_LEFT, Some(2.0), Some(1.0));
/// ```
#[derive(Debug)]
pub struct RenderAlign {
    /// The alignment within the available space
    pub alignment: Alignment,
    /// Width factor - if Some, the width is child_width * width_factor
    /// Otherwise, expands to fill available space
    pub width_factor: Option<f32>,
    /// Height factor - if Some, the height is child_height * height_factor
    /// Otherwise, expands to fill available space
    pub height_factor: Option<f32>,

    // Cached values from layout for paint phase
    child_offset: Offset,
}

impl RenderAlign {
    /// Create new RenderAlign with specified alignment
    pub fn new(alignment: Alignment) -> Self {
        Self {
            alignment,
            width_factor: None,
            height_factor: None,
            child_offset: Offset::ZERO,
        }
    }

    /// Create with alignment and size factors
    pub fn with_factors(
        alignment: Alignment,
        width_factor: Option<f32>,
        height_factor: Option<f32>,
    ) -> Self {
        Self {
            alignment,
            width_factor,
            height_factor,
            child_offset: Offset::ZERO,
        }
    }

    /// Set new alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    /// Set width factor
    pub fn set_width_factor(&mut self, width_factor: Option<f32>) {
        self.width_factor = width_factor;
    }

    /// Set height factor
    pub fn set_height_factor(&mut self, height_factor: Option<f32>) {
        self.height_factor = height_factor;
    }
}

impl Default for RenderAlign {
    fn default() -> Self {
        Self::new(Alignment::CENTER)
    }
}

impl RenderObject for RenderAlign {}

impl RenderBox<Optional> for RenderAlign {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Optional>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        if let Some(&child_id) = ctx.children.get() {
            // Layout child with loose constraints to get its natural size
            let child_size = ctx.layout_child(child_id, constraints.loosen(), true)?;

            // Calculate our size based on factors
            // Flutter-compatible behavior:
            // - If factor is set: size = child_size * factor (clamped to constraints)
            // - If no factor: expand to fill max constraints
            let width = if let Some(factor) = self.width_factor {
                (child_size.width * factor).clamp(constraints.min_width, constraints.max_width)
            } else {
                // No factor: expand to fill available width
                constraints.max_width
            };

            let height = if let Some(factor) = self.height_factor {
                (child_size.height * factor).clamp(constraints.min_height, constraints.max_height)
            } else {
                // No factor: expand to fill available height
                constraints.max_height
            };

            let size = Size::new(width, height);

            // Calculate aligned offset using Alignment's built-in method
            self.child_offset = self.alignment.calculate_offset(child_size, size);

            Ok(size)
        } else {
            // No child - take max size but handle infinity
            let width = if constraints.max_width.is_finite() {
                constraints.max_width
            } else {
                constraints.min_width
            };
            let height = if constraints.max_height.is_finite() {
                constraints.max_height
            } else {
                constraints.min_height
            };
            Ok(Size::new(width, height))
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Optional>) {
        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        if let Some(&child_id) = ctx.children.get() {
            // Paint child at aligned position (parent offset + cached alignment offset)
            let child_offset = ctx.offset + self.child_offset;
            ctx.paint_child(child_id, child_offset);
        }
        // If no child, nothing to paint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_align_new() {
        let align = RenderAlign::new(Alignment::TOP_LEFT);
        assert_eq!(align.alignment, Alignment::TOP_LEFT);
        assert_eq!(align.width_factor, None);
        assert_eq!(align.height_factor, None);
    }

    #[test]
    fn test_render_align_default() {
        let align = RenderAlign::default();
        assert_eq!(align.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_align_with_factors() {
        let align = RenderAlign::with_factors(Alignment::CENTER, Some(2.0), Some(1.5));
        assert_eq!(align.alignment, Alignment::CENTER);
        assert_eq!(align.width_factor, Some(2.0));
        assert_eq!(align.height_factor, Some(1.5));
    }

    #[test]
    fn test_render_align_set_alignment() {
        let mut align = RenderAlign::new(Alignment::TOP_LEFT);
        align.set_alignment(Alignment::BOTTOM_RIGHT);
        assert_eq!(align.alignment, Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn test_render_align_set_factors() {
        let mut align = RenderAlign::new(Alignment::CENTER);
        align.set_width_factor(Some(2.0));
        align.set_height_factor(Some(1.5));
        assert_eq!(align.width_factor, Some(2.0));
        assert_eq!(align.height_factor, Some(1.5));
    }
}
