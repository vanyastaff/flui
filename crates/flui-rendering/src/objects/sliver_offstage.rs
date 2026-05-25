//! `RenderSliverOffstage` — single-child sliver that can hide its
//! subtree entirely (zero geometry, skipped paint, no hit-testing).
//! The child is still laid out (Flutter parity — so that any
//! `scroll_offset_correction` surfaces), but its geometry is discarded
//! and `SliverGeometry::ZERO` is reported to the viewport instead.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderSliverOffstage`](https://api.flutter.dev/flutter/rendering/RenderSliverOffstage-class.html)
//! (`packages/flutter/lib/src/rendering/sliver.dart`). The
//! always-lay-out-the-child rule matches Flutter — important because
//! offscreen slivers may need to request scroll corrections (e.g. a
//! pinned header that just unpinned and needs to re-anchor).
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
use flui_types::Rect;

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
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
/// [`SliverConstraints`] and its geometry becomes the parent's
/// geometry.
#[derive(Debug, Clone)]
pub struct RenderSliverOffstage {
    /// When `true`, this sliver reports zero geometry and is hidden.
    offstage: bool,
    /// Last-applied constraints (required by [`RenderSliver`]).
    constraints: SliverConstraints,
    /// Computed geometry from the most recent [`Self::perform_layout`].
    geometry: SliverGeometry,
}

impl RenderSliverOffstage {
    /// Creates an offstage sliver render object. Default flag matches
    /// Flutter: `offstage = true`.
    #[must_use]
    pub const fn new(offstage: bool) -> Self {
        Self {
            offstage,
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
        }
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
        builder.add("offstage", self.offstage);
        builder.add("geometry", format!("{:?}", self.geometry));
    }
}

impl RenderSliver for RenderSliverOffstage {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    ) {
        let constraints = *ctx.constraints();
        self.constraints = constraints;

        if self.offstage {
            // Flutter parity: the child must still be laid out so any
            // scroll_offset_correction surfaces, but the geometry we
            // *report* upward is zero.
            if ctx.child_count() > 0 {
                let child_geometry = ctx.layout_child(0, constraints);
                if let Some(correction) = child_geometry.scroll_offset_correction {
                    let geometry = SliverGeometry::scroll_offset_correction(correction);
                    self.geometry = geometry;
                    ctx.complete(geometry);
                    return;
                }
            }
            self.geometry = SliverGeometry::ZERO;
            ctx.complete(SliverGeometry::ZERO);
            return;
        }

        // Transparent passthrough.
        let geometry = if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            SliverGeometry::ZERO
        };

        self.geometry = geometry;
        ctx.complete(geometry);
    }

    fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.geometry = geometry;
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
        // [`crate::objects::RenderSliverIgnorePointer::hit_test`] on
        // the sliver hit-test API surface.
        let position = ctx.main_axis_position();
        ctx.hit_test_child(0, position)
    }

    fn sliver_paint_bounds(&self) -> Rect {
        let size = self.get_absolute_size(self.geometry.paint_extent);
        Rect::from_origin_size(flui_types::Point::ZERO, size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderSliverOffstage {}
impl SemanticsCapability for RenderSliverOffstage {}
impl HotReloadCapability for RenderSliverOffstage {}

// ============================================================================
// Helpers
// ============================================================================

/// `SliverConstraints` constant used to initialise the cached
/// constraints field; `SliverConstraints::default()` is not `const`.
const fn empty_sliver_constraints() -> SliverConstraints {
    use flui_types::layout::AxisDirection;

    use crate::{constraints::GrowthDirection, view::ScrollDirection};

    SliverConstraints::new(
        AxisDirection::TopToBottom,
        GrowthDirection::Forward,
        ScrollDirection::Idle,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
        AxisDirection::LeftToRight,
        0.0,
        0.0,
        0.0,
    )
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
    fn initial_geometry_is_zero() {
        let node = RenderSliverOffstage::visible();
        assert_eq!(node.geometry().scroll_extent, 0.0);
        assert_eq!(node.geometry().paint_extent, 0.0);
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
        for required in ["offstage", "geometry"] {
            assert!(
                names.iter().any(|n| n == required),
                "missing diagnostic field: {required}"
            );
        }
    }
}
