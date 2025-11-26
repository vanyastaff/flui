//! ElementBase - Internal foundation for element lifecycle management
//!
//! This module provides the common foundation that all element types build upon.
//!
//! **Note**: ElementBase is typically not used directly by application code.
//! Instead, use the high-level Element struct.
//!
//! # Architecture
//!
//! ElementBase contains ONLY common fields:
//! - parent: Parent element reference
//! - slot: Position in parent
//! - lifecycle: Element lifecycle state
//! - flags: Atomic flags for lock-free dirty tracking
//! - depth: Cached depth in tree (0 = root, atomic for thread-safety)
//!
//! # Thread Safety
//!
//! ElementBase uses `AtomicElementFlags` for lock-free dirty tracking:
//! - `mark_dirty()` can be called from any thread
//! - `is_dirty()` can be called from any thread
//! - Zero memory overhead (same size as bool)
//! - Zero contention (lock-free atomic operations)

use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::{ElementId, Slot};

use super::element_flags::{AtomicElementFlags, ElementFlags};
use super::ElementLifecycle;

/// ElementBase - Common fields for all element types
///
/// **Internal API** - This type is used internally by the framework.
/// Users should not need to interact with ElementBase directly.
///
/// # Thread Safety
///
/// - `flags` field uses `AtomicElementFlags` for lock-free operations
/// - `depth` field uses `AtomicUsize` for lock-free operations
/// - `mark_dirty()` and `set_depth()` can be called from any thread safely
/// - No locks, no contention, scales to N threads
#[derive(Debug)]
pub struct ElementBase {
    /// Parent element ID (None for root)
    parent: Option<ElementId>,

    /// Slot position in parent's child list
    slot: Option<Slot>,

    /// Current lifecycle state
    lifecycle: ElementLifecycle,

    /// Atomic flags for lock-free dirty tracking
    flags: AtomicElementFlags,

    /// Cached depth in tree (0 = root, atomic for thread-safety)
    ///
    /// This is cached to avoid O(depth) tree walks during build scheduling.
    /// Updated when element is mounted or reparented.
    depth: AtomicUsize,
}

impl ElementBase {
    /// Create a new ElementBase
    ///
    /// # Initial State
    ///
    /// - `lifecycle`: Initial (not yet mounted)
    /// - `flags`: DIRTY set (needs initial build)
    /// - `parent`: None
    /// - `slot`: None
    /// - `depth`: 0 (will be updated on mount)
    #[inline]
    pub fn new() -> Self {
        let flags = AtomicElementFlags::new();
        flags.insert(ElementFlags::DIRTY); // Needs initial build

        Self {
            parent: None,
            slot: None,
            lifecycle: ElementLifecycle::Initial,
            flags,
            depth: AtomicUsize::new(0),
        }
    }

    // ========== Field Accessors ==========

    /// Get parent element ID
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Set parent element ID
    #[inline]
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    /// Get slot position in parent's child list
    #[inline]
    #[must_use]
    pub fn slot(&self) -> Option<Slot> {
        self.slot
    }

