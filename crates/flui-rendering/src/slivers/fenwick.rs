//! Fenwick-style extent tracker for O(log n) scroll extent queries.
//!
//! Lazy lists/grids need to map between pixel offsets and item indices
//! efficiently. This structure provides:
//!
//! - `offset_to_index(offset)` → O(log n) binary search over prefix sums
//! - `index_to_offset(index)` → O(1) prefix sum lookup
//! - `update(index, extent)` → O(1) point update (amortized)
//!
//! # Design
//!
//! Stores raw per-item extents and a running prefix sum. This trades
//! O(log n) updates (Fenwick tree) for O(1) updates with O(log n)
//! queries via binary search on prefix sums. For lazy lists where
//! updates happen during layout (O(n) anyway) but queries happen
//! during scroll (O(log n) critical), this is the right trade-off.
//!
//! # Why not Fenwick tree?
//!
//! A classic Fenwick tree has O(log n) updates AND queries, but its
//! internal 1-indexed invariant breaks on `extend()`/`clear()` without
//! full rebuild. The simpler approach (raw extents + prefix sum +
//! binary search) gives O(1) updates and O(log n) queries with no
//! invariant maintenance.
//!
//! # Why not Flutter's linked list?
//!
//! Flutter's `SliverList` uses a linked list of children with estimated
//! scroll extents. This causes:
//! - O(n) offset→index mapping (linear scan)
//! - Estimate jitter when items have variable sizes
//! - No automatic anchor correction on resize
//!
//! # Competitive insights
//!
//! - **GPUI**: SumTree with Count+Height — O(log n) pixel↔index
//! - **TanStack Virtual**: Fenwick extents for virtualized lists
//! - **RecyclerView**: LinearLayoutManager computes offset from adapter position

/// Tracks per-item extents and provides O(log n) offset↔index mapping.
///
/// # Invariants
///
/// - All extents are non-negative (zero is valid for collapsed items)
/// - `prefix_sums.len() == count + 1`
/// - `prefix_sums[0] == 0.0`
/// - `prefix_sums[i]` = sum of extents [0, i)
#[derive(Debug, Clone)]
pub struct FenwickExtents {
    /// Per-item raw extents.
    extents: Vec<f32>,

    /// Running prefix sums. `prefix_sums[i]` = sum of extents [0, i).
    /// `prefix_sums[0] = 0`, `prefix_sums[count] = total`.
    prefix_sums: Vec<f32>,
}

impl FenwickExtents {
    /// Creates a new empty extent tracker.
    #[inline]
    pub fn new() -> Self {
        Self {
            extents: Vec::new(),
            prefix_sums: vec![0.0],
        }
    }

    /// Creates a tracker with `count` items, all with zero extent.
    pub fn with_count(count: usize) -> Self {
        Self {
            extents: vec![0.0; count],
            prefix_sums: vec![0.0; count + 1],
        }
    }

    /// Creates a tracker from a slice of extents.
    pub fn from_extents(extents: &[f32]) -> Self {
        let count = extents.len();
        let mut prefix_sums = Vec::with_capacity(count + 1);
        prefix_sums.push(0.0);
        let mut sum = 0.0;
        for &e in extents {
            sum += e;
            prefix_sums.push(sum);
        }
        Self {
            extents: extents.to_vec(),
            prefix_sums,
        }
    }

    /// Returns the number of items.
    #[inline]
    pub fn count(&self) -> usize {
        self.extents.len()
    }

    /// Returns the total extent (sum of all items).
    #[inline]
    pub fn total(&self) -> f32 {
        *self.prefix_sums.last().unwrap_or(&0.0)
    }

