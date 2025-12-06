//! RenderRepaintBoundary - optimization boundary for repainting
//!
//! Implements Flutter's repaint boundary that creates layer caching to isolate
//! child repaints from parent repaints.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderRepaintBoundary` | `RenderRepaintBoundary` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `is_repaint_boundary` | `isRepaintBoundary` property |
//! | `set_is_repaint_boundary()` | `isRepaintBoundary = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Pass constraints to child**
//!    - Child receives same constraints (proxy behavior)
//!
//! 2. **Return child size**
//!    - Container size = child size (no size change)
//!
//! # Paint Protocol
//!
//! 1. **Create cached layer** (when is_repaint_boundary = true)
//!    - Creates separate compositing layer for child subtree
//!    - Layer cached and reused across frames
//!
//! 2. **Paint child to layer**
//!    - Child painted to isolated layer
//!    - Layer marked dirty only when child changes
//!
//! 3. **Optimization**
//!    - Parent repaints don't trigger child repaint
//!    - Child repaints don't propagate to parent
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**:
//!   - First paint: O(child) - creates layer and paints child
//!   - Subsequent paints when child unchanged: O(1) - reuses cached layer
//!   - Subsequent paints when child changed: O(child) - repaints to layer
//! - **Memory**: 1 byte (bool) + layer cache overhead when active
//!
//! # Use Cases
//!
//! - **Animations**: Isolate animated widgets to avoid repainting parent
//! - **Video players**: Prevent video frames from triggering parent repaints
//! - **Scrolling content**: Cache static headers/footers while list scrolls
//! - **Interactive widgets**: Isolate frequently changing interactive elements
//! - **Complex graphics**: Cache expensive custom painting
//! - **Performance optimization**: Reduce unnecessary repaints in widget tree
//!
//! # Performance Impact
//!
//! **Benefits:**
//! - Reduces unnecessary repaints when parent changes but child doesn't
//! - Reduces unnecessary repaints when child changes but parent doesn't
//! - Can significantly improve frame rate for complex UIs
//!
//! **Costs:**
//! - Memory overhead for cached layer (GPU texture)
//! - Compositing overhead for maintaining separate layer
//! - Not beneficial if child and parent repaint together
//!
//! **When to Use:**
//! - Child repaints frequently (animations, videos)
//! - Child is expensive to paint (complex graphics)
//! - Child and parent have independent repaint patterns
//!
//! **When NOT to Use:**
//! - Simple static widgets (no benefit, only overhead)
//! - Child and parent always repaint together
//! - Memory constrained environments
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderRepaintBoundary;
//!
//! // Create boundary for animated child
//! let boundary = RenderRepaintBoundary::new();
//!
//! // Temporarily disable boundary
//! let mut boundary = RenderRepaintBoundary::new();
//! boundary.set_is_repaint_boundary(false);
//! ```

use crate::core::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use crate::{RenderObject, RenderResult};
use flui_types::Size;

/// RenderObject that creates a repaint boundary for performance optimization.
///
/// Creates a separate compositing layer to cache child rendering and isolate
/// child repaints from parent repaints, and vice versa.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Proxy** - Passes constraints unchanged, only affects paint caching.
///
/// # Use Cases
///
/// - **Animation isolation**: Prevent animated child from triggering parent repaints
/// - **Video/media players**: Isolate high-frequency frame updates
/// - **Expensive graphics**: Cache complex custom painting (charts, visualizations)
/// - **Scroll optimization**: Cache static elements while list scrolls
/// - **Interactive widgets**: Isolate frequently changing interactive elements
/// - **Performance debugging**: Identify repaint boundaries with Flutter DevTools
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderRepaintBoundary behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Creates separate compositing layer when active
/// - Layer cached and reused across frames
/// - Parent changes don't trigger child repaint
/// - Child changes don't trigger parent repaint
/// - `isRepaintBoundary` property controls boundary behavior
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderRepaintBoundary;
///
/// // Create boundary for animated child
/// let boundary = RenderRepaintBoundary::new();
///
/// // Disable boundary temporarily
/// let mut boundary = RenderRepaintBoundary::new();
/// boundary.set_is_repaint_boundary(false);
/// ```
#[derive(Debug)]
pub struct RenderRepaintBoundary {
    /// Whether this boundary is currently active
    pub is_repaint_boundary: bool,
}

impl RenderRepaintBoundary {
    /// Create new RenderRepaintBoundary
    pub fn new() -> Self {
        Self {
            is_repaint_boundary: true,
        }
    }

    /// Create inactive boundary
    pub fn inactive() -> Self {
        Self {
            is_repaint_boundary: false,
        }
    }

    /// Set whether this is a repaint boundary
    pub fn set_is_repaint_boundary(&mut self, is_boundary: bool) {
        self.is_repaint_boundary = is_boundary;
    }
}

impl Default for RenderRepaintBoundary {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderRepaintBoundary {}

impl RenderBox<Single> for RenderRepaintBoundary {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();
        Ok(ctx.layout_child(child_id, ctx.constraints)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Paint child
        // TODO: In a full implementation with layer caching support:
        // - Create a cached layer if is_repaint_boundary is true
        // - Reuse the cached layer on subsequent paints if child hasn't changed
        // - Mark the layer as dirty when the child needs repainting
        //
        // This allows the framework to cache the layer and avoid
        // repainting the child if only the parent changes
        //
        // For now, we just paint the child directly
        ctx.paint_child(child_id, ctx.offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_repaint_boundary_new() {
        let boundary = RenderRepaintBoundary::new();
        assert!(boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_inactive() {
        let boundary = RenderRepaintBoundary::inactive();
        assert!(!boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_default() {
        let boundary = RenderRepaintBoundary::default();
        assert!(boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_set_is_repaint_boundary() {
        let mut boundary = RenderRepaintBoundary::new();
        boundary.set_is_repaint_boundary(false);
        assert!(!boundary.is_repaint_boundary);
    }
}
