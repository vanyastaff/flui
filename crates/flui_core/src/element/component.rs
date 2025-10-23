//! ComponentElement for StatelessWidget

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;
use crate::foundation::Key;

use crate::{ElementId, StatelessWidget, Context};
use crate::tree::ElementTree;
use super::{Element, DynElement, ElementLifecycle};
use crate::DynWidget;

/// Element for StatelessWidget (calls build() to create child)
///
/// Note: Element ID is its Slab index in ElementTree (not stored here)
pub struct ComponentElement<W: StatelessWidget> {
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    /// Child element created by build()
    child: Option<ElementId>,
    /// Reference to element tree for building children
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl<W: StatelessWidget> ComponentElement<W> {
    /// Create new component element from a widget
    pub fn new(widget: W) -> Self {
        Self {
            widget,
            parent: None,
            dirty: true,
            lifecycle: ElementLifecycle::Initial,
            child: None,
            tree: None,
        }
    }

    /// Perform rebuild
    ///
    /// Returns list of children to mount: (parent_id, child_widget, slot)
    fn perform_rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, Box<dyn crate::DynWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }

        self.dirty = false;

        let tree = match &self.tree {
            Some(t) => t.clone(),
            None => {
                // No tree reference yet - this happens during initial mount
                // The tree will be set later via set_tree()
                return Vec::new();
            }
        };

        // Create build context
        let context = Context::new(tree.clone(), element_id);

        // Call build() on the widget to get child widget
        let child_widget = self.widget.build(&context);

        // Mark old child for unmounting (will be handled by caller)
        self.child = None;

        // Return the child that needs to be mounted
        vec![(element_id, child_widget, 0)]
    }

    /// Set the child element ID after it's been mounted
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Get old child ID and clear it (for unmounting before rebuild)
    pub(crate) fn take_old_child(&mut self) -> Option<ElementId> {
        self.child.take()
    }
}

impl<W: StatelessWidget> fmt::Debug for ComponentElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentElement")
            .field("widget_type", &std::any::type_name::<W>())
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("child", &self.child)
            .finish()
    }
}

// ========== Implement DynElement for ComponentElement ==========

impl<W: StatelessWidget> DynElement for ComponentElement<W> {
    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn Key> {
        DynWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.lifecycle = ElementLifecycle::Active;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;

        // Unmount child if exists
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                let mut tree_guard = tree.write();
                tree_guard.remove(child_id);
            }
        }
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            self.widget = *widget;
            self.dirty = true;
        }
    }

    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, Box<dyn DynWidget>, usize)> {
        self.perform_rebuild(element_id)
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn lifecycle(&self) -> ElementLifecycle {
        self.lifecycle
    }

    fn deactivate(&mut self) {
        self.lifecycle = ElementLifecycle::Inactive;
        // Note: child stays attached but inactive
        // Will be unmounted if not reactivated before frame end
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        // Element is being reinserted into tree (GlobalKey reparenting)
        self.dirty = true; // Mark for rebuild in new location
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.child.into_iter())
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        self.take_old_child()
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.set_child(child_id)
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn widget(&self) -> &dyn crate::DynWidget {
        &self.widget
    }

    fn render_object(&self) -> Option<&dyn crate::DynRenderObject> {
        None // ComponentElement doesn't have RenderObject
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::DynRenderObject> {
        None // ComponentElement doesn't have RenderObject
    }

    fn did_change_dependencies(&mut self) {
        // Default: do nothing for StatelessWidget
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Default: do nothing (single child)
    }

    fn forget_child(&mut self, child_id: ElementId) {
        if self.child == Some(child_id) {
            self.child = None;
        }
    }
}

// ========== Implement Element for ComponentElement (with associated types) ==========

impl<W: StatelessWidget> Element for ComponentElement<W> {
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        // Zero-cost! No downcast needed!
        self.widget = new_widget;
        self.dirty = true;
    }

    fn widget(&self) -> &W {
        &self.widget
    }
}
