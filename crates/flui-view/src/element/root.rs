//! Root element handling for the element tree.
//!
//! Root elements are the entry points of element trees. They have special
//! requirements:
//! - No parent element
//! - Must be assigned a BuildOwner before mounting
//! - Responsible for propagating the owner to descendants

use std::sync::Arc;

use flui_foundation::ElementId;

use crate::owner::BuildOwner;

/// Trait for root elements that sit at the top of an element tree.
///
/// Root elements are special in that they:
/// - Have no parent
/// - Must have a BuildOwner assigned before mounting
/// - Propagate the BuildOwner to all descendants
///
/// The trait provides `assign_owner` to set the BuildOwner, and mounting
/// asserts the parent is `None`.
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

    fn set_depth(&mut self, depth: crate::view::ElementDepth) {
        self.depth = depth.get();
    }

    fn lifecycle(&self) -> crate::element::Lifecycle {
        self.lifecycle
    }

    fn mount(
        &mut self,
        parent: Option<ElementId>,
        slot: usize,
        _owner: &mut crate::ElementOwner<'_>,
    ) {
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

    fn unmount(&mut self, _owner: &mut crate::ElementOwner<'_>) {
        self.lifecycle = crate::element::Lifecycle::Defunct;
        self.child = None;
    }

    fn activate(&mut self, _owner: &mut crate::ElementOwner<'_>) {
        debug_assert!(
            self.lifecycle.can_activate(),
            "BUG: activate from {:?} — only an Inactive element may be \
             reactivated; Defunct in particular has disposed its state",
            self.lifecycle
        );
        self.lifecycle = crate::element::Lifecycle::Active;
    }

    fn deactivate(&mut self, _owner: &mut crate::ElementOwner<'_>) {
        debug_assert!(
            self.lifecycle.can_deactivate(),
            "BUG: deactivate from {:?} — only an Active element may be \
             deactivated",
            self.lifecycle
        );
        self.lifecycle = crate::element::Lifecycle::Inactive;
    }

    fn update(&mut self, _new_view: &dyn crate::view::View, _owner: &mut crate::ElementOwner<'_>) {
        // Root elements typically don't update from views
        self.mark_needs_build();
    }

    fn mark_needs_build(&mut self) {
        self.needs_build = true;
    }

    fn build_into_views(
        &mut self,
        _owner: &mut crate::ElementOwner<'_>,
    ) -> Vec<Box<dyn crate::view::View>> {
        self.needs_build = false;
        // This root has no *view* child to reconcile — its child is wired
        // by id (`set_child`). E3: child traversal is via the slab
        // `child_ids`, so there are no owned child views to return.
        Vec::new()
    }
}
