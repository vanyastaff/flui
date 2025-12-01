//! Widget-Element reconciliation traits.
//!
//! Reconciliation is the process of matching widgets to elements
//! during rebuild. This determines whether to:
//! - **Update** - Same type & key, reuse element with new widget
//! - **Replace** - Different type or key, unmount old, mount new
//! - **Insert** - No existing element, create new
//! - **Remove** - No new widget, unmount existing
//!
//! # Flutter's Algorithm
//!
//! Flutter uses a linear scan with keys:
//! 1. Match by key first (if present)
//! 2. Match by type if no key
//! 3. Handle insertions/deletions
//!
//! # Key Types
//!
//! This module uses `flui_foundation::Key` wrapped in `Option`:
//! - `Some(Key)` - Widget has a key for identity tracking
//! - `None` - Widget matched by type and position only
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::CanUpdate;
//! use flui_foundation::Key;
//!
//! // Widget can update element if same type and key matches
//! impl CanUpdate for MyWidget {
//!     fn can_update(&self, old: &Self) -> bool {
//!         self.key() == old.key()
//!     }
//! }
//! ```

use flui_foundation::{ElementId, Key};
use std::any::TypeId;

// ============================================================================
// CAN UPDATE TRAIT
// ============================================================================

/// Trait to determine if a widget can update an existing element.
///
/// This is the core decision in reconciliation:
/// - If `can_update` returns true, the element is reused
/// - If false, old element is unmounted and new one created
pub trait CanUpdate {
    /// Check if this widget can update an element created from `old` widget.
    ///
    /// Default implementation checks:
    /// 1. Same runtime type
    /// 2. Keys match (or both have no key)
    fn can_update(&self, old: &Self) -> bool
    where
        Self: Sized + 'static,
    {
        // Same type by definition (both are Self)
        // Keys must match: both None, or both Some with equal values
        self.key() == old.key()
    }

    /// Get the key for this widget.
    ///
    /// Returns `None` if widget has no key (matched by type/position).
    /// Returns `Some(Key)` for keyed widgets (matched by key value).
    fn key(&self) -> Option<Key> {
        None
    }

    /// Get the runtime type ID for comparison.
    fn widget_type_id(&self) -> TypeId
    where
        Self: 'static,
    {
        TypeId::of::<Self>()
    }
}

// ============================================================================
// RECONCILIATION RESULT
// ============================================================================

/// Result of reconciling old and new widget lists.
#[derive(Debug, Clone)]
pub struct ReconciliationResult {
    /// Elements to keep (update with new widget).
    pub updates: Vec<UpdateAction>,

    /// Elements to remove (unmount).
    pub removals: Vec<ElementId>,

    /// Elements to insert (new widgets).
    pub insertions: Vec<InsertAction>,

    /// Elements to move (reorder).
    pub moves: Vec<MoveAction>,
}

impl ReconciliationResult {
    /// Create empty result.
    pub fn new() -> Self {
        Self {
            updates: Vec::new(),
            removals: Vec::new(),
            insertions: Vec::new(),
            moves: Vec::new(),
        }
    }

    /// Check if any changes are needed.
    pub fn has_changes(&self) -> bool {
        !self.updates.is_empty()
            || !self.removals.is_empty()
            || !self.insertions.is_empty()
            || !self.moves.is_empty()
    }

    /// Total number of actions.
    pub fn action_count(&self) -> usize {
        self.updates.len() + self.removals.len() + self.insertions.len() + self.moves.len()
    }
}

impl Default for ReconciliationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Action to update an existing element.
#[derive(Debug, Clone)]
pub struct UpdateAction {
    /// Element to update.
    pub element: ElementId,
    /// New slot position.
    pub new_slot: usize,
}

/// Action to insert a new element.
#[derive(Debug, Clone)]
pub struct InsertAction {
    /// Position to insert at.
    pub slot: usize,
    /// Key of the new widget (for tracking).
    pub key: Option<Key>,
}

/// Action to move an element.
#[derive(Debug, Clone)]
pub struct MoveAction {
    /// Element to move.
    pub element: ElementId,
    /// Current slot.
    pub from_slot: usize,
    /// Target slot.
    pub to_slot: usize,
}

// ============================================================================
// RECONCILER TRAIT
// ============================================================================

/// Trait for reconciling widget lists with element lists.
///
/// Implementations can use different algorithms:
/// - Linear scan (Flutter default)
/// - Two-pointer
/// - LCS-based (for minimal moves)
pub trait Reconciler: Send + Sync {
    /// Reconcile old elements with new widget keys.
    ///
    /// # Arguments
    /// * `old_elements` - Current child elements with their keys (None = unkeyed)
    /// * `new_keys` - Keys of new widgets (in order, None = unkeyed)
    ///
    /// # Returns
    /// Actions needed to transform old list to new list.
    fn reconcile(
        &self,
        old_elements: &[(ElementId, Option<Key>)],
        new_keys: &[Option<Key>],
    ) -> ReconciliationResult;
}

/// Linear reconciler matching Flutter's algorithm.
///
/// Time complexity: O(n) for most cases
/// Uses keys for matching when available, falls back to position.
#[derive(Debug, Clone, Copy, Default)]
pub struct LinearReconciler;

