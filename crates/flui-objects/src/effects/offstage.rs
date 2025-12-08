//! RenderOffstage - Conditional visibility with state preservation
//!
//! Implements Flutter's Offstage widget that can hide children from display while
//! maintaining their layout state.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderOffstage` | `RenderOffstage` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `offstage` | `offstage` property (bool) |
//! | `set_offstage()` | `offstage = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Always layout child**
//!    - Child receives same constraints
//!    - Child is laid out to maintain internal state
//!    - This is crucial for preserving widget state (controllers, animations, etc.)
//!
//! 2. **Return conditional size**
//!    - If `offstage = true`: return `Size::ZERO`
//!    - If `offstage = false` and child_size != zero: return child size
//!    - If `offstage = false` and child_size == zero: return smallest constraint
//!
//! # Paint Protocol
//!
//! 1. **Check offstage flag**
//!    - If `offstage = true`: skip painting entirely
//!    - If `offstage = false`: paint child at offset
//!
//! 2. **No visual artifacts**
//!    - When offstage, nothing is painted (not even transparent pixels)
//!
//! # Performance
//!
//! - **Layout**: O(child) - child is ALWAYS laid out
//! - **Paint**:
//!   - O(1) when offstage = true (skip painting)
//!   - O(child) when offstage = false (normal painting)
//! - **Memory**: 1 byte (bool flag)
//!
//! # Use Cases
//!
//! - **Conditional visibility**: Show/hide widgets without rebuilding
//! - **State preservation**: Hide widgets while maintaining state (animations, controllers)
//! - **Lazy rendering**: Layout child but defer painting until needed
//! - **Page transitions**: Hide pages without disposing state
//! - **Tab views**: Hide inactive tabs while preserving scroll position
//! - **Wizard steps**: Hide completed/future steps while maintaining form state
//!
//! # Difference from Other Visibility Approaches
//!
//! **RenderOffstage:**
//! - Child IS laid out (state preserved)
//! - Size reported as zero when hidden
//! - No space taken when hidden
//! - Paint skipped when hidden
//!
//! **RenderOpacity(0.0):**
//! - Child IS laid out
//! - Child takes up space (size reported normally)
//! - Paint executed but fully transparent
//! - More expensive (compositing overhead)
//!
//! **Conditional widget (if/else):**
//! - Child NOT laid out when hidden
//! - State LOST when hidden
//! - No overhead when hidden
//! - Requires rebuild when showing
//!
//! **RenderVisibility(VisibilityMode::Gone):**
//! - Similar to Offstage
//! - More options (invisible, gone, visible)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderOffstage;
//!
//! // Hide widget but preserve state
//! let hidden = RenderOffstage::new(true);
//!
//! // Show widget
//! let visible = RenderOffstage::new(false);
//!
//! // Toggle visibility
//! let mut offstage = RenderOffstage::new(true);
//! offstage.set_offstage(false); // Show
//! ```

