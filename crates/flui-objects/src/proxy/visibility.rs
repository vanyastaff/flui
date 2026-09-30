//! `RenderVisibility` — single-child proxy that keeps its child laid out
//! while suppressing its paint.
//!
//! The render object behind `Visibility`'s `maintain_size` branch. Paint is
//! skipped while hidden; layout is pure pass-through, which is the entire
//! point: a hidden child keeps occupying exactly the space it would occupy
//! visible.
//!
//! # Why not `RenderOpacity`
//!
//! An opacity of zero would hide the child too, but a fully opaque opacity
//! object must leave its opacity layer in the layer tree, which forces every
//! ancestor to composite as well and can shatter one simple scene into many
//! layers. A paint gate has no layer and no compositing cost.
//!
//! # Not covered here
//!
//! Gating the subtree's semantics on `maintain_semantics || visible` is not
//! covered. FLUI's render traits expose no
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
/// level up through `IgnorePointer`.
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
    /// is why `maintain_size` maintains the size at all.
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
        // the one state worth seeing here, so this reports unconditionally.
        builder.add("visible", if self.visible { "visible" } else { "hidden" });
    }
}

impl RenderBox for RenderVisibility {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    fn skip_paint(&self) -> bool {
        !self.visible
    }

    flui_rendering::forward_single_child_box_hit_test!();
}

// ===========================================================================
// Tests
// ===========================================================================
