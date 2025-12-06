//! RenderConstraintsTransformBox - Apply custom transform to constraints
//!
//! Allows transforming the constraints passed to the child via a callback function.
//! This enables advanced constraint manipulation scenarios like:
//! - Removing max height to allow infinite scrolling
//! - Converting tight constraints to loose
//! - Applying custom constraint logic based on parent constraints
//!
//! The parent tries to match the child's size but respects its own constraints.

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
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

/// A render object that applies a custom transformation to constraints before passing them to the child.
///
/// This is useful for advanced constraint manipulation scenarios where the child needs
/// different constraints than what the parent provides.
///
/// # Layout Algorithm
///
/// 1. Apply `constraints_transform` to incoming constraints
/// 2. Layout child with transformed constraints
/// 3. Parent tries to adopt child's size (within parent's original constraints)
/// 4. If parent size != child size, align child using `alignment`
///
/// # Use Cases
///
/// - **Remove max constraints**: Allow child to be unbounded in one dimension
/// - **Loosen tight constraints**: Convert exact size requirement to range
/// - **Custom constraint logic**: Apply business rules to constraint transformation
///
/// # Example
///
/// ```rust,ignore
/// // Remove max height constraint to allow vertical scrolling
/// let transform = RenderConstraintsTransformBox::new(Box::new(|constraints| {
///     BoxConstraints::new(
///         constraints.min_width,
///         constraints.max_width,
///         constraints.min_height,
///         f32::INFINITY, // Remove max height
///     )
/// }));
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
    /// let transform = Box::new(|c: BoxConstraints| c.loosen());
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
        let child_id = *ctx.children.single();

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
        let child_id = *ctx.children.single();

        // Calculate child offset based on alignment (if sizes differ)
        let child_offset = if self.cached_parent_size != self.cached_child_size {
            self.alignment
                .calculate_offset(self.cached_child_size, self.cached_parent_size)
        } else {
            Offset::ZERO
        };

        // Paint child at calculated offset
        // Note: Child may overflow if transformed constraints allowed larger size
        let _ = ctx.paint_child(child_id, ctx.offset + child_offset);
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
