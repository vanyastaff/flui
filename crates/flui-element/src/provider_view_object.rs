//! ProviderViewObject trait - Extension for provider-specific operations
//!
//! This trait extends ViewObject with provider-specific methods.
//! Only ProviderViewWrapper implements this.

use std::any::Any;
use std::sync::Arc;

use crate::{ElementId, ViewObject};

/// Extension trait for ViewObjects that provide data to descendants.
///
/// Provides access to:
/// - The provided value (as Arc for sharing)
/// - List of dependent elements
/// - Notification logic
///
/// # Design
///
/// This is a separate trait (not part of base ViewObject) because:
/// 1. Only provider views need these methods
/// 2. Interface Segregation Principle
/// 3. Cleaner API - non-providers don't see these methods
///
/// # Implementors
///
/// - `ProviderViewWrapper<V, T>` - For ProviderView implementations
pub trait ProviderViewObject: ViewObject {
    /// Get provided value as Arc<dyn Any>.
    ///
    /// Returns an Arc to the value being provided to descendants.
    /// This allows sharing the value across multiple dependents without cloning.
    ///
    /// The provider implementation should wrap values in Arc internally.
    fn provided_value(&self) -> Arc<dyn Any + Send + Sync>;

    /// Get dependents list.
    ///
    /// Returns elements that depend on this provider's value.
    fn dependents(&self) -> &[ElementId];

    /// Get mutable dependents list.
    fn dependents_mut(&mut self) -> &mut Vec<ElementId>;

    /// Add a dependent element.
    fn add_dependent(&mut self, id: ElementId) {
        let deps = self.dependents_mut();
        if !deps.contains(&id) {
            deps.push(id);
        }
    }

    /// Remove a dependent element.
    fn remove_dependent(&mut self, id: ElementId) {
        self.dependents_mut().retain(|&dep| dep != id);
    }

    /// Check if dependents should be notified of value change.
    ///
    /// Called when the provider is updated to determine if
    /// dependent elements need to be rebuilt.
    fn should_notify_dependents(&self, old_value: &dyn Any) -> bool;
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Try to downcast a ViewObject to ProviderViewObject.
///
/// Returns None if the view object is not a provider.
pub fn as_provider(view_object: &dyn ViewObject) -> Option<&dyn ProviderViewObject> {
    if view_object.is_provider() {
        // The actual ProviderViewWrapper implements both ViewObject and ProviderViewObject
        // We need to downcast through the concrete type
        // This is handled by the specific wrapper types
        None // TODO: Implement via trait object coercion when available
    } else {
        None
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Test that the trait is object-safe
    fn _test_object_safe(_: &dyn ProviderViewObject) {}
}
