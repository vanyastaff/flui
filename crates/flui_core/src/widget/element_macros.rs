//! Macros for reducing boilerplate in Element implementations
//!
//! These macros help eliminate repetitive code in DynElement implementations,
//! particularly for single-child elements like ProxyElement, InheritedElement, etc.

/// Implement basic DynElement methods for single-child elements
///
/// This macro generates implementations for:
/// - `id()`, `parent()`, `key()`
/// - `is_dirty()`, `mark_dirty()`
/// - `lifecycle()`, `deactivate()`, `activate()`
/// - `children_iter()` (single child pattern)
/// - `widget_type_id()`, `render_object()`, `render_object_mut()`
///
/// # Requirements
///
/// The element type must have these fields:
/// - `id: ElementId`
/// - `parent: Option<ElementId>`
/// - `dirty: bool`
/// - `lifecycle: ElementLifecycle`
/// - `widget: W` (where W is the widget type)
/// - `child: Option<ElementId>`
///
/// # Example
///
/// ```rust,ignore
/// pub struct MyElement<W: MyWidget> {
///     id: ElementId,
///     parent: Option<ElementId>,
///     dirty: bool,
///     lifecycle: ElementLifecycle,
///     widget: W,
///     child: Option<ElementId>,
/// }
///
/// impl<W: MyWidget> DynElement for MyElement<W> {
///     impl_element_basics!(W);
///
///     // Custom methods...
///     fn mount(&mut self, parent: Option<ElementId>, slot: usize) {
///         // ...
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_element_basics {
    ($widget_type:ty) => {
        fn id(&self) -> $crate::ElementId {
            self.id
        }

        fn parent(&self) -> Option<$crate::ElementId> {
            self.parent
        }

        fn key(&self) -> Option<&dyn $crate::foundation::Key> {
            $crate::ProxyWidget::key(&self.widget)
        }

        #[inline]
        fn is_dirty(&self) -> bool {
            self.dirty
        }

        #[inline]
        fn mark_dirty(&mut self) {
            self.dirty = true;
        }

        fn lifecycle(&self) -> $crate::ElementLifecycle {
            self.lifecycle
        }

        fn deactivate(&mut self) {
            self.lifecycle = $crate::ElementLifecycle::Inactive;
        }

        fn activate(&mut self) {
            self.lifecycle = $crate::ElementLifecycle::Active;
        }

        fn children_iter(&self) -> Box<dyn Iterator<Item = $crate::ElementId> + '_> {
            Box::new(self.child.into_iter())
        }

        fn widget_type_id(&self) -> std::any::TypeId {
            std::any::TypeId::of::<$widget_type>()
        }

        fn render_object(&self) -> Option<&dyn $crate::DynRenderObject> {
            None
        }

        fn render_object_mut(&mut self) -> Option<&mut dyn $crate::DynRenderObject> {
            None
        }
    };
}

/// Implement single-child management methods
///
/// Generates implementations for:
/// - `set_tree_ref()`
/// - `take_old_child_for_rebuild()`
/// - `set_child_after_mount()`
/// - `forget_child()`
/// - `update_slot_for_child()` (no-op for single child)
/// - `did_change_dependencies()` (no-op by default)
///
/// # Requirements
///
/// The element type must have these fields:
/// - `tree: Option<Arc<RwLock<ElementTree>>>`
/// - `child: Option<ElementId>`
///
/// # Example
///
/// ```rust,ignore
/// impl<W: MyWidget> DynElement for MyElement<W> {
///     impl_element_basics!(W);
///     impl_single_child_management!();
///
///     // Other methods...
/// }
/// ```
#[macro_export]
macro_rules! impl_single_child_management {
    () => {
        fn set_tree_ref(&mut self, tree: std::sync::Arc<parking_lot::RwLock<$crate::ElementTree>>) {
            self.tree = Some(tree);
        }

        fn take_old_child_for_rebuild(&mut self) -> Option<$crate::ElementId> {
            self.child.take()
        }

        fn set_child_after_mount(&mut self, child_id: $crate::ElementId) {
            self.child = Some(child_id);
        }

        fn forget_child(&mut self, child_id: $crate::ElementId) {
            if self.child == Some(child_id) {
                self.child = None;
            }
        }

        fn update_slot_for_child(&mut self, _child_id: $crate::ElementId, _new_slot: usize) {
            // No-op for single child elements
        }

        fn did_change_dependencies(&mut self) {
            // Default: do nothing
        }
    };
}

