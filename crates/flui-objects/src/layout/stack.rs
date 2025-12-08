//! RenderStack - Layering container for overlapping widgets
//!
//! Implements Flutter's Stack layout that positions children in z-order layers,
//! allowing them to overlap. Supports both positioned children (with explicit
//! coordinates via PositionedMetadata) and non-positioned children (aligned
//! using alignment field).
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderStack` | `RenderStack` from `package:flutter/src/rendering/stack.dart` |
//! | `fit` | `fit` property (StackFit enum) |
//! | `alignment` | `alignment` property (AlignmentGeometry) |
//! | `set_fit()` | `fit = value` setter |
//! | `set_alignment()` | `alignment = value` setter |
//! | PositionedMetadata | Metadata from `Positioned` widget |
//! | `StackFit::Loose` | `StackFit.loose` (children can be smaller) |
//! | `StackFit::Expand` | `StackFit.expand` (children forced to max) |
//! | `StackFit::Passthrough` | `StackFit.passthrough` (no modification) |
//!
//! # Layout Protocol
//!
//! 1. **Determine child constraints based on fit**
//!    - `StackFit::Loose`: Loosen parent constraints (min → 0, max unchanged)
//!    - `StackFit::Expand`: Tight to parent's max size (force expansion)
//!    - `StackFit::Passthrough`: Use parent constraints unchanged
//!
//! 2. **Layout all children**
//!    - Non-positioned children: use fit-based constraints
//!    - Positioned children: TODO - compute constraints from PositionedMetadata
//!    - Track max width/height across all children
//!    - Store child sizes for paint phase
//!
//! 3. **Calculate stack size**
//!    - `StackFit::Expand`: use parent's biggest size
//!    - Otherwise: use max child size (width, height)
//!    - Clamp final size to parent constraints
//!
//! 4. **Calculate child offsets**
//!    - Non-positioned: apply alignment to position within stack bounds
//!    - Positioned: TODO - compute offset from PositionedMetadata
//!    - Cache offsets for efficient paint
//!
//! # Paint Protocol
//!
//! 1. **Paint in z-order**
//!    - Paint children in order (first child = bottom layer)
//!    - Last child paints on top (highest z-index)
//!
//! 2. **Apply cached offsets**
//!    - Use pre-computed offsets from layout phase
//!    - Paint each child at parent offset + child offset
//!
//! 3. **Allow overlap**
//!    - Children can overlap (later children paint over earlier)
//!    - No clipping by default (children can extend beyond stack)
//!
//! # Performance
//!
//! - **Layout**: O(n) - single pass through children for layout + offset calculation
//! - **Paint**: O(n) - paint each child in z-order
//! - **Memory**: 48 bytes base + O(n) for cached sizes/offsets (16 bytes per child)
//!
//! # Use Cases
//!
//! - **Overlays**: Position overlays on top of content (modals, tooltips, dialogs)
//! - **Layered UI**: Layer multiple widgets (backgrounds, content, badges)
//! - **Absolute positioning**: Position widgets at specific coordinates
//! - **Floating action buttons**: FABs positioned over scrollable content
//! - **Badge indicators**: Notification badges on corners of widgets
//! - **Image overlays**: Text, icons, or gradients over images
//! - **Z-index layouts**: Control visual stacking order
//! - **Card decorations**: Multiple decoration layers on cards
//!
//! # StackFit Behavior
//!
//! ```text
//! Loose (allow children to be smaller):
//!   Parent: min=0×0, max=400×600
//!   → Child: min=0×0, max=400×600 (loosened constraints)
//!   → Stack size: max of all child sizes
//!
//! Expand (force children to max):
//!   Parent: min=0×0, max=400×600
//!   → Child: tight 400×600 (forced to max size)
//!   → Stack size: 400×600 (parent's max)
//!
//! Passthrough (no modification):
//!   Parent: min=100×100, max=400×600
//!   → Child: min=100×100, max=400×600 (unchanged)
//!   → Stack size: max of all child sizes (clamped to parent)
//! ```
//!
//! # Positioned vs Non-Positioned
//!
//! **Positioned children** (have PositionedMetadata):
//! - Custom constraints computed from left/top/right/bottom
//! - Positioned at explicit coordinates
//! - Do not affect stack size by default
//! - TODO: Not yet implemented
//!
//! **Non-positioned children** (current implementation):
//! - Constraints from StackFit
//! - Aligned using alignment field
//! - Contribute to stack size (largest child determines size)
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderFlex**: Flex arranges children sequentially, Stack overlaps them
//! - **vs RenderPositionedBox**: PositionedBox is single-child, Stack is multi-child
//! - **vs RenderAlign**: Align is single-child, Stack handles multiple layers
//! - **vs RenderIndexedStack**: IndexedStack shows one child, Stack shows all
//! - **vs RenderLayer**: Layer manages GPU layers, Stack manages layout overlap
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderStack;
//! use flui_types::{StackFit, Alignment};
//!
//! // Loose fit (children can be smaller than max)
//! let stack = RenderStack::new();  // Default: Loose + TOP_LEFT
//!
//! // Expand fit (children forced to max size)
//! let mut stack = RenderStack::new();
//! stack.set_fit(StackFit::Expand);
//!
//! // Centered children
//! let stack = RenderStack::with_alignment(Alignment::CENTER);
//!
//! // Bottom-right aligned (for FABs)
//! let mut stack = RenderStack::new();
//! stack.set_alignment(Alignment::BOTTOM_RIGHT);
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, ChildrenAccess, RenderBox, Variable};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::constraints::BoxConstraints;
use flui_types::layout::StackFit;
use flui_types::{Alignment, Offset, Size};

