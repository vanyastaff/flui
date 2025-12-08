//! Combined render tree traits and utilities.
//!
//! This module provides the [`FullRenderTree`] trait that combines all rendering
//! phases (layout, paint, hit testing) into a single interface.

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::{BoxConstraints, Offset, Size};

use crate::error::RenderError;
use crate::hit_test_tree::HitTestTree;
use crate::layout_tree::LayoutTree;
use crate::paint_tree::PaintTree;

// ============================================================================
// COMBINED TRAIT
// ============================================================================

/// Combined trait for full render tree operations.
///
/// This trait combines all rendering phases (layout, paint, hit testing) into
/// a single interface. It's useful when you need all operations and want to
/// avoid multiple trait bounds.
///
/// # Usage
///
/// ```rust,ignore
/// fn render_element(tree: &mut dyn FullRenderTree, id: ElementId) -> Result<Canvas, RenderError> {
///     // Layout
///     let size = tree.perform_layout(id, constraints)?;
///
///     // Paint
///     let canvas = tree.perform_paint(id, Offset::ZERO)?;
///
///     Ok(canvas)
/// }
/// ```
pub trait FullRenderTree: LayoutTree + PaintTree + HitTestTree {
    /// Performs a complete render pass (layout + paint) on an element.
    ///
    /// This is a convenience method that combines layout and paint operations.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to render
    /// * `constraints` - Layout constraints
    /// * `offset` - Paint offset
    ///
    /// # Returns
    ///
    /// A tuple of (computed_size, canvas) or an error.
    fn render_element(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
        offset: Offset,
    ) -> Result<(Size, Canvas), RenderError> {
        let size = self.perform_layout(id, constraints)?;
        let canvas = self.perform_paint(id, offset)?;
        Ok((size, canvas))
    }

    /// Checks if any phase needs update for the given element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    ///
    /// # Returns
    ///
    /// `true` if layout or paint is needed, `false` otherwise.
    fn needs_update(&self, id: ElementId) -> bool {
        self.needs_layout(id) || self.needs_paint(id)
    }
}

// Blanket implementation for any type that implements all three traits
impl<T> FullRenderTree for T where T: LayoutTree + PaintTree + HitTestTree {}

// ============================================================================
// TRAIT ALIAS FOR COMPATIBILITY
// ============================================================================

/// Alias trait for `FullRenderTree` for compatibility with code using `RenderTreeOps`.
///
/// This trait is identical to `FullRenderTree` and exists for backward compatibility.
pub trait RenderTreeOps: FullRenderTree {}

// Blanket implementation: any FullRenderTree also implements RenderTreeOps
impl<T: FullRenderTree> RenderTreeOps for T {}

// ============================================================================
// DEBUG UTILITIES
// ============================================================================

/// Debug information about a render element.
#[derive(Debug, Clone)]
pub struct RenderElementDebugInfo {
    /// Element ID
    pub id: ElementId,
    /// Depth in the tree (0 = root)
    pub depth: usize,
    /// Whether the element needs layout
    pub needs_layout: bool,
    /// Whether the element needs paint
    pub needs_paint: bool,
    /// Current offset (if available)
    pub offset: Option<Offset>,
}

/// Collects debug information about a render element.
///
/// # Arguments
///
/// * `tree` - The render tree (must implement both LayoutTree and PaintTree)
/// * `id` - The element ID to inspect
/// * `depth` - The depth of this element in the tree
///
/// # Returns
///
/// Debug information about the element.
pub fn debug_element_info<T: LayoutTree + PaintTree>(
    tree: &T,
    id: ElementId,
    depth: usize,
) -> RenderElementDebugInfo {
    RenderElementDebugInfo {
        id,
        depth,
        needs_layout: tree.needs_layout(id),
        needs_paint: tree.needs_paint(id),
        offset: LayoutTree::get_offset(tree, id),
    }
}

/// Formats a render element for debug output.
///
/// Produces a single-line summary suitable for tree visualization.
///
/// # Format
///
/// ```text
/// [id:42] needs_layout=true, needs_paint=false, offset=(10.0, 20.0)
/// ```
pub fn format_element_debug(info: &RenderElementDebugInfo) -> String {
    let offset_str = match info.offset {
        Some(o) => format!("({:.1}, {:.1})", o.dx, o.dy),
        None => "none".to_string(),
    };

    format!(
        "[id:{}] needs_layout={}, needs_paint={}, offset={}",
        info.id.get(),
        info.needs_layout,
        info.needs_paint,
        offset_str
    )
}

/// Formats a render element as a tree node with indentation.
///
/// # Arguments
///
/// * `info` - Debug information about the element
/// * `indent` - Indentation string (e.g., "  " for 2-space indent)
///
/// # Returns
///
/// A formatted string with proper indentation.
pub fn format_tree_node(info: &RenderElementDebugInfo, indent: &str) -> String {
    let prefix = indent.repeat(info.depth);
    let marker = if info.depth == 0 { "─" } else { "├─" };
    format!("{}{} {}", prefix, marker, format_element_debug(info))
}
