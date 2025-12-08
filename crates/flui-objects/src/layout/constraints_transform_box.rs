//! RenderConstraintsTransformBox - Apply custom transform to constraints via callback
//!
//! Implements Flutter's constraint transformation pattern that applies a custom
//! function to transform parent constraints before passing to child. Enables
//! advanced constraint manipulation like removing max constraints, loosening
//! tight constraints, or applying business-rule-based constraint logic.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderConstraintsTransformBox` | Similar to `RenderConstraints` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `constraints_transform` | Transform function (BoxConstraints → BoxConstraints) |
//! | `alignment` | `alignment` property (AlignmentGeometry) |
//! | `unbounded_width()` | Remove max width constraint (infinite width) |
//! | `unbounded_height()` | Remove max height constraint (infinite height) |
//! | `loosen()` | Convert tight to loose constraints (min=0, same max) |
//! | `tighten()` | Force child to biggest size (tight at max) |
//!
//! # Layout Protocol
//!
//! 1. **Apply constraint transform**
//!    - Call transform function with parent constraints
//!    - Function returns modified constraints for child
//!    - Transform can do arbitrary constraint manipulation
//!
//! 2. **Layout child**
//!    - Child laid out with transformed constraints
//!    - Child determines size within transformed bounds
//!    - Child size cached for alignment calculation
//!
//! 3. **Calculate parent size**
//!    - Parent tries to match child size
//!    - Parent size constrained to original parent constraints
//!    - parent_size = parent_constraints.constrain(child_size)
//!
//! 4. **Return constrained size**
//!    - If parent_size == child_size: perfect fit
//!    - If parent_size != child_size: alignment needed during paint
//!
//! # Paint Protocol
//!
//! 1. **Calculate alignment offset** (if sizes differ)
//!    - If parent_size != child_size: apply alignment
//!    - Offset = alignment.calculate_offset(child_size, parent_size)
//!    - Otherwise: offset = (0, 0)
//!
//! 2. **Paint child at offset**
//!    - Child painted at parent offset + alignment offset
//!    - May overflow if transform allowed larger child
//!    - No clipping applied
//!
//! # Performance
//!
//! - **Layout**: O(1) - transform function call + single child layout
//! - **Paint**: O(1) - conditional alignment + child paint
//! - **Memory**: ~32 bytes (Box<dyn Fn> + Alignment + 2 × Size cache)
//!
//! # Use Cases
//!
//! - **Remove max constraints**: Allow unbounded scrolling (vertical/horizontal)
//! - **Loosen tight constraints**: Give child sizing freedom
//! - **Custom constraint logic**: Business rules for constraint transformation
//! - **Viewport unbounding**: Remove constraints for scrollable areas
//! - **Constraint debugging**: Log and analyze constraint flow
//! - **Conditional transforms**: Apply rules based on constraint values
//!
//! # Common Transforms
//!
//! ## Unbounded Width
//! ```text
//! Input:  BoxConstraints(min: 0-400, max: 0-600)
//! Output: BoxConstraints(min: 0-∞, max: 0-600)
//! Use case: Horizontal scrolling
//! ```
//!
//! ## Unbounded Height
//! ```text
//! Input:  BoxConstraints(min: 0-400, max: 0-600)
//! Output: BoxConstraints(min: 0-400, max: 0-∞)
//! Use case: Vertical scrolling
//! ```
//!
//! ## Loosen
//! ```text
//! Input:  BoxConstraints(min: 100-100, max: 100-100) [tight]
//! Output: BoxConstraints(min: 0-0, max: 100-100) [loose]
//! Use case: Give child sizing freedom
//! ```
//!
//! ## Tighten
//! ```text
//! Input:  BoxConstraints(min: 0-400, max: 0-600)
//! Output: BoxConstraints(min: 400-400, max: 600-600) [tight at max]
//! Use case: Force child to fill available space
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderConstrainedOverflowBox**: OverflowBox uses fixed values, TransformBox uses callback
//! - **vs RenderConstrainedBox**: ConstrainedBox adds constraints, TransformBox modifies them
//! - **vs RenderIntrinsicWidth/Height**: Intrinsic uses infinite, TransformBox is flexible
//! - **vs RenderLimitedBox**: LimitedBox caps infinite, TransformBox is arbitrary
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::{RenderConstraintsTransformBox, BoxConstraintsTransform};
//! use flui_types::{Alignment, BoxConstraints};
//!
//! // Remove max height for vertical scrolling
//! let scrollable = RenderConstraintsTransformBox::unbounded_height();
//!
//! // Loosen tight constraints
//! let flexible = RenderConstraintsTransformBox::loosen();
//!
//! // Custom transform: double max width
//! let custom = RenderConstraintsTransformBox::new(
//!     BoxConstraintsTransform::new(|c| {
//!         BoxConstraints::new(
//!             c.min_width,
//!             c.max_width * 2.0,
//!             c.min_height,
//!             c.max_height,
//!         )
//!     })
//! ).with_alignment(Alignment::TOP_LEFT);
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::{Alignment, BoxConstraints, Offset, Size};
use std::fmt::Debug;

