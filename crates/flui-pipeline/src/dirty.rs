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
//! use flui_pipeline::LockFreeDirtySet;
//! use flui_foundation::ElementId;
//!
//! let dirty_set = LockFreeDirtySet::new(1000);
//!
//! // Mark element dirty (from any thread)
//! let id = ElementId::new(42);
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

use flui_foundation::ElementId;
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
    #[inline]
    pub fn mark_dirty(&self, id: ElementId) {
        // ElementId is 1-based, convert to 0-based index
        let id_val = id.get();
        if id_val > self.capacity {
            return; // Silently ignore out-of-bounds
        }
        let index = id_val - 1;

        let word_idx = index / 64;
        let bit_idx = index % 64;
        let mask = 1u64 << bit_idx;

        self.bitmap[word_idx].fetch_or(mask, Ordering::Release);
    }

    /// Check if element is dirty (lock-free!)
    ///
    /// Reads the bit corresponding to this element ID using atomic load.
    #[inline]
    pub fn is_dirty(&self, id: ElementId) -> bool {
        let id_val = id.get();
        if id_val > self.capacity {
            return false;
        }
        let index = id_val - 1;

        let word_idx = index / 64;
        let bit_idx = index % 64;
        let mask = 1u64 << bit_idx;

        let word = self.bitmap[word_idx].load(Ordering::Acquire);
        (word & mask) != 0
    }

    /// Clear dirty flag for element (lock-free!)
    ///
    /// Clears the bit corresponding to this element ID using atomic AND.
    #[inline]
    pub fn clear_dirty(&self, id: ElementId) {
        let id_val = id.get();
        if id_val > self.capacity {
            return;
        }
        let index = id_val - 1;

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
    /// # Performance
    ///
    /// O(capacity/64) - scans all bitmap words.
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
    pub fn clear_all(&self) {
        for word in &self.bitmap {
            word.store(0, Ordering::Release);
        }
    }

    /// Mark all elements as dirty
    ///
    /// Used for global invalidation (resize, theme change).
    pub fn mark_all_dirty(&self) {
        for word in &self.bitmap {
            word.store(u64::MAX, Ordering::Release);
        }
    }

    /// Get current dirty count (approximate, may race)
    pub fn dirty_count(&self) -> usize {
        self.bitmap
            .iter()
            .map(|word| word.load(Ordering::Relaxed).count_ones() as usize)
            .sum()
    }

    /// Check if any element is dirty
    pub fn has_dirty(&self) -> bool {
        self.bitmap
            .iter()
            .any(|word| word.load(Ordering::Relaxed) != 0)
    }

    /// Get capacity
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Drain all dirty elements and clear
    pub fn drain(&self) -> Vec<ElementId> {
        let dirty = self.collect_dirty();
        self.clear_all();
        dirty
    }

    /// Get number of dirty elements (alias for dirty_count)
    #[inline]
    pub fn len(&self) -> usize {
        self.dirty_count()
    }

    /// Check if set is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        !self.has_dirty()
    }

    /// Clear the set (alias for clear_all)
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

// ============================================================================
// Simple DirtySet (HashSet-based, for smaller sets)
// ============================================================================

use parking_lot::RwLock;
use std::collections::HashSet;

/// A simple thread-safe dirty set using HashSet.
///
/// Less memory-efficient than `LockFreeDirtySet` for large sets,
/// but more flexible for dynamic element IDs.
#[derive(Debug, Default)]
pub struct DirtySet {
    elements: RwLock<HashSet<ElementId>>,
}

impl DirtySet {
    /// Creates a new empty dirty set.
    pub fn new() -> Self {
        Self {
            elements: RwLock::new(HashSet::new()),
        }
    }

    /// Marks an element as dirty.
    pub fn mark(&self, id: ElementId) {
        self.elements.write().insert(id);
    }

    /// Marks multiple elements as dirty.
    pub fn mark_many(&self, ids: impl IntoIterator<Item = ElementId>) {
        let mut set = self.elements.write();
        for id in ids {
            set.insert(id);
        }
    }

    /// Clears the dirty flag for an element.
    pub fn clear(&self, id: ElementId) {
        self.elements.write().remove(&id);
    }

    /// Checks if an element is dirty.
    pub fn is_dirty(&self, id: ElementId) -> bool {
        self.elements.read().contains(&id)
    }

    /// Returns true if any elements are dirty.
    pub fn has_dirty(&self) -> bool {
        !self.elements.read().is_empty()
    }

    /// Returns the number of dirty elements.
    pub fn len(&self) -> usize {
        self.elements.read().len()
    }

    /// Returns true if no elements are dirty.
    pub fn is_empty(&self) -> bool {
        self.elements.read().is_empty()
    }

    /// Takes all dirty elements, clearing the set.
    pub fn drain(&self) -> Vec<ElementId> {
        let mut set = self.elements.write();
        set.drain().collect()
    }

    /// Clears all dirty elements.
    pub fn clear_all(&self) {
        self.elements.write().clear();
    }

    /// Returns a copy of all dirty element IDs.
    pub fn iter(&self) -> Vec<ElementId> {
        self.elements.read().iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // ========== LockFreeDirtySet tests ==========

    #[test]
    fn test_lock_free_basic() {
        let set = LockFreeDirtySet::new(1000);

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(100);

        assert!(!set.is_dirty(id1));
        assert!(!set.has_dirty());

        set.mark_dirty(id1);
        assert!(set.is_dirty(id1));
        assert!(set.has_dirty());

        set.mark_dirty(id2);
        assert_eq!(set.dirty_count(), 2);

        let dirty = set.collect_dirty();
        assert_eq!(dirty.len(), 2);

        set.clear_dirty(id1);
        assert!(!set.is_dirty(id1));
        assert!(set.is_dirty(id2));

        set.clear_all();
        assert_eq!(set.dirty_count(), 0);
    }

    #[test]
    fn test_lock_free_out_of_bounds() {
        let set = LockFreeDirtySet::new(100);
        let id = ElementId::new(200);
        set.mark_dirty(id);
        assert!(!set.is_dirty(id));
    }

    #[test]
    fn test_lock_free_multi_thread() {
        let set = Arc::new(LockFreeDirtySet::new(10000));
        let mut handles = vec![];

        for thread_id in 0..8 {
            let set = Arc::clone(&set);
            let handle = thread::spawn(move || {
                for i in 0..1000 {
                    let id = ElementId::new(thread_id * 1000 + i + 1);
                    set.mark_dirty(id);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(set.dirty_count(), 8000);
    }

    #[test]
    fn test_lock_free_bitmap_edges() {
        let set = LockFreeDirtySet::new(200);
        let edges = [1, 64, 65, 128, 129];

        for &idx in &edges {
            let id = ElementId::new(idx);
            set.mark_dirty(id);
            assert!(set.is_dirty(id));
        }

        assert_eq!(set.dirty_count(), edges.len());
    }

    // ========== DirtySet tests ==========

    #[test]
    fn test_dirty_set_basic() {
        let set = DirtySet::new();
        let id = ElementId::new(1);

        assert!(!set.is_dirty(id));
        set.mark(id);
        assert!(set.is_dirty(id));
    }

    #[test]
    fn test_dirty_set_drain() {
        let set = DirtySet::new();
        set.mark(ElementId::new(1));
        set.mark(ElementId::new(2));

        assert_eq!(set.len(), 2);

        let drained = set.drain();
        assert_eq!(drained.len(), 2);
        assert!(set.is_empty());
    }
}