/// Implement standard mount/unmount for single-child elements
///
/// Generates implementations for:
/// - `mount()` - sets parent, lifecycle, marks dirty
/// - `unmount()` - removes child from tree, sets lifecycle to Defunct
///
/// # Requirements
///
/// The element type must have:
/// - `parent: Option<ElementId>`
/// - `lifecycle: ElementLifecycle`
/// - `dirty: bool`
/// - `child: Option<ElementId>`
/// - `tree: Option<Arc<RwLock<ElementTree>>>`
///
/// # Example
///
/// ```rust,ignore
/// impl<W: MyWidget> DynElement for MyElement<W> {
///     impl_element_basics!(W);
///     impl_single_child_management!();
///     impl_single_child_mount_unmount!();
///
///     // Other methods...
/// }
/// ```
#[macro_export]
macro_rules! impl_single_child_mount_unmount {
    () => {
        fn mount(&mut self, parent: Option<$crate::ElementId>, _slot: usize) {
            self.parent = parent;
            self.lifecycle = $crate::ElementLifecycle::Active;
            self.dirty = true;
        }

        fn unmount(&mut self) {
            // Unmount child first
            if let Some(child_id) = self.child.take() {
                if let Some(tree) = &self.tree {
                    tree.write().remove(child_id);
                }
            }

            self.lifecycle = $crate::ElementLifecycle::Defunct;
        }
    };
}

/// Implement standard rebuild for single-child proxy elements
///
/// Generates implementation for `rebuild()` that:
/// - Returns early if not dirty
/// - Clones child widget from `self.widget.child()`
/// - Clears old child
/// - Returns vec with single child to mount
///
/// # Requirements
///
/// The widget type must implement `ProxyWidget::child()`.
///
/// # Example
///
/// ```rust,ignore
/// impl<W: ProxyWidget> DynElement for MyElement<W> {
///     impl_element_basics!(W);
///     impl_single_child_management!();
///     impl_single_child_mount_unmount!();
///     impl_proxy_rebuild!();
///
///     // Other methods...
/// }
/// ```
#[macro_export]
macro_rules! impl_proxy_rebuild {
    () => {
        fn rebuild(&mut self) -> Vec<($crate::ElementId, Box<dyn $crate::DynWidget>, usize)> {
            if !self.dirty {
                return Vec::new();
            }
            self.dirty = false;

            // ProxyWidget just wraps its child widget
            let child_widget: Box<dyn $crate::DynWidget> =
                dyn_clone::clone_box(self.widget.child());

            // Mark old child for unmounting
            self.child = None;

            // Return the child that needs to be mounted
            vec![(self.id, child_widget, 0)]
        }
    };
}

/// Complete DynElement implementation for basic single-child proxy elements
///
/// Combines all single-child element macros into one.
/// Use this for simple proxy elements that don't need custom behavior.
///
/// # Example
///
/// ```rust,ignore
/// pub struct SimpleProxy<W: ProxyWidget> {
///     id: ElementId,
///     parent: Option<ElementId>,
///     dirty: bool,
///     lifecycle: ElementLifecycle,
///     tree: Option<Arc<RwLock<ElementTree>>>,
///     widget: W,
///     child: Option<ElementId>,
/// }
///
/// impl<W: ProxyWidget + Widget<Element = SimpleProxy<W>>> DynElement for SimpleProxy<W> {
///     impl_single_child_proxy_element!(W);
///
///     // Only need to implement update_any() now
///     fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
///         // Custom update logic
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_single_child_proxy_element {
    ($widget_type:ty) => {
        impl_element_basics!($widget_type);
        impl_single_child_management!();
        impl_single_child_mount_unmount!();
        impl_proxy_rebuild!();
    };
}

#[cfg(test)]
mod tests {
    // Tests would go in the actual implementation files
    // This module just defines macros
}