//! `RenderAbsorbPointer` — single-child proxy that, when active,
//! catches pointer events itself and prevents its child from
//! receiving them. Siblings *below* in the paint order also see
//! nothing (the box is "opaque" to pointers).
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderAbsorbPointer`](https://api.flutter.dev/flutter/rendering/RenderAbsorbPointer-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvements
//!
//! * `absorbing` is a typed `bool` boundary; setter returns
//!   `bool` change-flag for pipeline `mark_needs_paint` /
//!   `mark_needs_layout` short-circuit.
//! * Semantic contrast with [`crate::objects::RenderIgnorePointer`]
//!   ("absorb = self-catch, nothing below" vs "ignore = nothing
//!   here, pointers fall through") lives in `hit_test` — both objects
//!   share the same transparent-proxy layout/paint pipeline so the
//!   `hit_test` body is the *only* place the semantic differs.

use flui_tree::Single;
use flui_types::{Offset, Point, Rect, Size};

use crate::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

/// A render object that, when `absorbing` is true, takes any pointer
/// hit within its bounds for itself — its child is never tested.
///
/// Layout and paint are pure pass-throughs (the child is laid out
/// with the parent's constraints and painted normally); only
/// hit-test diverges.
#[derive(Debug, Clone)]
pub struct RenderAbsorbPointer {
    absorbing: bool,
    size: Size,
    has_child: bool,
}

impl RenderAbsorbPointer {
    /// Creates an absorb-pointer render object with the given flag.
    pub const fn new(absorbing: bool) -> Self {
        Self {
            absorbing,
            size: Size::ZERO,
            has_child: false,
        }
    }

    /// Returns whether pointer events are currently absorbed.
    #[inline]
    pub fn absorbing(&self) -> bool {
        self.absorbing
    }

    /// Updates the absorbing flag; returns true if the value changed.
    pub fn set_absorbing(&mut self, absorbing: bool) -> bool {
        if self.absorbing == absorbing {
            return false;
        }
        self.absorbing = absorbing;
        true
    }
}

impl Default for RenderAbsorbPointer {
    /// Defaults to `absorbing = true` (Flutter parity).
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::Diagnosticable for RenderAbsorbPointer {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("absorbing", self.absorbing, "absorbing");
    }
}

impl RenderBox for RenderAbsorbPointer {
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

    crate::forward_single_child_box_queries!();

    // paint: default pass-through (splices the child in order).

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }
        if self.absorbing {
            // We are the target. The child is never tested.
            // TODO(core.1): once the gesture system threads a target
            // id through hit-test contexts, call `ctx.add_self(id)`
            // here. For now the framework relies on the in-bounds
            // truthy return to keep the hit registered.
            true
        } else if self.has_child {
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
impl PaintEffectsCapability for RenderAbsorbPointer {}
impl SemanticsCapability for RenderAbsorbPointer {}
impl HotReloadCapability for RenderAbsorbPointer {}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn defaults_to_absorbing() {
        let node = RenderAbsorbPointer::default();
        assert!(node.absorbing());
    }

    #[test]
    fn new_round_trips_flag() {
        assert!(RenderAbsorbPointer::new(true).absorbing());
        assert!(!RenderAbsorbPointer::new(false).absorbing());
    }

    #[test]
    fn set_absorbing_returns_change_flag() {
        let mut node = RenderAbsorbPointer::new(false);
        assert!(node.set_absorbing(true));
        assert!(!node.set_absorbing(true));
        assert!(node.set_absorbing(false));
    }

    #[test]
    fn box_paint_bounds_matches_size() {
        let mut node = RenderAbsorbPointer::new(false);
        *node.size_mut() = Size::new(px(120.0), px(60.0));
        let r = node.box_paint_bounds();
        assert_eq!(r.width(), px(120.0));
        assert_eq!(r.height(), px(60.0));
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderAbsorbPointer::default();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "absorbing"),
            "missing diagnostic field: absorbing"
        );
    }
}
