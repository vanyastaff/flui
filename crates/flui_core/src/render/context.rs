//! Rendering contexts for layout and paint operations
//!
//! This module provides context structs that are passed to render objects
//! during layout and paint phases. Contexts encapsulate all necessary
//! information and provide convenience methods for common operations.

use crate::element::{ElementId, ElementTree};
use crate::render::Children;
use flui_painting::Canvas;
use flui_types::{constraints::BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

// ============================================================================
// LayoutContext
// ============================================================================

/// Context for layout operations
///
/// Provides access to the element tree, children, constraints, and
/// convenience methods for laying out children and accessing parent data.
///
/// # Lifetime
///
/// The `'a` lifetime ensures that the context cannot outlive the
/// element tree or children it references.
///
/// # Examples
///
/// ```rust,ignore
/// impl Render for RenderPadding {
///     fn layout(&mut self, ctx: &LayoutContext) -> Size {
///         let child_id = ctx.children.single();
///         let child_constraints = ctx.constraints.deflate(&self.padding);
///         let child_size = ctx.layout_child(child_id, child_constraints);
///
///         Size::new(
///             child_size.width + self.padding.horizontal_total(),
///             child_size.height + self.padding.vertical_total(),
///         )
///     }
/// }
/// ```
#[derive(Debug)]
pub struct LayoutContext<'a> {
    /// Reference to the element tree
    ///
    /// Provides access to all elements in the tree for child layout,
    /// parent data queries, and tree traversal.
    pub tree: &'a ElementTree,

    /// Children of this render object
    ///
    /// Encoded as a `Children` enum which can be:
    /// - `Children::None` for leaf nodes
    /// - `Children::Single(id)` for single-child wrappers
    /// - `Children::Multi(ids)` for multi-child layouts
    pub children: &'a Children,

    /// Layout constraints from parent
    ///
    /// The render object must return a size that satisfies these constraints.
    pub constraints: BoxConstraints,
}

impl<'a> LayoutContext<'a> {
    /// Create a new layout context
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `children`: Reference to children enum
    /// - `constraints`: Layout constraints from parent
    pub fn new(tree: &'a ElementTree, children: &'a Children, constraints: BoxConstraints) -> Self {
        Self {
            tree,
            children,
            constraints,
        }
    }

    /// Layout a child with constraints
    ///
    /// This is the primary method for laying out children. It delegates
    /// to the element tree which handles the layout logic and caching.
    ///
    /// # Parameters
    ///
    /// - `child_id`: The element ID of the child to layout
    /// - `constraints`: The constraints to apply to the child
    ///
    /// # Returns
    ///
    /// The size computed by the child's layout method.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let child_size = ctx.layout_child(child_id, BoxConstraints::tight(Size::new(100.0, 100.0)));
    /// ```
    #[inline]
    pub fn layout_child(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        self.tree.layout_child(child_id, constraints)
    }

    /// Get parent data from a child element
    ///
    /// Returns the downcasted parent data if it exists and has the correct type.
    /// Parent data is metadata attached by the parent render object to store
    /// layout-specific information about each child.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The expected parent data type (must be `'static`)
    ///
    /// # Returns
    ///
    /// - `Some(&T)` if the child has parent data of type `T`
    /// - `None` if the child doesn't exist, has no parent data, or has wrong type
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // In RenderFlex, query flex metadata from children
    /// for &child_id in ctx.children.as_slice() {
    ///     if let Some(flex_meta) = ctx.child_parent_data::<FlexItemMetadata>(child_id) {
    ///         if flex_meta.is_flexible() {
    ///             flexible_children.push(child_id);
    ///         }
    ///     }
    /// }
    /// ```
    pub fn child_parent_data<T: 'static>(&self, child_id: ElementId) -> Option<&T> {
        self.tree
            .get(child_id)?
            .as_render()?
            .parent_data()?
            .as_any()
            .downcast_ref::<T>()
    }

    // Note: set_child_parent_data removed due to borrow checker issues.
    // Parent data should be set before layout via ElementTree.get_mut()
    // or during View::build() phase, not during layout phase.

    /// Layout all children with the same constraints
    ///
    /// Convenience method for laying out all children with identical constraints.
    /// Returns a vector of child sizes in the same order as the children.
    ///
    /// # Parameters
    ///
    /// - `constraints`: The constraints to apply to all children
    ///
    /// # Returns
    ///
    /// Vector of child sizes (empty for `Children::None`)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let child_sizes = ctx.layout_all_children(ctx.constraints);
    /// ```
    pub fn layout_all_children(&self, constraints: BoxConstraints) -> Vec<Size> {
        self.children
            .as_slice()
            .iter()
            .map(|&child_id| self.layout_child(child_id, constraints))
            .collect()
    }
}

