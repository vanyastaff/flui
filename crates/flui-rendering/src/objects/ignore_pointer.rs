//! `RenderIgnorePointer` — single-child proxy that, when active, makes
//! its entire subtree invisible to pointer events. Pointers pass
//! through to whatever is painted *behind* the stack.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderIgnorePointer`](https://api.flutter.dev/flutter/rendering/RenderIgnorePointer-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvements
//!
//! * `ignoring` is a typed `bool` boundary; setter returns
//!   `bool` change-flag for pipeline `mark_needs_paint` /
//!   `mark_needs_layout` short-circuit.
//! * Layout / paint are pure pass-through — only `hit_test` differs
//!   from a transparent proxy. The semantic difference between
//!   `RenderIgnorePointer` and [`crate::objects::RenderAbsorbPointer`]
//!   ("ignore = pointers pass through" vs "absorb = pointer caught
//!   here, nothing below sees it") lives entirely in `hit_test`.

use flui_tree::Single;
use flui_types::{Offset, Point, Rect, Size};

use crate::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

/// A render object that, when `ignoring` is true, returns `false` from
/// hit testing — making the subtree (and itself) invisible to pointer
/// events, so the gesture system sees through to siblings below.
///
/// Layout and paint pass through transparently in all cases.
#[derive(Debug, Clone)]
pub struct RenderIgnorePointer {
    ignoring: bool,
    size: Size,
    has_child: bool,
}

impl RenderIgnorePointer {
    /// Creates an ignore-pointer render object with the given flag.
    pub const fn new(ignoring: bool) -> Self {
        Self {
            ignoring,
            size: Size::ZERO,
            has_child: false,
        }
    }

    /// Returns whether pointer events are currently ignored.
    #[inline]
    pub fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Updates the ignoring flag; returns true if the value changed.
    pub fn set_ignoring(&mut self, ignoring: bool) -> bool {
        if self.ignoring == ignoring {
            return false;
        }
        self.ignoring = ignoring;
        true
    }
}

impl Default for RenderIgnorePointer {
    /// Defaults to `ignoring = true` (Flutter parity).
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::Diagnosticable for RenderIgnorePointer {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("ignoring", self.ignoring, "ignoring");
    }
}

impl RenderBox for RenderIgnorePointer {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            self.size = child_size;
        } else {
            self.has_child = false;
            self.size = constraints.smallest();
        }
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    // paint: default pass-through (splices the child in order).

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if self.ignoring {
            // Pointer events pass straight through to siblings below.
            return false;
        }
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderIgnorePointer {}
impl SemanticsCapability for RenderIgnorePointer {}
impl HotReloadCapability for RenderIgnorePointer {}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn defaults_to_ignoring() {
        let node = RenderIgnorePointer::default();
        assert!(node.ignoring());
    }

    #[test]
    fn new_round_trips_flag() {
        assert!(RenderIgnorePointer::new(true).ignoring());
        assert!(!RenderIgnorePointer::new(false).ignoring());
    }

    #[test]
    fn set_ignoring_returns_change_flag() {
        let mut node = RenderIgnorePointer::new(false);
        assert!(node.set_ignoring(true));
        assert!(!node.set_ignoring(true));
        assert!(node.set_ignoring(false));
    }

    #[test]
    fn box_paint_bounds_matches_size() {
        let mut node = RenderIgnorePointer::new(false);
        *node.size_mut() = Size::new(px(80.0), px(40.0));
        let r = node.box_paint_bounds();
        assert_eq!(r.width(), px(80.0));
        assert_eq!(r.height(), px(40.0));
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderIgnorePointer::default();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "ignoring"),
            "missing diagnostic field: ignoring"
        );
    }
}
