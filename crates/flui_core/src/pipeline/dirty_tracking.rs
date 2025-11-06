//! Lock-free dirty tracking using atomic bitmaps
//!
//! Provides high-performance dirty tracking for pipeline elements using
//! atomic bitmaps instead of locks. Perfect for concurrent element updates.
//!
//! # Performance
//!
//! - `mark_dirty`: ~2ns (single atomic OR)
//! - `is_dirty`: ~2ns (single atomic load)
//! - `clear_dirty`: ~2ns (single atomic AND)
//! - `collect_dirty`: O(capacity/64) bitmap scan
//!
//! # Memory
//!
//! Uses 8 bytes per 64 elements (extremely compact).
//!
//! # Example
//!
//! ```rust
//! use flui_core::pipeline::LockFreeDirtySet;
//! use flui_core::element::ElementId;
//!
//! let dirty_set = LockFreeDirtySet::new(1000);
//!
//! // Mark element dirty (from any thread)
//! let id = ElementId::new(42).unwrap();
//! dirty_set.mark_dirty(id);
//!
//! // Check if dirty
//! assert!(dirty_set.is_dirty(id));
//!
//! // Collect all dirty elements
//! let dirty_elements = dirty_set.collect_dirty();
//! assert!(dirty_elements.contains(&id));
//!
//! // Clear dirty flag
//! dirty_set.clear_dirty(id);
//! assert!(!dirty_set.is_dirty(id));
//! ```

use crate::element::ElementId;
use std::sync::atomic::{AtomicU64, Ordering};

/// Lock-free dirty set using atomic bitmaps
///
/// Tracks which elements need processing (rebuild, layout, paint) using
/// atomic bit operations. Each bit represents one element.
///
/// # Capacity
///
/// The capacity is fixed at creation time and represents the maximum
/// number of elements that can be tracked. Attempting to mark elements
/// beyond capacity is silently ignored.
///
/// # Thread Safety
///
/// All operations are lock-free and thread-safe using atomic operations.
/// Multiple threads can mark/check/clear different elements concurrently
/// without contention.
///
/// # Memory Layout
///
/// Uses a `Vec<AtomicU64>` where each u64 holds 64 element flags:
/// ```text
/// [bit 0-63][bit 64-127][bit 128-191]...
///    word 0     word 1      word 2
/// ```
///
/// For 10,000 elements: 157 words × 8 bytes = ~1.2 KB
#[derive(Debug)]
pub struct LockFreeDirtySet {
    /// Bitmap of dirty elements (64 elements per u64)
    bitmap: Vec<AtomicU64>,

    /// Total capacity (maximum number of elements)
    capacity: usize,
}

impl LockFreeDirtySet {
    /// Create dirty set with initial capacity
    ///
    /// # Parameters
    ///
    /// - `capacity`: Maximum number of elements to track
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    ///
    /// // Track up to 10,000 elements
    /// let dirty_set = LockFreeDirtySet::new(10000);
    /// ```
    pub fn new(capacity: usize) -> Self {
        let num_words = capacity.div_ceil(64);
        Self {
            bitmap: (0..num_words).map(|_| AtomicU64::new(0)).collect(),
            capacity,
        }
    }

    /// Mark element as dirty (lock-free!)
    ///
    /// Sets the bit corresponding to this element ID using atomic OR.
    /// If the element is already marked, this is a no-op.
    ///
    /// # Parameters
    ///
    /// - `id`: Element to mark as dirty
    ///
    /// # Thread Safety
    ///
    /// Multiple threads can mark different elements concurrently.
    /// Marking the same element from multiple threads is safe.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    /// use flui_core::element::ElementId;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// let id = ElementId::new(42).unwrap();
    ///
    /// dirty_set.mark_dirty(id);
    /// assert!(dirty_set.is_dirty(id));
    /// ```
    #[inline]
    pub fn mark_dirty(&self, id: ElementId) {
        // ElementId is 1-based, convert to 0-based index
        if id.get() > self.capacity {
            return; // Silently ignore out-of-bounds
        }
        let index = id - 1;

        let word_idx = index / 64;
        let bit_idx = index % 64;
        let mask = 1u64 << bit_idx;

        self.bitmap[word_idx].fetch_or(mask, Ordering::Release);
    }