/// Function that transforms parent constraints into child constraints
///
/// Takes parent constraints and returns transformed constraints for the child.
pub struct BoxConstraintsTransform(Box<dyn Fn(BoxConstraints) -> BoxConstraints + Send + Sync>);

impl BoxConstraintsTransform {
    /// Create new transform from a function
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(BoxConstraints) -> BoxConstraints + Send + Sync + 'static,
    {
        Self(Box::new(f))
    }

    /// Apply the transform to constraints
    pub fn apply(&self, constraints: BoxConstraints) -> BoxConstraints {
        (self.0)(constraints)
    }
}

impl Debug for BoxConstraintsTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<BoxConstraintsTransform>")
    }
}

/// RenderObject that applies custom transformation to constraints via callback.
///
/// Transforms parent constraints using a callback function before passing to child.
/// Enables advanced constraint manipulation for scrolling, dynamic sizing, and
/// custom layout logic.
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
/// **Constraint Modifier with Custom Transform** - Applies callback function
/// to transform constraints, parent tries to match child size.
///
/// # Use Cases
///
/// - **Remove max constraints**: Unbounded scrolling (vertical/horizontal)
/// - **Loosen tight constraints**: Give child sizing freedom
/// - **Custom logic**: Business rules for constraint transformation
/// - **Viewport unbounding**: Remove constraints for scrollable areas
/// - **Debug constraints**: Log and analyze constraint flow
/// - **Conditional transforms**: Apply rules based on values
///
/// # Flutter Compliance
///
/// Similar to Flutter's constraint transformation patterns:
/// - Applies custom transform to parent constraints
/// - Lays out child with transformed constraints
/// - Parent tries to match child size (within parent constraints)
/// - Alignment applied when sizes differ
/// - Common transforms: unbounded, loosen, tighten
///
/// # Transform Function
///
/// Transform function signature:
/// ```rust,ignore
/// Fn(BoxConstraints) -> BoxConstraints
/// ```
///
/// The function receives parent constraints and returns child constraints.
/// It can perform arbitrary transformations including:
/// - Removing limits (infinite max)
/// - Loosening tight constraints (min=0)
/// - Tightening loose constraints (min=max)
/// - Scaling constraint values
/// - Conditional logic based on constraint values
///
/// # Overflow Potential
///
/// Child may overflow parent if transform allows larger size than parent
/// constraints. Consider wrapping in RenderClipRect if clipping is needed.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderConstraintsTransformBox, BoxConstraintsTransform};
/// use flui_types::{Alignment, BoxConstraints};
///
/// // Vertical scrolling (remove max height)
/// let vertical = RenderConstraintsTransformBox::unbounded_height();
///
/// // Flexible sizing (loosen constraints)
/// let flexible = RenderConstraintsTransformBox::loosen();
///
/// // Custom transform
/// let custom = RenderConstraintsTransformBox::new(
///     BoxConstraintsTransform::new(|c| {
///         // Remove both max constraints
///         BoxConstraints::new(c.min_width, f32::INFINITY, c.min_height, f32::INFINITY)
///     })
/// ).with_alignment(Alignment::CENTER);
/// ```
#[derive(Debug)]
pub struct RenderConstraintsTransformBox {
    /// Function that transforms parent constraints into child constraints
    constraints_transform: BoxConstraintsTransform,
    /// How to align the child if parent and child sizes differ
    pub alignment: Alignment,
    /// Cached parent size for paint phase
    cached_parent_size: Size,
    /// Cached child size for paint phase
    cached_child_size: Size,
}

impl RenderConstraintsTransformBox {
    /// Create new constraints transform box with transform function
    ///
    /// # Arguments
    /// * `constraints_transform` - Function to transform parent constraints
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let transform = BoxConstraintsTransform::new(|c| c.loosen());
    /// let box = RenderConstraintsTransformBox::new(transform);
    /// ```
    pub fn new(constraints_transform: BoxConstraintsTransform) -> Self {
        Self {
            constraints_transform,
            alignment: Alignment::CENTER,
            cached_parent_size: Size::ZERO,
            cached_child_size: Size::ZERO,
        }
    }

