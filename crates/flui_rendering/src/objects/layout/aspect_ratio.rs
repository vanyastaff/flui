//! RenderAspectRatio - maintains aspect ratio
//!
//! Implements Flutter's aspect ratio container that sizes child to maintain
//! a specific width-to-height ratio.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderAspectRatio` | `RenderAspectRatio` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `aspect_ratio` | `aspectRatio` property (width / height) |
//!
//! # Layout Protocol
//!
//! 1. **Determine target size**
//!    - If constraints are tight: use exact constrained size
//!    - Otherwise: calculate size to fill space while maintaining ratio
//!
//! 2. **Calculate dimensions**
//!    - Try width-based: `width = max_width`, `height = width / aspect_ratio`
//!    - If height exceeds max: use height-based sizing
//!    - Constrain final size to parent constraints
//!
//! 3. **Layout child**
//!    - Use tight constraints with calculated size
//!    - Child must match exact dimensions
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constant-time size calculation
//! - **Paint**: O(1) - direct child paint at offset (no transformation)
//! - **Memory**: 4 bytes (f32 aspect_ratio)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderAspectRatio;
//!
//! // 16:9 aspect ratio (widescreen)
//! let widescreen = RenderAspectRatio::new(16.0 / 9.0);
//!
//! // 4:3 aspect ratio (classic TV)
//! let classic = RenderAspectRatio::new(4.0 / 3.0);
//!
//! // 1:1 aspect ratio (square)
//! let square = RenderAspectRatio::new(1.0);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx};
use crate::core::{RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that maintains an aspect ratio.
///
/// Sizes child to maintain specified width-to-height ratio while
/// fitting within parent constraints.
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
/// - **Video/Image display**: Maintain 16:9, 4:3 ratios
/// - **Responsive design**: Proportional sizing across devices
/// - **Layout consistency**: Fixed proportions regardless of space
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderAspectRatio behavior:
/// - Calculates size to fill space while maintaining ratio
/// - Handles tight constraints by using exact size
/// - Prioritizes width-based sizing, falls back to height-based
/// - Constrains final size to parent bounds
#[derive(Debug)]
pub struct RenderAspectRatio {
    /// The aspect ratio to maintain (width / height)
    pub aspect_ratio: f32,
}

impl RenderAspectRatio {
    /// Create new RenderAspectRatio
    pub fn new(aspect_ratio: f32) -> Self {
        assert!(aspect_ratio > 0.0, "Aspect ratio must be positive");
        Self { aspect_ratio }
    }

    /// Set new aspect ratio
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32) {
        assert!(aspect_ratio > 0.0, "Aspect ratio must be positive");
        self.aspect_ratio = aspect_ratio;
    }
}

impl Default for RenderAspectRatio {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl RenderObject for RenderAspectRatio {}

impl RenderBox<Single> for RenderAspectRatio {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        let constraints = ctx.constraints;
        let aspect_ratio = self.aspect_ratio;

        // Calculate size maintaining aspect ratio
        let size = if constraints.is_tight() {
            // If constraints are tight, we must use them exactly
            constraints.smallest()
        } else {
            // Try to fill available space while maintaining aspect ratio
            let width = constraints.max_width;
            let height = width / aspect_ratio;

            if height <= constraints.max_height {
                // Width-based size fits
                Size::new(width, height)
            } else {
                // Use height-based size
                let height = constraints.max_height;
                let width = height * aspect_ratio;
                Size::new(width, height)
            }
        };

        // Constrain to bounds
        let final_size = constraints.constrain(size);

        // Layout child with tight constraints
        ctx.layout_child(ctx.single_child(), BoxConstraints::tight(final_size))?;

        Ok(final_size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Simply paint child - no transformation needed
        ctx.paint_child(ctx.single_child(), ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_aspect_ratio_new() {
        let aspect = RenderAspectRatio::new(16.0 / 9.0);
        assert!((aspect.aspect_ratio - 16.0 / 9.0).abs() < 0.001);
    }

    #[test]
    fn test_render_aspect_ratio_set() {
        let mut aspect = RenderAspectRatio::new(16.0 / 9.0);
        aspect.set_aspect_ratio(4.0 / 3.0);
        assert!((aspect.aspect_ratio - 4.0 / 3.0).abs() < 0.001);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_aspect_ratio_negative() {
        RenderAspectRatio::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "Aspect ratio must be positive")]
    fn test_aspect_ratio_zero() {
        RenderAspectRatio::new(0.0);
    }

    #[test]
    fn test_render_aspect_ratio_default() {
        let aspect = RenderAspectRatio::default();
        assert_eq!(aspect.aspect_ratio, 1.0);
    }
}
