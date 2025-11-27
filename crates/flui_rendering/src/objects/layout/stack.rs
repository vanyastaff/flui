//! RenderStack - layering container
//!
//! Flutter equivalent: `RenderStack`
//! Source: https://api.flutter.dev/flutter/rendering/RenderStack-class.html

use std::collections::HashMap;

use crate::core::{BoxProtocol, LayoutContext, PaintContext, RenderBox, StackParentData, Variable};
use flui_foundation::ElementId;
use flui_types::constraints::BoxConstraints;
use flui_types::layout::StackFit;
use flui_types::{Alignment, Offset, Size};

/// RenderObject for stack layout (layering)
///
/// Stack allows positioning children on top of each other. Children can be:
/// - **Non-positioned**: Sized according to the stack's fit and aligned
/// - **Positioned**: Placed at specific positions using StackParentData
///
/// # Layout Algorithm (Flutter-compatible)
///
/// 1. Layout non-positioned children based on `StackFit`
/// 2. Track maximum width/height from non-positioned children
/// 3. Layout positioned children with constraints derived from position values
/// 4. Final size = max of non-positioned children (or max constraints if none)
/// 5. Position non-positioned children using `alignment`
///
/// # Features
///
/// - Alignment-based positioning for non-positioned children
/// - StackFit control for child sizing (Loose, Expand, Passthrough)
/// - Positioned children via StackParentData (top/right/bottom/left/width/height)
/// - Offset caching for performance
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::objects::layout::RenderStack;
/// use flui_rendering::core::StackParentData;
///
/// let mut stack = RenderStack::new();
///
/// // Set up positioned children
/// stack.set_child_position(child1_id, StackParentData::positioned(10.0, None, None, 10.0)); // top-left
/// stack.set_child_position(child2_id, StackParentData::fill()); // fill entire stack
/// ```
#[derive(Debug)]
pub struct RenderStack {
    /// How to align non-positioned children
    pub alignment: Alignment,
    /// How to size non-positioned children
    pub fit: StackFit,

