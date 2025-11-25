//! Dirty tracking trait for layout and paint.
//!
//! This module provides the [`DirtyTracking`] trait for managing
//! layout and paint dirty flags on tree elements.

use flui_foundation::ElementId;

/// Dirty flag management for layout and paint phases.
///
/// This trait provides methods to mark elements as needing layout
/// or paint, and to query/clear those flags. It's designed for
/// efficient incremental updates where only dirty elements are
/// processed.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync`. The `mark_*` methods take
/// `&self` (not `&mut self`) to allow marking from multiple contexts.
/// This typically requires internal synchronization (atomic flags).
///
/// # Design Rationale
///
/// Dirty tracking is separated from other tree operations because:
/// 1. Flags are typically stored in RenderState, not the node itself
/// 2. Marking can happen from callbacks without mutable access
/// 3. Different implementations may use atomic vs. locked flags
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::DirtyTracking;
///
/// fn process_dirty_layout<T: DirtyTracking>(tree: &T, root: ElementId) {
///     // Collect dirty elements
///     let dirty: Vec<_> = collect_needing_layout(tree, root);
///
///     for id in dirty {
///         // Process layout...
///         tree.clear_needs_layout(id);
///     }
/// }
/// ```
pub trait DirtyTracking: Send + Sync {
    /// Marks an element as needing layout.
    ///
    /// This should propagate up to ancestors if they need to be
    /// re-laid-out as well (relayout boundary handling).
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark
    ///
    /// # Thread Safety
    ///
    /// This method takes `&self` to allow concurrent marking.
    /// Implementations should use atomic operations or other
    /// synchronization.
    fn mark_needs_layout(&self, id: ElementId);

    /// Marks an element as needing paint.
    ///
    /// Unlike layout, paint dirty flags typically don't propagate
    /// to ancestors (paint is local to the element).
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark
    fn mark_needs_paint(&self, id: ElementId);

    /// Clears the needs-layout flag for an element.
    ///
    /// Called after layout is complete for the element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to clear
    fn clear_needs_layout(&self, id: ElementId);

    /// Clears the needs-paint flag for an element.
    ///
    /// Called after paint is complete for the element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to clear
    fn clear_needs_paint(&self, id: ElementId);

    /// Returns `true` if the element needs layout.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    fn needs_layout(&self, id: ElementId) -> bool;

    /// Returns `true` if the element needs paint.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    fn needs_paint(&self, id: ElementId) -> bool;

    /// Returns `true` if the element needs either layout or paint.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    #[inline]
    fn is_dirty(&self, id: ElementId) -> bool {
        self.needs_layout(id) || self.needs_paint(id)
    }

    /// Marks an element as needing both layout and paint.
    ///
    /// Convenience method for full invalidation.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark
    #[inline]
    fn mark_needs_rebuild(&self, id: ElementId) {
        self.mark_needs_layout(id);
        self.mark_needs_paint(id);
    }

    /// Clears both layout and paint flags.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to clear
    #[inline]
    fn clear_dirty(&self, id: ElementId) {
        self.clear_needs_layout(id);
        self.clear_needs_paint(id);
    }
}

// ============================================================================
// EXTENDED DIRTY TRACKING
// ============================================================================

/// Extended dirty tracking with additional capabilities.
///
/// This trait adds batch operations and dirty element enumeration
/// for more efficient pipeline processing.
pub trait DirtyTrackingExt: DirtyTracking {
    /// Returns all elements that need layout.
    ///
    /// # Returns
    ///
    /// A vector of element IDs that have the needs-layout flag set.
    ///
    /// # Note
    ///
    /// Default implementation returns empty vector. Implementations
    /// that maintain dirty sets should override.
    #[inline]
    fn elements_needing_layout(&self) -> Vec<ElementId> {
        Vec::new()
    }

