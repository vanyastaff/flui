//! Hit test result accumulation.

use std::sync::Arc;

use flui_types::{Matrix4, Offset};

use super::{
    BoxHitTestEntry, HitTestEntry, HitTestTarget, MatrixTransformPart, SliverHitTestEntry,
};

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

// ============================================================================
// BoxHitTestResult
// ============================================================================

/// Result of a box hit test.
///
/// This is a simpler result type for RenderBox hit testing that doesn't
/// need the full target tracking system.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    entries: Vec<BoxHitTestEntry>,
    transforms: Vec<MatrixTransformPart>,
}

impl BoxHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wraps a `HitTestResult` to provide box-specific hit testing.
    ///
    /// This creates a new `BoxHitTestResult` that shares the underlying
    /// storage with the provided `HitTestResult`, allowing seamless
    /// integration between the two hit testing systems.
    ///
    /// Note: In this implementation, we create a new result that can be
    /// merged back if needed. For full Flutter compatibility, this would
    /// share the same storage.
    pub fn wrap(_result: &mut HitTestResult) -> Self {
        // In Flutter, this wraps the same underlying storage.
        // For our implementation, we create a new result.
        // The caller can merge results if needed.
        Self::new()
    }

    /// Adds an entry to the result.
    pub fn add(&mut self, entry: BoxHitTestEntry) {
        self.entries.push(entry);
    }

    /// Adds an entry with just a local position.
    pub fn add_with_position(&mut self, local_position: Offset) {
        self.entries.push(BoxHitTestEntry::new(local_position));
    }

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[BoxHitTestEntry] {
        &self.entries
    }

    /// Returns whether this result has any entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns the first entry, if any.
    pub fn first(&self) -> Option<&BoxHitTestEntry> {
        self.entries.first()
    }

    /// Returns the last entry, if any.
    pub fn last(&self) -> Option<&BoxHitTestEntry> {
        self.entries.last()
    }

    /// Clears all entries from the result.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.transforms.clear();
    }

    /// Extends this result with entries from another.
    pub fn extend(&mut self, other: &BoxHitTestResult) {
        self.entries.extend(other.entries.iter().cloned());
    }

    /// Pushes an offset transform.
    pub fn push_offset(&mut self, offset: Offset) {
        self.transforms.push(MatrixTransformPart::Offset(offset));
    }

    /// Pushes a matrix transform.
    pub fn push_transform(&mut self, transform: Matrix4) {
        self.transforms.push(MatrixTransformPart::Matrix(transform));
    }

    /// Pops a transform.
    pub fn pop_transform(&mut self) {
        self.transforms.pop();
    }

    /// Returns the current transform stack depth.
    pub fn transform_depth(&self) -> usize {
        self.transforms.len()
    }

    /// Executes hit test with a paint offset.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `BoxHitTestResult.addWithPaintOffset`.
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

    /// Executes hit test with a paint transform.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `BoxHitTestResult.addWithPaintTransform`.
    pub fn add_with_paint_transform<F>(
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

    /// Executes hit test with a raw transform matrix.
    ///
    /// Unlike `add_with_paint_transform`, this method does not invert the transform
    /// to compute the local position - the caller provides the transformed position.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `BoxHitTestResult.addWithRawTransform`.
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
            let result = hit_test(self, position);
            self.pop_transform();
            result
        } else {
            hit_test(self, position)
        }
    }

    /// Executes hit test with out-of-band position management.
    ///
    /// This method allows manual management of the position transformation.
    /// The `paint_offset` and `paint_transform` are pushed onto the transform
    /// stack, but the `hit_test_position` is used directly without transformation.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `BoxHitTestResult.addWithOutOfBandPosition`.
    pub fn add_with_out_of_band_position<F>(
        &mut self,
        paint_offset: Option<Offset>,
        paint_transform: Option<Matrix4>,
        hit_test_position: Offset,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut Self, Offset) -> bool,
    {
        let pushed_offset = paint_offset.is_some();
        let pushed_transform = paint_transform.is_some();

        if let Some(off) = paint_offset {
            self.push_offset(off);
        }
        if let Some(t) = paint_transform {
            self.push_transform(t);
        }

        let result = hit_test(self, hit_test_position);

        if pushed_transform {
            self.pop_transform();
        }
        if pushed_offset {
            self.pop_transform();
        }

        result
    }
}

// ============================================================================
// Iterator support for BoxHitTestResult
// ============================================================================

