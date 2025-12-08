//! RenderIgnorePointer - Makes widget and children transparent to pointer events
//!
//! Implements Flutter's IgnorePointer that can make a widget subtree transparent
//! to pointer events while still rendering normally. Unlike AbsorbPointer which
//! consumes events, IgnorePointer allows events to pass through to widgets beneath.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderIgnorePointer` | `RenderIgnorePointer` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `ignoring` | `ignoring` property (bool) |
//! | `set_ignoring()` | `ignoring = value` setter |
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
//! 1. **Paint child normally**
//!    - Child is always painted regardless of `ignoring` flag
//!    - Ignoring only affects hit testing, not visual rendering
//!
//! # Hit Test Protocol
//!
//! 1. **Check ignoring flag**
//!    - If `ignoring = true`: Return false (events pass through)
//!    - If `ignoring = false`: Test children normally
//!
//! 2. **Event pass-through**
//!    - When ignoring: Events ignored by this widget and children
//!    - Events continue to widgets beneath (pass-through)
//!    - Widgets behind can still receive events
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(1) - pass-through to child
//! - **Hit Test**: O(1) when ignoring (return false immediately), O(child) when not ignoring
//! - **Memory**: 1 byte (bool flag)
//!
//! # Use Cases
//!
//! - **Non-interactive overlays**: Display decorative overlays without blocking clicks
//! - **Watermarks**: Show watermarks that don't interfere with interactions
//! - **Ghost buttons**: Show disabled buttons that allow clicks through
//! - **Visual feedback**: Display indicators without blocking underlying widgets
//! - **Animation overlays**: Show animations over interactive content
//! - **Tooltips**: Non-interactive tooltips that don't block clicks
//!
//! # Difference from RenderAbsorbPointer
//!
//! **RenderIgnorePointer (this):**
//! - Ignores pointer events (they pass through)
//! - Widgets beneath CAN receive events
//! - Doesn't add itself to hit test result
//! - Events continue to widgets below
//! - Returns false from hit test
//!
//! **RenderAbsorbPointer:**
//! - Consumes pointer events (they don't pass through)
//! - Widgets beneath are BLOCKED
//! - Adds itself to hit test result
//! - Events stop at that widget
//! - Returns true from hit test
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderIgnorePointer;
//!
//! // Make widget transparent to pointer events
//! let transparent = RenderIgnorePointer::new(true);
//!
//! // Allow pointer events (normal behavior)
//! let interactive = RenderIgnorePointer::new(false);
//!
//! // Decorative overlay that doesn't block clicks
//! let mut overlay = RenderIgnorePointer::new(true);
//! // Overlay is visible but clicks pass through to content beneath
//!
//! // Re-enable interactions
//! overlay.set_ignoring(false);
//! ```

use flui_rendering::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_interaction::HitTestResult;
use flui_types::Size;

/// RenderObject that makes its subtree transparent to pointer events.
///
/// When ignoring is enabled, this widget and its children are invisible to
/// hit testing, allowing events to pass through to widgets beneath. Child is
/// still laid out and painted normally - only hit testing is affected.
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
/// **Proxy** - Passes constraints unchanged, only affects hit testing.
///
/// # Use Cases
///
/// - **Decorative overlays**: Non-interactive visual elements over content
/// - **Watermarks**: Visible but non-interactive branding
/// - **Ghost UI**: Show UI elements without blocking underlying interactions
/// - **Animation layers**: Animated effects that don't interfere with clicks
/// - **Visual indicators**: Status indicators that allow clicks through
/// - **Non-blocking tooltips**: Informational overlays that don't capture events
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderIgnorePointer behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Child always painted (ignoring doesn't affect painting)
/// - When ignoring: returns false from hit test (events pass through)
/// - When not ignoring: tests children normally
/// - Events NOT consumed when ignoring (pass through to widgets beneath)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderIgnorePointer;
///
/// // Decorative overlay that allows clicks through
/// let overlay = RenderIgnorePointer::new(true);
///
/// // Re-enable interactions when needed
/// let mut pointer = RenderIgnorePointer::new(true);
/// pointer.set_ignoring(false);
/// ```
#[derive(Debug)]
pub struct RenderIgnorePointer {
    /// Whether to ignore pointer events
    pub ignoring: bool,
}

impl RenderIgnorePointer {
    /// Create new RenderIgnorePointer
    pub fn new(ignoring: bool) -> Self {
        Self { ignoring }
    }

    /// Check if ignoring pointer events
    pub fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Set whether to ignore pointer events
    ///
    /// This doesn't affect layout or paint, only hit testing.
    pub fn set_ignoring(&mut self, ignoring: bool) {
        self.ignoring = ignoring;
        // Note: In a full implementation, this would mark needs hit test update
    }
}

impl Default for RenderIgnorePointer {
    fn default() -> Self {
        Self { ignoring: true }
    }
}

impl RenderObject for RenderIgnorePointer {}

impl RenderBox<Single> for RenderIgnorePointer {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: pass constraints unchanged to child
        Ok(ctx.layout_child(child_id, ctx.constraints)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: paint child at widget offset
        // Ignoring only affects hit testing, not visual rendering
        ctx.paint_child(child_id, ctx.offset);
    }

    fn hit_test(&self, ctx: &BoxHitTestCtx<'_, Single>, result: &mut HitTestResult) -> bool {
        if self.ignoring {
            // Ignore pointer events - return false to allow events to pass through
            // This makes this widget and its children transparent to hit testing
            // Events continue to widgets beneath (pass-through behavior)
            false // Events pass through
        } else {
            // Not ignoring - delegate to children using default hit test behavior
            // Events handled normally by child
            ctx.hit_test_children(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_ignore_pointer_new() {
        let ignore = RenderIgnorePointer::new(true);
        assert!(ignore.ignoring());

        let ignore = RenderIgnorePointer::new(false);
        assert!(!ignore.ignoring());
    }

    #[test]
    fn test_render_ignore_pointer_default() {
        let ignore = RenderIgnorePointer::default();
        assert!(ignore.ignoring());
    }

    #[test]
    fn test_render_ignore_pointer_set_ignoring() {
        let mut ignore = RenderIgnorePointer::new(true);

        ignore.set_ignoring(false);
        assert!(!ignore.ignoring());

        ignore.set_ignoring(true);
        assert!(ignore.ignoring());
    }
}