    /// Returns all elements that need paint.
    ///
    /// # Returns
    ///
    /// A vector of element IDs that have the needs-paint flag set.
    #[inline]
    fn elements_needing_paint(&self) -> Vec<ElementId> {
        Vec::new()
    }

    /// Returns the count of elements needing layout.
    #[inline]
    fn layout_dirty_count(&self) -> usize {
        self.elements_needing_layout().len()
    }

    /// Returns the count of elements needing paint.
    #[inline]
    fn paint_dirty_count(&self) -> usize {
        self.elements_needing_paint().len()
    }

    /// Returns `true` if any elements need layout.
    #[inline]
    fn has_dirty_layout(&self) -> bool {
        self.layout_dirty_count() > 0
    }

    /// Returns `true` if any elements need paint.
    #[inline]
    fn has_dirty_paint(&self) -> bool {
        self.paint_dirty_count() > 0
    }

    /// Marks multiple elements as needing layout.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to mark
    #[inline]
    fn mark_many_needs_layout(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.mark_needs_layout(id);
        }
    }

    /// Marks multiple elements as needing paint.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to mark
    #[inline]
    fn mark_many_needs_paint(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.mark_needs_paint(id);
        }
    }

    /// Clears layout flags for multiple elements.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to clear
    #[inline]
    fn clear_many_layout(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.clear_needs_layout(id);
        }
    }

    /// Clears paint flags for multiple elements.
    ///
    /// # Arguments
    ///
    /// * `ids` - The elements to clear
    #[inline]
    fn clear_many_paint(&self, ids: impl IntoIterator<Item = ElementId>) {
        for id in ids {
            self.clear_needs_paint(id);
        }
    }

    /// Clears all layout dirty flags.
    #[inline]
    fn clear_all_layout(&self) {
        for id in self.elements_needing_layout() {
            self.clear_needs_layout(id);
        }
    }

    /// Clears all paint dirty flags.
    #[inline]
    fn clear_all_paint(&self) {
        for id in self.elements_needing_paint() {
            self.clear_needs_paint(id);
        }
    }
}

// Blanket implementation
impl<T: DirtyTracking + ?Sized> DirtyTrackingExt for T {}

// ============================================================================
// ATOMIC DIRTY FLAGS
// ============================================================================

use std::sync::atomic::{AtomicU8, Ordering};

/// Compact atomic dirty flags for render elements.
///
/// This type provides lock-free dirty flag management using a single
/// atomic byte. It's designed to be embedded in RenderState.
///
/// # Flags
///
/// - Bit 0: `NEEDS_LAYOUT`
/// - Bit 1: `NEEDS_PAINT`
/// - Bit 2: `NEEDS_COMPOSITING_BITS_UPDATE`
/// - Bit 3: `NEEDS_SEMANTICS_UPDATE`
///
/// # Thread Safety
///
/// All operations use `Ordering::SeqCst` for simplicity. For
/// performance-critical code, consider using `Ordering::Relaxed`
/// with appropriate memory barriers.
#[derive(Debug)]
pub struct AtomicDirtyFlags {
    flags: AtomicU8,
}

impl AtomicDirtyFlags {
    /// Flag bit for needs-layout.
    pub const NEEDS_LAYOUT: u8 = 1 << 0;

    /// Flag bit for needs-paint.
    pub const NEEDS_PAINT: u8 = 1 << 1;

    /// Flag bit for needs-compositing-bits-update.
    pub const NEEDS_COMPOSITING_BITS_UPDATE: u8 = 1 << 2;

    /// Flag bit for needs-semantics-update.
    pub const NEEDS_SEMANTICS_UPDATE: u8 = 1 << 3;

    /// Creates new dirty flags with no flags set.
    #[inline]
    pub const fn new() -> Self {
        Self {
            flags: AtomicU8::new(0),
        }
    }

    /// Creates new dirty flags with initial layout dirty.
    #[inline]
    pub const fn new_needs_layout() -> Self {
        Self {
            flags: AtomicU8::new(Self::NEEDS_LAYOUT),
        }
    }