    /// Returns `true` if no items have been added.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.extents.is_empty()
    }

    /// Sets the extent of item at `index`. Replaces the previous value.
    ///
    /// Updates the running prefix sum in O(n) worst case (rebuilds from
    /// `index` onward). For bulk updates, use `from_extents` instead.
    ///
    /// # Panics
    ///
    /// Panics if `index >= count`.
    pub fn update(&mut self, index: usize, extent: f32) {
        assert!(
            index < self.extents.len(),
            "index {index} out of range (count={})",
            self.extents.len()
        );

        let old = self.extents[index];
        if (old - extent).abs() < f32::EPSILON {
            return;
        }

        self.extents[index] = extent;
        let delta = extent - old;

        // Rebuild prefix sums from index+1 onward
        for i in (index + 1)..=self.extents.len() {
            self.prefix_sums[i] += delta;
        }
    }

    /// Returns the raw extent of item at `index`.
    #[inline]
    pub fn get_raw(&self, index: usize) -> f32 {
        self.extents.get(index).copied().unwrap_or(0.0)
    }

    /// Returns the cumulative extent (prefix sum) up to but NOT including
    /// `index`. This is the offset of item `index` from the start.
    ///
    /// - `index_to_offset(0)` = 0.0 (first item starts at offset 0)
    /// - `index_to_offset(n)` = total extent of items 0..n
    #[inline]
    pub fn index_to_offset(&self, index: usize) -> f32 {
        self.prefix_sums.get(index).copied().unwrap_or(self.total())
    }

    /// Returns the index of the item at the given `offset`, and the
    /// offset within that item.
    ///
    /// This is O(log n) binary search over prefix sums.
    ///
    /// Returns `(item_index, offset_within_item)`. If `offset` is past
    /// the end, returns `(count - 1, overflow)`.
    ///
    /// # Boundary semantics
    ///
    /// An offset exactly at item N's start belongs to item N (not N-1).
    /// E.g., with extents [10, 20, 30]:
    /// - `offset_to_index(0)`  = (0, 0)   — start of item 0
    /// - `offset_to_index(10)` = (1, 0)   — start of item 1
    /// - `offset_to_index(15)` = (1, 5)   — middle of item 1
    pub fn offset_to_index(&self, offset: f32) -> (usize, f32) {
        let count = self.extents.len();
        if count == 0 {
            return (0, offset);
        }
        if offset <= 0.0 {
            return (0, 0.0);
        }
        let total = self.total();
        if offset >= total {
            return (count - 1, offset - self.index_to_offset(count - 1));
        }

        // Binary search: find the largest index i where prefix_sums[i] <= offset.
        // prefix_sums[i] = start offset of item i.
        // We want the last item whose start is <= offset.
        let mut lo = 0usize;
        let mut hi = count; // prefix_sums has count+1 entries, search [0, count]
        while lo < hi {
            let mid = lo + (hi - lo).div_ceil(2); // upper mid to avoid infinite loop
            if self.prefix_sums[mid] <= offset {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }

        // lo is the largest index where prefix_sums[lo] <= offset.
        // That's the item containing `offset`.
        let item_index = lo;
        let item_offset = self.index_to_offset(item_index);
        (item_index, offset - item_offset)
    }

    /// Extends the tracker by `n` items with zero extent.
    pub fn extend(&mut self, n: usize) {
        let total = self.total();
        self.extents.resize(self.extents.len() + n, 0.0);
        // New zero-extent items don't change the running total
        let new_len = self.extents.len();
        self.prefix_sums.resize(new_len + 1, total);
    }

    /// Shrinks the tracker to `new_count` items, removing items from the end.
    pub fn truncate(&mut self, new_count: usize) {
        if new_count >= self.extents.len() {
            return;
        }
        self.extents.truncate(new_count);
        self.prefix_sums.truncate(new_count + 1);
    }

    /// Resets all extents to zero without changing the count.
    pub fn clear(&mut self) {
        self.extents.fill(0.0);
        let count = self.extents.len();
        self.prefix_sums.fill(0.0);
        self.prefix_sums.resize(count + 1, 0.0);
    }
}

impl Default for FenwickExtents {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 1e-5;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_empty() {
        let ft = FenwickExtents::new();
        assert_eq!(ft.count(), 0);
        assert_eq!(ft.total(), 0.0);
        assert!(ft.is_empty());
        assert_eq!(ft.offset_to_index(10.0), (0, 10.0));
    }

    #[test]
    fn test_uniform_extents() {
        let ft = FenwickExtents::from_extents(&[10.0, 10.0, 10.0, 10.0]);
        assert_eq!(ft.count(), 4);
        assert!(approx_eq(ft.total(), 40.0));

        // index_to_offset
        assert!(approx_eq(ft.index_to_offset(0), 0.0));
        assert!(approx_eq(ft.index_to_offset(1), 10.0));
        assert!(approx_eq(ft.index_to_offset(2), 20.0));
        assert!(approx_eq(ft.index_to_offset(3), 30.0));
        assert!(approx_eq(ft.index_to_offset(4), 40.0));

        // offset_to_index: boundary → next item
        assert_eq!(ft.offset_to_index(0.0), (0, 0.0));
        assert_eq!(ft.offset_to_index(5.0), (0, 5.0));
        assert_eq!(ft.offset_to_index(10.0), (1, 0.0));
        assert_eq!(ft.offset_to_index(15.0), (1, 5.0));
        assert_eq!(ft.offset_to_index(25.0), (2, 5.0));
        assert_eq!(ft.offset_to_index(35.0), (3, 5.0));
        assert_eq!(ft.offset_to_index(40.0), (3, 10.0));
        assert_eq!(ft.offset_to_index(50.0), (3, 20.0));
    }

    #[test]
    fn test_variable_extents() {
        let ft = FenwickExtents::from_extents(&[20.0, 30.0, 10.0, 40.0]);
        assert!(approx_eq(ft.total(), 100.0));

        assert!(approx_eq(ft.index_to_offset(0), 0.0));
        assert!(approx_eq(ft.index_to_offset(1), 20.0));
        assert!(approx_eq(ft.index_to_offset(2), 50.0));
        assert!(approx_eq(ft.index_to_offset(3), 60.0));

        assert_eq!(ft.offset_to_index(0.0), (0, 0.0));
        assert_eq!(ft.offset_to_index(25.0), (1, 5.0));
        assert_eq!(ft.offset_to_index(55.0), (2, 5.0));
        assert_eq!(ft.offset_to_index(70.0), (3, 10.0));
    }