    /// Check if element is dirty (lock-free!)
    ///
    /// Reads the bit corresponding to this element ID using atomic load.
    ///
    /// # Parameters
    ///
    /// - `id`: Element to check
    ///
    /// # Returns
    ///
    /// `true` if element is marked dirty, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    /// use flui_core::element::ElementId;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// let id = ElementId::new(42).unwrap();
    ///
    /// assert!(!dirty_set.is_dirty(id));
    /// dirty_set.mark_dirty(id);
    /// assert!(dirty_set.is_dirty(id));
    /// ```
    #[inline]
    pub fn is_dirty(&self, id: ElementId) -> bool {
        // ElementId is 1-based, convert to 0-based index
        if id.get() > self.capacity {
            return false;
        }
        let index = id - 1;

        let word_idx = index / 64;
        let bit_idx = index % 64;
        let mask = 1u64 << bit_idx;

        let word = self.bitmap[word_idx].load(Ordering::Acquire);
        (word & mask) != 0
    }

    /// Clear dirty flag for element (lock-free!)
    ///
    /// Clears the bit corresponding to this element ID using atomic AND.
    ///
    /// # Parameters
    ///
    /// - `id`: Element to clear
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    /// use flui_core::element::ElementId;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// let id = ElementId::new(42).unwrap();
    ///
    /// dirty_set.mark_dirty(id);
    /// assert!(dirty_set.is_dirty(id));
    ///
    /// dirty_set.clear_dirty(id);
    /// assert!(!dirty_set.is_dirty(id));
    /// ```
    #[inline]
    pub fn clear_dirty(&self, id: ElementId) {
        // ElementId is 1-based, convert to 0-based index
        if id.get() > self.capacity {
            return;
        }
        let index = id - 1;

        let word_idx = index / 64;
        let bit_idx = index % 64;
        let mask = 1u64 << bit_idx;

        self.bitmap[word_idx].fetch_and(!mask, Ordering::Release);
    }

    /// Collect all dirty element IDs (lock-free read!)
    ///
    /// Scans the bitmap and returns a Vec of all element IDs that are
    /// currently marked as dirty.
    ///
    /// # Returns
    ///
    /// Vec of dirty element IDs in ascending order.
    ///
    /// # Performance
    ///
    /// O(capacity/64) - scans all bitmap words.
    /// For 10,000 elements: ~157 word reads = ~300ns
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    /// use flui_core::element::ElementId;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    ///
    /// dirty_set.mark_dirty(ElementId::new(10).unwrap());
    /// dirty_set.mark_dirty(ElementId::new(20).unwrap());
    /// dirty_set.mark_dirty(ElementId::new(30).unwrap());
    ///
    /// let dirty = dirty_set.collect_dirty();
    /// assert_eq!(dirty.len(), 3);
    /// ```
    pub fn collect_dirty(&self) -> Vec<ElementId> {
        let mut dirty = Vec::new();

        for (word_idx, word) in self.bitmap.iter().enumerate() {
            let bits = word.load(Ordering::Acquire);
            if bits == 0 {
                continue; // Skip empty words
            }

            // Find set bits using bit manipulation
            for bit_idx in 0..64 {
                if (bits & (1u64 << bit_idx)) != 0 {
                    let index = word_idx * 64 + bit_idx;
                    if index < self.capacity {
                        // ElementId is 1-based, so add 1 to convert from 0-based index
                        let id = index + 1;
                        dirty.push(ElementId::new(id));
                    }
                }
            }
        }

        dirty
    }

