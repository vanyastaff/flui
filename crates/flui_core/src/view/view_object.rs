//! Type-erased view object trait.

use std::any::Any;

use crate::element::Element;
use crate::render::RenderObject;
use crate::view::{BuildContext, ViewMode};

/// Type-erased view object.
///
/// Provides dynamic dispatch for view operations. Each view protocol
/// has a concrete wrapper that implements this trait.
pub trait ViewObject: Send {
    /// Build this view into an element.
    fn build(&mut self, ctx: &BuildContext) -> Element;

    /// Initialize after element is mounted (optional).
    fn init(&mut self, ctx: &BuildContext);

    /// Called when dependencies change (optional).
    fn did_change_dependencies(&mut self, ctx: &BuildContext);

    /// Update with new view configuration.
    fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext);

    /// Called when element is deactivated (optional).
    fn deactivate(&mut self, ctx: &BuildContext);

    /// Called when element is permanently removed.
    fn dispose(&mut self, ctx: &BuildContext);

    /// Returns the runtime view mode.
    fn mode(&self) -> ViewMode;

    /// Downcast to concrete view type (for debugging).
    fn as_any(&self) -> &dyn Any;

    /// Mutable downcast to concrete view type.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Access render object (if this is a render view).
    ///
    /// Returns `None` for component views (Stateless, Stateful, etc).
    fn render_object(&self) -> Option<&dyn RenderObject> {
        None
    }

    /// Mutable access to render object (if this is a render view).
    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        None
    }
}

// ============================================================================
// DEBUGGING SUPPORT
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
}
