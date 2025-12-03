//! `ProviderViewWrapper` - Wrapper that holds a `ProviderView`
//!
//! Implements `ViewObject` for `ProviderView` types.

use std::any::Any;
use std::sync::Arc;

use flui_foundation::ElementId;

use crate::traits::ProviderView;
use crate::{BuildContext, IntoView, ViewMode, ViewObject};

/// Wrapper for `ProviderView` that implements `ViewObject`
///
/// Provider views provide data to descendants via dependency injection.
/// Descendants register as dependents and get rebuilt when value changes.
///
/// # Value Storage
///
/// The provided value is wrapped in `Arc<T>` for efficient sharing across dependents.
/// The `ProviderView` trait returns `Arc<T>` directly.
pub struct ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    /// The provider view
    view: V,

    /// Elements that depend on this provider's value
    dependents: Vec<ElementId>,

    /// Type marker for the provided value
    _marker: std::marker::PhantomData<T>,
}

impl<V, T> ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    /// Create a new wrapper
    pub fn new(view: V) -> Self {
        Self {
            view,
            dependents: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Get reference to view
    pub fn view(&self) -> &V {
        &self.view
    }

    /// Get mutable reference to view
    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }

    /// Get the provided value (as `Arc`)
    pub fn value(&self) -> Arc<T> {
        self.view.value()
    }

    /// Register a dependent element
    pub fn add_dependent(&mut self, id: ElementId) {
        if !self.dependents.contains(&id) {
            self.dependents.push(id);
        }
    }

    /// Unregister a dependent element
    pub fn remove_dependent(&mut self, id: ElementId) {
        self.dependents.retain(|&dep| dep != id);
    }
}

impl<V, T> std::fmt::Debug for ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderViewWrapper")
            .field("dependents_count", &self.dependents.len())
            .finish()
    }
}

impl<V, T> ViewObject for ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    fn mode(&self) -> ViewMode {
        ViewMode::Provider
    }

    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Build the child
        Some(self.view.build(ctx).into_view())
    }

    fn init(&mut self, ctx: &dyn BuildContext) {
        self.view.init(ctx);
    }

    fn did_update(&mut self, old_view: &dyn Any, _ctx: &dyn BuildContext) {
        // Check if we should notify dependents
        if let Some(old) = old_view.downcast_ref::<ProviderViewWrapper<V, T>>() {
            let old_value = old.view.value();
            if self.view.should_notify(&*old_value) {
                // Dependents will be rebuilt by the framework
                // Just mark that notification is needed
                tracing::debug!(
                    "Provider {} should notify {} dependents",
                    std::any::type_name::<T>(),
                    self.dependents.len()
                );
            }
        }
    }

    fn dispose(&mut self, ctx: &dyn BuildContext) {
        self.view.dispose(ctx);
        self.dependents.clear();
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

    // ========== PROVIDER METHODS ==========

    fn provided_value(&self) -> Option<Arc<dyn Any + Send + Sync>> {
        // Get Arc<T> from view and upcast to Arc<dyn Any>
        let arc_t = self.view.value();
        Some(arc_t as Arc<dyn Any + Send + Sync>)
    }

    fn dependents(&self) -> &[ElementId] {
        &self.dependents
    }

    fn dependents_mut(&mut self) -> Option<&mut Vec<ElementId>> {
        Some(&mut self.dependents)
    }

    fn should_notify_dependents(&self, old_value: &dyn Any) -> bool {
        if let Some(old) = old_value.downcast_ref::<T>() {
            self.view.should_notify(old)
        } else {
            true // If types don't match, always notify
        }
    }
}

// ============================================================================
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate `ProviderView` from other view types
///
/// Use `Provider::new(my_view)` to create a provider view object.
#[derive(Debug)]
pub struct Provider<V, T>(pub V, std::marker::PhantomData<T>)
where
    V: ProviderView<T>,
    T: Send + Sync + 'static;

impl<V, T> Provider<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    /// Create a new Provider wrapper
    pub fn new(view: V) -> Self {
        Self(view, std::marker::PhantomData)
    }
}

impl<V, T> IntoView for Provider<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(ProviderViewWrapper::<V, T>::new(self.0))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MockBuildContext;

    #[derive(Clone)]
    struct TestTheme {
        primary_color: u32,
    }

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

    struct TestThemeProvider {
        theme: Arc<TestTheme>,
    }

    impl ProviderView<TestTheme> for TestThemeProvider {
        fn build(&mut self, _ctx: &dyn BuildContext) -> impl IntoView {
            EmptyIntoView
        }

        fn value(&self) -> Arc<TestTheme> {
            self.theme.clone()
        }

        fn should_notify(&self, old: &TestTheme) -> bool {
            self.theme.primary_color != old.primary_color
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = ProviderViewWrapper::new(TestThemeProvider {
            theme: Arc::new(TestTheme {
                primary_color: 0x00FF_0000,
            }),
        });
        assert_eq!(wrapper.mode(), ViewMode::Provider);
        assert_eq!(wrapper.value().primary_color, 0x00FF_0000);
    }

    #[test]
    fn test_dependents() {
        let mut wrapper = ProviderViewWrapper::new(TestThemeProvider {
            theme: Arc::new(TestTheme {
                primary_color: 0x00FF_0000,
            }),
        });

        let id = ElementId::new(1);
        wrapper.add_dependent(id);
        assert_eq!(ViewObject::dependents(&wrapper).len(), 1);

        wrapper.remove_dependent(id);
        assert_eq!(ViewObject::dependents(&wrapper).len(), 0);
    }

    #[test]
    fn test_into_view() {
        let view = TestThemeProvider {
            theme: Arc::new(TestTheme {
                primary_color: 0x00FF_0000,
            }),
        };
        let view_obj = Provider::new(view).into_view();
        assert_eq!(view_obj.mode(), ViewMode::Provider);
    }

    #[test]
    fn test_build() {
        let mut wrapper = ProviderViewWrapper::new(TestThemeProvider {
            theme: Arc::new(TestTheme {
                primary_color: 0x00FF_0000,
            }),
        });
        let ctx = MockBuildContext::new(ElementId::new(1));

        let result = wrapper.build(&ctx);
        assert!(result.is_some());
    }
}
