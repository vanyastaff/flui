//! ProxyWidget - Base for widgets that wrap a single child

use std::any::TypeId;
use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::{DynElement, DynWidget, Element, ElementId, ElementLifecycle, ElementTree};

/// Widget that wraps a single child and provides services
///
/// ProxyWidget is the base for widgets like InheritedWidget, ParentDataWidget, etc.
/// that wrap a single child and provide some service to that child or its descendants.
///
/// ProxyWidgets:
/// - Have exactly one child
/// - Don't create RenderObjects themselves
/// - Delegate layout/paint to their child
pub trait ProxyWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Get the child widget
    fn child(&self) -> &dyn DynWidget;

    /// Optional key for widget identification
    fn key(&self) -> Option<&dyn crate::foundation::Key> {
        None
    }

    /// Handle notification bubbling through this widget
    ///
    /// Called when a notification bubbles up through this element.
    /// Widgets like `NotificationListener` override this to intercept notifications.
    ///
    /// # Returns
    ///
    /// - `Some(true)` - Notification handled, stop bubbling
    /// - `Some(false)` - Notification handled, continue bubbling
    /// - `None` - This widget doesn't handle this notification type
    ///
    /// Default implementation returns `None` (don't handle).
    fn handle_notification(&self, _notification: &dyn crate::notification::AnyNotification) -> Option<bool> {
        None
    }
}

/// Element for ProxyWidget (delegates to single child)
pub struct ProxyElement<W: ProxyWidget> {    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    tree: Option<Arc<RwLock<ElementTree>>>,
    child: Option<ElementId>,
}

impl<W: ProxyWidget> ProxyElement<W> {
    pub fn new(widget: W) -> Self {
        Self {            widget,
            parent: None,
            dirty: true,
            lifecycle: ElementLifecycle::Initial,
            tree: None,
            child: None,
        }
    }

    /// Called when widget updates
    ///
    /// Subclasses can override this behavior by implementing custom update logic.
    /// This is called after the widget has been updated.
    pub fn updated(&mut self, _old_widget: &W) {
        self.notify_clients(_old_widget);
    }

    /// Notify dependents of changes
    ///
    /// Override point for subclasses to notify dependents.
    /// Default implementation does nothing.
    ///
    /// InheritedElement overrides this to notify dependent elements.
    pub fn notify_clients(&mut self, _old_widget: &W) {
        // Default: no-op
        // Subclasses like InheritedElement override this
    }

    /// Get the widget
    pub fn widget(&self) -> &W {
        &self.widget
    }
}

impl<W: ProxyWidget> fmt::Debug for ProxyElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyElement")
            .field("widget_type", &std::any::type_name::<W>())
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("lifecycle", &self.lifecycle)
            .field("child", &self.child)
            .finish()
    }
}

// ========== Implement DynElement for ProxyElement ==========

impl<W: ProxyWidget + crate::Widget<Element = ProxyElement<W>>> DynElement for ProxyElement<W> {    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn crate::foundation::Key> {
        ProxyWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Unmount child first
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                tree.write().remove(child_id);
            }
        }

        self.lifecycle = ElementLifecycle::Defunct;
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        // Try to downcast to our widget type
        if let Ok(new_widget_typed) = new_widget.downcast::<W>() {
            let old_widget = std::mem::replace(&mut self.widget, *new_widget_typed);
            self.updated(&old_widget);
        }
    }

    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, Box<dyn DynWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // ProxyWidget just wraps its child widget
        let child_widget: Box<dyn DynWidget> = dyn_clone::clone_box(self.widget.child());

        // Mark old child for unmounting
        self.child = None;

        // Return the child that needs to be mounted
        vec![(element_id, child_widget, 0)]
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        self.child.take()
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    fn widget_type_id(&self) -> TypeId {
        TypeId::of::<W>()
    }

    fn widget(&self) -> &dyn crate::DynWidget {
        &self.widget
    }

    fn render_object(&self) -> Option<&dyn crate::DynRenderObject> {
        None // ProxyElement doesn't have RenderObject
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::DynRenderObject> {
        None // ProxyElement doesn't have RenderObject
    }

    fn did_change_dependencies(&mut self) {
        // Default: do nothing
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Default: do nothing (single child)
    }

    fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }

    // ========== Notification System ==========

    fn visit_notification(&self, notification: &dyn crate::notification::AnyNotification) -> bool {
        // Ask widget if it wants to handle this notification
        if let Some(should_stop) = self.widget.handle_notification(notification) {
            return should_stop;
        }

        // Widget didn't handle it, continue bubbling
        false
    }
}