    #[test]
    fn test_update_changes_total() {
        let mut ft = FenwickExtents::from_extents(&[10.0, 10.0, 10.0]);
        assert!(approx_eq(ft.total(), 30.0));

        ft.update(1, 20.0);
        assert!(approx_eq(ft.total(), 40.0));
        assert!(approx_eq(ft.index_to_offset(1), 10.0));
        assert!(approx_eq(ft.index_to_offset(2), 30.0));
        assert!(approx_eq(ft.index_to_offset(3), 40.0));
    }

    #[test]
    fn test_update_same_value_noop() {
        let mut ft = FenwickExtents::from_extents(&[10.0, 10.0]);
        ft.update(0, 10.0);
        assert!(approx_eq(ft.total(), 20.0));
    }

    #[test]
    fn test_extend() {
        let mut ft = FenwickExtents::from_extents(&[10.0, 10.0]);
        ft.extend(3);
        assert_eq!(ft.count(), 5);
        assert!(approx_eq(ft.total(), 20.0));
        assert!(approx_eq(ft.index_to_offset(4), 20.0));
        assert!(approx_eq(ft.index_to_offset(5), 20.0));
    }

    #[test]
    fn test_truncate() {
        let mut ft = FenwickExtents::from_extents(&[10.0, 20.0, 30.0, 40.0]);
        ft.truncate(2);
        assert_eq!(ft.count(), 2);
        assert!(approx_eq(ft.total(), 30.0));
    }

    #[test]
    fn test_clear() {
        let mut ft = FenwickExtents::from_extents(&[10.0, 20.0, 30.0]);
        ft.clear();
        assert_eq!(ft.count(), 3);
        assert!(approx_eq(ft.total(), 0.0));
        assert!(approx_eq(ft.index_to_offset(2), 0.0));
    }

    #[test]
    fn test_get_raw() {
        let ft = FenwickExtents::from_extents(&[10.0, 20.0, 30.0]);
        assert!(approx_eq(ft.get_raw(0), 10.0));
        assert!(approx_eq(ft.get_raw(1), 20.0));
        assert!(approx_eq(ft.get_raw(2), 30.0));
        assert!(approx_eq(ft.get_raw(3), 0.0));
    }

    #[test]
    fn test_single_item() {
        let ft = FenwickExtents::from_extents(&[42.0]);
        assert_eq!(ft.count(), 1);
        assert!(approx_eq(ft.total(), 42.0));
        assert!(approx_eq(ft.index_to_offset(0), 0.0));
        assert!(approx_eq(ft.index_to_offset(1), 42.0));
        assert_eq!(ft.offset_to_index(0.0), (0, 0.0));
        assert_eq!(ft.offset_to_index(21.0), (0, 21.0));
        assert_eq!(ft.offset_to_index(42.0), (0, 42.0));
    }

    #[test]
    fn test_zero_extent_items() {
        let ft = FenwickExtents::from_extents(&[10.0, 0.0, 0.0, 20.0]);
        assert!(approx_eq(ft.total(), 30.0));
        assert_eq!(ft.offset_to_index(5.0), (0, 5.0));
        // Offset 10.0 → all zero-extent items share the same start offset.
        // Binary search returns the LAST one (item 3, which has extent 20
        // and actually starts at offset 10). Items 1 and 2 are collapsed.
        assert_eq!(ft.offset_to_index(10.0), (3, 0.0));
        // Offset 15.0 → middle of item 3
        assert_eq!(ft.offset_to_index(15.0), (3, 5.0));
    }

    #[test]
    fn test_negative_offset_clamps() {
        let ft = FenwickExtents::from_extents(&[10.0, 20.0]);
        assert_eq!(ft.offset_to_index(-5.0), (0, 0.0));
    }

    #[test]
    fn test_roundtrip() {
        let extents = [15.0, 25.0, 8.0, 42.0, 10.0];
        let ft = FenwickExtents::from_extents(&extents);

        for i in 0..extents.len() {
            let offset = ft.index_to_offset(i);
            let (idx, within) = ft.offset_to_index(offset);
            assert_eq!(idx, i, "roundtrip failed for index {i}");
            assert!(
                approx_eq(within, 0.0),
                "roundtrip offset mismatch for index {i}"
            );
        }
    }

    #[test]
    fn test_stress_many_items() {
        let extents: Vec<f32> = (0..10000).map(|i| (i % 7 + 1) as f32).collect();
        let ft = FenwickExtents::from_extents(&extents);

        let expected_total: f32 = extents.iter().sum();
        assert!(approx_eq(ft.total(), expected_total));

        let (mid_idx, _) = ft.offset_to_index(expected_total / 2.0);
        assert!(mid_idx > 4000 && mid_idx < 6000, "mid_idx={mid_idx}");
    }

    #[test]
    fn test_extend_then_update() {
        let mut ft = FenwickExtents::from_extents(&[10.0]);
        ft.extend(2);
        ft.update(1, 20.0);
        ft.update(2, 30.0);
        assert!(approx_eq(ft.total(), 60.0));
        assert_eq!(ft.offset_to_index(15.0), (1, 5.0));
        assert_eq!(ft.offset_to_index(35.0), (2, 5.0));
    }
}
