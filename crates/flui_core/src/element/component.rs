//! ComponentElement for StatelessWidget

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;
use crate::foundation::Key;

use crate::{ElementId, StatelessWidget, Context};
use crate::tree::ElementTree;
use super::{Element, AnyElement, ElementLifecycle};
use crate::AnyWidget;

/// Element for StatelessWidget (calls build() to create child)
pub struct ComponentElement<W: StatelessWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    /// Child element created by build()
    child: Option<ElementId>,
    /// Reference to element tree for building children
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl<W: StatelessWidget> ComponentElement<W> {
    /// Create new component element from a widget
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            child: None,
            tree: None,
        }
    }

    /// Perform rebuild
    ///
    /// Returns list of children to mount: (parent_id, child_widget, slot)
    fn perform_rebuild(&mut self) -> Vec<(ElementId, Box<dyn crate::AnyWidget>, usize)> {
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
        let context = Context::new(tree.clone(), self.id);

        // Call build() on the widget to get child widget
        let child_widget = self.widget.build(&context);

        // Mark old child for unmounting (will be handled by caller)
        self.child = None;

        // Return the child that needs to be mounted
        vec![(self.id, child_widget, 0)]
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
            .field("id", &self.id)
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .finish()
    }
}

// ========== Implement AnyElement for ComponentElement ==========

impl<W: StatelessWidget> AnyElement for ComponentElement<W> {
    fn id(&self) -> ElementId {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn Key> {
        AnyWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Unmount child if exists
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                let mut tree_guard = tree.write();
                tree_guard.remove(child_id);
            }
        }
    }

    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            self.widget = *widget;
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)> {
        self.perform_rebuild()
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

    fn render_object(&self) -> Option<&dyn crate::AnyRenderObject> {
        None // ComponentElement doesn't have RenderObject
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::AnyRenderObject> {
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
