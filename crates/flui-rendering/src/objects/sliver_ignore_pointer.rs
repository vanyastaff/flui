//! `RenderSliverIgnorePointer` — single-child sliver that, when active,
//! makes its entire subtree invisible to pointer events. Pointer
//! events flow past it to whatever sliver is painted beneath in the
//! viewport.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderSliverIgnorePointer`](https://api.flutter.dev/flutter/rendering/RenderSliverIgnorePointer-class.html)
//! (`packages/flutter/lib/src/rendering/sliver.dart`). Layout and
//! paint are pure passthroughs; only `hit_test` differs from a
//! transparent proxy.
//!
//! # Rust-native improvements
//!
//! * `ignoring` is a typed `bool` boundary; setter returns a `bool`
//!   change-flag for pipeline `mark_needs_paint` /
//!   `mark_needs_layout` short-circuit.
//! * No `ignoring_semantics` field for now — semantics-tree
//!   coordination lands with the semantics-pipeline workstream; the
//!   pointer-side toggle is a self-contained boolean here.

use flui_tree::Single;
use flui_types::Rect;

use crate::{
    constraints::{SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
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
    /// Last-applied constraints (required by [`RenderSliver`]).
    constraints: SliverConstraints,
    /// Computed geometry from the most recent [`Self::perform_layout`].
    geometry: SliverGeometry,
}

impl RenderSliverIgnorePointer {
    /// Creates an ignore-pointer sliver render object with the given flag.
    #[must_use]
    pub const fn new(ignoring: bool) -> Self {
        Self {
            ignoring,
            constraints: empty_sliver_constraints(),
            geometry: SliverGeometry::ZERO,
        }
    }

    /// Returns whether pointer events are currently ignored.
    #[inline]
    pub const fn ignoring(&self) -> bool {
        self.ignoring
    }

    /// Updates the `ignoring` flag; returns `true` iff the value changed.
    pub fn set_ignoring(&mut self, ignoring: bool) -> bool {
        if self.ignoring == ignoring {
            return false;
        }
        self.ignoring = ignoring;
        true
    }
}

impl Default for RenderSliverIgnorePointer {
    /// Defaults to `ignoring = true` (Flutter parity).
    fn default() -> Self {
        Self::new(true)
    }
}

impl flui_foundation::Diagnosticable for RenderSliverIgnorePointer {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("ignoring", self.ignoring, "ignoring");
    }
}

impl RenderSliver for RenderSliverIgnorePointer {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        self.constraints = constraints;

        let geometry = if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            SliverGeometry::ZERO
        };

        self.geometry = geometry;
        geometry
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
        if self.ignoring {
            // Pointer events pass straight through.
            return false;
        }

        // Defer to the child at its committed sliver paint offset.
        ctx.hit_test_child_at_layout_offset(0)
    }

    fn sliver_paint_bounds(&self) -> Rect {
        let size = self.get_absolute_size(self.geometry.paint_extent);
        Rect::from_origin_size(flui_types::Point::ZERO, size)
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderSliverIgnorePointer {}
impl SemanticsCapability for RenderSliverIgnorePointer {}
impl HotReloadCapability for RenderSliverIgnorePointer {}

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
    fn defaults_to_ignoring() {
        let node = RenderSliverIgnorePointer::default();
        assert!(node.ignoring());
    }

    #[test]
    fn new_round_trips_flag() {
        assert!(RenderSliverIgnorePointer::new(true).ignoring());
        assert!(!RenderSliverIgnorePointer::new(false).ignoring());
    }

    #[test]
    fn set_ignoring_returns_change_flag() {
        let mut node = RenderSliverIgnorePointer::new(false);
        assert!(node.set_ignoring(true));
        assert!(!node.set_ignoring(true)); // no-op
        assert!(node.set_ignoring(false));
    }

    #[test]
    fn initial_geometry_is_zero() {
        let node = RenderSliverIgnorePointer::default();
        assert_eq!(node.geometry().scroll_extent, 0.0);
        assert_eq!(node.geometry().paint_extent, 0.0);
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderSliverIgnorePointer::default();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "ignoring"),
            "missing diagnostic field: ignoring"
        );
    }
}
