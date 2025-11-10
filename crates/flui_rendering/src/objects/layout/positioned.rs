//! RenderPositioned - wrapper for positioned children in Stack layout
//!
//! This RenderObject wraps a child and provides PositionedMetadata
//! that the parent RenderStack uses to determine positioning.
//!
//! # Architecture
//!
//! Following the GAT Metadata pattern from FINAL_ARCHITECTURE_V2.md:
//! - PositionedMetadata is stored inline (not in separate ParentData)
//! - Parent (RenderStack) accesses metadata via GAT-based downcast
//! - Zero-cost when not using positioned children

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};

use flui_engine::BoxedLayer;
use flui_types::constraints::BoxConstraints;
use flui_types::{Offset, Size};

/// Metadata for positioned children in Stack layout
///
/// This metadata is read by the parent RenderStack during layout
/// to determine how to position and size this child.
///
/// # Example
///
/// ```rust,ignore
/// // Positioned at top-left
/// let top_left = PositionedMetadata {
///     left: Some(10.0),
///     top: Some(20.0),
///     right: None,
///     bottom: None,
///     width: None,
///     height: None,
/// };
///
/// // Fill entire stack
/// let fill = PositionedMetadata {
///     left: Some(0.0),
///     top: Some(0.0),
///     right: Some(0.0),
///     bottom: Some(0.0),
///     width: None,
///     height: None,
/// };
///
/// // Fixed size at center (via left/top calculation)
/// let centered = PositionedMetadata {
///     width: Some(100.0),
///     height: Some(100.0),
///     left: None,
///     top: None,
///     right: None,
///     bottom: None,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PositionedMetadata {
    /// Position from left edge (for positioned children)
    pub left: Option<f32>,

    /// Position from top edge (for positioned children)
    pub top: Option<f32>,

    /// Position from right edge (for positioned children)
    pub right: Option<f32>,

    /// Position from bottom edge (for positioned children)
    pub bottom: Option<f32>,

    /// Width override (for positioned children)
    pub width: Option<f32>,

    /// Height override (for positioned children)
    pub height: Option<f32>,
}

impl PositionedMetadata {
    /// Create new positioned metadata with all fields
    pub fn new(
        left: Option<f32>,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
        width: Option<f32>,
        height: Option<f32>,
    ) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
            width,
            height,
        }
    }

    /// Create positioned metadata that fills the entire stack
    pub fn fill() -> Self {
        Self {
            left: Some(0.0),
            top: Some(0.0),
            right: Some(0.0),
            bottom: Some(0.0),
            width: None,
            height: None,
        }
    }

    /// Create positioned metadata at specific position
    pub fn at(left: f32, top: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Check if this child is positioned
    pub fn is_positioned(&self) -> bool {
        self.left.is_some()
            || self.top.is_some()
            || self.right.is_some()
            || self.bottom.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }

    /// Compute constraints for this positioned child
    ///
    /// Calculates appropriate BoxConstraints based on positioning parameters:
    /// - If left AND right are specified → width is fixed
    /// - If only width is specified → width is fixed
    /// - If top AND bottom are specified → height is fixed
    /// - If only height is specified → height is fixed
    /// - Otherwise → loose constraints
    pub fn compute_constraints(&self, parent_constraints: BoxConstraints) -> BoxConstraints {
        let parent_width = parent_constraints.max_width;
        let parent_height = parent_constraints.max_height;

        // Compute width constraints
        let (min_width, max_width) = if let Some(width) = self.width {
            // Explicit width
            (width, width)
        } else if let (Some(left), Some(right)) = (self.left, self.right) {
            // Both left and right → width is determined
            let w = (parent_width - left - right).max(0.0);
            (w, w)
        } else {
            // Width is flexible
            (0.0, parent_width)
        };

        // Compute height constraints
        let (min_height, max_height) = if let Some(height) = self.height {
            // Explicit height
            (height, height)
        } else if let (Some(top), Some(bottom)) = (self.top, self.bottom) {
            // Both top and bottom → height is determined
            let h = (parent_height - top - bottom).max(0.0);
            (h, h)
        } else {
            // Height is flexible
            (0.0, parent_height)
        };

        BoxConstraints::new(min_width, max_width, min_height, max_height)
    }

    /// Calculate child offset based on positioning parameters
    ///
    /// # Returns
    ///
    /// The offset where this child should be painted, or None if not positioned
    pub fn calculate_offset(&self, child_size: Size, stack_size: Size) -> Option<Offset> {
        if !self.is_positioned() {
            return None;
        }

        let x = if let Some(left) = self.left {
            left
        } else if let Some(right) = self.right {
            stack_size.width - child_size.width - right
        } else {
            // Center horizontally if no left/right specified
            (stack_size.width - child_size.width) / 2.0
        };

        let y = if let Some(top) = self.top {
            top
        } else if let Some(bottom) = self.bottom {
            stack_size.height - child_size.height - bottom
        } else {
            // Center vertically if no top/bottom specified
            (stack_size.height - child_size.height) / 2.0
        };

        #[cfg(debug_assertions)]
        tracing::trace!(
            "PositionedMetadata::calculate_offset: left={:?}, top={:?}, right={:?}, bottom={:?}, child_size={:?}, stack_size={:?}, result=({:.1}, {:.1})",
            self.left, self.top, self.right, self.bottom, child_size, stack_size, x, y
        );

        Some(Offset::new(x, y))
    }
}

