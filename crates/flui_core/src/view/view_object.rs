//! Type-erased view object trait.
//!
//! # Architecture (Following Flutter)
//!
//! ViewObject handles View lifecycle. RenderObjects are accessed through
//! optional methods that return Some only for RenderView implementations.
//!
//! ```text
//! View (immutable config)
//!   ↓ implement ViewObject
//! ViewObject (dynamic dispatch)
//!   ├─ Component views: Stateless, Stateful, Proxy, Animated
//!   │   └─ build() returns child View (wrapped in Element)
//!   ├─ Provider views: Inherited data provider
//!   │   └─ build() returns child View, has provided_value(), dependents()
//!   └─ Render views: RenderBox, RenderSliver
//!       └─ has render_object(), render_state(), protocol(), arity()
//! ```

use std::any::Any;

use crate::element::{Element, ElementId, ElementTree};
use crate::view::{BuildContext, ViewMode};
use flui_painting::Canvas;
use flui_rendering::core::{LayoutProtocol, RenderObject, RenderState, RuntimeArity};
use flui_types::{constraints::BoxConstraints, Offset, Size};

/// Type-erased view object trait.
///
/// Provides dynamic dispatch for view lifecycle operations.
/// Each view type (Stateless, Stateful, Render, Provider, etc) implements this trait.
///
/// # Lifecycle
///
/// 1. `build()` - Create child element(s)
/// 2. `init()` - Called after element is mounted
/// 3. `did_change_dependencies()` - Called when inherited values change
/// 4. `did_update()` - Called when view is updated with new config
/// 5. `deactivate()` - Called when element is temporarily removed
/// 6. `dispose()` - Called when element is permanently removed
pub trait ViewObject: Send {
    // ========== CORE METHODS (required) ==========

    /// Returns the runtime view mode.
    fn mode(&self) -> ViewMode;

    /// Build this view into a child element.
    fn build(&mut self, ctx: &BuildContext) -> Element;

    // ========== LIFECYCLE (required) ==========

    /// Initialize after element is mounted.
    fn init(&mut self, ctx: &BuildContext);

    /// Called when dependencies change.
    fn did_change_dependencies(&mut self, ctx: &BuildContext);

    /// Update with new view configuration.
    fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext);

    /// Called when element is deactivated.
    fn deactivate(&mut self, ctx: &BuildContext);

    /// Called when element is permanently removed.
    fn dispose(&mut self, ctx: &BuildContext);

    // ========== DOWNCASTING ==========

    /// Downcast to concrete view type (for debugging).
    fn as_any(&self) -> &dyn Any;

    /// Mutable downcast to concrete view type.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // ========== RENDER-SPECIFIC (default: None) ==========

    /// Get render object if this is a RenderView.
    ///
    /// Returns `Some` only for `RenderViewWrapper`.
    fn render_object(&self) -> Option<&dyn RenderObject> {
        None
    }

    /// Get mutable render object if this is a RenderView.
    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        None
    }

    /// Get render state if this is a RenderView.
    fn render_state(&self) -> Option<&RenderState> {
        None
    }

    /// Get mutable render state if this is a RenderView.
    fn render_state_mut(&mut self) -> Option<&mut RenderState> {
        None
    }

    /// Get layout protocol if this is a RenderView.
    fn protocol(&self) -> Option<LayoutProtocol> {
        None
    }

    /// Get arity if this is a RenderView.
    fn arity(&self) -> Option<RuntimeArity> {
        None
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

    // ========== LAYOUT & PAINT (default: panic) ==========

    /// Performs layout computation for render views.
    ///
    /// Returns the computed size. Only valid for RenderView implementations.
    fn layout_render(
        &self,
        _tree: &ElementTree,
        _children: &[ElementId],
        _constraints: BoxConstraints,
    ) -> Size {
        panic!("layout_render called on non-render ViewObject")
    }

    /// Performs paint computation for render views.
    ///
    /// Returns the canvas with painted content. Only valid for RenderView implementations.
    fn paint_render(
        &self,
        _tree: &ElementTree,
        _children: &[ElementId],
        _offset: Offset,
    ) -> Canvas {
        panic!("paint_render called on non-render ViewObject")
    }

    /// Performs hit testing for render views.
    fn hit_test_render(
        &self,
        _tree: &ElementTree,
        _children: &[ElementId],
        _position: Offset,
        _geometry: &flui_rendering::core::Geometry,
    ) -> bool {
        false // Default implementation for non-render objects
    }
}

// ============================================================================
// HELPER METHODS
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
        matches!(self.mode(), ViewMode::RenderBox | ViewMode::RenderSliver)
    }

    /// Check if this is a provider view.
    #[inline]
    pub fn is_provider(&self) -> bool {
        matches!(self.mode(), ViewMode::Provider)
    }

    /// Check if this is a component view (stateless, stateful, proxy, animated).
    #[inline]
    pub fn is_component(&self) -> bool {
        matches!(
            self.mode(),
            ViewMode::Stateless | ViewMode::Stateful | ViewMode::Proxy | ViewMode::Animated
        )
    }
}
