//! MultiChildRenderObjectElement for RenderObjects with multiple children

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;
use smallvec::SmallVec;

use crate::{AnyElement, Element, ElementId, ElementTree, MultiChildRenderObjectWidget};
use super::super::ElementLifecycle;
use crate::AnyWidget;
use crate::foundation::Key;

/// Child list with inline storage for 4 children (stack allocated, heap fallback)
type ChildList = SmallVec<[ElementId; 4]>;

/// Element for RenderObjects with multiple children (Row, Column, Stack, etc.)
pub struct MultiChildRenderObjectElement<W: MultiChildRenderObjectWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    render_object: Option<Box<dyn crate::AnyRenderObject>>,
    /// Child element IDs (SmallVec for performance - inline storage for 0-4 children)
    children: ChildList,
    /// Reference to ElementTree for child management
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl<W: MultiChildRenderObjectWidget> MultiChildRenderObjectElement<W> {
    /// Create new multi child render object element from a widget
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            render_object: None,
            children: SmallVec::new(), // Inline storage, no heap allocation
            tree: None,
        }
    }

    /// Get reference to the render object
    pub fn render_object_ref(&self) -> Option<&dyn crate::AnyRenderObject> {
        self.render_object.as_ref().map(|r| r.as_ref())
    }

    /// Get mutable reference to the render object
    pub fn render_object_mut_ref(&mut self) -> Option<&mut dyn crate::AnyRenderObject> {
        self.render_object.as_mut().map(|r| r.as_mut())
    }

    /// Get child element IDs
    pub fn children_ids(&self) -> &[ElementId] {
        &self.children
    }

    /// Initialize the render object
    fn initialize_render_object(&mut self) {
        if self.render_object.is_none() {
            self.render_object = Some(self.widget.create_render_object());
        }
    }

    /// Update the render object with new widget configuration
    fn update_render_object(&mut self) {
        if let Some(render_object) = &mut self.render_object {
            self.widget.update_render_object(render_object.as_mut());
        }
    }

    /// Set child element IDs (test helper)
    #[cfg(test)]
    pub(crate) fn set_children(&mut self, children: ChildList) {
        self.children = children;
    }

    /// Set children from Vec (test helper)
    #[cfg(test)]
    pub(crate) fn set_children_vec(&mut self, children: Vec<ElementId>) {
        self.children = SmallVec::from_vec(children);
    }

    /// Take old children for rebuild (test helper)
    #[cfg(test)]
    pub(crate) fn take_old_children(&mut self) -> ChildList {
        std::mem::take(&mut self.children)
    }

    /// Add child element ID (test helper)
    #[cfg(test)]
    pub(crate) fn add_child(&mut self, child_id: ElementId) {
        self.children.push(child_id);
    }
}

impl<W: MultiChildRenderObjectWidget> fmt::Debug for MultiChildRenderObjectElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiChildRenderObjectElement")
            .field("id", &self.id)
            .field("widget_type", &std::any::type_name::<W>())
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("has_render_object", &self.render_object.is_some())
            .field("children_count", &self.children.len())
            .finish()
    }
}

// ========== Implement AnyElement for MultiChildRenderObjectElement ==========

