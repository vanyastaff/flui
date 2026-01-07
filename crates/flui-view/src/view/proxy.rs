//! ProxyView - Single-child wrapper Views.
//!
//! ProxyViews are Views that have exactly one child and typically add
//! some behavior or configuration without creating a RenderObject.

use super::view::View;

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
pub trait ProxyView: Clone + Send + Sync + 'static + Sized {
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
                use $crate::element::ProxyBehavior;
                Box::new($crate::ProxyElement::new(self, ProxyBehavior))
            }

        }
    };
}

// NOTE: ProxyElement implementation has been moved to unified Element architecture.
// See crates/flui-view/src/element/unified.rs and element/behavior.rs
// The type alias is exported from element/mod.rs:
//   pub type ProxyElement<V> = Element<V, Single, ProxyBehavior>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{Lifecycle, ProxyBehavior};
    use crate::view::{ElementBase, View};
    use crate::ProxyElement;
    use flui_foundation::ElementId;
    use std::any::TypeId;

    // A dummy child view
    #[derive(Clone)]
    struct DummyChild;

    impl View for DummyChild {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(DummyChildElement)
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
            Box::new(ProxyElement::new(self, ProxyBehavior))
        }
    }

    #[test]
    fn test_proxy_element_creation() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let element = ProxyElement::new(&view, ProxyBehavior);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        // Child not created until build
    }

    #[test]
    fn test_proxy_element_mount() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let mut element = ProxyElement::new(&view, ProxyBehavior);
        element.mount(None, 0);

        assert_eq!(element.lifecycle(), Lifecycle::Active);
    }

    #[test]
    fn test_proxy_element_update() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let mut element = ProxyElement::new(&view, ProxyBehavior);
        element.mount(None, 0);

        let new_view = TestProxyView {
            child: DummyChild,
            enabled: false,
        };

        element.update(&new_view);
        // Element is marked dirty after update
    }

    #[test]
    fn test_proxy_element_unmount() {
        let view = TestProxyView {
            child: DummyChild,
            enabled: true,
        };

        let mut element = ProxyElement::new(&view, ProxyBehavior);
        element.mount(None, 0);
        element.unmount();

        assert_eq!(element.lifecycle(), Lifecycle::Defunct);
    }
}