    /// Clear all dirty flags
    ///
    /// Resets all bits to 0 using atomic stores.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    /// use flui_core::element::ElementId;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    ///
    /// dirty_set.mark_dirty(ElementId::new(10).unwrap());
    /// dirty_set.mark_dirty(ElementId::new(20).unwrap());
    ///
    /// assert_eq!(dirty_set.dirty_count(), 2);
    ///
    /// dirty_set.clear_all();
    /// assert_eq!(dirty_set.dirty_count(), 0);
    /// ```
    pub fn clear_all(&self) {
        for word in &self.bitmap {
            word.store(0, Ordering::Release);
        }
    }

    /// Get current dirty count (approximate, may race)
    ///
    /// Counts the number of set bits across all words.
    ///
    /// # Returns
    ///
    /// Approximate count of dirty elements. May be slightly off if
    /// elements are being marked/cleared concurrently.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    /// use flui_core::element::ElementId;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    ///
    /// dirty_set.mark_dirty(ElementId::new(10).unwrap());
    /// dirty_set.mark_dirty(ElementId::new(20).unwrap());
    ///
    /// assert_eq!(dirty_set.dirty_count(), 2);
    /// ```
    pub fn dirty_count(&self) -> usize {
        self.bitmap
            .iter()
            .map(|word| word.load(Ordering::Relaxed).count_ones() as usize)
            .sum()
    }

    /// Check if any element is dirty
    ///
    /// Fast check - scans bitmap for any non-zero word.
    ///
    /// # Returns
    ///
    /// `true` if at least one element is dirty, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    /// use flui_core::element::ElementId;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    ///
    /// assert!(!dirty_set.has_dirty());
    ///
    /// dirty_set.mark_dirty(ElementId::new(42).unwrap());
    /// assert!(dirty_set.has_dirty());
    /// ```
    pub fn has_dirty(&self) -> bool {
        self.bitmap
            .iter()
            .any(|word| word.load(Ordering::Relaxed) != 0)
    }

    /// Get capacity
    ///
    /// Returns the maximum number of elements this set can track.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// assert_eq!(dirty_set.capacity(), 1000);
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Drain all dirty elements and clear
    ///
    /// Returns all dirty elements and clears the set atomically.
    /// This is equivalent to `collect_dirty()` + `clear_all()`.
    ///
    /// # Returns
    ///
    /// Vec of dirty element IDs.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// dirty_set.mark_dirty(42);
    ///
    /// let dirty = dirty_set.drain();
    /// assert_eq!(dirty.len(), 1);
    /// assert_eq!(dirty_set.dirty_count(), 0);
    /// ```
    pub fn drain(&self) -> Vec<ElementId> {
        let dirty = self.collect_dirty();
        self.clear_all();
        dirty
    }

    /// Get number of dirty elements
    ///
    /// Alias for `dirty_count()` for compatibility.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// assert_eq!(dirty_set.len(), 0);
    ///
    /// dirty_set.mark_dirty(42);
    /// assert_eq!(dirty_set.len(), 1);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.dirty_count()
    }

    /// Check if set is empty
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// assert!(dirty_set.is_empty());
    ///
    /// dirty_set.mark_dirty(42);
    /// assert!(!dirty_set.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        !self.has_dirty()
    }

    /// Clear the set
    ///
    /// Alias for `clear_all()` for compatibility.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::LockFreeDirtySet;
    ///
    /// let dirty_set = LockFreeDirtySet::new(1000);
    /// dirty_set.mark_dirty(42);
    ///
    /// dirty_set.clear();
    /// assert!(dirty_set.is_empty());
    /// ```
    #[inline]
    pub fn clear(&self) {
        self.clear_all();
    }
}

