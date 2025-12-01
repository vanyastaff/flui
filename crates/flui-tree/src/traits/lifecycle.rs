//! Element lifecycle management traits.
//!
//! This module defines **abstract traits** for managing element lifecycle
//! in a Flutter-like element tree architecture. The concrete `ElementLifecycle`
//! enum is defined in `flui-element` crate.
//!
//! # Traits vs Concrete Types
//!
//! - **flui-tree**: Defines `Lifecycle` trait (abstract interface)
//! - **flui-element**: Defines `ElementLifecycle` enum and implements `Lifecycle` trait
//!
//! This separation allows `flui-tree` to remain abstract and reusable.
//!
//! # Flutter Element Lifecycle
//!
//! ```text
//! createElement() → mount() → [update()/rebuild()] → deactivate() → unmount()
//!                      ↑              ↓
//!                      └── activate() ←┘
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::Lifecycle;
//! use flui_foundation::Slot;
//!
//! impl Lifecycle for MyElement {
//!     fn mount(&mut self, parent: Option<ElementId>, slot: Slot) {
//!         // Initialize resources, register with parent
//!     }
//!
//!     fn unmount(&mut self) {
//!         // Release resources, unregister from parent
//!     }
//!
//!     // ... other methods
//! }
//! ```

use flui_foundation::{ElementId, Slot};

// ============================================================================
// LIFECYCLE TRAIT
// ============================================================================

/// Core lifecycle management for elements.
///
/// This trait defines the fundamental lifecycle operations that all
/// elements must support. It mirrors Flutter's `Element` lifecycle methods.
///
/// # Note on Naming
///
/// This trait is named `Lifecycle` (not `ElementLifecycle`) to avoid
/// conflict with the `ElementLifecycle` enum in `flui-element` crate.
///
/// # Lifecycle Flow
///
/// 1. **mount()** - Called when element is inserted into tree
/// 2. **update()** - Called when widget configuration changes
/// 3. **rebuild()** - Called when element needs to rebuild (state change)
/// 4. **deactivate()** - Called when element is temporarily removed
/// 5. **activate()** - Called when inactive element is reinserted
/// 6. **unmount()** - Called when element is permanently removed
///
/// # Implementors
///
/// The concrete `Element` type in `flui-element` implements this trait.
pub trait Lifecycle: Send + Sync {
    /// Check if element is currently active (can participate in build/layout/paint).
    fn is_active(&self) -> bool;

    /// Check if element is mounted in the tree.
    fn is_mounted(&self) -> bool;

    /// Mount element into the tree.
    ///
    /// Called when element is first inserted into the tree.
    /// The element should:
    /// - Set lifecycle state to Active
    /// - Initialize any resources
    /// - Create render object if applicable
    /// - Register with inherited elements
    ///
    /// # Arguments
    /// * `parent` - Parent element ID (None if root)
    /// * `slot` - Position slot within parent
    fn mount(&mut self, parent: Option<ElementId>, slot: Slot);

    /// Unmount element from the tree.
    ///
    /// Called when element is permanently removed.
    /// The element should:
    /// - Set lifecycle state to Defunct
    /// - Release all resources
    /// - Detach render object
    /// - Unregister from inherited elements
    fn unmount(&mut self);

    /// Update element with new widget configuration.
    ///
    /// Called when parent rebuilds with a new widget that can update
    /// this element (same type, same key).
    ///
    /// Returns true if the update was successful.
    fn update(&mut self) -> bool {
        true
    }

    /// Mark element as needing rebuild.
    ///
    /// Called when element's state changes and it needs to
    /// rebuild its children.
    fn mark_needs_build(&mut self);

    /// Check if element needs rebuild.
    fn needs_build(&self) -> bool;

    /// Perform the build operation.
    ///
    /// Called during build phase to rebuild element's children.
    /// This is where StatefulElement calls State.build().
    fn perform_rebuild(&mut self);

    /// Deactivate element (temporary removal).
    ///
    /// Called when element is removed but may be reactivated
    /// (e.g., during GlobalKey reparenting).
    fn deactivate(&mut self) {
        // Default: no-op, concrete implementations override
    }

    /// Activate previously deactivated element.
    ///
    /// Called when an inactive element is reinserted into tree.
    fn activate(&mut self) {
        // Default: no-op, concrete implementations override
    }

    /// Called after element's dependencies change.
    ///
    /// This happens when an InheritedElement that this element
    /// depends on changes.
    fn did_change_dependencies(&mut self) {
        // Default: mark for rebuild
        self.mark_needs_build();
    }
}

