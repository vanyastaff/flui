//! Core Context struct and basic methods
//!
//! The Context struct is the primary interface for widgets to interact
//! with the framework during build.

use std::fmt;
use std::sync::Arc;
use parking_lot::RwLock;

use crate::{Element, ElementId};
use crate::tree::ElementTree;

/// Build context provides access to the element tree and framework services
///
/// Rust-idiomatic name for Flutter's BuildContext. Passed to build() methods.
///
/// Context is cheap to clone - it contains only Arc references to shared data.
#[derive(Clone)]
pub struct Context {
    /// Reference to the element tree
    pub(super) tree: Arc<RwLock<ElementTree>>,

    /// ID of the current element
    pub(super) element_id: ElementId,
}

impl Context {
    /// Create a new build context
    ///
    /// Internal API used by the framework.
    pub fn new(tree: Arc<RwLock<ElementTree>>, element_id: ElementId) -> Self {
        Self { tree, element_id }
    }

    /// Create an empty context
    ///
    /// Use this only when you don't have access to a real ElementTree.
    pub fn empty() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let element_id = ElementId::new();
        Self { tree, element_id }
    }

    /// Create a minimal context for testing
    #[cfg(test)]
    pub fn test() -> Self {
        Self::empty()
    }

    /// Get the element ID this context belongs to
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Get a reference to the element tree
    ///
    /// Internal API - prefer using higher-level methods.
    pub(crate) fn tree(&self) -> parking_lot::RwLockReadGuard<'_, ElementTree> {
        self.tree.read()
    }

    /// Get mutable reference to element tree
    ///
    /// Internal API
    pub(crate) fn tree_mut(&self) -> parking_lot::RwLockWriteGuard<'_, ElementTree> {
        self.tree.write()
    }

    /// Get parent element ID
    pub fn parent(&self) -> Option<ElementId> {
        let tree = self.tree();
        tree.get(self.element_id)
            .and_then(|element| element.parent())
    }

    /// Check if this context is still valid
    ///
    /// A context becomes invalid if its element has been unmounted.
    pub fn is_valid(&self) -> bool {
        let tree = self.tree();
        tree.get(self.element_id).is_some()
    }

    /// Check if this element is currently mounted
    ///
    /// Similar to Flutter's `mounted` property on State.
    pub fn mounted(&self) -> bool {
        self.is_valid()
    }

    /// Get debug info
    pub fn debug_info(&self) -> String {
        let tree = self.tree();

        if let Some(element) = tree.get(self.element_id) {
            let parent_str = match element.parent() {
                Some(parent_id) => format!("Some({})", parent_id),
                None => "None (root)".to_string(),
            };

            format!(
                "Context {{ element_id: {}, parent: {}, dirty: {} }}",
                self.element_id,
                parent_str,
                element.is_dirty()
            )
        } else {
            format!("Context {{ element_id: {} (invalid) }}", self.element_id)
        }
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("element_id", &self.element_id)
            .field("valid", &self.is_valid())
            .finish()
    }
}

// Backward compatibility alias
pub type BuildContext = Context;
