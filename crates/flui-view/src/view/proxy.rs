//! ProxyView - Single-child wrapper Views.
//!
//! ProxyViews are Views that have exactly one child and typically add
//! some behavior or configuration without creating a RenderObject.

use super::view::{ElementBase, View};
use crate::element::Lifecycle;
use flui_foundation::ElementId;
use std::any::TypeId;

/// A View that wraps a single child without creating a RenderObject.
///
/// ProxyViews are used for:
/// - Adding behavior (gesture detection, focus handling)
/// - Providing configuration (themes, localization)
/// - Composition without visual representation
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `ProxyWidget` and its subclasses like:
/// - `InheritedWidget` (though we have InheritedView separately)
/// - `ParentDataWidget`
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{ProxyView, BuildContext, View};
///
/// struct GestureDetector {
///     on_tap: Option<Box<dyn Fn() + Send + Sync>>,
///     child: Box<dyn View>,
/// }
///
/// impl ProxyView for GestureDetector {
///     fn child(&self) -> &dyn View {
///         &*self.child
///     }
/// }
/// ```
pub trait ProxyView: Send + Sync + 'static + Sized {
    /// Get the child View.
    fn child(&self) -> &dyn View;
}

/// Implement View for a ProxyView type.
///
/// This macro creates the View implementation for a ProxyView type.
///
/// ```rust,ignore
/// impl ProxyView for MyGestureDetector {
///     fn child(&self) -> &dyn View { &*self.child }
/// }
/// impl_proxy_view!(MyGestureDetector);
/// ```
#[macro_export]
macro_rules! impl_proxy_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                Box::new($crate::ProxyElement::new(self))
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
    };
}

// ============================================================================
// ProxyElement
// ============================================================================

/// Element for ProxyViews.
///
/// Manages the lifecycle of a ProxyView and its single child.
/// ProxyElements don't create RenderObjects themselves - they just
/// pass through to their child.
pub struct ProxyElement<V: ProxyView> {
    /// The current View configuration.
    view: V,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
    /// Child element.
    child: Option<Box<dyn ElementBase>>,
    /// Whether we need to rebuild.
    dirty: bool,
}

impl<V: ProxyView> ProxyElement<V>
where
    V: Clone,
{
    /// Create a new ProxyElement for the given View.
    pub fn new(view: &V) -> Self {
        Self {
            view: view.clone(),
            lifecycle: Lifecycle::Initial,
            depth: 0,
            child: None,
            dirty: true,
        }
    }

    /// Get a reference to the child element.
    pub fn child(&self) -> Option<&dyn ElementBase> {
        self.child.as_deref()
    }

    /// Get a mutable reference to the child element.
    pub fn child_mut(&mut self) -> Option<&mut dyn ElementBase> {
        self.child.as_deref_mut()
    }
}

impl<V: ProxyView + Clone> std::fmt::Debug for ProxyElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("dirty", &self.dirty)
            .field("has_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl<V: ProxyView + Clone> ElementBase for ProxyElement<V> {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn update(&mut self, new_view: &dyn View) {
        // Use View::as_any() for safe downcasting
        if let Some(v) = new_view.as_any().downcast_ref::<V>() {
            self.view = v.clone();
            self.dirty = true;
        }
    }

    fn mark_needs_build(&mut self) {
        self.dirty = true;
    }

    fn perform_build(&mut self) {
        if !self.dirty || !self.lifecycle.can_build() {
            return;
        }

        // In a full implementation, we would:
        // 1. Get the child View from view.child()
        // 2. Reconcile with existing child element
        // 3. Update or create child element
        self.dirty = false;
    }

    fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {
        self.lifecycle = Lifecycle::Active;
        self.dirty = true;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        if let Some(child) = &mut self.child {
            child.deactivate();
        }
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        if let Some(child) = &mut self.child {
            child.activate();
        }
    }

    fn unmount(&mut self) {
        self.lifecycle = Lifecycle::Defunct;
        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        // In a full implementation, we'd track child ElementIds
        let _ = visitor;
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A dummy child view
    #[derive(Clone)]
    struct DummyChild;

    impl View for DummyChild {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(DummyChildElement)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct DummyChildElement;

    impl ElementBase for DummyChildElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<DummyChild>()
        }
        fn lifecycle(&self) -> Lifecycle {
            Lifecycle::Active
        }
        fn update(&mut self, _: &dyn View) {}
        fn mark_needs_build(&mut self) {}
        fn perform_build(&mut self) {}
        fn mount(&mut self, _: Option<ElementId>, _: usize) {}
        fn deactivate(&mut self) {}
        fn activate(&mut self) {}
        fn unmount(&mut self) {}
        fn visit_children(&self, _: &mut dyn FnMut(ElementId)) {}
        fn depth(&self) -> usize {
            0
        }
    }

    /// A test proxy view (like GestureDetector)
    #[derive(Clone)]
    struct TestProxyView {
        child: DummyChild,
        enabled: bool,
    }

    impl ProxyView for TestProxyView {
        fn child(&self) -> &dyn View {
            &self.child
        }
    }

    impl View for TestProxyView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(ProxyElement::new(self))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_proxy_element_creation() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let element = ProxyElement::new(&view);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        assert!(element.child().is_none()); // Child not created until build
    }

    #[test]
    fn test_proxy_element_mount() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let mut element = ProxyElement::new(&view);
        element.mount(None, 0);

        assert_eq!(element.lifecycle(), Lifecycle::Active);
    }

    #[test]
    fn test_proxy_element_update() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let mut element = ProxyElement::new(&view);
        element.mount(None, 0);

        let new_view = TestProxyView {
            child: DummyChild,
            enabled: false,
        };

        element.update(&new_view);
        assert!(element.dirty);
    }

    #[test]
    fn test_proxy_element_unmount() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let mut element = ProxyElement::new(&view);
        element.mount(None, 0);
        element.unmount();

        assert_eq!(element.lifecycle(), Lifecycle::Defunct);
    }
}
