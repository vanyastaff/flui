//! RenderStack - layering container

use crate::core::{
    BoxLayoutCtx, ChildrenAccess, BoxPaintCtx, RenderBox, Variable,
};
use flui_types::constraints::BoxConstraints;
use flui_types::layout::StackFit;
use flui_types::{Alignment, Offset, Size};

/// RenderObject for stack layout (layering)
///
/// Stack allows positioning children on top of each other. Children can be:
/// - **Non-positioned**: Sized according to the stack's fit and aligned
/// - **Positioned**: Placed at specific positions using StackParentData
///
/// # Features
///
/// - Alignment-based positioning for non-positioned children
/// - StackFit control for child sizing
/// - Offset caching for performance
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
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox<Variable> for RenderStack {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Variable>) -> Size {
        let constraints = ctx.constraints;
        let children = ctx.children;

        let child_count = children.as_slice().len();
        if child_count == 0 {
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

        for child_id in children.iter() {
            // For now, all children use fit-based constraints
            // TODO: Add PositionedMetadata support for positioned children
            let child_constraints = match self.fit {
                StackFit::Loose => constraints.loosen(),
                StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
                StackFit::Passthrough => constraints,
            };

            let child_size = ctx.layout_child(child_id, child_constraints);
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

        #[cfg(debug_assertions)]
        tracing::trace!(
            "RenderStack::layout: fit={:?}, constraints={:?}, max_child_size=({:.1}, {:.1}), final_size={:?}",
            self.fit, constraints, max_width, max_height, size
        );

        // Calculate and save child offsets using alignment
        for child_size in &self.child_sizes {
            let child_offset = self.alignment.calculate_offset(*child_size, size);
            self.child_offsets.push(child_offset);
        }

        size
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
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
}
