//! RenderFractionallySizedBox - sizes child as percentage of parent constraints
//!
//! Used by FractionallySizedBox widget to create responsive layouts where
//! child size is a fraction (0.0 to 1.0+) of available parent space.

use flui_core::{BoxConstraints, DynRenderObject, ElementId};
use flui_types::{Alignment, Offset, Size};
use crate::RenderFlags;

/// RenderFractionallySizedBox sizes child as percentage of parent constraints
///
/// # Parameters
///
/// - `width_factor`: Optional multiplier for child width (0.0 to 1.0+ of parent.max_width)
/// - `height_factor`: Optional multiplier for child height (0.0 to 1.0+ of parent.max_height)
/// - `alignment`: Child alignment within parent (default: Alignment::CENTER)
///
/// # Layout Algorithm
///
/// 1. Compute child constraints based on factors:
///    - If width_factor is Some → child width = parent.max_width * width_factor
///    - If width_factor is None → child uses parent width constraints
///    - Same for height
/// 2. Layout child with computed constraints (tight if factors present)
/// 3. Parent size = constrain child size within parent constraints
/// 4. Compute child offset based on alignment
///
/// # Examples
///
/// ```rust
/// # use flui_rendering::RenderFractionallySizedBox;
/// # use flui_types::Alignment;
/// // Child takes 50% of parent width, full height
/// let mut render = RenderFractionallySizedBox::new(
///     Some(0.5),
///     None,
///     Alignment::CENTER
/// );
///
/// // Child takes 75% of both dimensions
/// let mut render = RenderFractionallySizedBox::new(
///     Some(0.75),
///     Some(0.75),
///     Alignment::CENTER
/// );
/// ```
#[derive(Debug)]
pub struct RenderFractionallySizedBox {
    /// Element ID for caching
    element_id: Option<ElementId>,
    /// Multiplier for child width (None = use parent constraints)
    width_factor: Option<f32>,
    /// Multiplier for child height (None = use parent constraints)
    height_factor: Option<f32>,
    /// Child alignment within parent
    alignment: Alignment,
    /// Child render object
    child: Option<Box<dyn DynRenderObject>>,
    /// Current size
    size: Size,
    /// Current constraints
    constraints: Option<BoxConstraints>,
    /// Child offset relative to parent (computed in layout)
    child_offset: Offset,
    /// Render flags (needs_layout, needs_paint, boundaries)
    flags: RenderFlags,
}

impl RenderFractionallySizedBox {
    /// Creates a new RenderFractionallySizedBox
    ///
    /// # Parameters
    ///
    /// - `width_factor`: Optional width multiplier (e.g., 0.5 for 50% width)
    /// - `height_factor`: Optional height multiplier (e.g., 0.75 for 75% height)
    /// - `alignment`: Child alignment (e.g., Alignment::CENTER)
    ///
    /// # Panics
    ///
    /// Panics if width_factor or height_factor are negative or NaN
    pub fn new(
        width_factor: Option<f32>,
        height_factor: Option<f32>,
        alignment: Alignment,
    ) -> Self {
        if let Some(factor) = width_factor {
            assert!(
                factor >= 0.0 && factor.is_finite(),
                "width_factor must be non-negative and finite, got {}",
                factor
            );
        }
        if let Some(factor) = height_factor {
            assert!(
                factor >= 0.0 && factor.is_finite(),
                "height_factor must be non-negative and finite, got {}",
                factor
            );
        }

        Self {
            element_id: None,
            width_factor,
            height_factor,
            alignment,
            child: None,
            size: Size::zero(),
            constraints: None,
            child_offset: Offset::ZERO,
            flags: RenderFlags::new(),
        }
    }

    /// Create RenderFractionallySizedBox with element ID for caching
    pub fn with_element_id(
        width_factor: Option<f32>,
        height_factor: Option<f32>,
        alignment: Alignment,
        element_id: ElementId,
    ) -> Self {
        if let Some(factor) = width_factor {
            assert!(
                factor >= 0.0 && factor.is_finite(),
                "width_factor must be non-negative and finite, got {}",
                factor
            );
        }
        if let Some(factor) = height_factor {
            assert!(
                factor >= 0.0 && factor.is_finite(),
                "height_factor must be non-negative and finite, got {}",
                factor
            );
        }

        Self {
            element_id: Some(element_id),
            width_factor,
            height_factor,
            alignment,
            child: None,
            size: Size::zero(),
            constraints: None,
            child_offset: Offset::ZERO,
            flags: RenderFlags::new(),
        }
    }

