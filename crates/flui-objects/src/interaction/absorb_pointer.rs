//! RenderAbsorbPointer - Blocks pointer events from reaching children
//!
//! Implements Flutter's AbsorbPointer that can selectively block pointer events
//! while still rendering the child normally. Unlike IgnorePointer which allows
//! events to pass through, AbsorbPointer consumes events preventing them from
//! reaching any widgets beneath.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderAbsorbPointer` | `RenderAbsorbPointer` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `absorbing` | `absorbing` property (bool) |
//! | `set_absorbing()` | `absorbing = value` setter |
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
//!    - Child is always painted regardless of `absorbing` flag
//!    - Absorbing only affects hit testing, not visual rendering
//!
//! # Hit Test Protocol
//!
//! 1. **Check absorbing flag**
//!    - If `absorbing = true`: Add self to hit test result, DON'T test children
//!    - If `absorbing = false`: Test children normally
//!
//! 2. **Event consumption**
//!    - When absorbing: Events are consumed (return true)
//!    - Child never receives pointer events when absorbing
//!    - Widgets beneath this are also blocked from receiving events
//!
//! # Performance
//!
//! - **Layout**: O(1) - pass-through to child
//! - **Paint**: O(1) - pass-through to child
//! - **Hit Test**: O(1) when absorbing (skip child test), O(child) when not absorbing
//! - **Memory**: 1 byte (bool flag)
//!
//! # Use Cases
//!
//! - **Disable interactions**: Prevent user input during loading states
//! - **Modal overlays**: Block interactions with content behind modal
//! - **Temporary disabling**: Disable form inputs without hiding them
//! - **Tutorial overlays**: Show UI but prevent interaction
//! - **Loading screens**: Display content but block user input
//! - **Conditional interactivity**: Enable/disable interactions dynamically
//!
//! # Difference from RenderIgnorePointer
//!
//! **RenderAbsorbPointer (this):**
//! - Consumes pointer events (they don't pass through)
//! - Widgets beneath are also blocked
//! - Adds itself to hit test result
//! - Events stop at this widget
//!
//! **RenderIgnorePointer:**
//! - Ignores pointer events (they pass through)
//! - Widgets beneath can still receive events
//! - Doesn't add itself to hit test result
//! - Events continue to widgets below
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderAbsorbPointer;
//!
//! // Block all pointer events
//! let blocking = RenderAbsorbPointer::new(true);
//!
//! // Allow pointer events (transparent)
//! let allowing = RenderAbsorbPointer::new(false);
//!
//! // Toggle during loading
//! let mut absorber = RenderAbsorbPointer::new(false);
//! absorber.set_absorbing(true);  // Disable interactions during load
//! // ... after loading ...
//! absorber.set_absorbing(false); // Re-enable interactions
//! ```

use flui_interaction::{HitTestEntry, HitTestResult};
use flui_rendering::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::{Offset, Rect, Size};

/// RenderObject that blocks pointer events from reaching its child.
///
/// Consumes pointer events when absorbing is enabled, preventing them from
/// reaching the child or any widgets beneath. Child is still laid out and
/// painted normally - only hit testing is affected.
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
/// - **Loading states**: Block interactions while loading
/// - **Modal backgrounds**: Prevent clicks on content behind modals
/// - **Disabled UI**: Show UI but prevent interaction
/// - **Tutorial mode**: Display interface without allowing user input
/// - **Form validation**: Disable submit during validation
/// - **Overlay protection**: Prevent accidental clicks during animations
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderAbsorbPointer behavior:
/// - Passes constraints unchanged to child (proxy for layout)
/// - Size determined by child
/// - Child always painted (absorbing doesn't affect painting)
/// - When absorbing: adds self to hit test result, doesn't test children
/// - When not absorbing: tests children normally
/// - Events consumed when absorbing (don't pass through)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderAbsorbPointer;
///
/// // Block pointer events during loading
/// let mut absorber = RenderAbsorbPointer::new(true);
///
/// // Re-enable after loading
/// absorber.set_absorbing(false);
/// ```
#[derive(Debug)]
pub struct RenderAbsorbPointer {
    /// Whether to absorb pointer events
    pub absorbing: bool,
}

impl RenderAbsorbPointer {
    /// Create new RenderAbsorbPointer
    pub fn new(absorbing: bool) -> Self {
        Self { absorbing }
    }

    /// Check if absorbing pointer events
    pub fn absorbing(&self) -> bool {
        self.absorbing
    }

    /// Set whether to absorb pointer events
    ///
    /// This doesn't affect layout or paint, only hit testing.
    pub fn set_absorbing(&mut self, absorbing: bool) {
        self.absorbing = absorbing;
        // Note: In a full implementation, this would mark needs hit test update
    }
}

impl Default for RenderAbsorbPointer {
    fn default() -> Self {
        Self { absorbing: true }
    }
}

impl RenderObject for RenderAbsorbPointer {}

impl RenderBox<Single> for RenderAbsorbPointer {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: pass constraints unchanged to child
        Ok(ctx.layout_child(child_id, ctx.constraints, true)?)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Proxy behavior: paint child at widget offset
        // Absorbing only affects hit testing, not visual rendering
        ctx.paint_child(child_id, ctx.offset);
    }

    fn hit_test(&self, ctx: &BoxHitTestCtx<'_, Single>, result: &mut HitTestResult) -> bool {
        if self.absorbing {
            // Absorb pointer events - add self to hit test result but DON'T test children
            // This consumes the event and prevents it from reaching the child or widgets beneath
            let bounds = Rect::from_min_size(Offset::ZERO, ctx.size());
            let entry = HitTestEntry::new(ctx.element_id(), ctx.position, bounds);
            result.add(entry);
            true // Event absorbed (consumed)
        } else {
            // Not absorbing - delegate to children using default hit test behavior
            // Events pass through to child normally
            ctx.hit_test_children(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_absorb_pointer_new() {
        let absorb = RenderAbsorbPointer::new(true);
        assert!(absorb.absorbing());

        let absorb = RenderAbsorbPointer::new(false);
        assert!(!absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_default() {
        let absorb = RenderAbsorbPointer::default();
        assert!(absorb.absorbing());
    }

    #[test]
    fn test_render_absorb_pointer_set_absorbing() {
        let mut absorb = RenderAbsorbPointer::new(true);

        absorb.set_absorbing(false);
        assert!(!absorb.absorbing());

        absorb.set_absorbing(true);
        assert!(absorb.absorbing());
    }
}