    /// Sets a flag.
    #[inline]
    pub fn set(&self, flag: u8) {
        self.flags.fetch_or(flag, Ordering::SeqCst);
    }

    /// Clears a flag.
    #[inline]
    pub fn clear(&self, flag: u8) {
        self.flags.fetch_and(!flag, Ordering::SeqCst);
    }

    /// Returns `true` if a flag is set.
    #[inline]
    pub fn is_set(&self, flag: u8) -> bool {
        (self.flags.load(Ordering::SeqCst) & flag) != 0
    }

    /// Returns the raw flags value.
    #[inline]
    pub fn get(&self) -> u8 {
        self.flags.load(Ordering::SeqCst)
    }

    /// Sets all flags to a specific value.
    #[inline]
    pub fn store(&self, value: u8) {
        self.flags.store(value, Ordering::SeqCst);
    }

    /// Clears all flags.
    #[inline]
    pub fn clear_all(&self) {
        self.flags.store(0, Ordering::SeqCst);
    }

    // Convenience methods

    /// Sets the needs-layout flag.
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.set(Self::NEEDS_LAYOUT);
    }

    /// Clears the needs-layout flag.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.clear(Self::NEEDS_LAYOUT);
    }

    /// Returns `true` if needs-layout is set.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.is_set(Self::NEEDS_LAYOUT)
    }

    /// Sets the needs-paint flag.
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.set(Self::NEEDS_PAINT);
    }

    /// Clears the needs-paint flag.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.clear(Self::NEEDS_PAINT);
    }

    /// Returns `true` if needs-paint is set.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.is_set(Self::NEEDS_PAINT)
    }

    /// Returns `true` if any flag is set.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.get() != 0
    }
}

impl Default for AtomicDirtyFlags {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AtomicDirtyFlags {
    fn clone(&self) -> Self {
        Self {
            flags: AtomicU8::new(self.get()),
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_dirty_flags_new() {
        let flags = AtomicDirtyFlags::new();
        assert!(!flags.needs_layout());
        assert!(!flags.needs_paint());
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_layout() {
        let flags = AtomicDirtyFlags::new();

        flags.mark_needs_layout();
        assert!(flags.needs_layout());
        assert!(!flags.needs_paint());
        assert!(flags.is_dirty());

        flags.clear_needs_layout();
        assert!(!flags.needs_layout());
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_paint() {
        let flags = AtomicDirtyFlags::new();

        flags.mark_needs_paint();
        assert!(!flags.needs_layout());
        assert!(flags.needs_paint());
        assert!(flags.is_dirty());

        flags.clear_needs_paint();
        assert!(!flags.needs_paint());
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_multiple() {
        let flags = AtomicDirtyFlags::new();

        flags.mark_needs_layout();
        flags.mark_needs_paint();

        assert!(flags.needs_layout());
        assert!(flags.needs_paint());

        flags.clear_needs_layout();
        assert!(!flags.needs_layout());
        assert!(flags.needs_paint());

        flags.clear_all();
        assert!(!flags.is_dirty());
    }

    #[test]
    fn test_atomic_dirty_flags_clone() {
        let flags = AtomicDirtyFlags::new();
        flags.mark_needs_layout();

        let cloned = flags.clone();
        assert!(cloned.needs_layout());
    }

    #[test]
    fn test_atomic_dirty_flags_thread_safety() {
        use std::sync::Arc;
        use std::thread;

        let flags = Arc::new(AtomicDirtyFlags::new());
        let mut handles = vec![];

        // Spawn multiple threads that mark flags
        for _ in 0..10 {
            let flags = Arc::clone(&flags);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    flags.mark_needs_layout();
                    flags.mark_needs_paint();
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Flags should be set (exact value depends on interleaving)
        assert!(flags.is_dirty());
    }
}
