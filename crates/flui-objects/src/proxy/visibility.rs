//! `RenderVisibility` — single-child proxy that keeps its child laid out
//! while suppressing its paint.
//!
//! The render object behind `Visibility`'s `maintain_size` branch. Paint is
//! skipped while hidden; layout is pure pass-through, which is the entire
//! point: a hidden child keeps occupying exactly the space it would occupy
//! visible.
//!
//! A visibility gate emits no opacity layer. Hidden children also stay out
//! of the semantics walk unless `maintain_semantics` explicitly retains them.
//! Layout, paint, accessibility and pointer participation are independent.

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
    maintain_semantics: bool,
    has_child: bool,
}

impl RenderVisibility {
    /// Creates a visibility gate with the given flag.
    #[must_use]
    pub const fn new(visible: bool) -> Self {
        Self {
            visible,
            maintain_semantics: false,
            has_child: false,
        }
    }

    /// Returns whether the child is currently painted.
    #[must_use]
    #[inline]
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Updates paint visibility and invalidates semantics when the child
    /// enters or leaves the accessibility tree. Layout is unaffected.
    pub fn set_visible(&mut self, visible: bool) -> RenderUpdateImpact {
        if self.visible == visible {
            return RenderUpdateImpact::NONE;
        }
        self.visible = visible;
        if self.maintain_semantics {
            RenderUpdateImpact::PAINT
        } else {
            RenderUpdateImpact::PAINT | RenderUpdateImpact::SEMANTICS
        }
    }

    /// Keeps hidden children in the accessibility tree when enabled.
    /// Only a change to effective child visitation invalidates semantics.
    pub fn set_maintain_semantics(&mut self, maintain_semantics: bool) -> RenderUpdateImpact {
        if self.maintain_semantics == maintain_semantics {
            return RenderUpdateImpact::NONE;
        }
        self.maintain_semantics = maintain_semantics;
        if self.visible {
            RenderUpdateImpact::NONE
        } else {
            RenderUpdateImpact::SEMANTICS
        }
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
        builder.add_flag("maintain_semantics", self.maintain_semantics, "retained");
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

    fn visits_child_for_semantics(&self, _child_slot: usize) -> bool {
        self.visible || self.maintain_semantics
    }

    flui_rendering::forward_single_child_box_hit_test!();
}

// ===========================================================================
// Tests
// ===========================================================================
