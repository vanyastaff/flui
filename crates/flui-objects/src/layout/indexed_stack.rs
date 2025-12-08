//! RenderIndexedStack - Selective display container for index-based view switching
//!
//! Implements Flutter's IndexedStack that displays only one child at a time
//! based on an index. All children are laid out (to maintain state) but only
//! the selected child is painted. Ideal for tab views, page views, and view
//! switching where inactive views must preserve state.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderIndexedStack` | `RenderIndexedStack` from `package:flutter/src/rendering/stack.dart` |
//! | `index` | `index` property (which child to show) |
//! | `alignment` | `alignment` property (how to align selected child) |
//! | `set_index()` | `index = value` setter |
//! | `set_alignment()` | `alignment = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Layout ALL children**
//!    - Pass parent constraints to each child
//!    - Layout every child (not just selected one)
//!    - This maintains state for all children
//!    - Store all child sizes
//!
//! 2. **Calculate container size**
//!    - Size = max of all child sizes (width, height)
//!    - Clamp to parent constraints
//!    - Even non-visible children affect size
//!
//! # Paint Protocol
//!
//! 1. **Paint ONLY selected child**
//!    - Check if index is valid
//!    - Get child at index
//!    - Calculate aligned offset for selected child
//!    - Paint only that child (others are invisible)
//!
//! 2. **If index is None**
//!    - Paint nothing (all children invisible)
//!    - Container still has size from layout
//!
//! # Performance
//!
//! - **Layout**: O(n) - layouts ALL children regardless of index
//! - **Paint**: O(1) - paints only selected child
//! - **Memory**: 40 bytes base + O(n) for cached sizes (8 bytes per child)
//!
//! # Use Cases
//!
//! - **Tab views**: Switch between tabs while preserving state
//! - **Page views**: Navigate between pages with state retention
//! - **View switching**: Toggle between views (e.g., list/grid)
//! - **Wizard flows**: Multi-step forms with back/forward navigation
//! - **Settings panels**: Different setting categories
//! - **Dashboard views**: Switch between dashboard sections
//! - **Game UI**: Switch between menu/play/settings screens
//!
//! # Layout vs Paint Difference
//!
//! **Key behavior**: All children are LAID OUT, but only one is PAINTED.
//!
//! ```text
//! Layout phase (all children):
//!   Child 0: Layout → Size 100×50
//!   Child 1: Layout → Size 150×75  ← Selected (index = 1)
//!   Child 2: Layout → Size 120×60
//!   Container size: 150×75 (max of all)
//!
//! Paint phase (only selected):
//!   Child 0: NOT painted
//!   Child 1: Painted ← Only this one visible
//!   Child 2: NOT painted
//! ```
//!
//! # Why Layout All Children?
//!
//! - **State preservation**: Children maintain their state even when not visible
//! - **Smooth transitions**: Switching is instant (no re-layout needed)
//! - **Consistent sizing**: Container size doesn't change when switching
//! - **Flutter compliance**: Matches Flutter's IndexedStack behavior
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderStack**: Stack shows ALL children, IndexedStack shows ONE
//! - **vs Conditional rendering**: IndexedStack preserves all child state
//! - **vs RenderOpacity(0)**: IndexedStack doesn't paint at all (more efficient)
//! - **vs RenderOffstage**: Offstage can optionally skip layout, IndexedStack always layouts
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::RenderIndexedStack;
//! use flui_types::Alignment;
//!
//! // Show first child (index 0)
//! let mut stack = RenderIndexedStack::new(Some(0));
//!
//! // Switch to second child
//! stack.set_index(Some(1));
//!
//! // Hide all children
//! stack.set_index(None);
//!
//! // Center the selected child
//! let stack = RenderIndexedStack::with_alignment(Some(0), Alignment::CENTER);
//! ```

use flui_rendering::{RenderObject, RenderResult};

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, ChildrenAccess, RenderBox, Variable};
use flui_types::{Alignment, Size};

