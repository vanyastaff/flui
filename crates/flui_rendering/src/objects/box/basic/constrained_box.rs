//! RenderConstrainedBox - applies additional constraints to its child.
//!
//! This render object imposes additional constraints on its child
//! beyond those from its parent. Useful for enforcing minimum/maximum
//! sizes on widgets.

use flui_types::{Offset, Size};

use crate::constraints::BoxConstraints;
use crate::containers::BoxChild;
use crate::pipeline::PaintingContext;
use crate::traits::{RenderBox, TextBaseline};

/// A render object that imposes additional constraints on its child.
///
/// The `additional_constraints` are applied on top of constraints from
/// the parent. The effective constraints are the intersection of both.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `RenderConstrainedBox` which extends `RenderProxyBox`.
/// Like Flutter, this stores child directly and delegates size to child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::r#box::basic::RenderConstrainedBox;
/// use flui_types::BoxConstraints;
///
/// // Minimum size of 100x100
/// let constraints = BoxConstraints::new(100.0, f32::INFINITY, 100.0, f32::INFINITY);
/// let mut constrained = RenderConstrainedBox::new(constraints);
/// ```
#[derive(Debug)]
pub struct RenderConstrainedBox {
    /// The child render object using type-safe container.
    child: BoxChild,

    /// Cached size from layout.
    size: Size,

    /// Additional constraints to apply.
    additional_constraints: BoxConstraints,
}

impl RenderConstrainedBox {
    /// Creates a new render constrained box with the given constraints.
    pub fn new(additional_constraints: BoxConstraints) -> Self {
        Self {
            child: BoxChild::new(),
            size: Size::ZERO,
            additional_constraints,
        }
    }

    // ========================================================================
    // Child access (using type-safe BoxChild container)
    // ========================================================================

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&dyn RenderBox> {
        self.child.get()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut dyn RenderBox> {
        self.child.get_mut()
    }

    /// Sets the child.
    pub fn set_child(&mut self, child: Option<Box<dyn RenderBox>>) {
        self.child.clear();
        if let Some(c) = child {
            self.child.set(c);
        }
    }

    /// Takes the child out of the container.
    pub fn take_child(&mut self) -> Option<Box<dyn RenderBox>> {
        self.child.take()
    }

    // ========================================================================
    // Constraint configuration
    // ========================================================================

    /// Returns the additional constraints.
    pub fn additional_constraints(&self) -> BoxConstraints {
        self.additional_constraints
    }

    /// Sets the additional constraints.
    pub fn set_additional_constraints(&mut self, constraints: BoxConstraints) {
        if self.additional_constraints != constraints {
            self.additional_constraints = constraints;
            // In real implementation: self.mark_needs_layout();
        }
    }

    /// Returns the current size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Computes the effective constraints by enforcing additional constraints.
    pub fn effective_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        constraints.enforce(self.additional_constraints)
    }

    /// Performs layout without a child.
    pub fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
        let effective = self.effective_constraints(constraints);
        self.size = effective.smallest();
        self.size
    }

    /// Performs layout with a child size.
    pub fn perform_layout_with_child(
        &mut self,
        constraints: BoxConstraints,
        child_size: Size,
    ) -> Size {
        let effective = self.effective_constraints(constraints);
        self.size = effective.constrain(child_size);
        self.size
    }

    /// Returns constraints for the child.
    pub fn constraints_for_child(&self, constraints: BoxConstraints) -> BoxConstraints {
        self.effective_constraints(constraints)
    }

    /// Paints this render object.
    pub fn paint(&self, context: &mut PaintingContext, offset: Offset) {
        // Child would be painted at offset
        let _ = (context, offset);
    }

    /// Computes minimum intrinsic width.
    pub fn compute_min_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        let width = child_width.unwrap_or(0.0);
        self.additional_constraints
            .constrain(Size::new(width, height))
            .width
    }

    /// Computes maximum intrinsic width.
    pub fn compute_max_intrinsic_width(&self, height: f32, child_width: Option<f32>) -> f32 {
        let width = child_width.unwrap_or(f32::INFINITY);
        self.additional_constraints
            .constrain(Size::new(width, height))
            .width
    }

    /// Computes minimum intrinsic height.
    pub fn compute_min_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        let height = child_height.unwrap_or(0.0);
        self.additional_constraints
            .constrain(Size::new(width, height))
            .height
    }

    /// Computes maximum intrinsic height.
    pub fn compute_max_intrinsic_height(&self, width: f32, child_height: Option<f32>) -> f32 {
        let height = child_height.unwrap_or(f32::INFINITY);
        self.additional_constraints
            .constrain(Size::new(width, height))
            .height
    }

    /// Computes distance to baseline.
    pub fn compute_distance_to_baseline(
        &self,
        _baseline: TextBaseline,
        child_baseline: Option<f32>,
    ) -> Option<f32> {
        child_baseline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constrained_box_new() {
        let constraints = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let constrained = RenderConstrainedBox::new(constraints);

        assert_eq!(constrained.additional_constraints(), constraints);
    }

    #[test]
    fn test_constrained_box_no_child() {
        let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let mut constrained = RenderConstrainedBox::new(additional);

        let parent_constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = constrained.perform_layout(parent_constraints);

        // Without child, uses smallest of effective constraints
        assert_eq!(size.width, 50.0);
        assert_eq!(size.height, 50.0);
    }

    #[test]
    fn test_effective_constraints() {
        let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let constrained = RenderConstrainedBox::new(additional);

        let parent = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let effective = constrained.effective_constraints(parent);

        // Effective should be intersection
        assert_eq!(effective.min_width, 50.0);
        assert_eq!(effective.max_width, 150.0);
        assert_eq!(effective.min_height, 50.0);
        assert_eq!(effective.max_height, 150.0);
    }

    #[test]
    fn test_constrained_box_tight() {
        let additional = BoxConstraints::tight(Size::new(100.0, 100.0));
        let mut constrained = RenderConstrainedBox::new(additional);

        let parent_constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = constrained.perform_layout(parent_constraints);

        assert_eq!(size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_layout_with_child() {
        let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let mut constrained = RenderConstrainedBox::new(additional);

        let parent = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let child_size = Size::new(100.0, 80.0);
        let size = constrained.perform_layout_with_child(parent, child_size);

        assert_eq!(size, Size::new(100.0, 80.0));
    }
}
