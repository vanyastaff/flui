//! Hot reload support
//!
//! Provides infrastructure for Flutter-style hot reload during development.
//! When code changes, widgets are rebuilt while preserving state.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::hot_reload::reassemble_application;
//!
//! // In development mode, when code changes:
//! reassemble_application(&mut owner);
//!
//! // This will:
//! // 1. Call reassemble() on all State objects
//! // 2. Rebuild all widgets
//! // 3. Preserve existing state
//! ```

use crate::{BuildOwner, ElementTree};
use tracing::{info, debug};

/// Reassemble the entire application (hot reload)
///
/// This triggers a full rebuild of the widget tree while preserving state.
/// Call this when code changes during development to apply updates.
///
/// # What happens:
///
/// 1. **Mark all elements dirty** - Forces complete rebuild
/// 2. **Call reassemble() on all State objects** - Clears caches, updates state
/// 3. **Rebuild tree** - Applies new widget configurations
/// 4. **Preserve state** - State objects keep their data
///
/// # Example
///
/// ```rust,ignore
/// // When code changes:
/// reassemble_application(&mut build_owner);
///
/// // All widgets rebuild with new code
/// // State data preserved
/// ```
pub fn reassemble_application(owner: &mut BuildOwner) {
    info!("Starting hot reload (reassemble)");

    let tree = owner.tree().clone();
    let mut tree_guard = tree.write();

    // Step 1: Call reassemble() on all State objects
    reassemble_all_states(&mut tree_guard);

    // Step 2: Mark all elements dirty to force rebuild
    mark_all_dirty(&mut tree_guard, owner);

    drop(tree_guard);

    // Step 3: Rebuild all dirty elements
    owner.build_scope(|o| {
        o.flush_build();
    });

    info!("Hot reload complete");
}

/// Call reassemble() on all State objects in the tree
///
/// This allows State objects to clear caches and update themselves
/// when code changes.
fn reassemble_all_states(tree: &mut ElementTree) {
    debug!("Calling reassemble() on all State objects");

    // Collect all element IDs first (to avoid mutable borrow issues)
    let mut element_ids = Vec::new();
    tree.visit_all_elements(&mut |id, _element| {
        element_ids.push(id);
    });

    // Call reassemble() on each element
    // StatefulElement overrides this to call state.reassemble()
    // Other elements have no-op default implementation
    let mut reassembled_count = 0;
    for id in element_ids {
        if let Some(element) = tree.get_mut(id) {
            element.reassemble();
            reassembled_count += 1;
        }
    }

    debug!("Reassembled {} elements", reassembled_count);
}

/// Mark all elements dirty to force complete rebuild
fn mark_all_dirty(tree: &mut ElementTree, owner: &mut BuildOwner) {
    debug!("Marking all elements dirty for rebuild");

    let mut dirty_count = 0;

    // Collect all element IDs first (to avoid borrow issues)
    let mut element_ids = Vec::new();
    tree.visit_all_elements(&mut |id, _element| {
        // Note: We use depth 0 for all - BuildOwner will sort by actual depth during flush
        element_ids.push(id);
    });

    // Schedule all for rebuild
    for id in element_ids {
        owner.schedule_build_for(id, 0); // Depth 0 - will be sorted
        dirty_count += 1;
    }

    debug!("Marked {} elements dirty", dirty_count);
}

/// Check if hot reload is enabled
///
/// Hot reload should only be enabled in development builds.
/// In production, this returns false.
#[inline]
pub fn is_hot_reload_enabled() -> bool {
    cfg!(debug_assertions)
}

/// Reassemble a specific subtree
///
/// Like `reassemble_application()` but only affects a specific subtree.
/// Useful for incremental hot reload of changed modules.
///
/// # Parameters
///
/// - `owner`: The BuildOwner
/// - `root_id`: Root element ID of the subtree to reassemble
///
/// # Example
///
/// ```rust,ignore
/// // Reassemble just the counter widget subtree
/// reassemble_subtree(&mut owner, counter_element_id);
/// ```
pub fn reassemble_subtree(owner: &mut BuildOwner, root_id: crate::ElementId) {
    debug!("Starting subtree reassemble for element {:?}", root_id);

    let tree = owner.tree().clone();
    let tree_guard = tree.read();

    // Get the root element
    if tree_guard.get(root_id).is_none() {
        debug!("Element {:?} not found, skipping reassemble", root_id);
        return;
    }

    drop(tree_guard);

    // Mark root and all descendants dirty
    mark_subtree_dirty(owner, root_id);

    // Rebuild
    owner.build_scope(|o| {
        o.flush_build();
    });

    debug!("Subtree reassemble complete");
}

/// Mark a subtree dirty for rebuild
fn mark_subtree_dirty(owner: &mut BuildOwner, root_id: crate::ElementId) {
    // Schedule root (depth doesn't matter - BuildOwner will sort)
    owner.schedule_build_for(root_id, 0);

    // TODO: Traverse descendants and schedule them too
    // For now, just scheduling root is sufficient (children will rebuild via normal flow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DynWidget, StatelessWidget, Context};

    #[derive(Debug, Clone)]
    struct TestWidget {
        value: i32,
    }

    impl StatelessWidget for TestWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(TestWidget { value: self.value })
        }
    }

    #[test]
    fn test_is_hot_reload_enabled() {
        // In debug mode
        #[cfg(debug_assertions)]
        assert!(is_hot_reload_enabled());

        // Would be false in release mode
        #[cfg(not(debug_assertions))]
        assert!(!is_hot_reload_enabled());
    }

    #[test]
    fn test_reassemble_application() {
        let mut owner = BuildOwner::new();

        // Mount widget
        owner.set_root(Box::new(TestWidget { value: 42 }));

        // Build
        owner.build_scope(|o| o.flush_build());
        assert_eq!(owner.dirty_count(), 0);

        // Reassemble
        reassemble_application(&mut owner);

        // Tree should be clean after reassemble
        assert_eq!(owner.dirty_count(), 0);
    }

    #[test]
    fn test_reassemble_subtree() {
        let mut owner = BuildOwner::new();

        // Mount widget
        let root_id = owner.set_root(Box::new(TestWidget { value: 42 }));

        // Build
        owner.build_scope(|o| o.flush_build());

        // Reassemble subtree
        reassemble_subtree(&mut owner, root_id);

        // Should be clean
        assert_eq!(owner.dirty_count(), 0);
    }

    #[test]
    fn test_reassemble_nonexistent_element() {
        let mut owner = BuildOwner::new();
        let fake_id = crate::ElementId::new();

        // Should not panic
        reassemble_subtree(&mut owner, fake_id);
    }
}
