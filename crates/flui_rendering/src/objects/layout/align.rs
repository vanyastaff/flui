//! RenderAlign - aligns child within available space
//!
//! Implements Flutter's alignment container for positioning a child within
//! available space with optional size factors.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderAlign` | `RenderPositionedBox` from `package:flutter/src/rendering/shifted_box.dart` |
//! | `alignment` | `alignment` property |
//! | `width_factor` | `widthFactor` property |
//! | `height_factor` | `heightFactor` property |
//!
//! # Layout Protocol
//!
//! 1. **Layout child**
//!    - Use loose constraints to get child's natural size
//!
//! 2. **Calculate container size**
//!    - If `width_factor` is set: `width = child_width * width_factor` (clamped)
//!    - If `width_factor` is None: `width = constraints.max_width` (expand)
//!    - Same logic for height
//!
//! 3. **Calculate alignment offset**
//!    - Use `Alignment::calculate_offset()` to position child within container
//!    - Cache offset for paint phase
//!
//! 4. **Handle no child case**
//!    - Return max constraints size (or min if max is infinite)
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with simple size calculation
//! - **Paint**: O(1) - direct child paint with cached offset
//! - **Memory**: 40 bytes (Alignment + 2 Option<f32> + Offset cache)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderAlign;
//! use flui_types::Alignment;
//!
//! // Center align with natural sizing
//! let align = RenderAlign::new(Alignment::CENTER);
//!
//! // Top-left align with size factors
//! let align = RenderAlign::with_factors(
//!     Alignment::TOP_LEFT,
//!     Some(2.0),   // Width = child_width * 2.0
//!     Some(1.5),   // Height = child_height * 1.5
//! );
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, Optional, RenderBox};
use crate::{RenderObject, RenderResult};
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
/// # Use Cases
///
/// - **Centering**: Position child at center of available space
/// - **Corner alignment**: Align to edges (top-left, bottom-right, etc.)
/// - **Sized alignment**: Control container size relative to child size
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderPositionedBox behavior:
/// - Layouts child with loose constraints
/// - Respects width_factor and height_factor for sizing
/// - Uses Alignment to calculate child offset
/// - Expands to fill space when factors are None
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

        // Check if we have a child
        if let Some(child_id) = ctx.children.get() {
            let child_id = *child_id;
            // Layout child with loose constraints to get its natural size
            let child_size = ctx.layout_child(child_id, constraints.loosen())?;

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
        // If we have a child, paint it at aligned position
        if let Some(child_id) = ctx.children.get() {
            let child_offset = ctx.offset + self.child_offset;
            ctx.paint_child(*child_id, child_offset);
        }
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