// ============================================================================
// PaintContext
// ============================================================================

/// Context for paint operations
///
/// Provides access to the element tree, children, and offset for painting.
/// Includes convenience methods for painting children at specific offsets.
///
/// # Lifetime
///
/// The `'a` lifetime ensures that the context cannot outlive the
/// element tree or children it references.
///
/// # Examples
///
/// ```rust,ignore
/// impl Render for RenderPadding {
///     fn paint(&self, ctx: &PaintContext) -> BoxedLayer {
///         let child_id = ctx.children.single();
///         let child_offset = Offset::new(self.padding.left, self.padding.top);
///         ctx.paint_child(child_id, ctx.offset + child_offset)
///     }
/// }
/// ```
#[derive(Debug)]
pub struct PaintContext<'a> {
    /// Reference to the element tree
    ///
    /// Provides access to all elements in the tree for child painting
    /// and tree traversal.
    pub tree: &'a ElementTree,

    /// Children of this render object
    ///
    /// Encoded as a `Children` enum which can be:
    /// - `Children::None` for leaf nodes
    /// - `Children::Single(id)` for single-child wrappers
    /// - `Children::Multi(ids)` for multi-child layouts
    pub children: &'a Children,

    /// Paint offset (position in parent's coordinate space)
    ///
    /// This is the offset at which this render object should paint itself
    /// and its children. Child offsets are relative to this offset.
    pub offset: Offset,
}

impl<'a> PaintContext<'a> {
    /// Create a new paint context
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `children`: Reference to children enum
    /// - `offset`: Paint offset in parent's coordinate space
    pub fn new(tree: &'a ElementTree, children: &'a Children, offset: Offset) -> Self {
        Self {
            tree,
            children,
            offset,
        }
    }

    /// Paint a child at the given offset
    ///
    /// This is the primary method for painting children. It delegates
    /// to the element tree which handles the paint logic.
    ///
    /// # Parameters
    ///
    /// - `child_id`: The element ID of the child to paint
    /// - `offset`: The offset at which to paint the child (in parent's coordinate space)
    ///
    /// # Returns
    ///
    /// A Canvas containing the child's drawing commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let child_canvas = ctx.paint_child(child_id, ctx.offset + Offset::new(10.0, 20.0));
    /// ```
    #[inline]
    pub fn paint_child(&self, child_id: ElementId, offset: Offset) -> Canvas {
        self.tree.paint_child(child_id, offset)
    }

    /// Paint all children at their cached offsets
    ///
    /// Convenience method for painting all children. The offsets slice must
    /// have the same length as the number of children.
    ///
    /// # Parameters
    ///
    /// - `offsets`: Slice of offsets (one per child, relative to `ctx.offset`)
    ///
    /// # Returns
    ///
    /// Vector of child canvases in the same order as the children.
    ///
    /// # Panics
    ///
    /// Panics if `offsets.len()` doesn't match `children.len()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let child_canvases = ctx.paint_all_children(&self.cached_offsets);
    /// for child_canvas in child_canvases {
    ///     canvas.draw_canvas(&child_canvas, offset);
    /// }
    /// ```
    pub fn paint_all_children(&self, offsets: &[Offset]) -> Vec<Canvas> {
        let child_ids = self.children.as_slice();

        assert_eq!(
            offsets.len(),
            child_ids.len(),
            "Offsets count must match children count"
        );

        child_ids
            .iter()
            .zip(offsets.iter())
            .map(|(&child_id, &child_offset)| {
                self.paint_child(child_id, self.offset + child_offset)
            })
            .collect()
    }

