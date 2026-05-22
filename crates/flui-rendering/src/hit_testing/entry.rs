//! Hit test entry types.

use std::sync::{Arc, Weak};

use flui_types::Offset;

use super::{HitTestTarget, MatrixTransformPart};

// Dummy target for entries without a real target
struct DummyTarget;

impl HitTestTarget for DummyTarget {
    fn handle_event(&self, _event: &super::target::PointerEvent, _entry: &HitTestEntry) {}
}

/// An entry in the hit test path.
///
/// Each entry represents a target that was hit during hit testing,
/// along with the transform needed to convert positions to local coordinates.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `HitTestEntry` class.
#[derive(Clone)]
pub struct HitTestEntry {
    /// The target that was hit.
    target: Weak<dyn HitTestTarget>,

    /// Transform from global coordinates to the target's local coordinates.
    transform: MatrixTransformPart,

    /// The local position of the hit in the target's coordinate system.
    local_position: Offset,
}

impl HitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(
        target: Arc<dyn HitTestTarget>,
        transform: MatrixTransformPart,
        local_position: Offset,
    ) -> Self {
        Self {
            target: Arc::downgrade(&target),
            transform,
            local_position,
        }
    }

    /// Creates a hit test entry with just a local position (no target yet).
    pub fn with_position(local_position: Offset) -> Self {
        Self {
            target: Weak::<DummyTarget>::new(),
            transform: MatrixTransformPart::default(),
            local_position,
        }
    }

    /// Creates a hit test entry for the render view (root entry).
    ///
    /// The render view is always added to hit test results as the root
    /// target, ensuring there's always at least one entry in the result.
    pub fn new_render_view() -> Self {
        Self {
            target: Weak::<DummyTarget>::new(),
            transform: MatrixTransformPart::default(),
            local_position: Offset::ZERO,
        }
    }

    /// Returns a strong reference to the target, if it still exists.
    pub fn target(&self) -> Option<Arc<dyn HitTestTarget>> {
        self.target.upgrade()
    }

    /// Returns the transform for this entry.
    pub fn transform(&self) -> &MatrixTransformPart {
        &self.transform
    }

    /// Returns the local position of the hit.
    pub fn local_position(&self) -> Offset {
        self.local_position
    }

    /// Transforms a global position to local coordinates for this entry.
    pub fn global_to_local(&self, global: Offset) -> Option<Offset> {
        self.transform.global_to_local(global)
    }

    /// Transforms a local position to global coordinates.
    pub fn local_to_global(&self, local: Offset) -> Offset {
        self.transform.local_to_global(local)
    }
}

impl std::fmt::Debug for HitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestEntry")
            .field("local_position", &self.local_position)
            .field("has_target", &self.target.upgrade().is_some())
            .finish()
    }
}

// ============================================================================
// CYCLE 4 U-3: dead parallel BoxHitTestEntry / SliverHitTestEntry deletion
// ============================================================================
//
// Pre-cycle this file carried duplicate `BoxHitTestEntry` and
// `SliverHitTestEntry` structs that competed with the live versions in
// `crates/flui-rendering/src/protocol/box_protocol.rs` and
// `crates/flui-rendering/src/protocol/sliver_protocol.rs`:
//
//   - This file's `BoxHitTestEntry::new(local_position: Offset)` took
//     ONE arg; the protocol version `BoxHitTestEntry::new(target_id:
//     u64, transform: Matrix4)` takes TWO. Production hit-test ctx
//     code uses the protocol version exclusively.
//
//   - This file's `SliverHitTestEntry::new(main, cross)` took TWO
//     `f32` args without a target id; the protocol version takes
//     `(target_id: u64, main_axis: f32)`. Production sliver ctx code
//     uses the protocol version exclusively.
//
// The two pairs were a `parallel-type` smell (cycle 2 PR #100 / cycle
// 4 R-7 family). Workspace grep confirmed zero external consumers of
// the `hit_testing` variants outside this module's own tests. Deletion
// is therefore strictly subtractive; the protocol-side versions remain
// canonical, and the `hit_testing` module continues to own the
// trait-dispatch entry type (`HitTestEntry`) and the `RenderId`-shaped
// types coming in U-4.
//
// See cycle 4 Wave 2 design doc (`docs/research/2026-05-22-cycle4-wave2-design.md`)
// for the trio's full migration order.

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_hit_test_entry_debug() {
        let entry = HitTestEntry::with_position(Offset::new(px(10.0), px(20.0)));
        let debug_str = format!("{:?}", entry);
        assert!(debug_str.contains("HitTestEntry"));
        assert!(debug_str.contains("local_position"));
    }
}
