//! ProxyViewWrapper - Wrapper that holds a ProxyView
//!
//! Implements ViewObject for ProxyView types.

use std::any::Any;

use flui_element::{Element, IntoElement};
use flui_types::Event;

use crate::context::BuildContext;
use crate::object::ViewObject;
use crate::protocol::ViewMode;
use crate::traits::ProxyView;

/// Wrapper for ProxyView that implements ViewObject
///
/// Proxy views wrap a single child without affecting layout.
/// They add behavior, metadata, or event handling.
pub struct ProxyViewWrapper<V: ProxyView> {
    /// The proxy view
    view: V,

    /// Cached child element from last build
    child: Option<Element>,
}

impl<V: ProxyView> ProxyViewWrapper<V> {
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self { view, child: None }
    }

    /// Get reference to view
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Get mutable reference to view
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }

    /// Handle an event, returning true if consumed
    pub fn handle_event(&mut self, event: &Event, ctx: &dyn BuildContext) -> bool {
        self.view.handle_event(event, ctx)
    }
}

impl<V: ProxyView> std::fmt::Debug for ProxyViewWrapper<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyViewWrapper")
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl<V: ProxyView> ViewObject for ProxyViewWrapper<V> {
    fn mode(&self) -> ViewMode {
        ViewMode::Proxy
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Element {
        // Call lifecycle hooks
        self.view.before_child_build(ctx);

        // Build the child
        let child = self.view.build_child(ctx).into_element();
        self.child = Some(child);

        // Call lifecycle hooks
        self.view.after_child_build(ctx);

        // Return the cached child or empty
        self.child.take().unwrap_or_else(Element::empty)
    }

    fn init(&mut self, ctx: &dyn BuildContext) {
        self.view.init(ctx);
    }

    fn dispose(&mut self, ctx: &dyn BuildContext) {
        self.view.dispose(ctx);
        self.child = None;
    }

    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// IntoElement IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate ProxyView from other view types
///
/// Use `Proxy(my_view)` to create a proxy element.
pub struct Proxy<V: ProxyView>(pub V);

impl<V: ProxyView> IntoElement for Proxy<V> {
    fn into_element(self) -> Element {
        let wrapper = ProxyViewWrapper::new(self.0);
        Element::with_mode(wrapper, ViewMode::Proxy)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestProxyView {
        child: Element,
        events_handled: std::sync::atomic::AtomicUsize,
    }

    impl ProxyView for TestProxyView {
        fn build_child(&mut self, _ctx: &BuildContext) -> impl IntoElement {
            std::mem::replace(&mut self.child, Element::empty())
        }

        fn handle_event(&mut self, _event: &Event, _ctx: &BuildContext) -> bool {
            self.events_handled
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true // Consume all events
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = ProxyViewWrapper::new(TestProxyView {
            child: Element::empty(),
            events_handled: std::sync::atomic::AtomicUsize::new(0),
        });
        assert_eq!(wrapper.mode(), ViewMode::Proxy);
    }

    #[test]
    fn test_into_element() {
        let view = TestProxyView {
            child: Element::empty(),
            events_handled: std::sync::atomic::AtomicUsize::new(0),
        };
        let element = Proxy(view).into_element();
        assert!(element.has_view_object());
    }
}
