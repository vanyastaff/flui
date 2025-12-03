//! ViewObject trait - Dynamic dispatch for view lifecycle
//!
//! ViewObject is the core trait that all view wrappers implement.
//! It's stored inside Element as `Box<dyn ViewObject>` for type erasure.
//!
//! # Architecture
//!
//! ```text
//! View (immutable config)
//!   ↓ implement ViewObject
//! ViewObject (dynamic dispatch)
//!   ├─ Component views: Stateless, Stateful, Proxy, Animated
//!   │   └─ build() returns child ViewObject via IntoView
//!   └─ Provider views: Inherited data provider
//!       └─ build() returns child ViewObject, has provided_value(), dependents()
//!
//! Render views are handled by RenderViewObject in flui_rendering.
//! ```
//!
//! # Design Principles
//!
//! This trait is intentionally minimal and has NO dependencies on:
//! - flui-element (Element, ElementTree, etc.)
//! - flui_rendering (RenderObject, RenderState, etc.)
//! - flui_painting (Canvas)

use std::any::Any;
use std::sync::Arc;

use flui_foundation::ElementId;

use crate::{BuildContext, ViewMode};

/// ViewObject - Core dynamic dispatch interface for view lifecycle
///
/// This trait defines the operations that ALL view types support.
/// Wrappers (StatelessViewWrapper, StatefulViewWrapper, etc.) implement this.
///
/// # Thread Safety
///
/// ViewObject requires `Send + Sync` for cross-thread element transfer.
///
/// # Lifecycle
///
/// 1. `build()` - Create child view object(s)
/// 2. `init()` - Called after element is mounted
/// 3. `did_change_dependencies()` - Called when inherited values change
/// 4. `did_update()` - Called when view is updated with new config
/// 5. `deactivate()` - Called when element is temporarily removed
/// 6. `dispose()` - Called when element is permanently removed
///
/// # Render Views
///
/// For render-specific methods (layout, paint, hit_test), see
/// `RenderViewObject` trait in `flui_rendering::view`.
pub trait ViewObject: Send + Sync + 'static {
    // ========== CORE METHODS (required) ==========

    /// Get the view mode (Stateless, Stateful, RenderBox, etc.)
    fn mode(&self) -> ViewMode;

    /// Build this view, producing child view object(s)
    ///
    /// Called during the build phase to create/update children.
    ///
    /// # Returns
    ///
    /// For component views: Returns the child view object (wrapped in Option)
    /// For render views: Returns None (render views don't have logical children built this way)
    ///
    /// # Note
    ///
    /// The return type is `Option<Box<dyn ViewObject>>` to avoid circular
    /// dependency between flui-view and flui-element. The Element wrapper
    /// in flui-element handles converting this to an Element.
    fn build(&mut self, ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>>;

    // ========== LIFECYCLE (with defaults) ==========

    /// Initialize after first mount
    ///
    /// Called once after element is mounted to tree.
    fn init(&mut self, _ctx: &dyn BuildContext) {}

    /// Called when dependencies change
    ///
    /// For views that depend on inherited data.
    fn did_change_dependencies(&mut self, _ctx: &dyn BuildContext) {}

    /// Update with new view configuration
    ///
    /// Called when parent rebuilds with same view type but different props.
    fn did_update(&mut self, _old_view: &dyn Any, _ctx: &dyn BuildContext) {}

    /// Called when element is deactivated (moved to cache)
    fn deactivate(&mut self, _ctx: &dyn BuildContext) {}

    /// Called when element is permanently removed
    ///
    /// Clean up resources here.
    fn dispose(&mut self, _ctx: &dyn BuildContext) {}

    // ========== RENDER STATE (for render views) ==========

    /// Returns the render state for render views.
    ///
    /// Default: None (non-render views don't have render state)
    /// Overridden by RenderObjectWrapper and RenderViewWrapper.
    fn render_state(&self) -> Option<&dyn Any> {
        None
    }

    /// Returns a mutable reference to the render state.
    ///
    /// Default: None (non-render views don't have render state)
    fn render_state_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    // ========== PROVIDER METHODS (for provider views) ==========

    /// Get provided value as Arc<dyn Any>.
    ///
    /// Only implemented by ProviderViewWrapper.
    /// Returns None for non-provider views.
    fn provided_value(&self) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }

    /// Get dependents list.
    ///
    /// Only implemented by ProviderViewWrapper.
    /// Returns empty slice for non-provider views.
    fn dependents(&self) -> &[ElementId] {
        &[]
    }

    /// Get mutable dependents list.
    ///
    /// Only implemented by ProviderViewWrapper.
    /// Returns None for non-provider views.
    fn dependents_mut(&mut self) -> Option<&mut Vec<ElementId>> {
        None
    }

    /// Add a dependent element.
    ///
    /// Only works for provider views.
    fn add_dependent(&mut self, id: ElementId) {
        if let Some(deps) = self.dependents_mut() {
            if !deps.contains(&id) {
                deps.push(id);
            }
        }
    }

    /// Remove a dependent element.
    ///
    /// Only works for provider views.
    fn remove_dependent(&mut self, id: ElementId) {
        if let Some(deps) = self.dependents_mut() {
            deps.retain(|&dep| dep != id);
        }
    }

    /// Check if dependents should be notified of value change.
    ///
    /// Only implemented by ProviderViewWrapper.
    /// Returns false for non-provider views.
    fn should_notify_dependents(&self, _old_value: &dyn Any) -> bool {
        false
    }

    // ========== DOWNCASTING ==========

    /// Upcast to Any for downcasting support
    fn as_any(&self) -> &dyn Any;

    /// Upcast to Any (mutable) for downcasting support
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // ========== DEBUG ==========

    /// Debug name for diagnostics
    fn debug_name(&self) -> &'static str {
        "ViewObject"
    }
}