impl<'a> IntoIterator for &'a BoxHitTestResult {
    type Item = &'a BoxHitTestEntry;
    type IntoIter = std::slice::Iter<'a, BoxHitTestEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

impl IntoIterator for BoxHitTestResult {
    type Item = BoxHitTestEntry;
    type IntoIter = std::vec::IntoIter<BoxHitTestEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

// ============================================================================
// SliverHitTestResult
// ============================================================================

/// Result of a sliver hit test.
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    entries: Vec<SliverHitTestEntry>,
}

impl SliverHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry to the result.
    pub fn add(&mut self, entry: SliverHitTestEntry) {
        self.entries.push(entry);
    }

    /// Adds an entry with axis positions.
    pub fn add_with_axis_position(&mut self, main: f32, cross: f32) {
        self.entries.push(SliverHitTestEntry::new(main, cross));
    }

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[SliverHitTestEntry] {
        &self.entries
    }

    /// Returns whether this result has any entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns the first entry, if any.
    pub fn first(&self) -> Option<&SliverHitTestEntry> {
        self.entries.first()
    }

    /// Returns the last entry, if any.
    pub fn last(&self) -> Option<&SliverHitTestEntry> {
        self.entries.last()
    }

    /// Clears all entries from the result.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Executes hit test with an axis offset.
    ///
    /// # Flutter Equivalence
    ///
    /// Corresponds to Flutter's `SliverHitTestResult.addWithAxisOffset`.
    pub fn add_with_axis_offset<F>(
        &mut self,
        main_axis_offset: f32,
        cross_axis_offset: f32,
        main_axis_position: f32,
        cross_axis_position: f32,
        hit_test: F,
    ) -> bool
    where
        F: FnOnce(&mut Self, f32, f32) -> bool,
    {
        let transformed_main = main_axis_position - main_axis_offset;
        let transformed_cross = cross_axis_position - cross_axis_offset;
        hit_test(self, transformed_main, transformed_cross)
    }
}

// ============================================================================
// Iterator support for SliverHitTestResult
// ============================================================================

impl<'a> IntoIterator for &'a SliverHitTestResult {
    type Item = &'a SliverHitTestEntry;
    type IntoIter = std::slice::Iter<'a, SliverHitTestEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

impl IntoIterator for SliverHitTestResult {
    type Item = SliverHitTestEntry;
    type IntoIter = std::vec::IntoIter<SliverHitTestEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[cfg(test)]
mod tests {
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
        result.push_offset(Offset::new(10.0, 20.0));
        assert_eq!(result.transforms.len(), 1);

        result.push_transform(Matrix4::IDENTITY);
        assert_eq!(result.transforms.len(), 2);

        result.pop_transform();
        assert_eq!(result.transforms.len(), 1);

        result.pop_transform();
        assert_eq!(result.transforms.len(), 0);
    }

    #[test]
    fn test_box_hit_test_result() {
        let mut result = BoxHitTestResult::new();
        assert!(result.is_empty());

        result.add_with_position(Offset::new(10.0, 20.0));
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
        assert_eq!(result.entries()[0].local_position.dx, 10.0);
    }