    /// Paint all children at the same offset
    ///
    /// Convenience method for painting all children at the same offset.
    /// Useful for layouts where all children are stacked at the same position.
    ///
    /// # Parameters
    ///
    /// - `offset`: The offset to paint all children at (relative to `ctx.offset`)
    ///
    /// # Returns
    ///
    /// Vector of child canvases in the same order as the children.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Stack layout - all children at same position
    /// let child_canvases = ctx.paint_all_children_at(Offset::ZERO);
    /// ```
    pub fn paint_all_children_at(&self, offset: Offset) -> Vec<Canvas> {
        self.children
            .as_slice()
            .iter()
            .map(|&child_id| self.paint_child(child_id, self.offset + offset))
            .collect()
    }
}

// ============================================================================
// SliverLayoutContext
// ============================================================================

/// Context for sliver layout operations
///
/// Provides access to the element tree, children, sliver constraints, and
/// convenience methods for laying out sliver children.
///
/// # Lifetime
///
/// The `'a` lifetime ensures that the context cannot outlive the
/// element tree or children it references.
///
/// # Examples
///
/// ```rust,ignore
/// impl RenderSliver for RenderSliverPadding {
///     fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
///         let child_id = ctx.children.single();
///         let child_constraints = self.adjust_constraints(&ctx.constraints);
///         let child_geometry = ctx.layout_child(child_id, child_constraints);
///
///         SliverGeometry {
///             scroll_extent: child_geometry.scroll_extent + self.padding.vertical_total(),
///             paint_extent: child_geometry.paint_extent + self.padding.vertical_total(),
///             ..child_geometry
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct SliverLayoutContext<'a> {
    /// Reference to the element tree
    ///
    /// Provides access to all elements in the tree for child layout,
    /// parent data queries, and tree traversal.
    pub tree: &'a ElementTree,

    /// Children of this sliver render object
    ///
    /// Encoded as a `Children` enum which can be:
    /// - `Children::None` for leaf nodes
    /// - `Children::Single(id)` for single-child wrappers
    /// - `Children::Multi(ids)` for multi-child layouts
    pub children: &'a Children,

    /// Sliver constraints from viewport
    ///
    /// The sliver render object must return geometry that respects these constraints.
    pub constraints: SliverConstraints,
}

impl<'a> SliverLayoutContext<'a> {
    /// Create a new sliver layout context
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `children`: Reference to children enum
    /// - `constraints`: Sliver constraints from viewport
    pub fn new(
        tree: &'a ElementTree,
        children: &'a Children,
        constraints: SliverConstraints,
    ) -> Self {
        Self {
            tree,
            children,
            constraints,
        }
    }

    /// Layout a sliver child with sliver constraints
    ///
    /// This is the primary method for laying out sliver children. It delegates
    /// to the element tree which handles the layout logic and caching.
    ///
    /// # Parameters
    ///
    /// - `child_id`: The element ID of the child to layout
    /// - `constraints`: The sliver constraints to apply to the child
    ///
    /// # Returns
    ///
    /// The sliver geometry computed by the child's layout method.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let child_geometry = ctx.layout_child(
    ///     child_id,
    ///     ctx.constraints.copy_with_scroll_offset(adjusted_offset)
    /// );
    /// ```
    #[inline]
    pub fn layout_child(
        &self,
        child_id: ElementId,
        constraints: SliverConstraints,
    ) -> SliverGeometry {
        self.tree.layout_sliver_child(child_id, constraints)
    }

    /// Get parent data from a child element
    ///
    /// Returns the downcasted parent data if it exists and has the correct type.
    /// Parent data is metadata attached by the parent render object to store
    /// layout-specific information about each child.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The expected parent data type (must be `'static`)
    ///
    /// # Returns
    ///
    /// - `Some(&T)` if the child has parent data of type `T`
    /// - `None` if the child doesn't exist, has no parent data, or has wrong type
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // In RenderSliverGrid, query grid metadata from children
    /// for &child_id in ctx.children.as_slice() {
    ///     if let Some(grid_meta) = ctx.child_parent_data::<SliverGridItemMetadata>(child_id) {
    ///         let cross_axis_cells = grid_meta.cross_axis_cells;
    ///         // Use metadata...
    ///     }
    /// }
    /// ```
    pub fn child_parent_data<T: 'static>(&self, child_id: ElementId) -> Option<&T> {
        self.tree
            .get(child_id)?
            .as_render()?
            .parent_data()?
            .as_any()
            .downcast_ref::<T>()
    }
}

