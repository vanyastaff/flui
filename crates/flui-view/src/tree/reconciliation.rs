//! O(N) Linear child reconciliation.
//!
//! Flutter insight: "Contrary to popular belief, Flutter does not employ a tree-diffing algorithm"
//!
//! This module provides O(N) linear reconciliation for children, matching
//! old and new Views efficiently using keys and position.

use crate::tree::ElementTree;
use crate::view::View;
use flui_foundation::ElementId;
use std::collections::HashMap;

/// Result of reconciling a single child.
#[derive(Debug)]
#[allow(dead_code)] // Will be used when full reconciliation is implemented
pub enum ReconcileAction {
    /// Keep the existing element and update it.
    Update(ElementId),
    /// Create a new element.
    Create,
    /// Remove the old element.
    Remove(ElementId),
    /// Move an existing element to a new slot.
    Move(ElementId, usize),
}

/// Reconcile old children with new Views.
///
/// This is an O(N) linear algorithm, NOT a tree-diff algorithm.
///
/// # Algorithm
///
/// 1. Match beginning of both lists (same key/type in same position)
/// 2. Match end of both lists
/// 3. Put remaining old children in HashMap by key
/// 4. Walk remaining new views, lookup by key
/// 5. Create new elements for unmatched, deactivate old unmatched
///
/// # Arguments
///
/// * `tree` - The element tree
/// * `parent` - Parent element ID
/// * `old_children` - Current child ElementIds
/// * `new_views` - New Views to reconcile against
///
/// # Returns
///
/// Updated list of child ElementIds in the new order.
pub fn reconcile_children(
    tree: &mut ElementTree,
    parent: ElementId,
    old_children: &[ElementId],
    new_views: &[&dyn View],
) -> Vec<ElementId> {
    // Fast path: empty lists
    if old_children.is_empty() && new_views.is_empty() {
        return Vec::new();
    }

    // Fast path: all new
    if old_children.is_empty() {
        return new_views
            .iter()
            .enumerate()
            .map(|(slot, view)| tree.insert(*view, parent, slot))
            .collect();
    }

    // Fast path: all removed
    if new_views.is_empty() {
        for &child_id in old_children {
            tree.remove(child_id);
        }
        return Vec::new();
    }

    let mut result = Vec::with_capacity(new_views.len());
    let mut old_index = 0;
    let mut new_index = 0;
    let old_len = old_children.len();
    let new_len = new_views.len();

    // HashMap for keyed elements
    let mut old_keyed: HashMap<u64, ElementId> = HashMap::new();
    let mut used_old: Vec<bool> = vec![false; old_len];

    // Build map of keyed old children
    for (i, &child_id) in old_children.iter().enumerate() {
        if let Some(node) = tree.get(child_id) {
            // Check if the element's view had a key
            // For now, we don't have direct access to the original View's key
            // This would need enhancement to store keys in ElementNode
            let _ = (i, node);
        }
    }

    // Phase 1: Match from start
    while old_index < old_len && new_index < new_len {
        let old_id = old_children[old_index];
        let new_view = new_views[new_index];

        if can_update_element(tree, old_id, new_view) {
            // Same type, update in place
            tree.update(old_id, new_view);
            result.push(old_id);
            used_old[old_index] = true;
            old_index += 1;
            new_index += 1;
        } else {
            break;
        }
    }

    // Phase 2: Match from end
    let mut old_end = old_len;
    let mut new_end = new_len;
    let mut end_matches: Vec<(ElementId, usize)> = Vec::new();

    while old_end > old_index && new_end > new_index {
        let old_id = old_children[old_end - 1];
        let new_view = new_views[new_end - 1];

        if can_update_element(tree, old_id, new_view) {
            end_matches.push((old_id, new_end - 1));
            used_old[old_end - 1] = true;
            old_end -= 1;
            new_end -= 1;
        } else {
            break;
        }
    }

    // Phase 3: Build map of remaining keyed old children
    for i in old_index..old_end {
        if !used_old[i] {
            let old_id = old_children[i];
            if let Some(node) = tree.get(old_id) {
                // Use view_type_id as a simple key for now
                let key = node.element().view_type_id();
                old_keyed.insert(hash_type_id(&key), old_id);
            }
        }
    }

    // Phase 4: Process middle section
    for (offset, &new_view) in new_views[new_index..new_end].iter().enumerate() {
        let slot = new_index + offset;

        // Try to find matching old element by key
        let key_hash = if let Some(key) = new_view.key() {
            key.key_hash()
        } else {
            hash_type_id(&new_view.view_type_id())
        };

        if let Some(&old_id) = old_keyed.get(&key_hash) {
            if can_update_element(tree, old_id, new_view) {
                // Found match, update and reuse
                tree.update(old_id, new_view);
                result.push(old_id);
                old_keyed.remove(&key_hash);

                // Mark as used
                if let Some(idx) = old_children.iter().position(|&id| id == old_id) {
                    used_old[idx] = true;
                }
                continue;
            }
        }

        // No match found, create new element
        let new_id = tree.insert(new_view, parent, slot);
        result.push(new_id);
    }

    // Add end matches (in reverse order since we collected them backwards)
    for (old_id, slot) in end_matches.into_iter().rev() {
        tree.update(old_id, new_views[slot]);
        result.push(old_id);
    }

    // Phase 5: Remove unused old elements
    for (i, &was_used) in used_old.iter().enumerate() {
        if !was_used {
            tree.remove(old_children[i]);
        }
    }

    result
}

