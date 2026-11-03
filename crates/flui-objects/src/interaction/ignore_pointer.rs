//! `RenderIgnorePointer` — single-child proxy that, when active, makes
//! its entire subtree invisible to pointer events. Pointers pass
//! through to whatever is painted *behind* the stack.
//!
//! # Design
//!
//! * `ignoring` is a typed `bool` boundary; setter returns
//!   `bool` change-flag for pipeline `mark_needs_paint` /
//!   `mark_needs_layout` short-circuit.
//! * Layout / paint are pure pass-through — only `hit_test` differs
//!   from a transparent proxy. The semantic difference between
//!   `RenderIgnorePointer` and [`crate::RenderAbsorbPointer`]
//!   ("ignore = pointers pass through" vs "absorb = pointer caught
//!   here, nothing below sees it") lives entirely in `hit_test`.

use flui_foundation::Single;
use flui_foundation::geometry::Offset;

use flui_rendering::{context::BoxHitTestContext, parent_data::BoxParentData, traits::RenderBox};

/// A render object that, when `ignoring` is true, returns `false` from
/// hit testing — making the subtree (and itself) invisible to pointer
/// events, so the gesture system sees through to siblings below.
///
/// Layout and paint pass through transparently in all cases.
#[derive(Debug, Clone)]
pub struct RenderIgnorePointer {
    ignoring: bool,
    has_child: bool,
}

impl RenderIgnorePointer {
    /// Creates an ignore-pointer render object with the given flag.
    pub const fn new(ignoring: bool) -> Self {
        Self {
            ignoring,
            has_child: false,
        }
    }

    /// Returns whether pointer events are currently ignored.
    #[inline]
    pub fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Updates the ignoring flag and reports semantics when changed. The input regions say
    /// whether presses reach the subtree and are collected on composited frames, so a change
    /// marks paint too.
    pub fn set_ignoring(&mut self, ignoring: bool) -> flui_rendering::RenderUpdateImpact {
        if self.ignoring == ignoring {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.ignoring = ignoring;
        flui_rendering::RenderUpdateImpact::SEMANTICS
            .union(flui_rendering::RenderUpdateImpact::PAINT)
    }
}

impl Default for RenderIgnorePointer {
    /// Defaults to `ignoring = true`.
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::inspect::Inspect for RenderIgnorePointer {
    fn kind(&self) -> &'static str {
        "RenderIgnorePointer"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("ignoring", &self.ignoring).default(&false);
    }
}

impl RenderBox for RenderIgnorePointer {
    type Arity = Single;
    type ParentData = BoxParentData;

    flui_rendering::forward_single_child_box_layout!();

    flui_rendering::forward_single_child_box_queries!();

    // paint: default pass-through (splices the child in order).

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if self.ignoring {
            // Pointer events pass straight through to siblings below.
            return false;
        }
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    /// Presses here reach nothing of the subtree, so under an application's title bar a
    /// press-taking descendant keeps moving the window instead of leaving a dead cut-out.
    fn describe_input_regions(
        &self,
        _size: flui_foundation::geometry::Size,
        regions: &mut flui_rendering::input_regions::InputRegionSink,
    ) {
        if self.ignoring {
            regions.pass_presses_through();
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================
