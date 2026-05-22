//! Hit test result accumulation.
//!
//! Cycle 4 U-3 deleted the parallel `BoxHitTestResult` and
//! `SliverHitTestResult` structs that used to live alongside
//! `HitTestResult` in this file. Both had zero workspace consumers
//! outside their own tests, and they competed with the protocol-
//! canonical versions in `crates/flui-rendering/src/protocol/box_protocol.rs`
//! and `crates/flui-rendering/src/protocol/sliver_protocol.rs`. See
//! cycle 4 audit R-7 and Wave 2 design doc.
//!
//! The remaining `HitTestResult` here (with `Arc<dyn HitTestTarget>`-
//! based entries) is U-4's target — to be replaced by a re-export +
//! protocol-extension adapter over `flui_interaction::routing::HitTestResult`.

use std::sync::Arc;

use flui_types::{Matrix4, Offset};

use super::{HitTestEntry, HitTestTarget, MatrixTransformPart};

// ============================================================================
// HitTestResult
// ============================================================================

/// Accumulates hit test entries during traversal.
///
/// As the hit test traverses the render tree, each hit target adds itself
/// to the result. The path is ordered from the deepest target (front) to
/// the shallowest (back).
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `HitTestResult` class.
#[derive(Default)]
pub struct HitTestResult {
    /// The path of hit test entries, from front (deepest) to back.
    path: Vec<HitTestEntry>,

    /// Stack of local transforms for coordinate conversion.
    transforms: Vec<MatrixTransformPart>,
}

impl HitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a result with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            path: Vec::with_capacity(capacity),
            transforms: Vec::new(),
        }
    }

    /// Returns the path of hit test entries.
    pub fn path(&self) -> &[HitTestEntry] {
        &self.path
    }

    /// Returns whether the result is empty.
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// Returns the number of entries in the path.
    pub fn len(&self) -> usize {
        self.path.len()
    }

    /// Returns the first entry (deepest/front-most target).
    pub fn first(&self) -> Option<&HitTestEntry> {
        self.path.first()
    }

    /// Returns the last entry (shallowest/back-most target).
    pub fn last(&self) -> Option<&HitTestEntry> {
        self.path.last()
    }

    /// Clears all entries and transforms.
    pub fn clear(&mut self) {
        self.path.clear();
        self.transforms.clear();
    }

    /// Adds a hit test entry directly.
    pub fn add(&mut self, entry: HitTestEntry) {
        self.path.push(entry);
    }

    /// Adds a target at the current transform position.
    pub fn add_target(&mut self, target: Arc<dyn HitTestTarget>, local_position: Offset) {
        let transform = self.current_transform();
        self.path
            .push(HitTestEntry::new(target, transform, local_position));
    }

    /// Adds an entry with just a local position.
    pub fn add_with_position(&mut self, local_position: Offset) {
        let transform = self.current_transform();
        // Add entry with a dummy target
        self.path.push(HitTestEntry::new(
            Arc::new(DummyTarget),
            transform,
            local_position,
        ));
    }

    /// Returns the current accumulated transform.
    fn current_transform(&self) -> MatrixTransformPart {
        if self.transforms.is_empty() {
            MatrixTransformPart::default()
        } else {
            // Combine all transforms
            let mut result = Matrix4::IDENTITY;
            for t in &self.transforms {
                result *= t.to_matrix();
            }
            MatrixTransformPart::Matrix(result)
        }
    }

    // ========================================================================
    // Transform Stack Management
    // ========================================================================

    /// Pushes an offset transform onto the stack.
    pub fn push_offset(&mut self, offset: Offset) {
        self.transforms.push(MatrixTransformPart::Offset(offset));
    }

    /// Pushes a matrix transform onto the stack.
    pub fn push_transform(&mut self, transform: Matrix4) {
        self.transforms.push(MatrixTransformPart::Matrix(transform));
    }

    /// Pops a transform from the stack.
    pub fn pop_transform(&mut self) {
        self.transforms.pop();
    }

    /// Executes a closure with an offset transform pushed.
    pub fn add_with_paint_offset<F>(
        &mut self,
        offset: Option<Offset>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut Self, Offset) -> bool,
    {
        let effective_offset = offset.unwrap_or(Offset::ZERO);
        let transformed_position = Offset::new(
            position.dx - effective_offset.dx,
            position.dy - effective_offset.dy,
        );

        if let Some(off) = offset {
            self.push_offset(off);
        }

        let result = hit_test(self, transformed_position);

        if offset.is_some() {
            self.pop_transform();
        }

        result
    }

    /// Executes a closure with a raw transform pushed.
    pub fn add_with_raw_transform<F>(
        &mut self,
        transform: Option<Matrix4>,
        position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut Self, Offset) -> bool,
    {
        if let Some(t) = transform {
            self.push_transform(t);

            // Transform position to local coordinates
            let local_pos = t
                .try_inverse()
                .map(|inverse| {
                    let (x, y) = inverse.transform_point(position.dx, position.dy);
                    Offset::new(x, y)
                })
                .unwrap_or(position);

            let result = hit_test(self, local_pos);
            self.pop_transform();
            result
        } else {
            hit_test(self, position)
        }
    }

    /// Returns the current transform stack depth.
    pub fn transform_depth(&self) -> usize {
        self.transforms.len()
    }
}

