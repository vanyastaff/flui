//! `RenderSliverOffstage` — single-child sliver that can hide its
//! subtree entirely (zero geometry, skipped paint, no hit-testing).
//! The child is still laid out (Flutter parity), but when offstage its
//! geometry — including any `scroll_offset_correction` it produced — is
//! discarded: `SliverGeometry::ZERO` is reported to the viewport
//! unconditionally.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderSliverOffstage`](https://api.flutter.dev/flutter/rendering/RenderSliverOffstage-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_sliver.dart`). The child is
//! always laid out, then — when offstage — `geometry` is set to
//! `SliverGeometry.zero` unconditionally, so an offstage sliver reports zero
//! and never forwards a child scroll correction to the viewport.
//!
//! # Rust-native improvements
//!
//! * The `offstage` flag is a typed `bool` boundary; no `Visibility`
//!   enum overload. Setter returns a `bool` change-flag for pipeline
//!   `mark_needs_layout` short-circuit.
//! * Scroll-offset correction returned by the offstage child is
//!   propagated upward unchanged — the viewport reruns layout next
//!   frame with the corrected offset, identical to the on-stage
//!   passthrough.

use flui_tree::Single;

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
    /// Creates an offstage sliver render object. Default flag matches
    /// Flutter: `offstage = true`.
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

    /// Updates the `offstage` flag; returns `true` iff the value changed.
    pub fn set_offstage(&mut self, offstage: bool) -> bool {
        if self.offstage == offstage {
            return false;
        }
        self.offstage = offstage;
        true
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
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();

        if self.offstage {
            // Flutter parity (proxy_sliver.dart RenderSliverOffstage.performLayout):
            // the child is still laid out, but when offstage `geometry` is set to
            // zero *unconditionally* — a hidden sliver must not forward the
            // child's scroll_offset_correction to the viewport.
            if ctx.child_count() > 0 {
                let _ = ctx.layout_child(0, constraints);
            }
            return SliverGeometry::ZERO;
        }

        // Transparent passthrough.
        if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            SliverGeometry::ZERO
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_hidden() {
        let node = RenderSliverOffstage::default();
        assert!(node.offstage());
    }

    #[test]
    fn constructors_round_trip_flag() {
        assert!(RenderSliverOffstage::hidden().offstage());
        assert!(!RenderSliverOffstage::visible().offstage());
        assert!(RenderSliverOffstage::new(true).offstage());
        assert!(!RenderSliverOffstage::new(false).offstage());
    }

    #[test]
    fn set_offstage_returns_change_flag() {
        let mut node = RenderSliverOffstage::visible();
        assert!(node.set_offstage(true));
        assert!(!node.set_offstage(true)); // no-op
        assert!(node.set_offstage(false));
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderSliverOffstage::hidden();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "offstage"),
            "missing diagnostic field: offstage"
        );
    }
}
