//! RenderStack - layering container

use flui_types::{Offset, Size, constraints::BoxConstraints, Alignment};
use flui_types::layout::StackFit;
use flui_core::DynRenderObject;
use crate::core::{ContainerRenderBox, RenderBoxMixin};

/// Data for RenderStack
#[derive(Debug, Clone, PartialEq)]
pub struct StackData {
    /// How to align non-positioned children
    pub alignment: Alignment,
    /// How to size non-positioned children
    pub fit: StackFit,
}

impl StackData {
    /// Create new stack data
    pub fn new() -> Self {
        Self {
            alignment: Alignment::TOP_LEFT,
            fit: StackFit::default(),
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(alignment: Alignment) -> Self {
        Self {
            alignment,
            fit: StackFit::default(),
        }
    }
}

impl Default for StackData {
    fn default() -> Self {
        Self::new()
    }
}

/// RenderObject for stack layout (layering)
///
/// Stack allows positioning children on top of each other. Children can be:
/// - **Non-positioned**: Sized according to the stack's fit and aligned
/// - **Positioned**: Placed at specific positions using StackParentData
///
/// # Features
///
/// - ✅ StackParentData for positioned children
/// - ✅ Positioned widget support (top, left, right, bottom, width, height)
/// - ✅ Offset caching for performance
/// - ✅ Default hit_test_children via ParentDataWithOffset
/// - 🚧 Overflow handling (future)
/// - 🚧 Clip behavior (future)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{ContainerRenderBox, objects::layout::StackData};
///
/// let mut stack = ContainerRenderBox::new(StackData::new());
/// ```
pub type RenderStack = ContainerRenderBox<StackData>;

// ===== Public API =====

impl RenderStack {
    /// Get reference to type-specific data
    pub fn data(&self) -> &StackData {
        &self.data
    }

    /// Get mutable reference to type-specific data
    pub fn data_mut(&mut self) -> &mut StackData {
        &mut self.data
    }

    /// Get the alignment
    pub fn alignment(&self) -> Alignment {
        self.data().alignment
    }

    /// Set new alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        if self.data().alignment != alignment {
            self.data_mut().alignment = alignment;
            self.mark_needs_layout();
        }
    }

    /// Get the fit
    pub fn fit(&self) -> StackFit {
        self.data().fit
    }

