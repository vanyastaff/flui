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

use flui_foundation::Single;

use flui_rendering::{RenderUpdateImpact, parent_data::BoxParentData, traits::RenderBox};

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

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    fn skip_paint(&self) -> bool {
        // Oracle: `if (!visible) { return; }` before `super.paint`.
        !self.visible
    }

    flui_rendering::forward_single_child_box_hit_test!();
}

// ===========================================================================
// Tests
// ===========================================================================
