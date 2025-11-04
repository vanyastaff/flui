//! Root element management
//!
//! The RootManager is responsible for:
//! - Setting and tracking the root element
//! - Inflating widgets into elements (planned for Phase 5)
//! - Managing root element lifecycle
//!
//! # Design
//!
//! RootManager has a SINGLE responsibility: manage the root element.
//! It does NOT:
//! - Build the tree (that's BuildPipeline's job)
//! - Store elements (that's ElementTree's job)
//! - Schedule builds (that's PipelineOwner's job)
//!
//! # Example
//!
//! ```rust,ignore
//! let mut root_mgr = RootManager::new();
//!
//! // Set root element
//! let root_id = root_mgr.set_root(&mut tree, element);
//!
//! // Get root ID
//! if let Some(id) = root_mgr.root_id() {
//!     println!("Root element: {:?}", id);
//! }
//! ```

use parking_lot::RwLock;
use std::sync::Arc;

use super::ElementTree;
use crate::element::{Element, ElementId};

/// Manages the root element of the element tree
///
/// This is a focused component with ONE responsibility: tracking and
/// managing the root element.
///
/// # Single Responsibility
///
/// RootManager ONLY manages the root element. It delegates:
/// - Element storage → ElementTree
/// - Build scheduling → PipelineOwner
/// - Element lifecycle → Element itself
///
/// # Example
///
/// ```rust,ignore
/// let tree = Arc::new(RwLock::new(ElementTree::new()));
/// let mut root_mgr = RootManager::new();
///
/// // Set root element
/// let component = ComponentElement::new(MyApp);
/// let root_id = root_mgr.set_root(&tree, Element::Component(component));
///
/// // Access root ID later
/// assert_eq!(root_mgr.root_id(), Some(root_id));
/// ```
#[derive(Debug, Default)]
pub struct RootManager {
    /// Root element ID
    root_id: Option<ElementId>,
}

impl RootManager {
    /// Create a new root manager
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let root_mgr = RootManager::new();
    /// ```
    pub fn new() -> Self {
        Self { root_id: None }
    }

    /// Get the root element ID
    ///
    /// # Returns
    ///
    /// The root element ID, or None if no root has been set.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(root_id) = root_mgr.root_id() {
    ///     println!("Root element: {:?}", root_id);
    /// }
    /// ```
    pub fn root_id(&self) -> Option<ElementId> {
        self.root_id
    }

    /// Mount an element as the root of the tree
    ///
    /// # Arguments
    ///
    /// - `tree`: The element tree to insert into
    /// - `root_element`: The element to set as root (typically ComponentElement or RenderElement)
    ///
    /// # Returns
    ///
    /// The ElementId of the root element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tree = Arc::new(RwLock::new(ElementTree::new()));
    /// let mut root_mgr = RootManager::new();
    ///
    /// let component = ComponentElement::new(MyApp);
    /// let root = Element::Component(component);
    /// let root_id = root_mgr.set_root(&tree, root);
    /// ```
    pub fn set_root(&mut self, tree: &Arc<RwLock<ElementTree>>, mut root_element: Element) -> ElementId {
        let mut tree_guard = tree.write();

        // Mount the element (no parent, slot 0)
        root_element.mount(None, Some(crate::foundation::Slot::new(0)));

        // Insert into tree
        let id = tree_guard.insert(root_element);

        // TODO(Phase 5): Call view.build() to create child

        drop(tree_guard);

        self.root_id = Some(id);

        id
    }

    /// Clear the root element
    ///
    /// Sets the root ID to None. Does NOT remove the element from the tree
    /// (that's the caller's responsibility if desired).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// root_mgr.clear_root();
    /// assert_eq!(root_mgr.root_id(), None);
    /// ```
    pub fn clear_root(&mut self) {
        self.root_id = None;
    }

    // Future: inflate_root() for View → Element conversion (Phase 5)
    // pub fn inflate_root(&mut self, tree: &Arc<RwLock<ElementTree>>, view: View) -> ElementId {
    //     let element = inflate_view(view, tree);
    //     self.set_root(tree, element)
    // }
}

// Tests removed - need to be rewritten with View API
