//! `ProxyViewWrapper` - Wrapper that holds a `ProxyView`
//!
//! Implements `ViewObject` for `ProxyView` types.

use std::any::Any;

use crate::handle::ViewConfig;
use crate::traits::ProxyView;
use crate::{BuildContext, IntoView, IntoViewConfig, ViewMode, ViewObject};
use flui_interaction::events::Event;

/// Wrapper for `ProxyView` that implements `ViewObject`
///
/// Proxy views wrap a single child without affecting layout.
/// They add behavior, metadata, or event handling.
pub struct ProxyViewWrapper<V: ProxyView> {
    /// The proxy view
    view: V,
}

impl<V: ProxyView> ProxyViewWrapper<V> {
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self { view }
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

    /// Extract the inner view, consuming the wrapper.
    #[inline]
    pub fn into_inner(self) -> V {
        self.view
    }
}

impl<V: ProxyView> std::fmt::Debug for ProxyViewWrapper<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyViewWrapper").finish()
    }
}

impl<V: ProxyView> ViewObject for ProxyViewWrapper<V> {
    fn mode(&self) -> ViewMode {
        ViewMode::Proxy
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Call lifecycle hooks
        self.view.before_child_build(ctx);

        // Build the child
        let child = self.view.build_child(ctx).into_view();

        // Call lifecycle hooks
        self.view.after_child_build(ctx);

        Some(child)
    }

    fn init(&mut self, ctx: &dyn BuildContext) {
        self.view.init(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn BuildContext) {
        self.view.did_change_dependencies(ctx);
    }

    fn deactivate(&mut self, ctx: &dyn BuildContext) {
        self.view.deactivate(ctx);
    }

    fn activate(&mut self, ctx: &dyn BuildContext) {
        self.view.activate(ctx);
    }

    fn dispose(&mut self, ctx: &dyn BuildContext) {
        self.view.dispose(ctx);
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
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate `ProxyView` from other view types
///
/// Use `Proxy(my_view)` to create a proxy view object.
#[derive(Debug)]
pub struct Proxy<V: ProxyView>(pub V);

impl<V: ProxyView> IntoView for Proxy<V> {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(ProxyViewWrapper::new(self.0))
    }
}

// ============================================================================
// IntoViewConfig IMPLEMENTATION
// ============================================================================

/// Implementation for `ProxyViewWrapper`.
///
/// This allows proxy views to be converted to `ViewConfig` when wrapped:
///
/// ```rust,ignore
/// use flui_view::{Proxy, ProxyView, IntoViewConfig};
///
/// let config = ProxyViewWrapper::new(MyProxy { ... }).into_view_config();
/// ```
impl<V> IntoViewConfig for ProxyViewWrapper<V>
where
    V: ProxyView + Clone + Send + Sync + 'static,
{
    fn into_view_config(self) -> ViewConfig {
        let view = self.view;
        ViewConfig::new_with_factory(view, |v: &V| Box::new(ProxyViewWrapper::new(v.clone())))
    }
}

/// Implementation for `Proxy` helper.
///
/// ```rust,ignore
/// use flui_view::{Proxy, IntoViewConfig};
///
/// let config = Proxy(MyProxy { ... }).into_view_config();
/// ```
impl<V> IntoViewConfig for Proxy<V>
where
    V: ProxyView + Clone + Send + Sync + 'static,
{
    fn into_view_config(self) -> ViewConfig {
        ViewConfig::new_with_factory(self.0, |v: &V| Box::new(ProxyViewWrapper::new(v.clone())))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MockBuildContext;
    use flui_foundation::ElementId;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Helper for tests - represents an empty view
    struct EmptyIntoView;

    impl IntoView for EmptyIntoView {
        fn into_view(self) -> Box<dyn ViewObject> {
            Box::new(EmptyViewObject)
        }
    }

    struct EmptyViewObject;

    impl ViewObject for EmptyViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
            None
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    struct TestProxyView {
        events_handled: AtomicUsize,
    }

    impl ProxyView for TestProxyView {
        fn build_child(&mut self, _ctx: &dyn BuildContext) -> impl IntoView {
            EmptyIntoView
        }

        fn handle_event(&mut self, _event: &Event, _ctx: &dyn BuildContext) -> bool {
            self.events_handled.fetch_add(1, Ordering::Relaxed);
            true // Consume all events
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = ProxyViewWrapper::new(TestProxyView {
            events_handled: AtomicUsize::new(0),
        });
        assert_eq!(wrapper.mode(), ViewMode::Proxy);
    }

    #[test]
    fn test_into_view() {
        let view = TestProxyView {
            events_handled: AtomicUsize::new(0),
        };
        let view_obj = Proxy(view).into_view();
        assert_eq!(view_obj.mode(), ViewMode::Proxy);
    }

    #[test]
    fn test_build() {
        let mut wrapper = ProxyViewWrapper::new(TestProxyView {
            events_handled: AtomicUsize::new(0),
        });
        let ctx = MockBuildContext::new(ElementId::new(1));

        let result = wrapper.build(&ctx);
        assert!(result.is_some());
    }
}
