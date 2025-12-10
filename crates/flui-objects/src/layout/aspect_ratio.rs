//! RenderAspectRatio - Maintains specific width-to-height aspect ratio
//!
//! Implements Flutter's aspect ratio container that sizes child to maintain
//! a specific width-to-height ratio. Calculates size to fill available space
//! while preserving the aspect ratio, using tight constraints for the child.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderAspectRatio` | `RenderAspectRatio` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `aspect_ratio` | `aspectRatio` property (width / height as f64) |
//! | `set_aspect_ratio()` | `aspectRatio = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Check constraint tightness**
//!    - If constraints are tight (min == max): use exact size
//!    - Otherwise: calculate optimal size within constraints
//!
//! 2. **Calculate dimensions maintaining aspect ratio**
//!    - Try width-based: `width = max_width`, `height = width / aspect_ratio`
//!    - If height exceeds max_height: switch to height-based sizing
//!    - Height-based: `height = max_height`, `width = height * aspect_ratio`
//!
//! 3. **Constrain final size**
//!    - Ensure size satisfies parent constraints
//!    - May adjust slightly if calculated size is out of bounds
//!
//! 4. **Layout child with tight constraints**
//!    - Child receives tight constraints (exact size required)
//!    - Child must match calculated dimensions exactly
//!
//! # Paint Protocol
//!
//! 1. **Paint child directly**
//!    - Child painted at parent offset
//!    - No transformation or alignment needed (child fills exactly)
//!
//! # Performance
//!
//! - **Layout**: O(1) - constant-time aspect ratio calculation + single child layout
//! - **Paint**: O(1) - direct child paint at offset (no transformation)
//! - **Memory**: 4 bytes (f32 aspect_ratio)
//!
//! # Use Cases
//!
//! - **Video/Image display**: Maintain 16:9, 4:3, 21:9 ratios
//! - **Responsive design**: Proportional sizing across different screen sizes
//! - **Layout consistency**: Fixed proportions regardless of available space
//! - **Media players**: Letterboxing/pillarboxing for content
//! - **Image galleries**: Uniform aspect ratios in grids
//! - **Profile pictures**: Circular or square aspect-constrained avatars
//!
//! # Common Aspect Ratios
//!
//! ```text
//! 16:9 = 1.777... (widescreen, HD, Full HD)
//! 4:3 = 1.333... (classic TV, iPad)
//! 21:9 = 2.333... (ultrawide cinema)
//! 1:1 = 1.0 (square, Instagram)
//! 3:2 = 1.5 (classic photography)
//! 9:16 = 0.5625 (portrait phone, TikTok)
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderFractionallySizedBox**: AspectRatio uses ratio, FractionallySizedBox uses percentages
//! - **vs RenderSizedBox**: SizedBox forces exact dimensions, AspectRatio maintains proportion
//! - **vs RenderConstrainedBox**: ConstrainedBox sets min/max, AspectRatio enforces ratio
//! - **vs RenderAlign**: Align positions child, AspectRatio sizes child
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderAspectRatio;
//!
//! // 16:9 aspect ratio (widescreen video)
//! let widescreen = RenderAspectRatio::new(16.0 / 9.0);
//!
//! // 4:3 aspect ratio (classic TV)
//! let classic = RenderAspectRatio::new(4.0 / 3.0);
//!
//! // 1:1 aspect ratio (square, Instagram)
//! let square = RenderAspectRatio::new(1.0);
//!
//! // 9:16 aspect ratio (portrait phone)
//! let portrait = RenderAspectRatio::new(9.0 / 16.0);
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx};
use flui_rendering::{RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that maintains a specific aspect ratio.
///
/// Sizes child to maintain specified width-to-height ratio while
/// fitting within parent constraints. Calculates optimal size to
/// fill available space, then forces child to match exactly using
/// tight constraints.
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
/// **Constraint Modifier with Aspect Ratio** - Calculates size maintaining
/// ratio, then forces child to exact size using tight constraints.
///
/// # Use Cases
///
/// - **Video/Image display**: Maintain 16:9, 4:3, 21:9 ratios
/// - **Responsive design**: Proportional sizing across screen sizes
/// - **Layout consistency**: Fixed proportions regardless of space
/// - **Media players**: Letterboxing/pillarboxing for content
/// - **Image galleries**: Uniform aspect ratios in grids
/// - **Profile pictures**: Aspect-constrained avatars
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderAspectRatio behavior:
/// - Calculates size to fill space while maintaining ratio
/// - Handles tight constraints by using exact size
/// - Prioritizes width-based sizing, falls back to height-based
/// - Constrains final size to parent bounds
/// - Uses tight constraints for child (exact size match)
/// - Extends RenderProxyBox base class
///
/// # Sizing Strategy
///
/// The algorithm tries width-based sizing first:
/// 1. `width = max_width`, `height = width / aspect_ratio`
/// 2. If `height > max_height`: switch to height-based
/// 3. `height = max_height`, `width = height * aspect_ratio`
///
/// This ensures the child fills as much space as possible while
/// maintaining the aspect ratio.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAspectRatio;
///
/// // 16:9 widescreen
/// let widescreen = RenderAspectRatio::new(16.0 / 9.0);
///
/// // 1:1 square
/// let square = RenderAspectRatio::new(1.0);
///
/// // 9:16 portrait
/// let portrait = RenderAspectRatio::new(9.0 / 16.0);
/// ```
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
        ctx.layout_child(ctx.single_child(), BoxConstraints::tight(final_size), true)?;

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
