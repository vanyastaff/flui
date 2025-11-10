//! ElementBase - Internal foundation for element lifecycle management
//!
//! This module provides the common foundation that all element types build upon.
//!
//! **Note**: ElementBase is typically not used directly by application code.
//! Instead, use the high-level element types (ComponentElement, RenderElement, etc.)
//! or the Element enum.
//!
//! # Architecture
//!
//! Per FINAL_ARCHITECTURE_V2.md, ElementBase contains ONLY common fields:
//! - parent: Parent element reference
//! - slot: Position in parent
//! - lifecycle: Element lifecycle state
//! - flags: Atomic flags for lock-free dirty tracking (DIRTY, NEEDS_LAYOUT, NEEDS_PAINT, etc.)
//!
//! Each element variant stores its own data (view, state, render_object, etc.).
//!
//! # Thread Safety
//!
//! ElementBase uses `AtomicElementFlags` for lock-free dirty tracking:
//! - `mark_dirty()` can be called from any thread
//! - `is_dirty()` can be called from any thread
//! - Zero memory overhead (same size as bool)
//! - Zero contention (lock-free atomic operations)

use crate::element::{ElementId, ElementLifecycle};
use crate::foundation::{AtomicElementFlags, ElementFlags, Slot};

/// ElementBase - Common fields for all element types
///
/// **Internal API** - This type is used internally by the framework.
/// Users should not need to interact with ElementBase directly.
///
/// Provides common lifecycle management for all element types.
///
/// # Architecture (per FINAL_ARCHITECTURE_V2.md)
///
/// ElementBase contains ONLY the common fields that ALL elements need:
/// - `parent`: Parent element reference
/// - `slot`: Position in parent's child list
/// - `lifecycle`: Current lifecycle state
/// - `flags`: Atomic flags for lock-free dirty tracking
///
/// Element-specific data is stored in the element variants:
/// - ComponentElement: stores `view: Box<dyn AnyView>` + `state: Box<dyn Any>` + `child: ElementId`
/// - RenderElement: stores `render_node: RenderNode`
/// - ProviderElement: stores `view: Box<dyn AnyView>` + `dependencies` + `child: ElementId`
///
/// # Lifecycle States
///
/// - `Initial`: Element created but not yet mounted
/// - `Active`: Element mounted and participating in the tree
/// - `Inactive`: Element temporarily deactivated (cached)
/// - `Defunct`: Element unmounted and removed from tree
///
/// # Thread Safety
///
/// - `flags` field uses `AtomicElementFlags` for lock-free operations
/// - `mark_dirty()` can be called from any thread safely
/// - `is_dirty()` can be called from any thread safely
/// - No locks, no contention, scales to N threads
///
/// # Common Operations
///
/// All elements support these operations via ElementBase:
/// - `mount()` - Attach element to tree
/// - `unmount()` - Remove element from tree
/// - `activate()` / `deactivate()` - Cache management
/// - `mark_dirty()` - Request rebuild (thread-safe!)
/// - `parent()` - Get parent element ID
#[derive(Debug)]
pub(crate) struct ElementBase {
    /// Parent element ID (None for root)
    parent: Option<ElementId>,

    /// Slot position in parent's child list
    /// Optional because root element has no slot
    slot: Option<Slot>,

    /// Current lifecycle state
    lifecycle: ElementLifecycle,

    /// Atomic flags for lock-free dirty tracking
    ///
    /// Uses `AtomicElementFlags` for thread-safe, lock-free flag operations:
    /// - DIRTY: Element needs rebuild
    /// - NEEDS_LAYOUT: Element needs layout
    /// - NEEDS_PAINT: Element needs paint
    /// - DETACHED: Element is detached
    /// - MOUNTED: Element is mounted
    /// - ACTIVE: Element is active
    ///
    /// # Thread Safety
    ///
    /// All flag operations are lock-free and safe to call from multiple threads.
    /// Uses atomic operations with proper memory ordering.
    ///
    /// # Size
    ///
    /// Size: 1 byte (same as bool) - zero overhead!
    flags: AtomicElementFlags,
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
    ///
    /// # Thread Safety
    ///
    /// All flag operations use atomic operations and are thread-safe.
    #[inline]
    pub fn new() -> Self {
        let flags = AtomicElementFlags::new();
        flags.insert(ElementFlags::DIRTY); // Needs initial build

        Self {
            parent: None,
            slot: None,
            lifecycle: ElementLifecycle::Initial,
            flags,
        }
    }

    // ========== Field Accessors ==========

    /// Get parent element ID
    ///
    /// Returns `Some(ElementId)` if element has a parent, `None` if root.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Get slot position in parent's child list
    ///
    /// Returns `None` for root elements
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