// ========== Implement Element for ProxyElement ==========

impl<W: ProxyWidget + crate::Widget<Element = ProxyElement<W>>> Element for ProxyElement<W> {
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        let old_widget = std::mem::replace(&mut self.widget, new_widget);
        self.updated(&old_widget);
    }

    fn widget(&self) -> &W {
        &self.widget
    }
}

/// Macro to implement Widget for ProxyWidget types
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyProxy {
///     child: Box<dyn DynWidget>,
/// }
///
/// impl ProxyWidget for MyProxy {
///     fn child(&self) -> &dyn DynWidget {
///         &*self.child
///     }
/// }
///
/// impl_widget_for_proxy!(MyProxy);
/// ```
#[macro_export]
macro_rules! impl_widget_for_proxy {
    ($widget_type:ty) => {
        impl $crate::Widget for $widget_type {
            type Element = $crate::ProxyElement<$widget_type>;

            fn into_element(self) -> Self::Element {
                $crate::ProxyElement::new(self)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, StatelessWidget};

    // Test proxy widget
    #[derive(Debug, Clone)]
    struct TestProxy {
        value: i32,
        child: Box<dyn DynWidget>,
    }

    impl ProxyWidget for TestProxy {
        fn child(&self) -> &dyn DynWidget {
            &*self.child
        }
    }

    impl_widget_for_proxy!(TestProxy);

    // Dummy child widget
    #[derive(Debug, Clone)]
    struct ChildWidget;

    impl StatelessWidget for ChildWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(ChildWidget)
        }
    }

    #[test]
    fn test_proxy_widget_create_element() {
        let widget = TestProxy {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let element = widget.create_element();

        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_proxy_element_mount() {
        let widget = TestProxy {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ProxyElement::new(widget);

        let parent_id = unsafe { ElementId::from_raw(100) };
        element.mount(Some(parent_id), 0);

        assert_eq!(element.parent(), Some(parent_id));
        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
    }

    #[test]
    fn test_proxy_element_update() {
        let widget1 = TestProxy {
            value: 1,
            child: Box::new(ChildWidget),
        };
        let mut element = ProxyElement::new(widget1);

        let widget2 = TestProxy {
            value: 2,
            child: Box::new(ChildWidget),
        };
        element.update(widget2);

        assert_eq!(element.widget().value, 2);
    }

    #[test]
    fn test_proxy_element_rebuild() {
        let widget = TestProxy {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ProxyElement::new(widget);

        let updates = element.rebuild();

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, element.id());
        assert!(!element.is_dirty());
    }

    #[test]
    fn test_proxy_element_unmount() {
        let widget = TestProxy {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ProxyElement::new(widget);
        element.mount(None, 0);

        element.unmount();

        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }

    #[test]
    fn test_proxy_element_lifecycle() {
        let widget = TestProxy {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ProxyElement::new(widget);

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

        element.mount(None, 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }

    #[test]
    fn test_proxy_element_children_iter() {
        let widget = TestProxy {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ProxyElement::new(widget);

        // No child initially
        let children: Vec<_> = element.children_iter().collect();
        assert_eq!(children.len(), 0);

        // Set child
        let child_id = ElementId::new();
        element.set_child_after_mount(child_id);

        let children: Vec<_> = element.children_iter().collect();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child_id);
    }

    #[test]
    fn test_proxy_element_forget_child() {
        let widget = TestProxy {
            value: 42,
            child: Box::new(ChildWidget),
        };
        let mut element = ProxyElement::new(widget);

        let child_id = ElementId::new();
        element.set_child_after_mount(child_id);

        element.forget_child(child_id);

        let children: Vec<_> = element.children_iter().collect();
        assert_eq!(children.len(), 0);
    }
}