    /// Sets element ID for caching
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }

    /// Gets element ID
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Sets the child
    pub fn set_child(&mut self, child: Option<Box<dyn DynRenderObject>>) {
        self.child = child;
        self.mark_needs_layout();
    }

    /// Returns a reference to the child
    pub fn child(&self) -> Option<&dyn DynRenderObject> {
        self.child.as_deref()
    }

    /// Sets the width_factor
    ///
    /// # Panics
    ///
    /// Panics if factor is negative or NaN
    pub fn set_width_factor(&mut self, width_factor: Option<f32>) {
        if let Some(factor) = width_factor {
            assert!(
                factor >= 0.0 && factor.is_finite(),
                "width_factor must be non-negative and finite, got {}",
                factor
            );
        }
        if self.width_factor != width_factor {
            self.width_factor = width_factor;
            self.mark_needs_layout();
        }
    }

    /// Returns the width_factor
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }

    /// Sets the height_factor
    ///
    /// # Panics
    ///
    /// Panics if factor is negative or NaN
    pub fn set_height_factor(&mut self, height_factor: Option<f32>) {
        if let Some(factor) = height_factor {
            assert!(
                factor >= 0.0 && factor.is_finite(),
                "height_factor must be non-negative and finite, got {}",
                factor
            );
        }
        if self.height_factor != height_factor {
            self.height_factor = height_factor;
            self.mark_needs_layout();
        }
    }

    /// Returns the height_factor
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }

    /// Sets the alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.alignment != alignment {
            self.alignment = alignment;
            self.mark_needs_layout();
        }
    }

    /// Returns the current alignment
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Computes child constraints based on parent constraints and factors
    fn compute_child_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        let child_width = if let Some(factor) = self.width_factor {
            // Child width = parent.max_width * factor
            let width = constraints.max_width * factor;
            // Create tight constraint for width
            (width, width)
        } else {
            // Use parent width constraints as-is
            (constraints.min_width, constraints.max_width)
        };

        let child_height = if let Some(factor) = self.height_factor {
            // Child height = parent.max_height * factor
            let height = constraints.max_height * factor;
            // Create tight constraint for height
            (height, height)
        } else {
            // Use parent height constraints as-is
            (constraints.min_height, constraints.max_height)
        };

        BoxConstraints::new(child_width.0, child_width.1, child_height.0, child_height.1)
    }
}