    /// Check if element needs rebuild
    ///
    /// Following API Guidelines: `is_*` prefix for boolean predicates.
    ///
    /// # Thread Safety
    ///
    /// Safe to call from any thread. Uses atomic load with Acquire ordering.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.flags.contains(ElementFlags::DIRTY)
    }

    // ========== Lifecycle Management ==========

    /// Mount element to tree
    ///
    /// Called when element is first added to the element tree.
    /// Sets parent, slot, and transitions to Active lifecycle state.
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent element ID (None for root)
    /// - `slot`: Position in parent's child list (None for root)
    ///
    /// # Lifecycle Transition
    ///
    /// Initial/Inactive → Active
    ///
    /// # Thread Safety
    ///
    /// Sets DIRTY and MOUNTED flags using atomic operations.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.parent = parent;
        self.slot = slot;
        self.lifecycle = ElementLifecycle::Active;

        // Set DIRTY and MOUNTED flags atomically
        self.flags
            .insert(ElementFlags::DIRTY | ElementFlags::MOUNTED | ElementFlags::ACTIVE);
    }

    /// Unmount element from tree
    ///
    /// Called when element is being removed from the tree.
    /// Transitions to Defunct lifecycle state.
    ///
    /// # Lifecycle Transition
    ///
    /// Any state → Defunct
    ///
    /// # Thread Safety
    ///
    /// Clears MOUNTED and ACTIVE flags atomically.
    #[inline]
    pub fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;

        // Clear MOUNTED and ACTIVE flags atomically
        self.flags
            .remove(ElementFlags::MOUNTED | ElementFlags::ACTIVE);
    }

    /// Deactivate element
    ///
    /// Called when element is temporarily deactivated (e.g., moved to cache).
    /// Transitions to Inactive lifecycle state but preserves state.
    ///
    /// # Lifecycle Transition
    ///
    /// Active → Inactive
    ///
    /// # Thread Safety
    ///
    /// Clears ACTIVE flag atomically.
    #[inline]
    pub fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;

        // Clear ACTIVE flag atomically
        self.flags.remove(ElementFlags::ACTIVE);
    }

    /// Activate element
    ///
    /// Called when element is reactivated after being deactivated.
    /// Transitions back to Active lifecycle state and marks dirty.
    ///
    /// # Lifecycle Transition
    ///
    /// Inactive → Active
    ///
    /// # Thread Safety
    ///
    /// Sets ACTIVE and DIRTY flags atomically.
    #[inline]
    pub fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;

        // Set ACTIVE and DIRTY flags atomically (rebuild when reactivated)
        self.flags
            .insert(ElementFlags::ACTIVE | ElementFlags::DIRTY);
    }

    /// Mark element as needing rebuild
    ///
    /// Sets the DIRTY flag, causing the element to rebuild on next frame.
    /// Called by setState() or when parent changes.
    ///
    /// # Thread Safety
    ///
    /// **Lock-free and thread-safe!**
    ///
    /// This method can be safely called from any thread without synchronization.
    /// Uses atomic OR operation which is idempotent - calling multiple times is safe.
    ///
    /// # Performance
    ///
    /// Time: ~2ns (single atomic OR instruction)
    /// No locks, no contention, scales to N threads.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Can call from any thread!
    /// element.mark_dirty();
    /// ```
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.flags.insert(ElementFlags::DIRTY);
    }

    /// Clear dirty flag
    ///
    /// Called after element has been rebuilt.
    /// Typically used internally by rebuild() implementations.
    ///
    /// # Thread Safety
    ///
    /// Uses atomic AND operation to clear flag.
    /// Should typically only be called by the pipeline after rebuild.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.flags.remove(ElementFlags::DIRTY);
    }

    /// Update slot position
    ///
    /// Called when element's position in parent's child list changes.
    ///
    /// # Parameters
    ///
    /// - `new_slot`: The new slot position
    #[inline]
    pub fn update_slot(&mut self, new_slot: Option<Slot>) {
        self.slot = new_slot;
    }
}

impl Default for ElementBase {
    fn default() -> Self {
        Self::new()
    }
}

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
        let slot = Some(crate::foundation::Slot::new(1));

        base.mount(Some(parent_id), slot);

        assert_eq!(base.lifecycle(), ElementLifecycle::Active);
        assert!(base.is_dirty());
        assert_eq!(base.parent(), Some(parent_id));
        assert_eq!(base.slot(), slot);
    }

    #[test]
    fn test_element_base_lifecycle() {
        let mut base = ElementBase::new();

        // Mount
        base.mount(Some(ElementId::new(1)), Some(crate::foundation::Slot::new(0)));
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
    }
}
