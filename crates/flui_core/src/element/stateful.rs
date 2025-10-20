//! StatefulElement for StatefulWidget

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::{ElementId, StatefulWidget, State, Context};
use crate::tree::ElementTree;
use super::{AnyElement, Element, ElementLifecycle};
use crate::AnyWidget;
use flui_foundation::Key;

/// Element for StatefulWidget (holds State that persists across rebuilds)
pub struct StatefulElement<W: StatefulWidget> {
    id: ElementId,
    parent: Option<ElementId>,
    dirty: bool,
    widget: W,
    state: Box<W::State>,
    child: Option<ElementId>,
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl<W: StatefulWidget> StatefulElement<W> {
    /// Create new stateful element with widget and state
    pub fn new(widget: W) -> Self {
        let state = widget.create_state();
        Self {
            id: ElementId::new(),
            parent: None,
            dirty: true,
            widget,
            state: Box::new(state),
            child: None,
            tree: None,
        }
    }

    /// Set tree reference (test helper)
    #[cfg(test)]
    pub(crate) fn set_tree(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    /// Set child element ID
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Get child element ID (test helper)
    #[cfg(test)]
    pub(crate) fn child(&self) -> Option<ElementId> {
        self.child
    }

    /// Take old child ID
    pub(crate) fn take_old_child(&mut self) -> Option<ElementId> {
        self.child.take()
    }

    /// Reassemble the element (hot reload support)
    ///
    /// Called during hot reload to give the state a chance to reinitialize.
    /// This is a Phase 2 enhancement for development workflows.
    ///
    /// # Phase 2 Enhancement
    ///
    /// Enables hot reload support by calling reassemble() on the state.
    pub fn reassemble(&mut self) {
        self.state.reassemble();
        self.dirty = true;
    }
}

impl<W: StatefulWidget> fmt::Debug for StatefulElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatefulElement")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("widget", &self.widget)
            .field("child", &self.child)
            .finish()
    }
}

// ========== Implement AnyElement for StatefulElement ==========

impl<W> AnyElement for StatefulElement<W>
where
    W: StatefulWidget + crate::Widget<Element = StatefulElement<W>>,
{
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

        // Call init_state() on first mount
        self.state.init_state();
        // Phase 2: Call did_change_dependencies() after init_state()
        self.state.did_change_dependencies();
    }

    fn unmount(&mut self) {
        // Phase 2: Call deactivate() before cleanup
        self.state.deactivate();

        // Unmount child first
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                tree.write().remove(child_id);
            }
        }

        // Phase 2: Call dispose() after deactivate()
        self.state.dispose();
    }

    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            // Store old widget for did_update_widget
            let old_widget = std::mem::replace(&mut self.widget, *widget);

            // Call did_update_widget() on state
            self.state.did_update_widget(&old_widget);

            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Call build() on state
        if let Some(tree) = &self.tree {
            let context = Context::new(tree.clone(), self.id);
            let child_widget = self.state.build(&context);

            // Mark old child for unmounting
            self.child = None;

            // Return child to mount
            return vec![(self.id, child_widget, 0)];
        }

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
        self.state.deactivate();
    }

    fn activate(&mut self) {
        self.state.activate();
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
        None // StatefulElement doesn't have RenderObject
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::AnyRenderObject> {
        None // StatefulElement doesn't have RenderObject
    }

    fn did_change_dependencies(&mut self) {
        self.state.did_change_dependencies();
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

// ========== Implement Element for StatefulElement (with associated types) ==========

impl<W> Element for StatefulElement<W>
where
    W: StatefulWidget + crate::Widget<Element = StatefulElement<W>>,
{
    type Widget = W;

    fn update(&mut self, new_widget: W) {
        // Zero-cost! No downcast needed!
        // Store old widget for did_update_widget
        let old_widget = std::mem::replace(&mut self.widget, new_widget);

        // Call did_update_widget() on state
        self.state.did_update_widget(&old_widget);

        self.dirty = true;
    }

    fn widget(&self) -> &W {
        &self.widget
    }
}