// ============================================================================
// SliverPaintContext
// ============================================================================

/// Context for sliver paint operations
///
/// Provides access to the element tree, children, and offset for painting slivers.
/// Includes convenience methods for painting sliver children at specific offsets.
///
/// # Lifetime
///
/// The `'a` lifetime ensures that the context cannot outlive the
/// element tree or children it references.
///
/// # Examples
///
/// ```rust,ignore
/// impl RenderSliver for RenderSliverPadding {
///     fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
///         let child_id = ctx.children.single();
///         let child_offset = ctx.offset + self.padding.top_left_offset();
///         ctx.paint_child(child_id, child_offset)
///     }
/// }
/// ```
#[derive(Debug)]
pub struct SliverPaintContext<'a> {
    /// Reference to the element tree
    ///
    /// Provides access to all elements in the tree for child painting
    /// and tree traversal.
    pub tree: &'a ElementTree,

    /// Children of this sliver render object
    ///
    /// Encoded as a `Children` enum which can be:
    /// - `Children::None` for leaf nodes
    /// - `Children::Single(id)` for single-child wrappers
    /// - `Children::Multi(ids)` for multi-child layouts
    pub children: &'a Children,

    /// Paint offset (position in viewport's coordinate space)
    ///
    /// This is the offset at which this sliver render object should paint itself
    /// and its children. Child offsets are relative to this offset.
    pub offset: Offset,
}

impl<'a> SliverPaintContext<'a> {
    /// Create a new sliver paint context
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `children`: Reference to children enum
    /// - `offset`: Paint offset in viewport's coordinate space
    pub fn new(tree: &'a ElementTree, children: &'a Children, offset: Offset) -> Self {
        Self {
            tree,
            children,
            offset,
        }
    }

    /// Paint a sliver child at the given offset
    ///
    /// This is the primary method for painting sliver children. It delegates
    /// to the element tree which handles the paint logic.
    ///
    /// # Parameters
    ///
    /// - `child_id`: The element ID of the child to paint
    /// - `offset`: The offset at which to paint the child (in viewport's coordinate space)
    ///
    /// # Returns
    ///
    /// A Canvas containing the child's drawing commands.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let child_canvas = ctx.paint_child(child_id, ctx.offset + Offset::new(0.0, item_offset));
    /// ```
    #[inline]
    pub fn paint_child(&self, child_id: ElementId, offset: Offset) -> Canvas {
        self.tree.paint_sliver_child(child_id, offset)
    }

    /// Paint all sliver children at their cached offsets
    ///
    /// Convenience method for painting all children. The offsets slice must
    /// have the same length as the number of children.
    ///
    /// # Parameters
    ///
    /// - `offsets`: Slice of offsets (one per child, relative to `ctx.offset`)
    ///
    /// # Returns
    ///
    /// Vector of child canvases in the same order as the children.
    ///
    /// # Panics
    ///
    /// Panics if `offsets.len()` doesn't match `children.len()`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let child_canvases = ctx.paint_all_children(&self.cached_offsets);
    /// for child_canvas in child_canvases {
    ///     canvas.draw_canvas(&child_canvas, offset);
    /// }
    /// ```
    pub fn paint_all_children(&self, offsets: &[Offset]) -> Vec<Canvas> {
        let child_ids = self.children.as_slice();

        assert_eq!(
            offsets.len(),
            child_ids.len(),
            "Offsets count must match children count"
        );

        child_ids
            .iter()
            .zip(offsets.iter())
            .map(|(&child_id, &child_offset)| {
                self.paint_child(child_id, self.offset + child_offset)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full integration tests require ElementTree setup
    // These are basic structural tests

    #[test]
    fn test_layout_context_creation() {
        let tree = ElementTree::new();
        let children = Children::None;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        let ctx = LayoutContext::new(&tree, &children, constraints);

        assert_eq!(ctx.constraints, constraints);
        assert!(ctx.children.is_empty());
    }

    #[test]
    fn test_paint_context_creation() {
        use crate::ElementId;
        let tree = ElementTree::new();
        let children = Children::Single(ElementId::new(1));
        let offset = Offset::new(10.0, 20.0);

        let ctx = PaintContext::new(&tree, &children, offset);

        assert_eq!(ctx.offset, offset);
        assert_eq!(ctx.children.len(), 1);
    }
}