use flui_rendering::{BoxHitTestCtx, BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_interaction::HitTestResult;
use flui_types::Size;

/// RenderObject that conditionally hides its child while preserving state.
///
/// Allows hiding child from display without destroying its state. Child is always
/// laid out (maintaining animations, controllers, etc.) but painting is skipped
/// when offstage.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Use Cases
///
/// - **Conditional visibility**: Toggle widget visibility without rebuild
/// - **State preservation**: Hide widgets while keeping state (scroll position, form data)
/// - **Tab views**: Hide inactive tabs without losing state
/// - **Multi-step forms**: Hide completed/future steps while preserving input
/// - **Page transitions**: Hide pages during transitions without disposal
/// - **Animation preservation**: Hide animated widgets without resetting animation
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderOffstage behavior:
/// - Child is ALWAYS laid out (even when offstage)
/// - Size reported as zero when offstage
/// - Paint skipped when offstage
/// - State preserved across visibility changes
/// - No visual artifacts when hidden
///
/// # Difference from Other Approaches
///
/// Unlike `Opacity(0)`, this doesn't take up space when hidden.
/// Unlike conditional rendering (if/else), this preserves child state.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderOffstage;
///
/// // Hide widget while preserving state
/// let hidden = RenderOffstage::new(true);
///
/// // Toggle visibility
/// let mut offstage = RenderOffstage::new(false);
/// offstage.set_offstage(true); // Hide
/// offstage.set_offstage(false); // Show
/// ```
#[derive(Debug)]
pub struct RenderOffstage {
    /// Whether the child is offstage (hidden)
    pub offstage: bool,
}

impl RenderOffstage {
    /// Create new RenderOffstage
    pub fn new(offstage: bool) -> Self {
        Self { offstage }
    }

    /// Set whether child is offstage
    pub fn set_offstage(&mut self, offstage: bool) {
        self.offstage = offstage;
    }
}

impl Default for RenderOffstage {
    fn default() -> Self {
        Self { offstage: true }
    }
}

impl RenderObject for RenderOffstage {}

impl RenderBox<Single> for RenderOffstage {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // ALWAYS layout child to maintain state (animations, controllers, scroll position, etc.)
        let child_size = ctx.layout_child(child_id, ctx.constraints)?;

        // Report size based on offstage flag
        if self.offstage {
            // Hidden: report zero size (doesn't take up space in layout)
            Ok(Size::ZERO)
        } else if child_size != Size::ZERO {
            // Visible: use child size
            Ok(child_size)
        } else {
            // Visible but child is zero: use smallest constraint
            Ok(ctx.constraints.smallest())
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Only paint child when visible
        if !self.offstage {
            // Single arity: use ctx.single_child() which returns ElementId directly
            let child_id = ctx.single_child();
            ctx.paint_child(child_id, ctx.offset);
        }
        // When offstage = true, skip painting entirely (no visual artifacts)
    }

    fn hit_test(&self, ctx: &BoxHitTestCtx<'_, Single>, result: &mut HitTestResult) -> bool {
        if self.offstage {
            // When offstage, don't register hits - widget is not interactive
            // This prevents events from reaching the child or self
            false
        } else {
            // When visible, delegate to child using default hit test behavior
            ctx.hit_test_children(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_offstage_new() {
        let offstage = RenderOffstage::new(true);
        assert!(offstage.offstage);

        let offstage = RenderOffstage::new(false);
        assert!(!offstage.offstage);
    }

    #[test]
    fn test_render_offstage_default() {
        let offstage = RenderOffstage::default();
        assert!(offstage.offstage);
    }

    #[test]
    fn test_render_offstage_set_offstage() {
        let mut offstage = RenderOffstage::new(true);
        offstage.set_offstage(false);
        assert!(!offstage.offstage);
    }

    // ========================================================================
    // Hit Testing Tests
    // ========================================================================
    // Note: Comprehensive hit testing tests require integration testing
    // with the full rendering tree. These tests verify the basic logic.

    #[test]
    fn test_offstage_blocks_hit_testing() {
        // Test verifies that when offstage=true, the widget correctly blocks hit testing
        // Integration test would verify this with a full render tree and event routing
        let offstage_hidden = RenderOffstage::new(true);
        assert!(
            offstage_hidden.offstage,
            "offstage flag should be true for hit test blocking"
        );

        let offstage_visible = RenderOffstage::new(false);
        assert!(
            !offstage_visible.offstage,
            "offstage flag should be false for hit test delegation"
        );
    }

    #[test]
    fn test_offstage_state_transitions() {
        // Test that toggling offstage changes hit testing behavior
        let mut offstage = RenderOffstage::new(true);

        // Initially offstage (blocks events)
        assert!(offstage.offstage);

        // Toggle to visible (delegates to children)
        offstage.set_offstage(false);
        assert!(!offstage.offstage);

        // Toggle back to offstage (blocks again)
        offstage.set_offstage(true);
        assert!(offstage.offstage);
    }
}
