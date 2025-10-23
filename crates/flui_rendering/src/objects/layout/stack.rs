//! RenderStack - layering container

use flui_types::{Offset, Size, constraints::BoxConstraints, Alignment};
use flui_core::DynRenderObject;
use crate::core::{ContainerRenderBox, RenderBoxMixin};

/// How to size the non-positioned children in the stack
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackFit {
    /// The constraints passed to the stack from its parent are loosened
    Loose,
    /// The constraints passed to the stack from its parent are tightened to the biggest size
    Expand,
    /// The non-positioned children are given unconst constraints
    Passthrough,
}

impl Default for StackFit {
    fn default() -> Self {
        StackFit::Loose
    }
}

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
/// - Non-positioned: Sized according to the stack's fit and aligned
/// - Positioned: Placed at specific positions (requires StackParentData - TODO)
///
/// This is a simplified implementation. A full implementation would include:
/// - StackParentData for positioned children
/// - Positioned widget support (top, left, right, bottom)
/// - Overflow handling
/// - Clip behavior
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
            RenderBoxMixin::mark_needs_layout(self);
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
            RenderBoxMixin::mark_needs_layout(self);
        }
    }
}

// ===== DynRenderObject Implementation =====

impl DynRenderObject for RenderStack {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Store constraints
        self.state_mut().constraints = Some(constraints);

        let fit = self.data().fit;

        if self.children.is_empty() {
            // No children - use smallest size
            let size = constraints.smallest();
            self.state_mut().size = Some(size);
            self.clear_needs_layout();
            return size;
        }

        // Simplified layout algorithm
        // TODO: This is a basic implementation. A full implementation would:
        // 1. Separate positioned and non-positioned children via StackParentData
        // 2. Layout positioned children with their specific constraints
        // 3. Handle overflow and clipping

        // Calculate child constraints based on fit
        let child_constraints = match fit {
            StackFit::Loose => constraints.loosen(),
            StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
            StackFit::Passthrough => constraints,
        };

        // Layout all children and track max size
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;

        for child in &mut self.children {
            let child_size = child.layout(child_constraints);
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

        // Store size and clear needs_layout flag
        self.state_mut().size = Some(size);
        self.clear_needs_layout();

        size
    }

    fn paint(&self, painter: &egui::Painter, offset: Offset) {
        let size = self.state().size.unwrap_or(Size::ZERO);
        let alignment = self.data().alignment;

        // Paint children in order (first child in back, last child on top)
        for child in &self.children {
            let child_size = child.size();

            // Calculate aligned position
            // TODO: For positioned children, use their position from StackParentData
            let child_offset = alignment.calculate_offset(child_size, size);

            let paint_offset = Offset::new(
                offset.dx + child_offset.dx,
                offset.dy + child_offset.dy,
            );

            child.paint(painter, paint_offset);
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
        assert!(RenderBoxMixin::needs_layout(&stack));
    }

    #[test]
    fn test_render_stack_set_fit() {
        let mut stack = ContainerRenderBox::new(StackData::new());

        stack.set_fit(StackFit::Expand);
        assert_eq!(stack.fit(), StackFit::Expand);
        assert!(RenderBoxMixin::needs_layout(&stack));
    }

    #[test]
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