    #[test]
    fn test_box_hit_test_with_paint_offset() {
        let mut result = BoxHitTestResult::new();

        let hit = result.add_with_paint_offset(
            Some(Offset::new(10.0, 10.0)),
            Offset::new(25.0, 35.0),
            |result, position| {
                // Position should be transformed
                assert_eq!(position.dx, 15.0);
                assert_eq!(position.dy, 25.0);
                result.add_with_position(position);
                true
            },
        );

        assert!(hit);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_sliver_hit_test_result() {
        let mut result = SliverHitTestResult::new();
        assert!(result.is_empty());

        result.add_with_axis_position(100.0, 50.0);
        assert!(!result.is_empty());
        assert_eq!(result.len(), 1);
        assert_eq!(result.entries()[0].main_axis_position, 100.0);
        assert_eq!(result.entries()[0].cross_axis_position, 50.0);
    }

    #[test]
    fn test_sliver_hit_test_with_axis_offset() {
        let mut result = SliverHitTestResult::new();

        let hit = result.add_with_axis_offset(
            10.0, // main offset
            5.0,  // cross offset
            50.0, // main position
            25.0, // cross position
            |result, main, cross| {
                // Positions should be transformed
                assert_eq!(main, 40.0);
                assert_eq!(cross, 20.0);
                result.add_with_axis_position(main, cross);
                true
            },
        );

        assert!(hit);
        assert_eq!(result.len(), 1);
    }

    // ===== New enhancement tests =====

    #[test]
    fn test_hit_test_result_first_last() {
        let mut result = HitTestResult::new();
        assert!(result.first().is_none());
        assert!(result.last().is_none());

        result.add_with_position(Offset::new(10.0, 20.0));
        result.add_with_position(Offset::new(30.0, 40.0));

        assert!(result.first().is_some());
        assert!(result.last().is_some());
        assert_eq!(result.first().unwrap().local_position().dx, 10.0);
        assert_eq!(result.last().unwrap().local_position().dx, 30.0);
    }

    #[test]
    fn test_hit_test_result_clear() {
        let mut result = HitTestResult::new();
        result.add_with_position(Offset::new(10.0, 20.0));
        result.push_offset(Offset::new(5.0, 5.0));

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
        result.add_with_position(Offset::new(10.0, 20.0));
        result.add_with_position(Offset::new(30.0, 40.0));

        let positions: Vec<_> = result
            .path()
            .iter()
            .map(|e| e.local_position().dx)
            .collect();
        assert_eq!(positions, vec![10.0, 30.0]);
    }

    #[test]
    fn test_box_hit_test_result_first_last() {
        let mut result = BoxHitTestResult::new();
        assert!(result.first().is_none());

        result.add_with_position(Offset::new(10.0, 20.0));
        result.add_with_position(Offset::new(30.0, 40.0));

        assert_eq!(result.first().unwrap().local_position.dx, 10.0);
        assert_eq!(result.last().unwrap().local_position.dx, 30.0);
    }

    #[test]
    fn test_box_hit_test_result_clear() {
        let mut result = BoxHitTestResult::new();
        result.add_with_position(Offset::new(10.0, 20.0));
        result.push_offset(Offset::new(5.0, 5.0));

        result.clear();
        assert!(result.is_empty());
        assert_eq!(result.transform_depth(), 0);
    }

    #[test]
    fn test_box_hit_test_result_extend() {
        let mut result1 = BoxHitTestResult::new();
        result1.add_with_position(Offset::new(10.0, 20.0));

        let mut result2 = BoxHitTestResult::new();
        result2.add_with_position(Offset::new(30.0, 40.0));

        result1.extend(&result2);
        assert_eq!(result1.len(), 2);
    }

    #[test]
    fn test_box_hit_test_result_iterator() {
        let mut result = BoxHitTestResult::new();
        result.add_with_position(Offset::new(10.0, 20.0));
        result.add_with_position(Offset::new(30.0, 40.0));

        let positions: Vec<_> = (&result).into_iter().map(|e| e.local_position.dx).collect();
        assert_eq!(positions, vec![10.0, 30.0]);
    }

    #[test]
    fn test_box_hit_test_with_raw_transform() {
        let mut result = BoxHitTestResult::new();

        let hit = result.add_with_raw_transform(
            Some(Matrix4::translation(10.0, 20.0, 0.0)),
            Offset::new(25.0, 35.0),
            |result, position| {
                // Position passed as-is (not transformed)
                assert_eq!(position.dx, 25.0);
                assert_eq!(position.dy, 35.0);
                result.add_with_position(position);
                true
            },
        );

        assert!(hit);
        assert_eq!(result.len(), 1);
        assert_eq!(result.transform_depth(), 0); // Transform should be popped
    }

    #[test]
    fn test_box_hit_test_with_out_of_band_position() {
        let mut result = BoxHitTestResult::new();

        let hit = result.add_with_out_of_band_position(
            Some(Offset::new(10.0, 20.0)),
            None,
            Offset::new(100.0, 200.0),
            |result, position| {
                // Position used directly without transformation
                assert_eq!(position.dx, 100.0);
                assert_eq!(position.dy, 200.0);
                result.add_with_position(position);
                true
            },
        );

        assert!(hit);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_sliver_hit_test_result_first_last() {
        let mut result = SliverHitTestResult::new();
        assert!(result.first().is_none());

        result.add_with_axis_position(10.0, 20.0);
        result.add_with_axis_position(30.0, 40.0);

        assert_eq!(result.first().unwrap().main_axis_position, 10.0);
        assert_eq!(result.last().unwrap().main_axis_position, 30.0);
    }

    #[test]
    fn test_sliver_hit_test_result_clear() {
        let mut result = SliverHitTestResult::new();
        result.add_with_axis_position(10.0, 20.0);

        result.clear();
        assert!(result.is_empty());
    }

    #[test]
    fn test_sliver_hit_test_result_iterator() {
        let mut result = SliverHitTestResult::new();
        result.add_with_axis_position(10.0, 20.0);
        result.add_with_axis_position(30.0, 40.0);

        let positions: Vec<_> = (&result)
            .into_iter()
            .map(|e| e.main_axis_position)
            .collect();
        assert_eq!(positions, vec![10.0, 30.0]);
    }
}