impl DynRenderObject for RenderFractionallySizedBox {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        crate::impl_cached_layout!(self, constraints, {
            // Compute child constraints based on factors (before borrowing child)
            let child_constraints = self.compute_child_constraints(constraints);

            if let Some(child) = &mut self.child {
                // Layout child with computed constraints
                let child_size = child.layout(child_constraints);

                // Parent adopts child size (constrained within parent bounds)
                let size = constraints.constrain(child_size);

                // Compute child offset for alignment
                self.child_offset = self.alignment.calculate_offset(child_size, size);

                size
            } else {
                // Without child, use smallest size
                self.child_offset = Offset::ZERO;
                constraints.smallest()
            }
        })
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        if let Some(child) = &self.child {
            // Paint child with offset for alignment
            child.paint(painter, offset + self.child_offset);
        }
    }

    fn hit_test_children(
        &self,
        result: &mut flui_types::events::HitTestResult,
        position: Offset,
    ) -> bool {
        if let Some(child) = &self.child {
            // Adjust position for child offset
            let child_position = Offset::new(
                position.dx - self.child_offset.dx,
                position.dy - self.child_offset.dy,
            );
            child.hit_test(result, child_position)
        } else {
            false
        }
    }

    fn size(&self) -> Size {
        self.size
    }

    fn constraints(&self) -> Option<BoxConstraints> {
        self.constraints
    }

    fn needs_layout(&self) -> bool {
        self.flags.needs_layout()
    }

    fn mark_needs_layout(&mut self) {
        self.flags.mark_needs_layout();
    }

    fn needs_paint(&self) -> bool {
        self.flags.needs_paint()
    }

    fn mark_needs_paint(&mut self) {
        self.flags.mark_needs_paint();
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        if let Some(child) = &self.child {
            visitor(&**child);
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        if let Some(child) = &mut self.child {
            visitor(&mut **child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::RenderBox;

    #[test]
    fn test_render_fractionally_sized_box_new() {
        let render = RenderFractionallySizedBox::new(Some(0.5), Some(0.75), Alignment::CENTER);
        assert_eq!(render.width_factor(), Some(0.5));
        assert_eq!(render.height_factor(), Some(0.75));
        assert_eq!(render.alignment(), Alignment::CENTER);
        assert!(render.child().is_none());
    }

    #[test]
    #[should_panic(expected = "width_factor must be non-negative")]
    fn test_render_fractionally_sized_box_invalid_width_factor() {
        RenderFractionallySizedBox::new(Some(-0.5), None, Alignment::CENTER);
    }

    #[test]
    #[should_panic(expected = "height_factor must be non-negative")]
    fn test_render_fractionally_sized_box_invalid_height_factor() {
        RenderFractionallySizedBox::new(None, Some(-1.0), Alignment::CENTER);
    }

    #[test]
    fn test_render_fractionally_sized_box_width_factor_50_percent() {
        let mut render = RenderFractionallySizedBox::new(Some(0.5), None, Alignment::CENTER);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Parent constraints: 0-400 x 0-300
        let constraints = BoxConstraints::new(0.0, 400.0, 0.0, 300.0);
        let size = render.layout(constraints);

        // Child should get tight constraint: width = 400 * 0.5 = 200
        // Child (RenderBox) will use biggest() for height = 300
        // So child size should be ~200x300
        // Parent adopts child size
        assert!(size.width <= 200.1 && size.width >= 199.9, "Expected width ~200, got {}", size.width);
        assert!(size.height <= 300.1 && size.height >= 299.9, "Expected height ~300, got {}", size.height);
    }

    #[test]
    fn test_render_fractionally_sized_box_height_factor_75_percent() {
        let mut render = RenderFractionallySizedBox::new(None, Some(0.75), Alignment::CENTER);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Parent constraints: 0-400 x 0-300
        let constraints = BoxConstraints::new(0.0, 400.0, 0.0, 300.0);
        let size = render.layout(constraints);

        // Child should get tight constraint: height = 300 * 0.75 = 225
        // Child (RenderBox) will use biggest() for width = 400
        assert!(size.width <= 400.1 && size.width >= 399.9, "Expected width ~400, got {}", size.width);
        assert!(size.height <= 225.1 && size.height >= 224.9, "Expected height ~225, got {}", size.height);
    }

    #[test]
    fn test_render_fractionally_sized_box_both_factors() {
        let mut render = RenderFractionallySizedBox::new(Some(0.5), Some(0.5), Alignment::CENTER);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Parent constraints: 0-200 x 0-200
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let size = render.layout(constraints);

        // Child gets tight 100x100 (200 * 0.5)
        assert!(size.width <= 100.1 && size.width >= 99.9, "Expected width ~100, got {}", size.width);
        assert!(size.height <= 100.1 && size.height >= 99.9, "Expected height ~100, got {}", size.height);
    }

    #[test]
    fn test_render_fractionally_sized_box_no_factors() {
        let mut render = RenderFractionallySizedBox::new(None, None, Alignment::CENTER);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Parent constraints: 0-300 x 0-200
        let constraints = BoxConstraints::new(0.0, 300.0, 0.0, 200.0);
        let size = render.layout(constraints);

        // Child gets parent constraints, RenderBox uses biggest()
        assert!(size.width <= 300.1 && size.width >= 299.9, "Expected width ~300, got {}", size.width);
        assert!(size.height <= 200.1 && size.height >= 199.9, "Expected height ~200, got {}", size.height);
    }

    #[test]
    fn test_render_fractionally_sized_box_no_child() {
        let mut render = RenderFractionallySizedBox::new(Some(0.5), Some(0.5), Alignment::CENTER);

        let constraints = BoxConstraints::new(50.0, 200.0, 50.0, 200.0);
        let size = render.layout(constraints);

        // Without child, use smallest size
        assert_eq!(size, Size::new(50.0, 50.0));
    }

    #[test]
    fn test_render_fractionally_sized_box_factor_greater_than_one() {
        let mut render = RenderFractionallySizedBox::new(Some(1.5), Some(1.5), Alignment::CENTER);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        // Parent constraints: 0-100 x 0-100
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);
        let size = render.layout(constraints);

        // Child wants 100 * 1.5 = 150x150
        // But parent constraints limit to max 100x100
        assert!(size.width <= 100.1 && size.width >= 99.9, "Expected width ~100, got {}", size.width);
        assert!(size.height <= 100.1 && size.height >= 99.9, "Expected height ~100, got {}", size.height);
    }

    #[test]
    fn test_render_fractionally_sized_box_set_width_factor() {
        let mut render = RenderFractionallySizedBox::new(Some(0.5), None, Alignment::CENTER);
        render.set_width_factor(Some(0.75));
        assert_eq!(render.width_factor(), Some(0.75));
        assert!(render.needs_layout());
    }

    #[test]
    fn test_render_fractionally_sized_box_set_height_factor() {
        let mut render = RenderFractionallySizedBox::new(None, Some(0.5), Alignment::CENTER);
        render.set_height_factor(Some(0.8));
        assert_eq!(render.height_factor(), Some(0.8));
        assert!(render.needs_layout());
    }

    #[test]
    fn test_render_fractionally_sized_box_set_alignment() {
        let mut render = RenderFractionallySizedBox::new(Some(0.5), None, Alignment::CENTER);
        render.set_alignment(Alignment::TOP_LEFT);
        assert_eq!(render.alignment(), Alignment::TOP_LEFT);
        assert!(render.needs_layout());
    }

    #[test]
    fn test_render_fractionally_sized_box_visit_children() {
        let mut render = RenderFractionallySizedBox::new(Some(0.5), None, Alignment::CENTER);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_render_fractionally_sized_box_visit_children_no_child() {
        let render = RenderFractionallySizedBox::new(Some(0.5), None, Alignment::CENTER);

        let mut count = 0;
        render.visit_children(&mut |_| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_render_fractionally_sized_box_remove_child() {
        let mut render = RenderFractionallySizedBox::new(Some(0.5), None, Alignment::CENTER);
        let child = Box::new(RenderBox::new());
        render.set_child(Some(child));

        assert!(render.child().is_some());

        render.set_child(None);
        assert!(render.child().is_none());
        assert!(render.needs_layout());
    }
}
