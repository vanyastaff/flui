//! `RenderVisibility` — single-child proxy that keeps its child laid out
//! while suppressing its paint.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of the private `_RenderVisibility`
//! (`packages/flutter/lib/src/widgets/indexed_stack.dart`), the render object
//! behind `Visibility`'s `maintainSize` branch:
//!
//! ```dart
//! @override
//! void paint(PaintingContext context, Offset offset) {
//!   if (!visible) {
//!     return;
//!   }
//!   super.paint(context, offset);
//! }
//! ```
//!
//! Layout is pure pass-through, which is the entire point: a hidden child
//! keeps occupying exactly the space it would occupy visible.
//!
//! # Why not `RenderOpacity`
//!
//! An opacity of zero would hide the child too, and Flutter's own
//! `Visibility` was written that way once. The oracle moved to a dedicated
//! render object for a specific reason, quoted at `_SliverVisibility`'s
//! definition: a fully opaque opacity widget must leave its opacity layer in
//! the layer tree, which forces every ancestor to composite as well and can
//! shatter one simple scene into many layers. A paint gate has no layer and
//! no compositing cost.
//!
//! # Not covered here
//!
//! The oracle also overrides `visitChildrenForSemantics` to gate the subtree
//! on `maintainSemantics || visible`. FLUI's render traits expose no
//! semantics-visiting hook, so that half is **not** implemented and
//! `Visibility` deliberately carries no `maintain_semantics` knob to imply
//! otherwise.

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    RenderUpdateImpact,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A render object that lays its child out normally but paints it only while
/// `visible`.
///
/// Hit-testing is left alone: whether a hidden child still receives pointer
/// events is `Visibility::maintain_interactivity`'s business, composed one
/// level up through `IgnorePointer`, exactly as the oracle composes it.
#[derive(Debug, Clone)]
pub struct RenderVisibility {
    visible: bool,
    has_child: bool,
}

impl RenderVisibility {
    /// Creates a visibility gate with the given flag.
    #[must_use]
    pub const fn new(visible: bool) -> Self {
        Self {
            visible,
            has_child: false,
        }
    }

    /// Returns whether the child is currently painted.
    #[must_use]
    #[inline]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Updates the visible flag, reporting whether a repaint is needed.
    ///
    /// Paint only — the child's geometry does not depend on this flag, which
    /// is why `maintainSize` maintains the size at all. The oracle's setter
    /// calls `markNeedsPaint()` for the same reason.
    pub fn set_visible(&mut self, visible: bool) -> RenderUpdateImpact {
        if self.visible == visible {
            return RenderUpdateImpact::NONE;
        }
        self.visible = visible;
        RenderUpdateImpact::PAINT
    }
}

impl Default for RenderVisibility {
    /// Defaults to visible, matching `Visibility`'s own `visible: true`.
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::Diagnosticable for RenderVisibility {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        // `add_flag` prints nothing when the flag is false, which would hide
        // the one state worth seeing here. The oracle prints both sides —
        // `FlagProperty('visible', ifFalse: 'hidden', ifTrue: 'visible')` — so
        // this reports unconditionally.
        builder.add("visible", if self.visible { "visible" } else { "hidden" });
    }
}

impl RenderBox for RenderVisibility {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn skip_paint(&self) -> bool {
        // Oracle: `if (!visible) { return; }` before `super.paint`.
        !self.visible
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_visible() {
        assert!(RenderVisibility::default().visible());
    }

    #[test]
    fn new_round_trips_flag() {
        assert!(RenderVisibility::new(true).visible());
        assert!(!RenderVisibility::new(false).visible());
    }

    /// The flag drives paint and nothing else — a layout impact here would
    /// defeat the point, since `maintainSize` exists precisely so hiding does
    /// not disturb geometry.
    #[test]
    fn set_visible_reports_paint_only_and_short_circuits() {
        let mut node = RenderVisibility::new(true);
        assert_eq!(node.set_visible(false), RenderUpdateImpact::PAINT);
        assert_eq!(node.set_visible(false), RenderUpdateImpact::NONE);
        assert_eq!(node.set_visible(true), RenderUpdateImpact::PAINT);
    }

    #[test]
    fn skip_paint_tracks_the_flag() {
        assert!(!RenderVisibility::new(true).skip_paint());
        assert!(RenderVisibility::new(false).skip_paint());
    }

    /// Both states must be reportable. A flag that prints only when true
    /// would omit exactly the case someone is debugging.
    #[test]
    fn debug_fill_properties_reports_either_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        for (visible, expected) in [(true, "visible"), (false, "hidden")] {
            let mut builder = DiagnosticsBuilder::new();
            RenderVisibility::new(visible).debug_fill_properties(&mut builder);
            let built = builder.build();
            let property = built
                .iter()
                .find(|p| p.name() == "visible")
                .unwrap_or_else(|| panic!("visible={visible}: missing diagnostic field"));
            assert_eq!(property.value(), expected);
        }
    }
}