/// Check if an element can be updated with the given view.
fn can_update_element(tree: &ElementTree, element_id: ElementId, view: &dyn View) -> bool {
    if let Some(node) = tree.get(element_id) {
        node.element().view_type_id() == view.view_type_id()
    } else {
        false
    }
}

/// Hash a TypeId for use in HashMap.
fn hash_type_id(type_id: &std::any::TypeId) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    type_id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildContext, StatelessElement, StatelessView, View};

    #[derive(Clone)]
    struct TestView {
        id: u32,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(self.clone())
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn crate::ElementBase> {
            Box::new(StatelessElement::new(self))
        }

    }

    #[test]
    fn test_reconcile_empty_to_empty() {
        let mut tree = ElementTree::new();
        let root = TestView { id: 0 };
        let parent = tree.mount_root(&root);

        let result = reconcile_children(&mut tree, parent, &[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_reconcile_empty_to_some() {
        let mut tree = ElementTree::new();
        let root = TestView { id: 0 };
        let parent = tree.mount_root(&root);

        let v1 = TestView { id: 1 };
        let v2 = TestView { id: 2 };
        let new_views: Vec<&dyn View> = vec![&v1, &v2];

        let result = reconcile_children(&mut tree, parent, &[], &new_views);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_reconcile_some_to_empty() {
        let mut tree = ElementTree::new();
        let root = TestView { id: 0 };
        let parent = tree.mount_root(&root);

        let v1 = TestView { id: 1 };
        let v2 = TestView { id: 2 };

        let child1 = tree.insert(&v1, parent, 0);
        let child2 = tree.insert(&v2, parent, 1);

        let result = reconcile_children(&mut tree, parent, &[child1, child2], &[]);
        assert!(result.is_empty());
        assert!(!tree.contains(child1));
        assert!(!tree.contains(child2));
    }

    #[test]
    fn test_reconcile_same_length() {
        let mut tree = ElementTree::new();
        let root = TestView { id: 0 };
        let parent = tree.mount_root(&root);

        let v1 = TestView { id: 1 };
        let v2 = TestView { id: 2 };

        let child1 = tree.insert(&v1, parent, 0);
        let child2 = tree.insert(&v2, parent, 1);

        // Update with same types
        let v1_new = TestView { id: 10 };
        let v2_new = TestView { id: 20 };
        let new_views: Vec<&dyn View> = vec![&v1_new, &v2_new];

        let result = reconcile_children(&mut tree, parent, &[child1, child2], &new_views);

        // Should reuse existing elements
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], child1);
        assert_eq!(result[1], child2);
    }
}