impl Reconciler for LinearReconciler {
    fn reconcile(
        &self,
        old_elements: &[(ElementId, Option<Key>)],
        new_keys: &[Option<Key>],
    ) -> ReconciliationResult {
        let mut result = ReconciliationResult::new();

        // Build key -> element map for old elements with keys
        let mut keyed_old: std::collections::HashMap<Key, (usize, ElementId)> =
            std::collections::HashMap::new();
        for (idx, (elem, key)) in old_elements.iter().enumerate() {
            if let Some(k) = key {
                keyed_old.insert(*k, (idx, *elem));
            }
        }

        let mut old_index = 0;
        let mut used_old: std::collections::HashSet<usize> = std::collections::HashSet::new();

        for (new_slot, new_key) in new_keys.iter().enumerate() {
            // Try to match by key first
            if let Some(k) = new_key {
                if let Some(&(old_slot, elem)) = keyed_old.get(k) {
                    used_old.insert(old_slot);
                    if old_slot != new_slot {
                        result.moves.push(MoveAction {
                            element: elem,
                            from_slot: old_slot,
                            to_slot: new_slot,
                        });
                    } else {
                        result.updates.push(UpdateAction {
                            element: elem,
                            new_slot,
                        });
                    }
                    continue;
                }
            }

            // No key match, try positional match for unkeyed widgets
            if new_key.is_none() {
                // Find next unkeyed old element
                while old_index < old_elements.len() {
                    let (elem, old_key) = &old_elements[old_index];
                    if old_key.is_none() && !used_old.contains(&old_index) {
                        // Match!
                        used_old.insert(old_index);
                        result.updates.push(UpdateAction {
                            element: *elem,
                            new_slot,
                        });
                        old_index += 1;
                        break;
                    }
                    old_index += 1;
                }
                if old_index > old_elements.len() {
                    // No match found, insert
                    result.insertions.push(InsertAction {
                        slot: new_slot,
                        key: *new_key,
                    });
                }
            } else {
                // Has key but no match, must insert
                result.insertions.push(InsertAction {
                    slot: new_slot,
                    key: *new_key,
                });
            }
        }

        // Remove unused old elements
        for (idx, (elem, _)) in old_elements.iter().enumerate() {
            if !used_old.contains(&idx) {
                result.removals.push(*elem);
            }
        }

        result
    }
}

// ============================================================================
// GLOBAL KEY REGISTRY
// ============================================================================

/// Trait for global key registration and lookup.
///
/// Global keys allow finding elements from anywhere in the tree.
/// Only one element with a given global key can exist at a time.
///
/// Uses `flui_foundation::Key` for key identity.
pub trait GlobalKeyRegistry: Send + Sync {
    /// Register an element with a global key.
    ///
    /// Returns the old element if one was already registered.
    fn register(&mut self, key: Key, element: ElementId) -> Option<ElementId>;

    /// Unregister an element's global key.
    fn unregister(&mut self, key: Key) -> Option<ElementId>;

    /// Look up element by global key.
    fn lookup(&self, key: Key) -> Option<ElementId>;

    /// Check if global key is registered.
    fn contains(&self, key: Key) -> bool {
        self.lookup(key).is_some()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foundation_key_equality() {
        // Using flui_foundation::Key
        let k1 = Key::from_str("test");
        let k2 = Key::from_str("test");
        let k3 = Key::from_str("other");

        assert_eq!(k1, k2);
        assert_ne!(k1, k3);

        // Option<Key> equality
        assert_eq!(Some(k1), Some(k2));
        assert_ne!(Some(k1), Some(k3));
        assert_eq!(None::<Key>, None::<Key>);
        assert_ne!(Some(k1), None);
    }

    #[test]
    fn test_unique_keys() {
        let k1 = Key::new();
        let k2 = Key::new();

        assert_eq!(k1, k1);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_linear_reconciler_simple() {
        let reconciler = LinearReconciler;

        // Empty to empty
        let result = reconciler.reconcile(&[], &[]);
        assert!(!result.has_changes());

        // Insert into empty (unkeyed)
        let result = reconciler.reconcile(&[], &[None, None]);
        assert_eq!(result.insertions.len(), 2);

        // Remove all
        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);
        let result = reconciler.reconcile(&[(e1, None), (e2, None)], &[]);
        assert_eq!(result.removals.len(), 2);
    }

    #[test]
    fn test_linear_reconciler_with_keys() {
        let reconciler = LinearReconciler;

        let e1 = ElementId::new(1);
        let e2 = ElementId::new(2);
        let k1 = Key::from_str("a");
        let k2 = Key::from_str("b");

        // Swap order
        let old = vec![(e1, Some(k1)), (e2, Some(k2))];
        let new_keys = vec![Some(k2), Some(k1)];

        let result = reconciler.reconcile(&old, &new_keys);
        assert!(!result.removals.is_empty() || !result.moves.is_empty());
    }

    #[test]
    fn test_reconciliation_result() {
        let mut result = ReconciliationResult::new();
        assert!(!result.has_changes());
        assert_eq!(result.action_count(), 0);

        result.insertions.push(InsertAction { slot: 0, key: None });
        assert!(result.has_changes());
        assert_eq!(result.action_count(), 1);
    }

    #[test]
    fn test_can_update_default() {
        struct TestWidget {
            key: Option<Key>,
        }

        impl CanUpdate for TestWidget {
            fn key(&self) -> Option<Key> {
                self.key
            }
        }

        let w1 = TestWidget { key: None };
        let w2 = TestWidget { key: None };
        let w3 = TestWidget {
            key: Some(Key::from_str("test")),
        };
        let w4 = TestWidget {
            key: Some(Key::from_str("test")),
        };
        let w5 = TestWidget {
            key: Some(Key::from_str("other")),
        };

        // Both unkeyed - can update
        assert!(w1.can_update(&w2));

        // Same key - can update
        assert!(w3.can_update(&w4));

        // Different keys - cannot update
        assert!(!w3.can_update(&w5));

        // Keyed vs unkeyed - cannot update
        assert!(!w1.can_update(&w3));
        assert!(!w3.can_update(&w1));
    }
}
