//! RenderStack - layering container

use flui_core::element::{ElementId, ElementTree};
use flui_core::render::MultiRender;
use flui_engine::{layer::pool, BoxedLayer};
use flui_types::layout::StackFit;
use flui_types::{constraints::BoxConstraints, Alignment, Offset, Size};

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

    // TODO: Positioned children support will be implemented via GAT Metadata
    // (similar to FlexItemMetadata pattern shown in FINAL_ARCHITECTURE_V2.md)
    // For now, all children are treated as non-positioned and aligned according to stack alignment.
}

impl Default for RenderStack {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiRender for RenderStack {
    /// No metadata needed
    type Metadata = ();

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
            // Check if child has PositionedMetadata (via RenderPositioned wrapper)
            let positioned_metadata = if let Some(element) = tree.get(child) {
                if let Some(render_node_guard) = element.render_object() {
                    if let Some(metadata_any) = render_node_guard.metadata() {
                        metadata_any
                            .downcast_ref::<super::positioned::PositionedMetadata>()
                            .copied()
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let child_constraints = if let Some(pos_meta) = positioned_metadata {
                if pos_meta.is_positioned() {
                    // Child is positioned - use computed constraints
                    pos_meta.compute_constraints(constraints)
                } else {
                    // Child has PositionedMetadata but is not positioned - use fit-based constraints
                    match self.fit {
                        StackFit::Loose => constraints.loosen(),
                        StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
                        StackFit::Passthrough => constraints,
                    }
                }
            } else {
                // Child has no PositionedMetadata - use fit-based constraints
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
        for (i, &child) in child_ids.iter().enumerate() {
            let child_size = self.child_sizes[i];

            // Check if child has PositionedMetadata
            let positioned_metadata = if let Some(element) = tree.get(child) {
                if let Some(render_node_guard) = element.render_object() {
                    if let Some(metadata_any) = render_node_guard.metadata() {
                        metadata_any
                            .downcast_ref::<super::positioned::PositionedMetadata>()
                            .copied()
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let child_offset = if let Some(pos_meta) = positioned_metadata {
                if let Some(offset) = pos_meta.calculate_offset(child_size, size) {
                    // Child is positioned - use calculated offset
                    offset
                } else {
                    // Child has PositionedMetadata but is not positioned - use alignment
                    self.alignment.calculate_offset(child_size, size)
                }
            } else {
                // Child has no PositionedMetadata - use alignment-based positioning
                self.alignment.calculate_offset(child_size, size)
            };

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
