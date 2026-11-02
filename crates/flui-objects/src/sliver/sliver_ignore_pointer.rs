//! `RenderSliverIgnorePointer` — single-child sliver that, when active,
//! makes its entire subtree invisible to pointer events. Pointer
//! events flow past it to whatever sliver is painted beneath in the
//! viewport.
//!
//! Layout and paint are pure passthroughs; only `hit_test` differs from a
//! transparent proxy.
//!
//! # Design notes
//!
//! * `ignoring` is a typed `bool` boundary; its setter returns the exact
//!   pipeline impact.
//!   change-flag for pipeline `mark_needs_paint` /
//!   `mark_needs_layout` short-circuit.
//! * No `ignoring_semantics` field for now — semantics-tree
//!   coordination lands with the semantics-pipeline workstream; the
//!   pointer-side toggle is a self-contained boolean here.

use flui_foundation::Single;

use flui_rendering::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

// ============================================================================
// RenderSliverIgnorePointer
// ============================================================================

/// A sliver render object that, when `ignoring` is `true`, returns
/// `false` from hit-testing so pointers pass through to siblings
/// painted beneath this sliver in the viewport.
///
/// Layout and paint are unconditional passthroughs.
#[derive(Debug, Clone)]
pub struct RenderSliverIgnorePointer {
    /// When `true`, hit-testing returns `false` unconditionally.
    ignoring: bool,
}

impl RenderSliverIgnorePointer {
    /// Creates an ignore-pointer sliver render object with the given flag.
    #[must_use]
    pub const fn new(ignoring: bool) -> Self {
        Self { ignoring }
    }

    /// Returns whether pointer events are currently ignored.
    #[inline]
    pub const fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Updates the `ignoring` flag and reports semantics when changed. The input regions say
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

impl Default for RenderSliverIgnorePointer {
    /// Defaults to `ignoring = true`.
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::inspect::Inspect for RenderSliverIgnorePointer {
    fn kind(&self) -> &'static str {
        "RenderSliverIgnorePointer"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("ignoring", &self.ignoring).default(&false);
    }
}

impl RenderSliver for RenderSliverIgnorePointer {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    ) -> flui_rendering::RenderResult<SliverGeometry> {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            Ok(SliverGeometry::ZERO)
        }
    }

    fn hit_test(
        &self,
        ctx: &mut SliverHitTestContext<'_, Single, SliverPhysicalParentData>,
    ) -> bool {
        if self.ignoring {
            // Pointer events pass straight through.
            return false;
        }

        // Defer to the child at its committed sliver paint offset.
        ctx.hit_test_child_at_layout_offset(0)
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

// ============================================================================
// Tests
// ============================================================================