    /// Set new fit
    pub fn set_fit(&mut self, fit: StackFit) {
        if self.data().fit != fit {
            self.data_mut().fit = fit;
            self.mark_needs_layout();
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderStack {
    fn layout(&self, state: &mut flui_core::RenderState, constraints: BoxConstraints, ctx: &flui_core::RenderContext) -> Size {
        // Store constraints
        *state.constraints.lock() = Some(constraints);

        let fit = self.data().fit;
        let children_ids = ctx.children();
        let child_count = children_ids.len();

        if children_ids.is_empty() {
            // No children - use smallest size
            let size = constraints.smallest();
            *state.size.lock() = Some(size);
            state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);
            return size;
        }

        // Layout algorithm:
        // 1. Separate positioned and non-positioned children via StackParentData
        // 2. Layout non-positioned children with fit-based constraints
        // 3. Layout positioned children with position-based constraints

        // Layout all children and track max size
        // CRITICAL: Pass child_count to enable proper cache invalidation when children change
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;

        for &child_id in children_ids.iter() {
            // Check if child is positioned
            let is_positioned = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
                if let Some(stack_data) = parent_data.downcast_ref::<crate::parent_data::StackParentData>() {
                    stack_data.is_positioned()
                } else {
                    false
                }
            } else {
                false
            };

            let child_constraints = if is_positioned {
                // Positioned children get constraints based on their positioning parameters
                // Calculate constraints from StackParentData
                if let Some(parent_data) = ctx.tree().parent_data(child_id) {
                    if let Some(stack_data) = parent_data.downcast_ref::<crate::parent_data::StackParentData>() {
                        Self::compute_positioned_constraints(stack_data, constraints)
                    } else {
                        constraints.loosen()
                    }
                } else {
                    constraints.loosen()
                }
            } else {
                // Non-positioned children use fit-based constraints
                match fit {
                    StackFit::Loose => constraints.loosen(),
                    StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
                    StackFit::Passthrough => constraints,
                }
            };

            // Use cached layout with child_count for proper cache invalidation
            let child_size = ctx.layout_child_cached(child_id, child_constraints, Some(child_count));
            max_width = max_width.max(child_size.width);
            max_height = max_height.max(child_size.height);
        }

        // Determine final stack size
        let size = match fit {
            StackFit::Expand => constraints.biggest(),
            _ => Size::new(
                max_width.clamp(constraints.min_width, constraints.max_width),
                max_height.clamp(constraints.min_height, constraints.max_height),
            ),
        };

        // ========== Calculate and save child offsets in StackParentData ==========
        // This avoids recalculating positions in paint() and hit_test()

        let alignment = self.data().alignment;

        for &child_id in children_ids.iter() {
            let child_size = ctx.child_size(child_id);

            // Calculate child offset based on StackParentData (if positioned) or alignment
            let child_offset = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
                if let Some(stack_data) = parent_data.downcast_ref::<crate::parent_data::StackParentData>() {
                    if stack_data.is_positioned() {
                        // Positioned child - calculate position from edges
                        let mut x = 0.0;
                        let mut y = 0.0;

                        // Calculate x position
                        if let Some(left) = stack_data.left {
                            x = left;
                        } else if let Some(right) = stack_data.right {
                            x = size.width - child_size.width - right;
                        } else {
                            // Center horizontally if no left/right specified
                            x = (size.width - child_size.width) / 2.0;
                        }

                        // Calculate y position
                        if let Some(top) = stack_data.top {
                            y = top;
                        } else if let Some(bottom) = stack_data.bottom {
                            y = size.height - child_size.height - bottom;
                        } else {
                            // Center vertically if no top/bottom specified
                            y = (size.height - child_size.height) / 2.0;
                        }

                        Offset::new(x, y)
                    } else {
                        // Non-positioned child - use alignment
                        alignment.calculate_offset(child_size, size)
                    }
                } else {
                    // No StackParentData - use alignment
                    alignment.calculate_offset(child_size, size)
                }
            } else {
                // No parent data - use alignment
                alignment.calculate_offset(child_size, size)
            };

            // Save offset in StackParentData
            if let Some(mut parent_data) = ctx.tree().parent_data_mut(child_id) {
                if let Some(stack_data) = parent_data.downcast_mut::<crate::parent_data::StackParentData>() {
                    stack_data.offset = child_offset;
                }
            }
        }

        // Store size and clear needs_layout flag
        *state.size.lock() = Some(size);
        state.flags.lock().remove(flui_core::RenderFlags::NEEDS_LAYOUT);

        size
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

    fn paint(&self, _state: &flui_core::RenderState, painter: &egui::Painter, offset: Offset, ctx: &flui_core::RenderContext) {
        let children_ids = ctx.children();

        // Paint children in order (first child in back, last child on top)
        for &child_id in children_ids {
            // Read offset from StackParentData (calculated during layout)
            let local_offset = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
                if let Some(stack_data) = parent_data.downcast_ref::<crate::parent_data::StackParentData>() {
                    stack_data.offset
                } else {
                    Offset::ZERO
                }
            } else {
                Offset::ZERO
            };

            // Add parent offset to local offset
            let paint_offset = Offset::new(
                offset.dx + local_offset.dx,
                offset.dy + local_offset.dy,
            );

            ctx.paint_child(child_id, painter, paint_offset);
        }
    }

    // Delegate all other methods to RenderBoxMixin
    delegate_to_mixin!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_data_new() {
        let data = StackData::new();
        assert_eq!(data.alignment, Alignment::TOP_LEFT);
        assert_eq!(data.fit, StackFit::Loose);
    }

    #[test]
    fn test_stack_data_with_alignment() {
        let data = StackData::with_alignment(Alignment::CENTER);
        assert_eq!(data.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_stack_new() {
        let stack = ContainerRenderBox::new(StackData::new());
        assert_eq!(stack.alignment(), Alignment::TOP_LEFT);
        assert_eq!(stack.fit(), StackFit::Loose);
        assert_eq!(stack.children().len(), 0);
    }

    #[test]
    fn test_render_stack_set_alignment() {
        let mut stack = ContainerRenderBox::new(StackData::new());

        stack.set_alignment(Alignment::CENTER);
        assert_eq!(stack.alignment(), Alignment::CENTER);
        assert!(stack.needs_layout());
    }

    #[test]
    fn test_render_stack_set_fit() {
        let mut stack = ContainerRenderBox::new(StackData::new());

        stack.set_fit(StackFit::Expand);
        assert_eq!(stack.fit(), StackFit::Expand);
        assert!(stack.needs_layout());
    }

    #[test]
    #[cfg(disabled_test)] // TODO: Update test to use RenderContext
    fn test_render_stack_layout_no_children() {
        let mut stack = ContainerRenderBox::new(StackData::new());
        let constraints = BoxConstraints::new(0.0, 100.0, 0.0, 100.0);

        let size = stack.layout(constraints);

        // Should use smallest size
        assert_eq!(size, Size::new(0.0, 0.0));
    }

    #[test]
    fn test_stack_fit_variants() {
        assert_eq!(StackFit::Loose, StackFit::Loose);
        assert_eq!(StackFit::Expand, StackFit::Expand);
        assert_eq!(StackFit::Passthrough, StackFit::Passthrough);

        assert_ne!(StackFit::Loose, StackFit::Expand);
    }
}
