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
use flui_foundation::{ElementId, ViewMode};

use crate::context::BuildContext;

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

    // ========== RENDER-SPECIFIC (default: None) ==========
    //
    // These methods are implemented by RenderViewWrapper/RenderObjectWrapper
    // in flui_rendering. Component views return None for all of these.
    //
    // Note: We use `dyn Any` instead of concrete types to avoid depending
    // on flui_rendering from flui-view. The actual implementations in
    // flui_rendering downcast to the correct types.

    /// Get render object if this is a render view.
    ///
    /// Returns None for component views (Stateless, Stateful, etc.)
    fn render_object(&self) -> Option<&dyn Any> {
        None
    }

    /// Get mutable render object if this is a render view.
    fn render_object_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    /// Get render state if this is a render view.
    ///
    /// RenderState contains cached size, offset, and dirty flags.
    fn render_state(&self) -> Option<&dyn Any> {
        None
    }

    /// Get mutable render state if this is a render view.
    fn render_state_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    /// Get layout protocol if this is a render view.
    ///
    /// Returns the protocol discriminant as u8:
    /// - 0: None (component view)
    /// - 1: Box
    /// - 2: Sliver
    fn protocol_discriminant(&self) -> u8 {
        0 // None/Component
    }

    /// Get arity discriminant if this is a render view.
    ///
    /// Returns the arity discriminant as u8:
    /// - 0: None (component view)
    /// - 1: Leaf
    /// - 2: Single
    /// - 3: Multi
    fn arity_discriminant(&self) -> u8 {
        0 // None/Component
    }

    // ========== RENDER OPERATIONS (default: panic) ==========
    //
    // These methods are implemented by RenderViewWrapper/RenderObjectWrapper
    // in flui_rendering. Component views should never call these.

    /// Perform layout on a render view.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-render view (Stateless, Stateful, etc.)
    fn layout_render(
        &self,
        _tree: &dyn Any,
        _children: &[ElementId],
        _constraints: &dyn Any,
    ) -> (f32, f32) {
        panic!(
            "layout_render called on non-render ViewObject: {}",
            self.debug_name()
        );
    }

    /// Perform paint on a render view.
    ///
    /// # Panics
    ///
    /// Panics if called on a non-render view (Stateless, Stateful, etc.)
    fn paint_render(
        &self,
        _tree: &dyn Any,
        _children: &[ElementId],
        _offset: (f32, f32),
    ) -> Box<dyn Any> {
        panic!(
            "paint_render called on non-render ViewObject: {}",
            self.debug_name()
        );
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
/// Since Element now stores `view_mode` directly, type queries no longer
/// require downcasting. For actual ViewObject access, use the specific
/// `view_object_as::<ConcreteWrapper>()` method on Element.
///
/// # Usage
///
/// ```rust,ignore
/// use flui_view::ElementViewObjectExt;
///
/// // Type queries use stored view_mode (no downcasting needed)
/// if element.is_component() {
///     // For actual ViewObject access, downcast to concrete type:
///     if let Some(wrapper) = element.view_object_as::<StatelessViewWrapper<MyView>>() {
///         let child = wrapper.build(ctx);
///     }
/// }
/// ```
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

    #[test]
    fn test_element_view_mode_queries() {
        // Element now has direct view_mode field
        let element = Element::with_mode(42i32, ViewMode::Stateless);
        assert!(element.is_component());
        assert!(!element.is_render());
        assert!(!element.is_provider());

        let render_element = Element::with_mode(42i32, ViewMode::RenderBox);
        assert!(render_element.is_render());
        assert!(!render_element.is_component());

        let provider_element = Element::with_mode(42i32, ViewMode::Provider);
        assert!(provider_element.is_provider());
        assert!(provider_element.is_component()); // Provider is also a component
    }
}
