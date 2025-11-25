//! ProviderViewWrapper - Wrapper that holds a ProviderView
//!
//! Implements ViewObject for ProviderView types.

use std::any::Any;

use flui_element::{Element, ElementId, IntoElement};

use crate::context::BuildContext;
use crate::object::ViewObject;
use crate::protocol::ViewMode;
use crate::traits::ProviderView;

/// Wrapper for ProviderView that implements ViewObject
///
/// Provider views provide data to descendants via dependency injection.
/// Descendants register as dependents and get rebuilt when value changes.
pub struct ProviderViewWrapper<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    /// The provider view
    view: V,

    /// Cached child element from last build
    child: Option<Element>,

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
            child: None,
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

    /// Get the provided value
    pub fn value(&self) -> &T {
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
            .field("has_child", &self.child.is_some())
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

    fn build(&mut self, ctx: &dyn BuildContext) -> Element {
        // Build the child
        let child = self.view.build(ctx).into_element();
        self.child = Some(child);

        // Return the cached child or empty
        self.child.take().unwrap_or_else(Element::empty)
    }

    fn init(&mut self, ctx: &dyn BuildContext) {
        self.view.init(ctx);
    }

    fn did_update(&mut self, old_view: &dyn Any, _ctx: &dyn BuildContext) {
        // Check if we should notify dependents
        if let Some(old) = old_view.downcast_ref::<ProviderViewWrapper<V, T>>() {
            if self.view.should_notify(old.view.value()) {
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
        self.child = None;
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

    // ========== PROVIDER-SPECIFIC ==========

    fn provided_value(&self) -> Option<&(dyn Any + Send + Sync)> {
        Some(self.view.value())
    }

    fn dependents(&self) -> Option<&[ElementId]> {
        Some(&self.dependents)
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
// IntoElement IMPLEMENTATION
// ============================================================================

/// Helper struct to disambiguate ProviderView from other view types
///
/// Use `Provider::new(my_view)` to create a provider element.
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

impl<V, T> IntoElement for Provider<V, T>
where
    V: ProviderView<T>,
    T: Send + Sync + 'static,
{
    fn into_element(self) -> Element {
        let wrapper = ProviderViewWrapper::<V, T>::new(self.0);
        Element::new(wrapper)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTheme {
        primary_color: u32,
    }

    struct TestThemeProvider {
        theme: TestTheme,
        child: Element,
    }

    impl ProviderView<TestTheme> for TestThemeProvider {
        fn build(&mut self, _ctx: &BuildContext) -> impl IntoElement {
            std::mem::replace(&mut self.child, Element::empty())
        }

        fn value(&self) -> &TestTheme {
            &self.theme
        }

        fn should_notify(&self, old: &TestTheme) -> bool {
            self.theme.primary_color != old.primary_color
        }
    }

    #[test]
    fn test_wrapper_creation() {
        let wrapper = ProviderViewWrapper::new(TestThemeProvider {
            theme: TestTheme {
                primary_color: 0xFF0000,
            },
            child: Element::empty(),
        });
        assert_eq!(wrapper.mode(), ViewMode::Provider);
        assert_eq!(wrapper.value().primary_color, 0xFF0000);
    }

    #[test]
    fn test_dependents() {
        let mut wrapper = ProviderViewWrapper::new(TestThemeProvider {
            theme: TestTheme {
                primary_color: 0xFF0000,
            },
            child: Element::empty(),
        });

        let id = ElementId::new(1);
        wrapper.add_dependent(id);
        assert_eq!(wrapper.dependents().unwrap().len(), 1);

        wrapper.remove_dependent(id);
        assert_eq!(wrapper.dependents().unwrap().len(), 0);
    }

    #[test]
    fn test_into_element() {
        let view = TestThemeProvider {
            theme: TestTheme {
                primary_color: 0xFF0000,
            },
            child: Element::empty(),
        };
        let element = Provider(view).into_element();
        assert!(element.has_view_object());
    }
}
