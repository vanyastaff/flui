//! RenderStack - layering container

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::MultiRender;
use flui_engine::{BoxedLayer, layer::pool};
use flui_types::layout::StackFit;
use flui_types::{Alignment, Offset, Size, constraints::BoxConstraints};

/// RenderObject for stack layout (layering)
///
/// Stack allows positioning children on top of each other. Children can be:
/// - **Non-positioned**: Sized according to the stack's fit and aligned
/// - **Positioned**: Placed at specific positions using StackParentData
///
/// # Features
///
/// - StackParentData for positioned children
/// - Positioned widget support (top, left, right, bottom, width, height)
/// - Offset caching for performance
/// - Default hit_test_children via ParentDataWithOffset
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderStack;
///
/// let mut stack = RenderStack::new();
/// ```
#[derive(Debug)]
pub struct RenderStack {
    /// How to align non-positioned children
    pub alignment: Alignment,
    /// How to size non-positioned children
    pub fit: StackFit,

    // Cache for paint
    child_sizes: Vec<Size>,
    child_offsets: Vec<Offset>,
}

impl RenderStack {
    /// Create new stack
    pub fn new() -> Self {
        Self {
            alignment: Alignment::TOP_LEFT,
            fit: StackFit::default(),
            child_sizes: Vec::new(),
            child_offsets: Vec::new(),
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(alignment: Alignment) -> Self {
        Self {
            alignment,
            fit: StackFit::default(),
            child_sizes: Vec::new(),
            child_offsets: Vec::new(),
        }
    }

    /// Set new alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }

    /// Set new fit
    pub fn set_fit(&mut self, fit: StackFit) {
        self.fit = fit;
    }

    /// Compute constraints for a positioned child based on its StackParentData
    ///
    /// Calculates appropriate BoxConstraints for a child based on its positioning parameters:
    /// - If left AND right are specified → width is fixed
    /// - If only width is specified → width is fixed
    /// - If top AND bottom are specified → height is fixed
    /// - If only height is specified → height is fixed
    /// - Otherwise → loose constraints
    ///
    /// # Example Scenarios:
    ///
    /// ```rust,ignore
    /// // left: 10, right: 20, parent width: 400
    /// // → child width must be: 400 - 10 - 20 = 370
    ///
    /// // top: 10, height: 50
    /// // → child height must be: 50
    ///
    /// // left: 10 (no right, no width)
    /// // → child width can be anything (loose)
    /// ```
    fn compute_positioned_constraints(
        stack_data: &crate::parent_data::StackParentData,
        parent_constraints: BoxConstraints,
    ) -> BoxConstraints {
        let parent_width = parent_constraints.max_width;
        let parent_height = parent_constraints.max_height;

        // Compute width constraints
        let (min_width, max_width) = if let Some(width) = stack_data.width {
            // Explicit width
            (width, width)
        } else if let (Some(left), Some(right)) = (stack_data.left, stack_data.right) {
            // Both left and right → width is determined
            let w = (parent_width - left - right).max(0.0);
            (w, w)
        } else {
            // Width is flexible
            (0.0, parent_width)
        };

        // Compute height constraints
        let (min_height, max_height) = if let Some(height) = stack_data.height {
            // Explicit height
            (height, height)
        } else if let (Some(top), Some(bottom)) = (stack_data.top, stack_data.bottom) {
            // Both top and bottom → height is determined
            let h = (parent_height - top - bottom).max(0.0);
            (h, h)
        } else {
            // Height is flexible
            (0.0, parent_height)
        };

        BoxConstraints::new(min_width, max_width, min_height, max_height)
    }

    /// Calculate child offset based on StackParentData
    fn calculate_child_offset(
        child_size: Size,
        stack_size: Size,
        alignment: Alignment,
        parent_data: Option<&crate::parent_data::StackParentData>,
    ) -> Offset {
        if let Some(stack_data) = parent_data
            && stack_data.is_positioned()
        {
            // Positioned child - calculate position from edges
            let x;
            let y;

            // Calculate x position
            if let Some(left) = stack_data.left {
                x = left;
            } else if let Some(right) = stack_data.right {
                x = stack_size.width - child_size.width - right;
            } else {
                // Center horizontally if no left/right specified
                x = (stack_size.width - child_size.width) / 2.0;
            }

            // Calculate y position
            if let Some(top) = stack_data.top {
                y = top;
            } else if let Some(bottom) = stack_data.bottom {
                y = stack_size.height - child_size.height - bottom;
            } else {
                // Center vertically if no top/bottom specified
                y = (stack_size.height - child_size.height) / 2.0;
            }

            return Offset::new(x, y);
        }

        // Non-positioned child - use alignment
        alignment.calculate_offset(child_size, stack_size)
    }
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiRender for RenderStack {
    fn layout(
        &mut self,
        tree: &ElementTree,
        child_ids: &[ElementId],
        constraints: BoxConstraints,
    ) -> Size {
        if child_ids.is_empty() {
            self.child_sizes.clear();
            self.child_offsets.clear();
            return constraints.smallest();
        }

        // Clear caches
        self.child_sizes.clear();
        self.child_offsets.clear();

        // Layout all children and track max size
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;

        for child in child_ids.iter().copied() {
            // TODO: Implement tree.parent_data() method to query parent data from elements
            // Check if child has StackParentData and is positioned
            let stack_data: Option<&crate::parent_data::StackParentData> = None; // tree.parent_data::<crate::parent_data::StackParentData>(child);

            let child_constraints = if let Some(data) = stack_data
                && data.is_positioned()
            {
                // Child is positioned - use computed constraints based on positioning
                Self::compute_positioned_constraints(data, constraints)
            } else {
                // Child is not positioned - use fit-based constraints
                match self.fit {
                    StackFit::Loose => constraints.loosen(),
                    StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
                    StackFit::Passthrough => constraints,
                }
            };

            let child_size = tree.layout_child(child, child_constraints);
            self.child_sizes.push(child_size);
            max_width = max_width.max(child_size.width);
            max_height = max_height.max(child_size.height);
        }

        // Determine final stack size
        let size = match self.fit {
            StackFit::Expand => constraints.biggest(),
            _ => Size::new(
                max_width.clamp(constraints.min_width, constraints.max_width),
                max_height.clamp(constraints.min_height, constraints.max_height),
            ),
        };

        // Calculate and save child offsets
        for (i, &_child) in child_ids.iter().enumerate() {
            let child_size = self.child_sizes[i];
            // TODO: Implement tree.parent_data() method to query parent data from elements
            let stack_data: Option<&crate::parent_data::StackParentData> = None; // tree.parent_data::<crate::parent_data::StackParentData>(child);

            // Use the existing calculate_child_offset function
            let child_offset =
                Self::calculate_child_offset(child_size, size, self.alignment, stack_data);
            self.child_offsets.push(child_offset);
        }

        size
    }

    fn paint(&self, tree: &ElementTree, child_ids: &[ElementId], offset: Offset) -> BoxedLayer {
        let mut container = pool::acquire_container();

        // Paint children in order (first child in back, last child on top)
        for (i, &child_id) in child_ids.iter().enumerate() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);

            // Paint child with combined offset
            let child_layer = tree.paint_child(child_id, offset + child_offset);
            container.add_child(child_layer);
        }

        Box::new(container)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_new() {
        let stack = RenderStack::new();
        assert_eq!(stack.alignment, Alignment::TOP_LEFT);
        assert_eq!(stack.fit, StackFit::Loose);
    }

    #[test]
    fn test_stack_with_alignment() {
        let stack = RenderStack::with_alignment(Alignment::CENTER);
        assert_eq!(stack.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_stack_set_alignment() {
        let mut stack = RenderStack::new();
        stack.set_alignment(Alignment::CENTER);
        assert_eq!(stack.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_stack_set_fit() {
        let mut stack = RenderStack::new();
        stack.set_fit(StackFit::Expand);
        assert_eq!(stack.fit, StackFit::Expand);
    }

    #[test]
    fn test_stack_fit_variants() {
        assert_eq!(StackFit::Loose, StackFit::Loose);
        assert_eq!(StackFit::Expand, StackFit::Expand);
        assert_eq!(StackFit::Passthrough, StackFit::Passthrough);

        assert_ne!(StackFit::Loose, StackFit::Expand);
    }
}