impl std::fmt::Debug for HitTestResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestResult")
            .field("path_len", &self.path.len())
            .field("transform_stack_depth", &self.transforms.len())
            .finish()
    }
}

// ============================================================================
// Iterator support for HitTestResult
// ============================================================================

impl<'a> IntoIterator for &'a HitTestResult {
    type Item = &'a HitTestEntry;
    type IntoIter = std::slice::Iter<'a, HitTestEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.path.iter()
    }
}

impl IntoIterator for HitTestResult {
    type Item = HitTestEntry;
    type IntoIter = std::vec::IntoIter<HitTestEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.path.into_iter()
    }
}

// Dummy target for entries that don't have a real target
struct DummyTarget;

impl HitTestTarget for DummyTarget {
    fn handle_event(&self, _event: &super::target::PointerEvent, _entry: &HitTestEntry) {}
    fn debug_label(&self) -> &'static str {
        "DummyTarget"
    }
}

// `BoxHitTestResult` and `SliverHitTestResult` (with their Iterator
// impls + `add_with_paint_offset`/`add_with_paint_transform`/...
// helpers) lived here pre-cycle as parallels to the protocol-side
// versions at `protocol/box_protocol.rs` and
// `protocol/sliver_protocol.rs`. Cycle 4 U-3 deleted both -- zero
// workspace consumers outside this module's own tests, and the
// protocol-side versions are what production hit-test ctx code uses.
// See module-level docstring above.

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    #[test]
    fn test_hit_test_result_empty() {
        let result = HitTestResult::new();
        assert!(result.is_empty());
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_hit_test_result_push_pop() {
        let mut result = HitTestResult::new();
        result.push_offset(Offset::new(px(10.0), px(20.0)));
        assert_eq!(result.transforms.len(), 1);

        result.push_transform(Matrix4::IDENTITY);
        assert_eq!(result.transforms.len(), 2);

        result.pop_transform();
        assert_eq!(result.transforms.len(), 1);

        result.pop_transform();
        assert_eq!(result.transforms.len(), 0);
    }

    // Tests for `BoxHitTestResult` and `SliverHitTestResult` were
    // removed in cycle 4 U-3 alongside the parallel types they
    // exercised. The remaining `HitTestResult` tests exercise the
    // single canonical surface that U-4 migrates to flui-interaction.

    #[test]
    fn test_hit_test_result_first_last() {
        let mut result = HitTestResult::new();
        assert!(result.first().is_none());
        assert!(result.last().is_none());

        result.add_with_position(Offset::new(px(10.0), px(20.0)));
        result.add_with_position(Offset::new(px(30.0), px(40.0)));

        assert!(result.first().is_some());
        assert!(result.last().is_some());
        assert_eq!(result.first().unwrap().local_position().dx, 10.0);
        assert_eq!(result.last().unwrap().local_position().dx, 30.0);
    }

    #[test]
    fn test_hit_test_result_clear() {
        let mut result = HitTestResult::new();
        result.add_with_position(Offset::new(px(10.0), px(20.0)));
        result.push_offset(Offset::new(px(5.0), px(5.0)));

        assert!(!result.is_empty());
        assert_eq!(result.transform_depth(), 1);

        result.clear();
        assert!(result.is_empty());
        assert_eq!(result.transform_depth(), 0);
    }

    #[test]
    fn test_hit_test_result_with_capacity() {
        let result = HitTestResult::with_capacity(10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_hit_test_result_iterator() {
        let mut result = HitTestResult::new();
        result.add_with_position(Offset::new(px(10.0), px(20.0)));
        result.add_with_position(Offset::new(px(30.0), px(40.0)));

        let positions: Vec<_> = result
            .path()
            .iter()
            .map(|e| e.local_position().dx)
            .collect();
        assert_eq!(positions, vec![10.0, 30.0]);
    }

    // Tests for parallel `BoxHitTestResult` and
    // `SliverHitTestResult` (first/last/clear/extend/iterator,
    // add_with_raw_transform, add_with_out_of_band_position) were
    // deleted in cycle 4 U-3 alongside the types they exercised.
}