// ============================================================================
// HELPER METHODS ON dyn ViewObject
// ============================================================================

impl dyn ViewObject {
    /// Try to downcast to concrete view type.
    pub fn downcast_ref<V: 'static>(&self) -> Option<&V> {
        self.as_any().downcast_ref::<V>()
    }

    /// Try to downcast to concrete view type (mutable).
    pub fn downcast_mut<V: 'static>(&mut self) -> Option<&mut V> {
        self.as_any_mut().downcast_mut::<V>()
    }

    /// Check if this is a render view.
    #[inline]
    pub fn is_render(&self) -> bool {
        self.mode().is_render()
    }

    /// Check if this is a provider view.
    #[inline]
    pub fn is_provider(&self) -> bool {
        matches!(self.mode(), ViewMode::Provider)
    }

    /// Check if this is a component view (stateless, stateful, proxy, animated).
    #[inline]
    pub fn is_component(&self) -> bool {
        self.mode().is_component()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple test fixture
    struct TestViewObject;

    impl ViewObject for TestViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
            None // Empty build
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_view_mode_is_render() {
        assert!(ViewMode::RenderBox.is_render());
        assert!(ViewMode::RenderSliver.is_render());
        assert!(!ViewMode::Stateless.is_render());
        assert!(!ViewMode::Provider.is_render());
    }

    #[test]
    fn test_view_mode_is_component() {
        assert!(ViewMode::Stateless.is_component());
        assert!(ViewMode::Stateful.is_component());
        assert!(ViewMode::Animated.is_component());
        assert!(ViewMode::Provider.is_component());
        assert!(ViewMode::Proxy.is_component());
        assert!(!ViewMode::RenderBox.is_component());
    }

    #[test]
    fn test_view_object_helpers() {
        let obj: &dyn ViewObject = &TestViewObject;
        assert!(!obj.is_render());
        assert!(!obj.is_provider());
        assert!(obj.is_component());
    }
}
