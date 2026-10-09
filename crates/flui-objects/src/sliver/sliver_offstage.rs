//! `RenderSliverOffstage` — single-child sliver that can hide its
//! subtree entirely (zero geometry, skipped paint, no hit-testing).
//! The child is still laid out, but when offstage its
//! geometry — including any `scroll_offset_correction` it produced — is
//! discarded: `SliverGeometry::ZERO` is reported to the viewport
//! unconditionally, so an offstage sliver reports zero and never forwards a
//! child scroll correction to the viewport.
//!
//! # Design notes
//!
//! * The `offstage` flag is a typed `bool` boundary; no `Visibility`
//!   enum overload. The setter returns the exact pipeline impact.
//!   `mark_needs_layout` short-circuit.
//! * Scroll-offset correction returned by the offstage child is
//!   propagated upward unchanged — the viewport reruns layout next
//!   frame with the corrected offset, identical to the on-stage
//!   passthrough.

use flui_foundation::Single;

use flui_rendering::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

// ============================================================================
// RenderSliverOffstage
// ============================================================================

/// A sliver render object that, when `offstage` is `true`, collapses
/// its reported geometry to [`SliverGeometry::ZERO`], skips painting,
/// and is unreachable by hit-testing.
///
/// When `offstage` is `false`, it behaves as a transparent
/// single-child proxy: child receives the parent's
/// [`flui_rendering::constraints::SliverConstraints`] and its geometry becomes the parent's
/// geometry.
#[derive(Debug, Clone)]
pub struct RenderSliverOffstage {
    /// When `true`, this sliver reports zero geometry and is hidden.
    offstage: bool,
}

impl RenderSliverOffstage {
    /// Creates an offstage sliver render object with the given flag.
    #[must_use]
    pub const fn new(offstage: bool) -> Self {
        Self { offstage }
    }

    /// Creates an offstage sliver render object that is currently hidden.
    #[must_use]
    pub const fn hidden() -> Self {
        Self::new(true)
    }

    /// Creates an offstage sliver render object that is currently visible.
    #[must_use]
    pub const fn visible() -> Self {
        Self::new(false)
    }

    /// Returns whether the subtree is currently offstage (hidden).
    #[inline]
    pub const fn offstage(&self) -> bool {
        self.offstage
    }

    /// Updates the `offstage` flag and reports layout plus semantics when changed.
    pub fn set_offstage(&mut self, offstage: bool) -> flui_rendering::RenderUpdateImpact {
        if self.offstage == offstage {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.offstage = offstage;
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }
}

impl Default for RenderSliverOffstage {
    /// Defaults to hidden (`offstage = true`).
    fn default() -> Self {
        Self::hidden()
    }
}

impl flui_foundation::Diagnosticable for RenderSliverOffstage {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("offstage", self.offstage, "offstage");
    }
}

impl RenderSliver for RenderSliverOffstage {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    ) -> flui_rendering::RenderResult<SliverGeometry> {
        let constraints = *ctx.constraints();

        if self.offstage {
            // The child is still laid out, but when offstage the geometry is
            // zero *unconditionally* — a hidden sliver must not forward the
            // child's scroll_offset_correction to the viewport.
            if ctx.child_count() > 0 {
                ctx.layout_child(0, constraints)?;
            }
            return Ok(SliverGeometry::ZERO);
        }

        // Transparent passthrough.
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
        if self.offstage {
            // Unreachable while hidden.
            return false;
        }

        // Transparent passthrough — forward the current position to
        // the child unchanged. See the note in
        // [`crate::RenderSliverIgnorePointer`] on
        // the sliver hit-test API surface.
        let position = ctx.main_axis_position();
        ctx.hit_test_child(0, position)
    }
}

// ============================================================================
// Tests
// ============================================================================