    /// Per-child positioning data (top, right, bottom, left, width, height)
    child_parent_data: HashMap<ElementId, StackParentData>,

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
            child_parent_data: HashMap::new(),
            child_sizes: Vec::new(),
            child_offsets: Vec::new(),
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(alignment: Alignment) -> Self {
        Self {
            alignment,
            fit: StackFit::default(),
            child_parent_data: HashMap::new(),
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

    // ========== STACK PARENT DATA MANAGEMENT ==========

    /// Set position data for a child.
    ///
    /// This determines where the child is positioned within the stack:
    /// - Non-positioned (None): Uses alignment-based positioning
    /// - Positioned: Uses explicit top/right/bottom/left values
    pub fn set_child_position(&mut self, child_id: ElementId, parent_data: StackParentData) {
        self.child_parent_data.insert(child_id, parent_data);
    }

    /// Get position data for a child.
    ///
    /// Returns `None` if no parent data was set (child is treated as non-positioned).
    pub fn get_child_position(&self, child_id: ElementId) -> Option<&StackParentData> {
        self.child_parent_data.get(&child_id)
    }

    /// Remove position data for a child.
    pub fn remove_child_position(&mut self, child_id: ElementId) -> Option<StackParentData> {
        self.child_parent_data.remove(&child_id)
    }

    /// Clear all position data.
    pub fn clear_child_positions(&mut self) {
        self.child_parent_data.clear();
    }

    /// Check if a child is positioned (has any position values set).
    fn is_child_positioned(&self, child_id: ElementId) -> bool {
        self.child_parent_data
            .get(&child_id)
            .map(|pd| pd.is_positioned())
            .unwrap_or(false)
    }

    /// Calculate constraints for a positioned child based on its parent data.
    ///
    /// If both top AND bottom are set, height is constrained.
    /// If both left AND right are set, width is constrained.
    /// Otherwise, dimensions are loose (0 to stack size).
    fn positioned_child_constraints(
        &self,
        parent_data: &StackParentData,
        stack_size: Size,
    ) -> BoxConstraints {
        // Calculate width constraints
        let (min_width, max_width) = if let Some(width) = parent_data.width {
            // Explicit width
            (width, width)
        } else if parent_data.left.is_some() && parent_data.right.is_some() {
            // Width derived from left + right
            let left = parent_data.left.unwrap_or(0.0);
            let right = parent_data.right.unwrap_or(0.0);
            let width = (stack_size.width - left - right).max(0.0);
            (width, width)
        } else {
            // Loose width
            (0.0, stack_size.width)
        };

        // Calculate height constraints
        let (min_height, max_height) = if let Some(height) = parent_data.height {
            // Explicit height
            (height, height)
        } else if parent_data.top.is_some() && parent_data.bottom.is_some() {
            // Height derived from top + bottom
            let top = parent_data.top.unwrap_or(0.0);
            let bottom = parent_data.bottom.unwrap_or(0.0);
            let height = (stack_size.height - top - bottom).max(0.0);
            (height, height)
        } else {
            // Loose height
            (0.0, stack_size.height)
        };

        BoxConstraints::new(min_width, max_width, min_height, max_height)
    }

    /// Calculate offset for a positioned child.
    fn positioned_child_offset(
        &self,
        parent_data: &StackParentData,
        child_size: Size,
        stack_size: Size,
    ) -> Offset {
        // X position: prefer left, fall back to right-based calculation
        let x = if let Some(left) = parent_data.left {
            left
        } else if let Some(right) = parent_data.right {
            stack_size.width - child_size.width - right
        } else {
            0.0 // Default to left edge
        };

        // Y position: prefer top, fall back to bottom-based calculation
        let y = if let Some(top) = parent_data.top {
            top
        } else if let Some(bottom) = parent_data.bottom {
            stack_size.height - child_size.height - bottom
        } else {
            0.0 // Default to top edge
        };

        Offset::new(x, y)
    }
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox<Variable> for RenderStack {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Variable, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;
        let children = ctx.children;

        // Collect child IDs for layout_child calls
        let child_ids: Vec<ElementId> = children.iter().collect();
        let child_count = child_ids.len();

        if child_count == 0 {
            self.child_sizes.clear();
            self.child_offsets.clear();
            return constraints.smallest();
        }

        // Clear caches
        self.child_sizes.clear();
        self.child_offsets.clear();
        self.child_sizes.resize(child_count, Size::ZERO);
        self.child_offsets.resize(child_count, Offset::ZERO);

        // ========== FLUTTER-COMPATIBLE STACK LAYOUT ==========
        // Pass 1: Layout non-positioned children to determine stack size
        // Pass 2: Layout positioned children with size-based constraints

        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;
        let mut has_non_positioned = false;

        // ========== PASS 1: Layout non-positioned children ==========
        for (i, &child_id) in child_ids.iter().enumerate() {
            if self.is_child_positioned(child_id) {
                // Skip positioned children in pass 1
                continue;
            }

            has_non_positioned = true;

            // Non-positioned children use fit-based constraints
            let child_constraints = match self.fit {
                StackFit::Loose => constraints.loosen(),
                StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
                StackFit::Passthrough => constraints,
            };

            let child_size = ctx.layout_child(child_id, child_constraints);
            self.child_sizes[i] = child_size;
            max_width = max_width.max(child_size.width);
            max_height = max_height.max(child_size.height);
        }

        // Determine final stack size based on non-positioned children
        let size = match self.fit {
            StackFit::Expand => constraints.biggest(),
            _ => {
                if has_non_positioned {
                    Size::new(
                        max_width.clamp(constraints.min_width, constraints.max_width),
                        max_height.clamp(constraints.min_height, constraints.max_height),
                    )
                } else {
                    // No non-positioned children: use max constraints
                    constraints.biggest()
                }
            }
        };

        // ========== PASS 2: Layout positioned children ==========
        for (i, &child_id) in child_ids.iter().enumerate() {
            let parent_data = match self.child_parent_data.get(&child_id) {
                Some(pd) if pd.is_positioned() => pd,
                _ => continue, // Non-positioned, already laid out
            };

            // Calculate constraints based on position values
            let child_constraints = self.positioned_child_constraints(parent_data, size);
            let child_size = ctx.layout_child(child_id, child_constraints);
            self.child_sizes[i] = child_size;
        }

        #[cfg(debug_assertions)]
        tracing::trace!(
            "RenderStack::layout: fit={:?}, constraints={:?}, max_child_size=({:.1}, {:.1}), final_size={:?}",
            self.fit, constraints, max_width, max_height, size
        );

        // ========== Calculate child offsets ==========
        for (i, &child_id) in child_ids.iter().enumerate() {
            let child_size = self.child_sizes[i];

            let offset = if let Some(parent_data) = self.child_parent_data.get(&child_id) {
                if parent_data.is_positioned() {
                    // Positioned child: use explicit position
                    self.positioned_child_offset(parent_data, child_size, size)
                } else {
                    // Non-positioned child: use alignment
                    self.alignment.calculate_offset(child_size, size)
                }
            } else {
                // No parent data: use alignment
                self.alignment.calculate_offset(child_size, size)
            };

            self.child_offsets[i] = offset;
        }

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: crate::core::PaintTree,
    {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        // Paint children in order (first child in back, last child on top)
        for (i, child_id) in child_ids.into_iter().enumerate() {
            let child_offset = self.child_offsets.get(i).copied().unwrap_or(Offset::ZERO);
            ctx.paint_child(child_id, offset + child_offset);
        }
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

    #[test]
    fn test_stack_parent_data_management() {
        let mut stack = RenderStack::new();
        let child_id = ElementId::new(1);

        // Initially no parent data
        assert!(stack.get_child_position(child_id).is_none());
        assert!(!stack.is_child_positioned(child_id));

        // Set positioned parent data
        stack.set_child_position(
            child_id,
            StackParentData::positioned().with_top(10.0).with_left(20.0),
        );
        assert!(stack.get_child_position(child_id).is_some());
        assert!(stack.is_child_positioned(child_id));

        let pd = stack.get_child_position(child_id).unwrap();
        assert_eq!(pd.top, Some(10.0));
        assert_eq!(pd.left, Some(20.0));
        assert_eq!(pd.right, None);
        assert_eq!(pd.bottom, None);

        // Remove parent data
        let removed = stack.remove_child_position(child_id);
        assert!(removed.is_some());
        assert!(stack.get_child_position(child_id).is_none());
    }

    #[test]
    fn test_stack_parent_data_clear() {
        let mut stack = RenderStack::new();
        let child1 = ElementId::new(1);
        let child2 = ElementId::new(2);

        stack.set_child_position(
            child1,
            StackParentData::positioned().with_top(10.0).with_left(10.0),
        );
        stack.set_child_position(child2, StackParentData::fill());

        assert!(stack.get_child_position(child1).is_some());
        assert!(stack.get_child_position(child2).is_some());

        stack.clear_child_positions();

        assert!(stack.get_child_position(child1).is_none());
        assert!(stack.get_child_position(child2).is_none());
    }

    #[test]
    fn test_positioned_child_constraints_explicit_size() {
        let stack = RenderStack::new();
        let stack_size = Size::new(400.0, 300.0);

        // Explicit width and height
        let pd = StackParentData::positioned()
            .with_top(10.0)
            .with_left(10.0)
            .with_width(100.0)
            .with_height(50.0);

        let constraints = stack.positioned_child_constraints(&pd, stack_size);
        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 50.0);
        assert_eq!(constraints.max_height, 50.0);
    }

    #[test]
    fn test_positioned_child_constraints_derived_size() {
        let stack = RenderStack::new();
        let stack_size = Size::new(400.0, 300.0);

        // Width derived from left + right, height derived from top + bottom
        let pd = StackParentData::positioned()
            .with_top(10.0)
            .with_right(20.0)
            .with_bottom(30.0)
            .with_left(40.0);

        let constraints = stack.positioned_child_constraints(&pd, stack_size);
        // width = 400 - 40 - 20 = 340
        assert_eq!(constraints.min_width, 340.0);
        assert_eq!(constraints.max_width, 340.0);
        // height = 300 - 10 - 30 = 260
        assert_eq!(constraints.min_height, 260.0);
        assert_eq!(constraints.max_height, 260.0);
    }

    #[test]
    fn test_positioned_child_constraints_partial() {
        let stack = RenderStack::new();
        let stack_size = Size::new(400.0, 300.0);

        // Only top and left set - loose constraints
        let pd = StackParentData::positioned().with_top(10.0).with_left(20.0);

        let constraints = stack.positioned_child_constraints(&pd, stack_size);
        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, 400.0);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, 300.0);
    }

    #[test]
    fn test_positioned_child_offset_top_left() {
        let stack = RenderStack::new();
        let stack_size = Size::new(400.0, 300.0);
        let child_size = Size::new(100.0, 50.0);

        let pd = StackParentData::positioned().with_top(10.0).with_left(20.0);
        let offset = stack.positioned_child_offset(&pd, child_size, stack_size);

        assert_eq!(offset.dx, 20.0);
        assert_eq!(offset.dy, 10.0);
    }

    #[test]
    fn test_positioned_child_offset_bottom_right() {
        let stack = RenderStack::new();
        let stack_size = Size::new(400.0, 300.0);
        let child_size = Size::new(100.0, 50.0);

        let pd = StackParentData::positioned()
            .with_right(10.0)
            .with_bottom(20.0);
        let offset = stack.positioned_child_offset(&pd, child_size, stack_size);

        // x = 400 - 100 - 10 = 290
        assert_eq!(offset.dx, 290.0);
        // y = 300 - 50 - 20 = 230
        assert_eq!(offset.dy, 230.0);
    }

    #[test]
    fn test_non_positioned_defaults() {
        let stack = RenderStack::new();
        let child_id = ElementId::new(1);

        // Without parent data, child is non-positioned
        assert!(!stack.is_child_positioned(child_id));
    }
}
