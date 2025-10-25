//! ProxyWidget - Base trait for widgets that wrap a single child
//!
//! ProxyWidget is used by widgets like InheritedWidget and ParentDataWidget
//! that don't create RenderObjects themselves but provide services to their
//! child widget.

use std::fmt;

use super::DynWidget;

/// ProxyWidget - widget that wraps a single child and provides services
///
/// ProxyWidgets are wrapper widgets that:
/// - Have exactly one child
/// - Don't create RenderObjects themselves
/// - Delegate layout/paint to their child
/// - Provide some service to the child or its descendants
///
/// # Examples of ProxyWidgets
///
/// - **InheritedWidget** - Propagates data down the tree
/// - **ParentDataWidget** - Attaches metadata to descendant RenderObjects
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{ProxyWidget, DynWidget};
///
/// #[derive(Debug, Clone)]
/// struct MyWrapper {
///     child: Box<dyn DynWidget>,
/// }
///
/// impl ProxyWidget for MyWrapper {
///     fn child(&self) -> &dyn DynWidget {
///         &*self.child
///     }
/// }
/// ```
///
/// # No Automatic Widget Implementation
///
/// Unlike StatelessWidget, ProxyWidget does NOT automatically implement
/// `Widget` and `DynWidget`. Subtypes like `InheritedWidget` and
/// `ParentDataWidget` have their own blanket implementations.
pub trait ProxyWidget: fmt::Debug + Clone + Send + Sync + 'static {
    /// Get the child widget
    ///
    /// Returns a reference to the single child widget that this proxy wraps.
    fn child(&self) -> &dyn DynWidget;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Widget, RenderObjectWidget, RenderObject, LeafArity, LayoutCx, PaintCx, RenderObjectKind};
    use flui_types::Size;
    use flui_engine::{BoxedLayer, ContainerLayer};

    // Test proxy widget
    #[derive(Debug)]
    struct TestProxy {
        child: Box<dyn DynWidget>,
    }

    impl Clone for TestProxy {
        fn clone(&self) -> Self {
            Self {
                child: self.child.clone(),
            }
        }
    }

    impl ProxyWidget for TestProxy {
        fn child(&self) -> &dyn DynWidget {
            &*self.child
        }
    }

    // Dummy child widget for testing
    #[derive(Debug, Clone)]
    struct DummyWidget;

    impl Widget for DummyWidget {
        type Kind = RenderObjectKind;
    }

    impl DynWidget for DummyWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    impl RenderObjectWidget for DummyWidget {
        type Arity = LeafArity;
        type Render = DummyRender;

        fn create_render_object(&self) -> Self::Render {
            DummyRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct DummyRender;

    impl RenderObject for DummyRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::ZERO)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_proxy_widget_creation() {
        let proxy = TestProxy {
            child: Box::new(DummyWidget),
        };

        // Verify child access
        let _child = proxy.child();
    }

    #[test]
    fn test_proxy_widget_clone() {
        let proxy = TestProxy {
            child: Box::new(DummyWidget),
        };

        let cloned = proxy.clone();
        let _child = cloned.child();
    }

    #[test]
    fn test_proxy_widget_debug() {
        let proxy = TestProxy {
            child: Box::new(DummyWidget),
        };

        let debug_str = format!("{:?}", proxy);
        assert!(debug_str.contains("TestProxy"));
    }
}