    /// Set alignment for child positioning when sizes differ
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Create a transform that removes max width constraint
    ///
    /// Useful for allowing horizontal scrolling.
    pub fn unbounded_width() -> Self {
        Self::new(BoxConstraintsTransform::new(|constraints| {
            BoxConstraints::new(
                constraints.min_width,
                f32::INFINITY,
                constraints.min_height,
                constraints.max_height,
            )
        }))
    }

    /// Create a transform that removes max height constraint
    ///
    /// Useful for allowing vertical scrolling.
    pub fn unbounded_height() -> Self {
        Self::new(BoxConstraintsTransform::new(|constraints| {
            BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                constraints.min_height,
                f32::INFINITY,
            )
        }))
    }

    /// Create a transform that loosens all constraints
    ///
    /// Converts tight constraints (min == max) to loose (min = 0, same max).
    pub fn loosen() -> Self {
        Self::new(BoxConstraintsTransform::new(|constraints| {
            constraints.loosen()
        }))
    }

    /// Create a transform that tightens to biggest size
    ///
    /// Forces child to be exactly the maximum size allowed by parent.
    pub fn tighten() -> Self {
        Self::new(BoxConstraintsTransform::new(|constraints| {
            BoxConstraints::tight(constraints.biggest())
        }))
    }
}

impl RenderObject for RenderConstraintsTransformBox {}

impl RenderBox<Single> for RenderConstraintsTransformBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Apply transform to parent constraints
        let child_constraints = self.constraints_transform.apply(ctx.constraints);

        // Layout child with transformed constraints
        let child_size = ctx.layout_child(child_id, child_constraints)?;
        self.cached_child_size = child_size;

        // Parent tries to match child size, but respects its own constraints
        let parent_size = ctx.constraints.constrain(child_size);
        self.cached_parent_size = parent_size;

        Ok(parent_size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Calculate child offset based on alignment (if sizes differ)
        let child_offset = if self.cached_parent_size != self.cached_child_size {
            self.alignment
                .calculate_offset(self.cached_child_size, self.cached_parent_size)
        } else {
            Offset::ZERO
        };

        // Paint child at calculated offset
        // Note: Child may overflow if transformed constraints allowed larger size
        ctx.paint_child(child_id, ctx.offset + child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unbounded_width() {
        let transform_box = RenderConstraintsTransformBox::unbounded_width();
        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);

        let child = transform_box.constraints_transform.apply(parent);

        assert_eq!(child.min_width, 50.0);
        assert_eq!(child.max_width, f32::INFINITY);
        assert_eq!(child.min_height, 30.0);
        assert_eq!(child.max_height, 150.0);
    }

    #[test]
    fn test_unbounded_height() {
        let transform_box = RenderConstraintsTransformBox::unbounded_height();
        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);

        let child = transform_box.constraints_transform.apply(parent);

        assert_eq!(child.min_width, 50.0);
        assert_eq!(child.max_width, 200.0);
        assert_eq!(child.min_height, 30.0);
        assert_eq!(child.max_height, f32::INFINITY);
    }

    #[test]
    fn test_loosen() {
        let transform_box = RenderConstraintsTransformBox::loosen();
        let parent = BoxConstraints::tight(Size::new(100.0, 100.0));

        let child = transform_box.constraints_transform.apply(parent);

        assert_eq!(child.min_width, 0.0);
        assert_eq!(child.max_width, 100.0);
        assert_eq!(child.min_height, 0.0);
        assert_eq!(child.max_height, 100.0);
    }

    #[test]
    fn test_tighten() {
        let transform_box = RenderConstraintsTransformBox::tighten();
        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);

        let child = transform_box.constraints_transform.apply(parent);

        // Should be tight at biggest size
        assert_eq!(child.min_width, 200.0);
        assert_eq!(child.max_width, 200.0);
        assert_eq!(child.min_height, 150.0);
        assert_eq!(child.max_height, 150.0);
    }

    #[test]
    fn test_custom_transform() {
        let transform_box = RenderConstraintsTransformBox::new(BoxConstraintsTransform::new(|c| {
            // Custom: double the max width
            BoxConstraints::new(c.min_width, c.max_width * 2.0, c.min_height, c.max_height)
        }));

        let parent = BoxConstraints::new(50.0, 100.0, 30.0, 150.0);
        let child = transform_box.constraints_transform.apply(parent);

        assert_eq!(child.max_width, 200.0); // Doubled
    }

    #[test]
    fn test_with_alignment() {
        let transform_box =
            RenderConstraintsTransformBox::loosen().with_alignment(Alignment::TOP_LEFT);

        assert_eq!(transform_box.alignment, Alignment::TOP_LEFT);
    }
}
