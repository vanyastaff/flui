//! Root element handling for the element tree.
//!
//! Root elements are the entry points of element trees. They have special
//! requirements:
//! - No parent element
//! - Must be assigned a BuildOwner before mounting
//! - Responsible for propagating the owner to descendants
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `RootElementMixin`.

use crate::owner::BuildOwner;
use flui_foundation::ElementId;
use std::sync::Arc;

/// Trait for root elements that sit at the top of an element tree.
///
/// Root elements are special in that they:
/// - Have no parent
/// - Must have a BuildOwner assigned before mounting
/// - Propagate the BuildOwner to all descendants
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `RootElementMixin` which provides:
/// - `assignOwner()` - set the BuildOwner
/// - `mount()` override that asserts parent is null
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{RootElement, BuildOwner, ElementBase};
///
/// struct MyRootElement {
///     owner: Option<Arc<BuildOwner>>,
///     // ... other fields
/// }
///
/// impl RootElement for MyRootElement {
///     fn assign_owner(&mut self, owner: Arc<BuildOwner>) {
///         self.owner = Some(owner);
///     }
///
///     fn owner(&self) -> Option<&Arc<BuildOwner>> {
///         self.owner.as_ref()
///     }
/// }
/// ```
pub trait RootElement: crate::view::ElementBase {
    /// Assign the BuildOwner to this root element.
    ///
    /// Must be called before `mount()`. The owner will be propagated
    /// to all descendants during the build phase.
    ///
    /// # Arguments
    ///
    /// * `owner` - The BuildOwner that manages the dirty elements list
    fn assign_owner(&mut self, owner: Arc<BuildOwner>);

    /// Get the BuildOwner assigned to this root element.
    fn owner(&self) -> Option<&Arc<BuildOwner>>;

    /// Mount this root element.
    ///
    /// This is the root-specific mount that:
    /// - Asserts no parent exists (root elements have no parent)
    /// - Asserts an owner has been assigned
    /// - Initializes the element tree from this point
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - A parent is provided (root elements must have no parent)
    /// - No owner has been assigned via `assign_owner()`
    fn mount_root(&mut self) {
        debug_assert!(
            self.owner().is_some(),
            "RootElement must have an owner assigned before mounting. Call assign_owner() first."
        );
    }
}

/// A concrete root element implementation.
///
/// This provides a base implementation for root elements that can be used
/// directly or as a reference for custom implementations.
#[derive(Debug)]
pub struct RootElementImpl {
    /// The BuildOwner managing this tree.
    owner: Option<Arc<BuildOwner>>,
    /// The child element.
    child: Option<ElementId>,
    /// Current lifecycle state.
    lifecycle: crate::element::Lifecycle,
    /// Depth in tree (always 0 for root).
    depth: usize,
    /// Whether this element needs a rebuild.
    needs_build: bool,
}

impl RootElementImpl {
    /// Create a new root element.
    pub fn new() -> Self {
        Self {
            owner: None,
            child: None,
            lifecycle: crate::element::Lifecycle::Initial,
            depth: 0,
            needs_build: true,
        }
    }

    /// Get the child element ID.
    pub fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Set the child element ID.
    pub fn set_child(&mut self, child: Option<ElementId>) {
        self.child = child;
    }
}

impl Default for RootElementImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl RootElement for RootElementImpl {
    fn assign_owner(&mut self, owner: Arc<BuildOwner>) {
        self.owner = Some(owner);
    }

    fn owner(&self) -> Option<&Arc<BuildOwner>> {
        self.owner.as_ref()
    }
}

impl crate::view::ElementBase for RootElementImpl {
    fn view_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<RootElementImpl>()
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn lifecycle(&self) -> crate::element::Lifecycle {
        self.lifecycle
    }

    fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
        // Root elements must have no parent
        debug_assert!(parent.is_none(), "Root element cannot have a parent");
        debug_assert!(slot == 0, "Root element slot must be 0");
        debug_assert!(
            self.owner.is_some(),
            "Root element must have owner assigned before mounting"
        );

        self.lifecycle = crate::element::Lifecycle::Active;
        self.needs_build = true;
    }

    fn unmount(&mut self) {
        self.lifecycle = crate::element::Lifecycle::Defunct;
        self.child = None;
    }

    fn activate(&mut self) {
        self.lifecycle = crate::element::Lifecycle::Active;
    }

    fn deactivate(&mut self) {
        self.lifecycle = crate::element::Lifecycle::Inactive;
    }

    fn update(&mut self, _new_view: &dyn crate::view::View) {
        // Root elements typically don't update from views
        self.mark_needs_build();
    }

    fn mark_needs_build(&mut self) {
        self.needs_build = true;
    }

    fn perform_build(&mut self) {
        self.needs_build = false;
        // Actual build logic would rebuild the child tree
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        if let Some(child) = self.child {
            visitor(child);
        }
    }


}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::ElementBase;

    #[test]
    fn test_root_element_creation() {
        let root = RootElementImpl::new();
        assert!(root.owner().is_none());
        assert!(root.child().is_none());
        assert_eq!(root.depth, 0);
    }

    #[test]
    fn test_root_element_assign_owner() {
        let mut root = RootElementImpl::new();
        let owner = Arc::new(BuildOwner::new());

        root.assign_owner(Arc::clone(&owner));

        assert!(root.owner().is_some());
    }

    #[test]
    fn test_root_element_mount() {
        let mut root = RootElementImpl::new();
        let owner = Arc::new(BuildOwner::new());

        root.assign_owner(owner);
        root.mount(None, 0);

        assert_eq!(root.lifecycle(), crate::element::Lifecycle::Active);
    }

    #[test]
    #[should_panic(expected = "Root element cannot have a parent")]
    fn test_root_element_mount_with_parent_panics() {
        let mut root = RootElementImpl::new();
        let owner = Arc::new(BuildOwner::new());

        root.assign_owner(owner);
        // This should panic - root elements can't have parents
        let fake_parent = ElementId::new(1);
        root.mount(Some(fake_parent), 0);
    }

    #[test]
    fn test_root_element_lifecycle() {
        let mut root = RootElementImpl::new();
        let owner = Arc::new(BuildOwner::new());

        root.assign_owner(owner);
        root.mount(None, 0);
        assert_eq!(root.lifecycle(), crate::element::Lifecycle::Active);

        root.deactivate();
        assert_eq!(root.lifecycle(), crate::element::Lifecycle::Inactive);

        root.activate();
        assert_eq!(root.lifecycle(), crate::element::Lifecycle::Active);

        root.unmount();
        assert_eq!(root.lifecycle(), crate::element::Lifecycle::Defunct);
    }

    #[test]
    fn test_root_element_child_management() {
        let mut root = RootElementImpl::new();
        assert!(root.child().is_none());

        let child_id = ElementId::new(42);
        root.set_child(Some(child_id));
        assert_eq!(root.child(), Some(child_id));

        root.set_child(None);
        assert!(root.child().is_none());
    }
}