/// RenderObject that wraps a child and provides positioned metadata
///
/// This is a pass-through render object that simply delegates layout
/// and paint to its child, but provides PositionedMetadata that the parent
/// RenderStack can query.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{RenderPositioned, PositionedMetadata};
///
/// // Create a positioned item at (10, 20)
/// let positioned = RenderPositioned::new(PositionedMetadata::at(10.0, 20.0));
///
/// // Create a positioned item that fills the stack
/// let fill = RenderPositioned::new(PositionedMetadata::fill());
/// ```
#[derive(Debug)]
pub struct RenderPositioned {
    /// The positioned metadata for this child
    pub metadata: PositionedMetadata,
}

impl RenderPositioned {
    /// Create new RenderPositioned with specified metadata
    pub fn new(metadata: PositionedMetadata) -> Self {
        Self { metadata }
    }

    /// Create RenderPositioned at specific position
    pub fn at(left: f32, top: f32) -> Self {
        Self {
            metadata: PositionedMetadata::at(left, top),
        }
    }

    /// Create RenderPositioned that fills the entire stack
    pub fn fill() -> Self {
        Self {
            metadata: PositionedMetadata::fill(),
        }
    }

    /// Get the positioned metadata
    pub fn positioned_metadata(&self) -> &PositionedMetadata {
        &self.metadata
    }

    /// Get mutable positioned metadata
    pub fn positioned_metadata_mut(&mut self) -> &mut PositionedMetadata {
        &mut self.metadata
    }
}

impl Render for RenderPositioned {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // Pass-through: just layout child with same constraints
        tree.layout_child(child_id, constraints)
    }

    fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;
        // Pass-through: just paint child at same offset
        tree.paint_child(child_id, offset)
    }

    // Note: metadata() method removed - not part of unified Render trait
    // Parent data should be queried via RenderElement.parent_data() instead
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Variable // Default - update if needed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positioned_metadata_default() {
        let meta = PositionedMetadata::default();
        assert!(!meta.is_positioned());
    }

    #[test]
    fn test_positioned_metadata_fill() {
        let meta = PositionedMetadata::fill();
        assert!(meta.is_positioned());
        assert_eq!(meta.left, Some(0.0));
        assert_eq!(meta.top, Some(0.0));
        assert_eq!(meta.right, Some(0.0));
        assert_eq!(meta.bottom, Some(0.0));
    }

    #[test]
    fn test_positioned_metadata_at() {
        let meta = PositionedMetadata::at(10.0, 20.0);
        assert!(meta.is_positioned());
        assert_eq!(meta.left, Some(10.0));
        assert_eq!(meta.top, Some(20.0));
    }

    #[test]
    fn test_positioned_metadata_is_positioned() {
        let mut meta = PositionedMetadata::default();
        assert!(!meta.is_positioned());

        meta.width = Some(100.0);
        assert!(meta.is_positioned());
    }

    #[test]
    fn test_positioned_metadata_compute_constraints() {
        let parent = BoxConstraints::new(0.0, 400.0, 0.0, 600.0);

        // left + right → fixed width
        let meta = PositionedMetadata::new(Some(10.0), None, Some(20.0), None, None, None);
        let constraints = meta.compute_constraints(parent);
        assert_eq!(constraints.min_width, 370.0);
        assert_eq!(constraints.max_width, 370.0);

        // Explicit width
        let meta = PositionedMetadata::new(None, None, None, None, Some(100.0), None);
        let constraints = meta.compute_constraints(parent);
        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
    }

    #[test]
    fn test_positioned_metadata_calculate_offset() {
        let child_size = Size::new(100.0, 50.0);
        let stack_size = Size::new(400.0, 600.0);

        // left + top
        let meta = PositionedMetadata::at(10.0, 20.0);
        let offset = meta.calculate_offset(child_size, stack_size);
        assert_eq!(offset, Some(Offset::new(10.0, 20.0)));

        // right + bottom
        let meta = PositionedMetadata::new(None, None, Some(10.0), Some(20.0), None, None);
        let offset = meta.calculate_offset(child_size, stack_size);
        assert_eq!(offset, Some(Offset::new(290.0, 530.0)));

        // Not positioned
        let meta = PositionedMetadata::default();
        let offset = meta.calculate_offset(child_size, stack_size);
        assert_eq!(offset, None);
    }

    #[test]
    fn test_render_positioned_new() {
        let positioned = RenderPositioned::new(PositionedMetadata::at(10.0, 20.0));
        assert_eq!(positioned.metadata.left, Some(10.0));
        assert_eq!(positioned.metadata.top, Some(20.0));
    }

    #[test]
    fn test_render_positioned_at() {
        let positioned = RenderPositioned::at(10.0, 20.0);
        assert_eq!(positioned.metadata.left, Some(10.0));
        assert_eq!(positioned.metadata.top, Some(20.0));
    }

    #[test]
    fn test_render_positioned_fill() {
        let positioned = RenderPositioned::fill();
        assert_eq!(positioned.metadata.left, Some(0.0));
        assert_eq!(positioned.metadata.top, Some(0.0));
        assert_eq!(positioned.metadata.right, Some(0.0));
        assert_eq!(positioned.metadata.bottom, Some(0.0));

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn arity(&self) -> Arity {
            Arity::Exact(1)
        }
    }
}