/// RenderObject that layers children on top of each other.
///
/// Positions children in z-order layers with customizable sizing (StackFit)
/// and alignment. Supports both positioned children (with explicit coordinates
/// via PositionedMetadata) and non-positioned children (aligned within bounds).
/// Children paint in order - first child is bottom layer, last child is top.
///
/// # Arity
///
/// `Variable` - Can have any number of children (0+).
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Multi-child Layering Container** - Overlaps children with z-order control,
/// applies StackFit to determine sizing, uses Alignment to position non-positioned
/// children within bounds.
///
/// # Use Cases
///
/// - **Overlays**: Position modals, tooltips, dialogs over content
/// - **Layered UI**: Layer backgrounds, content, badges, decorations
/// - **Absolute positioning**: Position widgets at specific coordinates
/// - **Floating buttons**: FABs positioned over scrollable content
/// - **Badge indicators**: Notification badges on widget corners
/// - **Image overlays**: Text, icons, gradients over images
/// - **Z-index layouts**: Control visual stacking order
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderStack behavior:
/// - Respects StackFit for child sizing (Loose/Expand/Passthrough)
/// - Applies Alignment to position non-positioned children
/// - Paints in child order (first=back, last=front) for z-order
/// - Size determined by largest child (or max for Expand fit)
/// - TODO: Support positioned children via PositionedMetadata queries
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderStack;
/// use flui_types::{StackFit, Alignment};
///
/// // Stack with loose fit and center alignment
/// let mut stack = RenderStack::with_alignment(Alignment::CENTER);
///
/// // Expand to fill parent
/// stack.set_fit(StackFit::Expand);
///
/// // Bottom-right alignment for FABs
/// stack.set_alignment(Alignment::BOTTOM_RIGHT);
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

impl RenderObject for RenderStack {}

impl RenderBox<Variable> for RenderStack {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        let child_count = ctx.children.len();
        if child_count == 0 {
            self.child_sizes.clear();
            self.child_offsets.clear();
            return Ok(constraints.smallest());
        }

        // Clear caches
        self.child_sizes.clear();
        self.child_offsets.clear();

        // Layout all children and track max size
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;

        for child_id in ctx.children() {
            // For now, all children use fit-based constraints
            // TODO: Add PositionedMetadata support for positioned children
            let child_constraints = match self.fit {
                StackFit::Loose => constraints.loosen(),
                StackFit::Expand => BoxConstraints::tight(constraints.biggest()),
                StackFit::Passthrough => constraints,
            };

            let child_size = ctx.layout_child(child_id, child_constraints)?;
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

        Ok(size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children().collect();

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
