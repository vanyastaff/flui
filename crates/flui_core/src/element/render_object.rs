//! RenderObjectElement - Element for RenderObjectWidget
//!
//! Manages the lifecycle of render object widgets. Holds the RenderObject
//! that performs layout and painting.

use std::fmt;

use crate::foundation::Key;

use crate::{DynElement, Element, ElementId};
use crate::render::widget::RenderObjectWidget;
use super::ElementLifecycle;
use crate::DynWidget;

/// RenderObjectElement - for RenderObjectWidget
///
/// Manages lifecycle of render object widgets. Holds the RenderObject that performs
/// layout and painting.
///
///
/// # Architecture
///
/// RenderObjectElement is one of the three main element types:
/// - **ComponentElement** - For StatelessWidget
/// - **StatefulElement** - For StatefulWidget
/// - **RenderObjectElement** - For RenderObjectWidget (this type)
///
/// RenderObjectElements are the foundation of the rendering system:
/// 1. Create and hold a RenderObject
/// 2. Update RenderObject when widget changes
/// 3. Form the third tree (Widget → Element → RenderObject)
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct MyBox {
///     width: f32,
///     height: f32,
/// }
///
/// impl RenderObjectWidget for MyBox {
///     type RenderObject = RenderBox;
///
///     fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
///         Box::new(RenderBox::new(self.width, self.height))
///     }
///
///     fn update_render_object(&self, render_object: &mut dyn crate::DynRenderObject) {
///         if let Some(box_obj) = render_object.downcast_mut::<RenderBox>() {
///             box_obj.set_size(self.width, self.height);
///         }
///     }
/// }
///
/// // RenderObjectElement<MyBox> is created automatically
/// let element = RenderObjectElement::new(MyBox { width: 100.0, height: 50.0 });
/// ```
pub struct RenderObjectElement<W: RenderObjectWidget> {    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    render_object: Option<Box<dyn crate::DynRenderObject>>,
}

impl<W: RenderObjectWidget> RenderObjectElement<W> {
    /// Create new render object element from a widget
    ///
    /// Note: ID is initially 0 and will be set by ElementTree when inserted
    pub fn new(widget: W) -> Self {
        Self {            widget,
            parent: None,
            dirty: true,
            render_object: None,
        }
    }

    /// Get reference to the render object
    pub fn render_object(&self) -> Option<&dyn crate::DynRenderObject> {
        self.render_object.as_ref().map(|r| r.as_ref())
    }

    /// Get mutable reference to the render object
    pub fn render_object_mut(&mut self) -> Option<&mut dyn crate::DynRenderObject> {
        self.render_object.as_mut().map(|r| r.as_mut())
    }

    /// Initialize the render object
    fn initialize_render_object(&mut self) {
        if self.render_object.is_none() {
            let render_object = self.widget.create_render_object();
            self.render_object = Some(render_object);
        }
    }

    /// Update the render object with new widget configuration
    fn update_render_object(&mut self) {
        if let Some(ref mut render_object) = self.render_object {
            self.widget.update_render_object(render_object.as_mut());
        }
    }
}

impl<W: RenderObjectWidget> fmt::Debug for RenderObjectElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderObjectElement")
            .field("widget_type", &std::any::type_name::<W>())
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("has_render_object", &self.render_object.is_some())
            .finish()
    }
}

// ========== Implement DynElement for RenderObjectElement ==========

impl<W: RenderObjectWidget> DynElement for RenderObjectElement<W> {    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn Key> {
        DynWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.initialize_render_object();
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Clean up render object
        self.render_object = None;
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        if let Ok(widget) = new_widget.downcast::<W>() {
            self.widget = *widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self, _element_id: ElementId) -> Vec<(ElementId, Box<dyn DynWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Update render object if needed
        self.update_render_object();

        // RenderObjectElement typically doesn't have child elements
        // (those are managed by specific subclasses)
        Vec::new()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn lifecycle(&self) -> ElementLifecycle {
        ElementLifecycle::Active // Default
    }

    fn deactivate(&mut self) {
        // Default: do nothing
    }

    fn activate(&mut self) {
        // Default: do nothing
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(std::iter::empty()) // RenderObjectElement base has no children
    }

    fn set_tree_ref(&mut self, _tree: std::sync::Arc<parking_lot::RwLock<crate::ElementTree>>) {
        // RenderObjectElement doesn't need tree reference
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        None // No children in base implementation
    }

    fn set_child_after_mount(&mut self, _child_id: ElementId) {
        // No children in base implementation
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn widget(&self) -> &dyn crate::DynWidget {
        &self.widget
    }

    fn render_object(&self) -> Option<&dyn crate::DynRenderObject> {
        self.render_object.as_ref().map(|ro| ro.as_ref())
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::DynRenderObject> {
        self.render_object.as_mut().map(|ro| ro.as_mut())
    }

    fn did_change_dependencies(&mut self) {
        // Default: do nothing
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // No children
    }

    fn forget_child(&mut self, _child_id: ElementId) {
        // No children
    }
}

// ========== Implement Element for RenderObjectElement (with associated types) ==========

impl<W: RenderObjectWidget> Element for RenderObjectElement<W> {
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        // Zero-cost! No downcast needed!
        self.widget = new_widget;
        self.update_render_object();
        self.dirty = true;
    }

    fn widget(&self) -> &W {
        &self.widget
    }
}
