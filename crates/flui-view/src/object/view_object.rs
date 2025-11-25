//! ViewObject trait - Dynamic dispatch for view lifecycle
//!
//! ViewObject is the trait that all view wrappers implement.
//! It's stored inside Element as `Box<dyn Any + Send>` and accessed
//! via downcasting.
//!
//! # Architecture
//!
//! ```text
//! View (immutable config)
//!   ↓ implement ViewObject
//! ViewObject (dynamic dispatch)
//!   ├─ Component views: Stateless, Stateful, Proxy, Animated
//!   │   └─ build() returns child View (wrapped in Element)
//!   └─ Provider views: Inherited data provider
//!       └─ build() returns child View, has provided_value(), dependents()
//!
//! Render views are handled by RenderViewObject in flui_rendering.
//! ```
//!
//! # Design Principle
//!
//! This trait is intentionally minimal and has NO dependencies on:
//! - flui_rendering (RenderObject, RenderState, etc.)
//! - flui_painting (Canvas)
//! - flui_types (BoxConstraints, Size, Offset)
//!
//! Render-specific methods are in `RenderViewObject` trait in flui_rendering.

use std::any::Any;

use flui_element::Element;
use flui_foundation::ElementId;

use crate::context::BuildContext;
use crate::protocol::ViewMode;

/// ViewObject - Dynamic dispatch interface for view lifecycle
///
/// This trait defines the operations that all view types support.
/// Wrappers (StatelessViewWrapper, StatefulViewWrapper, etc.) implement this.
///
/// # Thread Safety
///
/// ViewObject requires `Send` for cross-thread element transfer.
///
/// # Lifecycle
///
/// 1. `build()` - Create child element(s)
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
pub trait ViewObject: Send + 'static {
    // ========== CORE METHODS (required) ==========

    /// Get the view mode (Stateless, Stateful, RenderBox, etc.)
    fn mode(&self) -> ViewMode;

    /// Build this view, producing child element(s)
    ///
    /// Called during the build phase to create/update children.
    /// For RenderViews, this typically panics - they create RenderObjects, not children.
    fn build(&mut self, ctx: &dyn BuildContext) -> Element;

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

    // ========== PROVIDER-SPECIFIC (default: None) ==========

    /// Get provided value if this is a ProviderView.
    fn provided_value(&self) -> Option<&(dyn Any + Send + Sync)> {
        None
    }

    /// Get dependents list if this is a ProviderView.
    fn dependents(&self) -> Option<&[ElementId]> {
        None
    }

    /// Get mutable dependents list if this is a ProviderView.
    fn dependents_mut(&mut self) -> Option<&mut Vec<ElementId>> {
        None
    }

    /// Check if dependents should be notified.
    fn should_notify_dependents(&self, _old_value: &dyn Any) -> bool {
        true
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
// HELPER TRAIT FOR ELEMENT ACCESS
// ============================================================================

/// Extension trait for accessing ViewObject from Element
///
/// Provides methods to downcast Element's type-erased view_object
/// to concrete ViewObject implementations.
pub trait ElementViewObjectExt {
    /// Try to downcast view_object to a specific ViewObject implementation.
    fn view_object_downcast<V: ViewObject + Sync>(&self) -> Option<&V>;

    /// Try to downcast view_object to a specific ViewObject implementation (mutable).
    fn view_object_downcast_mut<V: ViewObject + Sync>(&mut self) -> Option<&mut V>;
}

impl ElementViewObjectExt for Element {
    fn view_object_downcast<V: ViewObject + Sync>(&self) -> Option<&V> {
        self.view_object_as::<V>()
    }

    fn view_object_downcast_mut<V: ViewObject + Sync>(&mut self) -> Option<&mut V> {
        self.view_object_as_mut::<V>()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
}