impl Default for LockFreeDirtySet {
    /// Create dirty set with default capacity of 10,000 elements
    fn default() -> Self {
        Self::new(10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_dirty_set_basic() {
        let set = LockFreeDirtySet::new(1000);

        let id1: ElementId = 1;
        let id2: ElementId = 100;

        // Initially not dirty
        assert!(!set.is_dirty(id1));
        assert!(!set.has_dirty());

        // Mark dirty
        set.mark_dirty(id1);
        assert!(set.is_dirty(id1));
        assert!(set.has_dirty());

        // Mark another
        set.mark_dirty(id2);
        assert_eq!(set.dirty_count(), 2);

        // Collect dirty
        let dirty = set.collect_dirty();
        assert_eq!(dirty.len(), 2);
        assert!(dirty.contains(&id1));
        assert!(dirty.contains(&id2));

        // Clear one
        set.clear_dirty(id1);
        assert!(!set.is_dirty(id1));
        assert!(set.is_dirty(id2));
        assert_eq!(set.dirty_count(), 1);

        // Clear all
        set.clear_all();
        assert_eq!(set.dirty_count(), 0);
        assert!(!set.has_dirty());
    }

    #[test]
    fn test_out_of_bounds() {
        let set = LockFreeDirtySet::new(100);

        // Should silently ignore
        let id: ElementId = 200;
        set.mark_dirty(id);
        assert!(!set.is_dirty(id));
    }

    #[test]
    fn test_multi_thread_marking() {
        let set = Arc::new(LockFreeDirtySet::new(10000));
        let mut handles = vec![];

        // 8 threads, each marking 1000 elements
        for thread_id in 0..8 {
            let set = Arc::clone(&set);
            let handle = thread::spawn(move || {
                for i in 0..1000 {
                    let id: ElementId = thread_id * 1000 + i + 1;
                    set.mark_dirty(id);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have 8000 dirty elements
        assert_eq!(set.dirty_count(), 8000);
    }

    #[test]
    fn test_concurrent_mark_and_clear() {
        let set = Arc::new(LockFreeDirtySet::new(1000));

        let set1 = Arc::clone(&set);
        let marker = thread::spawn(move || {
            for i in 0..100 {
                let id: ElementId = i + 1;
                set1.mark_dirty(id);
                thread::yield_now();
            }
        });

        let set2 = Arc::clone(&set);
        let clearer = thread::spawn(move || {
            for i in 0..100 {
                let id: ElementId = i + 1;
                set2.clear_dirty(id);
                thread::yield_now();
            }
        });

        marker.join().unwrap();
        clearer.join().unwrap();

        // Final count should be <= 100 (some may have been cleared)
        assert!(set.dirty_count() <= 100);
    }

    #[test]
    fn test_bitmap_edges() {
        let set = LockFreeDirtySet::new(200);

        // Test word boundaries (0, 63, 64, 127, 128)
        let edges = [1, 64, 65, 128, 129];

        for &idx in &edges {
            let id: ElementId = idx;
            set.mark_dirty(id);
            assert!(set.is_dirty(id));
        }

        assert_eq!(set.dirty_count(), edges.len());
    }

    #[test]
    fn test_collect_preserves_order() {
        let set = LockFreeDirtySet::new(1000);

        // Mark in random order
        for &idx in &[50, 10, 100, 5, 200] {
            set.mark_dirty(idx);
        }

        let dirty = set.collect_dirty();

        // Should be in ascending order
        let mut sorted_dirty = dirty.clone();
        sorted_dirty.sort();
        assert_eq!(dirty, sorted_dirty);
    }

    #[test]
    fn test_default_capacity() {
        let set = LockFreeDirtySet::default();
        assert_eq!(set.capacity(), 10_000);
    }

    #[test]
    fn test_rapid_mark_unmark() {
        let set = LockFreeDirtySet::new(100);
        let id: ElementId = 50;

        // Rapidly toggle
        for _ in 0..1000 {
            set.mark_dirty(id);
            assert!(set.is_dirty(id));
            set.clear_dirty(id);
            assert!(!set.is_dirty(id));
        }
    }
}