impl<W: MultiChildRenderObjectWidget> AnyElement for MultiChildRenderObjectElement<W> {
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
        self.initialize_render_object();
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Unmount all children first
        if let Some(tree) = &self.tree {
            for child_id in self.children.drain(..) {
                tree.write().remove(child_id);
            }
        }
        // Then clear render object
        self.render_object = None;
    }

    fn update_any(&mut self, new_widget: Box<dyn AnyWidget>) {
        if let Ok(new_widget) = new_widget.downcast::<W>() {
            self.widget = *new_widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn AnyWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Update render object
        self.update_render_object();

        // Return all child widgets to be mounted/updated by ElementTree
        // Clone each child widget using dyn_clone::clone_box
        let children = self.widget.children();
        children
            .iter()
            .enumerate()
            .map(|(slot, child_widget)| (self.id, dyn_clone::clone_box(child_widget.as_ref()), slot))
            .collect()
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
        Box::new(self.children.iter().copied())
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        None // Multi-child elements manage children differently
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.children.push(child_id);
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn render_object(&self) -> Option<&dyn crate::AnyRenderObject> {
        self.render_object.as_ref().map(|ro| ro.as_ref())
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::AnyRenderObject> {
        self.render_object.as_mut().map(|ro| ro.as_mut())
    }

    fn did_change_dependencies(&mut self) {
        // Default: do nothing
    }

    fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Multi-child elements handle slot management differently
    }

    fn forget_child(&mut self, child_id: ElementId) {
        self.children.retain(|id| *id != child_id);
    }
}

// ========== Implement Element for MultiChildRenderObjectElement (with associated types) ==========

impl<W: MultiChildRenderObjectWidget> Element for MultiChildRenderObjectElement<W> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxConstraints, Context, RenderObjectWidget, StatelessWidget};
    use flui_types::{Offset, Size};

    // Mock RenderObject for testing
    #[derive(Debug)]
    struct MockRenderFlex {
        size: Size,
        needs_layout_flag: bool,
        needs_paint_flag: bool,
    }

    impl MockRenderFlex {
        fn new() -> Self {
            Self {
                size: Size::zero(),
                needs_layout_flag: true,
                needs_paint_flag: true,
            }
        }
    }

    impl RenderObject for MockRenderFlex {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.size = constraints.smallest();
            self.needs_layout_flag = false;
            self.size
        }

        fn paint(&self, _painter: &egui::Painter, _offset: Offset) {}

        fn size(&self) -> Size {
            self.size
        }

        fn constraints(&self) -> Option<BoxConstraints> {
            None
        }

        fn needs_layout(&self) -> bool {
            self.needs_layout_flag
        }

        fn mark_needs_layout(&mut self) {
            self.needs_layout_flag = true;
        }

        fn needs_paint(&self) -> bool {
            self.needs_paint_flag
        }

        fn mark_needs_paint(&mut self) {
            self.needs_paint_flag = true;
        }

        fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn crate::AnyRenderObject)) {}

        fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn crate::AnyRenderObject)) {}
    }

    // Mock child widget
    #[derive(Debug, Clone)]
    struct MockChildWidget;

    impl StatelessWidget for MockChildWidget {
        fn build(&self, _context: &Context) -> Box<dyn AnyWidget> {
            Box::new(MockChildWidget)
        }
    }

    // Mock parent widget (like Column)
    #[derive(Debug, Clone)]
    struct MockColumnWidget {
        children: Vec<Box<dyn AnyWidget>>,
    }

    impl Widget for MockColumnWidget {
        type Element = MultiChildRenderObjectElement<Self>;

        fn into_element(self) -> Self::Element {
            MultiChildRenderObjectElement::new(self)
        }
    }

    impl RenderObjectWidget for MockColumnWidget {
        fn create_render_object(&self) -> Box<dyn crate::AnyRenderObject> {
            Box::new(MockRenderFlex::new())
        }

        fn update_render_object(&self, _render_object: &mut dyn crate::AnyRenderObject) {}
    }

    impl MultiChildRenderObjectWidget for MockColumnWidget {
        fn children(&self) -> &[Box<dyn AnyWidget>] {
            &self.children
        }
    }

    #[test]
    fn test_multi_child_element_new() {
        let widget = MockColumnWidget {
            children: Vec::new(),
        };
        let element = MultiChildRenderObjectElement::new(widget);
        assert!(element.parent.is_none());
        assert!(element.dirty);
        assert!(element.render_object.is_none());
        assert!(element.children.is_empty());
    }

    #[test]
    fn test_multi_child_element_mount() {
        let widget = MockColumnWidget {
            children: vec![
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
            ],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        assert!(element.dirty);
        assert!(element.render_object.is_some());
    }

    #[test]
    fn test_multi_child_element_render_object_creation() {
        let widget = MockColumnWidget {
            children: vec![
                Box::new(MockChildWidget),
            ],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        assert!(element.render_object_ref().is_some());
    }

    #[test]
    fn test_multi_child_element_rebuild_returns_children() {
        let widget = MockColumnWidget {
            children: vec![
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
            ],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        let children = element.rebuild();
        assert_eq!(children.len(), 3);

        // All children should have parent_id = element.id()
        for (i, (parent_id, _widget, slot)) in children.iter().enumerate() {
            assert_eq!(*parent_id, element.id());
            assert_eq!(*slot, i);
        }
    }

    #[test]
    fn test_multi_child_element_child_management() {
        let widget = MockColumnWidget {
            children: vec![
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
            ],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);

        let child_id1 = ElementId::new();
        let child_id2 = ElementId::new();

        element.set_children(SmallVec::from_vec(vec![child_id1, child_id2]));
        assert_eq!(element.children_ids(), &[child_id1, child_id2]);

        let taken = element.take_old_children();
        assert_eq!(taken.as_slice(), &[child_id1, child_id2]);
        assert_eq!(element.children_ids().len(), 0);
    }

    #[test]
    fn test_multi_child_element_add_child() {
        let widget = MockColumnWidget {
            children: Vec::new(),
        };
        let mut element = MultiChildRenderObjectElement::new(widget);

        assert_eq!(element.children_ids().len(), 0);

        let child_id = ElementId::new();
        element.add_child(child_id);

        assert_eq!(element.children_ids(), &[child_id]);
    }

    #[test]
    fn test_multi_child_element_child_ids() {
        let widget = MockColumnWidget {
            children: vec![
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
            ],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);

        assert_eq!(element.children_iter().collect::<Vec<_>>(), Vec::<ElementId>::new());

        let child_id1 = ElementId::new();
        let child_id2 = ElementId::new();
        element.set_children(SmallVec::from_vec(vec![child_id1, child_id2]));

        assert_eq!(element.children_iter().collect::<Vec<_>>(), vec![child_id1, child_id2]);
    }

    #[test]
    fn test_multi_child_element_update() {
        let widget = MockColumnWidget {
            children: vec![Box::new(MockChildWidget)],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);
        element.mount(None, 0);

        let new_widget = MockColumnWidget {
            children: vec![
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
            ],
        };
        element.update(new_widget);

        assert!(element.dirty);

        let children = element.rebuild();
        assert_eq!(children.len(), 3);
    }
}