// ============================================================================
// ELEMENT TREE OPERATIONS TRAIT
// ============================================================================

/// Tree mutation operations for elements.
///
/// These operations modify the tree structure - adding, removing,
/// and moving elements.
pub trait ElementTreeOps: Lifecycle {
    /// Attach a child element at the given slot.
    ///
    /// The child will be mounted with this element as parent.
    fn attach_child(&mut self, child: ElementId, slot: Slot);

    /// Detach a child element.
    ///
    /// The child will be deactivated (not unmounted) in case
    /// it needs to be reattached elsewhere.
    fn detach_child(&mut self, child: ElementId);

    /// Move a child to a different slot.
    ///
    /// Used for reordering children without unmounting.
    fn move_child(&mut self, child: ElementId, new_slot: Slot);

    /// Update child's slot without detaching.
    fn update_slot(&mut self, child: ElementId, new_slot: Slot);

    /// Get the slot for a child element.
    fn slot_for_child(&self, child: ElementId) -> Option<Slot>;

    /// Visit all children in slot order.
    fn visit_children<F>(&self, visitor: F)
    where
        F: FnMut(ElementId, Slot);
}

// ============================================================================
// REBUILD SCHEDULING
// ============================================================================

/// Priority levels for rebuild scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RebuildPriority {
    /// Idle priority - rebuild when nothing else to do.
    Idle = 0,

    /// Normal priority - standard rebuild.
    Normal = 1,

    /// High priority - user interaction response.
    High = 2,

    /// Immediate - must happen this frame.
    Immediate = 3,
}

impl Default for RebuildPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Trait for scheduling element rebuilds.
///
/// The scheduler decides when and in what order to rebuild
/// dirty elements.
pub trait RebuildScheduler: Send + Sync {
    /// Schedule an element for rebuild.
    fn schedule_rebuild(&self, element: ElementId, priority: RebuildPriority);

    /// Cancel a scheduled rebuild.
    fn cancel_rebuild(&self, element: ElementId);

    /// Check if element is scheduled for rebuild.
    fn is_scheduled(&self, element: ElementId) -> bool;

    /// Get next element to rebuild (highest priority first).
    fn next_rebuild(&mut self) -> Option<ElementId>;

    /// Check if there are pending rebuilds.
    fn has_pending_rebuilds(&self) -> bool;

    /// Flush all pending rebuilds.
    ///
    /// Processes all scheduled rebuilds in priority order.
    fn flush_rebuilds<F>(&mut self, rebuild_fn: F)
    where
        F: FnMut(ElementId);
}

// ============================================================================
// DEPTH TRACKING
// ============================================================================

/// Trait for tracking element depth in tree.
///
/// Depth is used for:
/// - Build order (parents before children)
/// - Optimization (skip deep subtrees)
/// - Debugging
pub trait DepthTracking {
    /// Get element's depth in tree (root = 0).
    fn depth(&self) -> usize;

    /// Set element's depth.
    fn set_depth(&mut self, depth: usize);

    /// Update depth based on parent.
    fn update_depth_from_parent(&mut self, parent_depth: usize) {
        self.set_depth(parent_depth + 1);
    }
}

// ============================================================================
// OWNER TRACKING
// ============================================================================

/// Trait for tracking element ownership.
///
/// The owner is the `BuildOwner` in Flutter - it manages the
/// build phase for a tree of elements.
pub trait OwnerTracking {
    /// Owner ID type.
    type OwnerId: Copy + Eq;

    /// Get the owner of this element.
    fn owner(&self) -> Option<Self::OwnerId>;

    /// Set the owner of this element.
    fn set_owner(&mut self, owner: Option<Self::OwnerId>);

    /// Check if element has an owner.
    fn has_owner(&self) -> bool {
        self.owner().is_some()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_from_foundation() {
        // Using flui-foundation Slot
        let slot = Slot::new(5);
        assert_eq!(slot.index(), 5);

        let slot_with_sibling = Slot::with_previous_sibling(2, Some(42));
        assert_eq!(slot_with_sibling.index(), 2);
        assert_eq!(slot_with_sibling.previous_sibling(), Some(42));

        assert_eq!(Slot::default().index(), 0);
    }

    #[test]
    fn test_rebuild_priority_ordering() {
        assert!(RebuildPriority::Immediate > RebuildPriority::High);
        assert!(RebuildPriority::High > RebuildPriority::Normal);
        assert!(RebuildPriority::Normal > RebuildPriority::Idle);
    }
}