    /// Get current lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    /// Get cached depth in tree (0 = root)
    ///
    /// **Lock-free and thread-safe!**
    ///
    /// This is O(1) as depth is cached. Updated on mount/reparent.
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth.load(Ordering::Relaxed)
    }

    /// Set cached depth
    ///
    /// **Lock-free and thread-safe!**
    ///
    /// Called when element is mounted or reparented to update cached depth.
    #[inline]
    pub fn set_depth(&self, depth: usize) {
        self.depth.store(depth, Ordering::Relaxed);
    }

    /// Check if element needs rebuild (DIRTY flag)
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.flags.contains(ElementFlags::DIRTY)
    }

    /// Check if element needs layout
    #[inline]
    #[must_use]
    pub fn needs_layout(&self) -> bool {
        self.flags.contains(ElementFlags::NEEDS_LAYOUT)
    }

    /// Check if element needs paint
    #[inline]
    #[must_use]
    pub fn needs_paint(&self) -> bool {
        self.flags.contains(ElementFlags::NEEDS_PAINT)
    }

    /// Check if element is mounted
    #[inline]
    #[must_use]
    pub fn is_mounted(&self) -> bool {
        self.flags.contains(ElementFlags::MOUNTED)
    }

    // ========== Lifecycle Management ==========

    /// Mount element to tree
    ///
    /// # Lifecycle Transition
    ///
    /// Initial/Inactive → Active
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent element ID (None for root)
    /// - `slot`: Slot position in parent
    /// - `depth`: Depth in tree (0 for root, parent.depth() + 1 for children)
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>, depth: usize) {
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;
        self.depth.store(depth, Ordering::Relaxed);
        self.flags
            .insert(ElementFlags::DIRTY | ElementFlags::MOUNTED | ElementFlags::ACTIVE);
    }

    /// Unmount element from tree
    ///
    /// # Lifecycle Transition
    ///
    /// Any state → Defunct
    #[inline]
    pub fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        self.flags
            .remove(ElementFlags::MOUNTED | ElementFlags::ACTIVE);
    }

    /// Deactivate element
    ///
    /// # Lifecycle Transition
    ///
    /// Active → Inactive
    #[inline]
    pub fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
        self.flags.remove(ElementFlags::ACTIVE);
    }

    /// Activate element
    ///
    /// # Lifecycle Transition
    ///
    /// Inactive → Active
    #[inline]
    pub fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        self.flags
            .insert(ElementFlags::ACTIVE | ElementFlags::DIRTY);
    }

    // ========== Dirty Tracking ==========

    /// Mark element as needing rebuild
    ///
    /// **Lock-free and thread-safe!**
    #[inline]
    pub fn mark_dirty(&self) {
        self.flags.insert(ElementFlags::DIRTY);
    }

    /// Clear dirty flag
    #[inline]
    pub fn clear_dirty(&self) {
        self.flags.remove(ElementFlags::DIRTY);
    }

    /// Mark element as needing layout
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.flags.insert(ElementFlags::NEEDS_LAYOUT);
    }

    /// Clear needs layout flag
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.flags.remove(ElementFlags::NEEDS_LAYOUT);
    }

    /// Mark element as needing paint
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.flags.insert(ElementFlags::NEEDS_PAINT);
    }

    /// Clear needs paint flag
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.flags.remove(ElementFlags::NEEDS_PAINT);
    }

    /// Update slot position
    #[inline]
    pub fn update_slot(&mut self, new_slot: Option<Slot>) {
        self.slot = new_slot;
    }

    /// Get raw flags for inspection
    #[inline]
    #[must_use]
    pub fn flags(&self) -> &AtomicElementFlags {
        &self.flags
    }
}

impl Default for ElementBase {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_base_creation() {
        let base = ElementBase::new();

        assert_eq!(base.lifecycle(), ElementLifecycle::Initial);
        assert!(base.is_dirty());
        assert_eq!(base.parent(), None);
        assert_eq!(base.slot(), None);
    }

    #[test]
    fn test_element_base_mount() {
        let mut base = ElementBase::new();
        let parent_id = ElementId::new(42);
        let slot = Some(Slot::new(1));

        base.mount(Some(parent_id), slot);

        assert_eq!(base.lifecycle(), ElementLifecycle::Active);
        assert!(base.is_dirty());
        assert!(base.is_mounted());
        assert_eq!(base.parent(), Some(parent_id));
        assert_eq!(base.slot(), slot);
    }

    #[test]
    fn test_element_base_lifecycle() {
        let mut base = ElementBase::new();

        // Mount
        base.mount(Some(ElementId::new(1)), Some(Slot::new(0)));
        assert_eq!(base.lifecycle(), ElementLifecycle::Active);

        // Deactivate
        base.deactivate();
        assert_eq!(base.lifecycle(), ElementLifecycle::Inactive);

        // Activate
        base.activate();
        assert_eq!(base.lifecycle(), ElementLifecycle::Active);
        assert!(base.is_dirty());

        // Unmount
        base.unmount();
        assert_eq!(base.lifecycle(), ElementLifecycle::Defunct);
        assert!(!base.is_mounted());
    }

    #[test]
    fn test_dirty_tracking() {
        let base = ElementBase::new();

        // Initially dirty
        assert!(base.is_dirty());

        // Clear dirty
        base.clear_dirty();
        assert!(!base.is_dirty());

        // Mark dirty again
        base.mark_dirty();
        assert!(base.is_dirty());
    }

    #[test]
    fn test_layout_paint_flags() {
        let base = ElementBase::new();

        // Initially no layout/paint needed
        assert!(!base.needs_layout());
        assert!(!base.needs_paint());

        // Mark needs layout
        base.mark_needs_layout();
        assert!(base.needs_layout());

        // Mark needs paint
        base.mark_needs_paint();
        assert!(base.needs_paint());

        // Clear
        base.clear_needs_layout();
        base.clear_needs_paint();
        assert!(!base.needs_layout());
        assert!(!base.needs_paint());
    }
}