/// RenderObject that displays only one child from multiple children by index.
///
/// Lays out ALL children (to preserve state) but paints only the child at
/// the specified index. Ideal for tab views and page navigation where
/// inactive views must maintain state. Container size is max of all children.
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
/// **Selective Display Container** - Layouts all children for state preservation,
/// paints only selected child, sizes to largest child, applies alignment to
/// selected child.
///
/// # Use Cases
///
/// - **Tab views**: Switch between tabs while preserving each tab's state
/// - **Page views**: Navigate between pages with full state retention
/// - **View switching**: Toggle between different views (list/grid/table)
/// - **Wizard flows**: Multi-step forms with back/forward navigation
/// - **Settings panels**: Switch between setting categories
/// - **Dashboard**: Switch between dashboard sections
/// - **Game UI**: Menu/play/settings screen switching
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderIndexedStack behavior:
/// - Layouts ALL children regardless of index (state preservation)
/// - Paints only child at specified index
/// - Size is max of all children (not just visible child)
/// - index = None shows nothing
/// - Applies alignment to selected child
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderIndexedStack;
/// use flui_types::Alignment;
///
/// // Tab view showing first tab
/// let mut tabs = RenderIndexedStack::new(Some(0));
///
/// // Switch to second tab (first tab retains state)
/// tabs.set_index(Some(1));
///
/// // Centered child
/// let centered = RenderIndexedStack::with_alignment(Some(0), Alignment::CENTER);
/// ```
#[derive(Debug)]
pub struct RenderIndexedStack {
    /// Index of child to display (None = show nothing)
    pub index: Option<usize>,
    /// How to align the selected child
    pub alignment: Alignment,

    // Cache for paint
    child_sizes: Vec<Size>,
    size: Size,
}

impl RenderIndexedStack {
    /// Create new indexed stack
    pub fn new(index: Option<usize>) -> Self {
        Self {
            index,
            alignment: Alignment::TOP_LEFT,
            child_sizes: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Create with specific alignment
    pub fn with_alignment(index: Option<usize>, alignment: Alignment) -> Self {
        Self {
            index,
            alignment,
            child_sizes: Vec::new(),
            size: Size::ZERO,
        }
    }

    /// Set new index
    pub fn set_index(&mut self, index: Option<usize>) {
        self.index = index;
    }

    /// Set new alignment
    pub fn set_alignment(&mut self, alignment: Alignment) {
        self.alignment = alignment;
    }
}

impl Default for RenderIndexedStack {
    fn default() -> Self {
        Self::new(None)
    }
}

impl RenderObject for RenderIndexedStack {}

impl RenderBox<Variable> for RenderIndexedStack {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Variable>) -> RenderResult<Size> {
        let constraints = ctx.constraints;
        let children = ctx.children;

        if children.as_slice().is_empty() {
            self.child_sizes.clear();
            return Ok(constraints.smallest());
        }

        // Layout all children (to maintain their state)
        let mut max_width: f32 = 0.0;
        let mut max_height: f32 = 0.0;
        self.child_sizes.clear();

        for child in children.iter() {
            let child_size = ctx.layout_child(*child, constraints)?;
            self.child_sizes.push(child_size);
            max_width = max_width.max(child_size.width);
            max_height = max_height.max(child_size.height);
        }

        // Size is the max of all children
        self.size = Size::new(
            max_width.clamp(constraints.min_width, constraints.max_width),
            max_height.clamp(constraints.min_height, constraints.max_height),
        );
        Ok(self.size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Variable>) {
        let offset = ctx.offset;

        // Collect child IDs first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        // Only paint the selected child
        if let Some(index) = self.index {
            if let (Some(&child_id), Some(&child_size)) =
                (child_ids.get(index), self.child_sizes.get(index))
            {
                // Calculate aligned position
                let child_offset = self.alignment.calculate_offset(child_size, self.size);

                // Paint child
                ctx.paint_child(*child_id, offset + child_offset);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_indexed_stack_new() {
        let stack = RenderIndexedStack::new(Some(0));
        assert_eq!(stack.index, Some(0));
        assert_eq!(stack.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_render_indexed_stack_with_alignment() {
        let stack = RenderIndexedStack::with_alignment(Some(1), Alignment::CENTER);
        assert_eq!(stack.index, Some(1));
        assert_eq!(stack.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_render_indexed_stack_default() {
        let stack = RenderIndexedStack::default();
        assert_eq!(stack.index, None);
    }

    #[test]
    fn test_render_indexed_stack_set_index() {
        let mut stack = RenderIndexedStack::new(Some(0));
        stack.set_index(Some(1));
        assert_eq!(stack.index, Some(1));
    }

    #[test]
    fn test_render_indexed_stack_set_alignment() {
        let mut stack = RenderIndexedStack::new(Some(0));
        stack.set_alignment(Alignment::CENTER);
        assert_eq!(stack.alignment, Alignment::CENTER);
    }
}
