//! MultiChildRenderObjectElement for RenderObjects with multiple children

use std::fmt;
use std::sync::Arc;

use parking_lot::RwLock;
use smallvec::SmallVec;

use crate::{DynElement, DynWidget, Element, ElementId, ElementTree, MultiChildRenderObjectWidget};
use crate::foundation::{Key, Slot};  // Added Slot for updateChildren
use super::super::ElementLifecycle;

/// Type alias for a list of child element IDs (optimized for small lists)
type ChildList = SmallVec<[ElementId; 8]>;

/// Element for RenderObjects with multiple children (Column, Row, Stack, etc.)
///
/// MultiChildRenderObjectElement is specialized for widgets that:
/// - Create a RenderObject for layout and painting
/// - Have MULTIPLE children widgets
/// - Manage child lists efficiently (e.g., Column, Row, Stack, Wrap)
///
/// # Examples
///
/// ```rust,ignore
/// // Column widget creates a MultiChildRenderObjectElement
/// let column = Column::new(vec![
///     Text::new("Hello").into_widget(),
///     Text::new("World").into_widget(),
/// ]);
/// let element = column.into_element(); // MultiChildRenderObjectElement<Column>
/// ```
///
/// # See Also
///
/// - [`LeafRenderObjectElement`] - For widgets with no children
/// - [`SingleChildRenderObjectElement`] - For widgets with one child
pub struct MultiChildRenderObjectElement<W: MultiChildRenderObjectWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    lifecycle: ElementLifecycle,
    render_object: Option<Box<dyn crate::DynRenderObject>>,
    /// Child element IDs (managed by ElementTree)
    children: ChildList,
    /// Reference to ElementTree for child management
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl<W: MultiChildRenderObjectWidget> MultiChildRenderObjectElement<W> {
    /// Creates a new multi-child render object element
    #[must_use]
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            lifecycle: ElementLifecycle::Initial,
            render_object: None,
            children: SmallVec::new(),
            tree: None,
        }
    }

    /// Returns a slice of child element IDs
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Iterate over child element IDs
    pub fn children_iter(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.children.iter().copied()
    }

    /// Set children (used by tests)
    #[cfg(test)]
    pub(crate) fn set_children(&mut self, children: ChildList) {
        self.children = children;
    }

    /// Add a child (used by tests)
    #[cfg(test)]
    pub(crate) fn add_child(&mut self, child_id: ElementId) {
        self.children.push(child_id);
    }

    /// Take old children (used by tests)
    #[cfg(test)]
    pub(crate) fn take_old_children(&mut self) -> ChildList {
        std::mem::take(&mut self.children)
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

    // ========== Multi-Child Update Algorithm (ACTIVATED) ==========
    // Efficient incremental child update using Flutter's updateChildren() algorithm
    /// Update children efficiently using Flutter's updateChildren() algorithm
    ///
    /// This implements the three-phase scan algorithm:
    /// 1. Scan from start - update matching children in-place
    /// 2. Scan from end - update matching children in-place
    /// 3. Handle middle section - reuse keyed children, insert/remove as needed
    ///
    /// Returns the new list of child element IDs.
    #[allow(dead_code)]
    #[tracing::instrument(
        level = "debug",
        skip(self, old_children, new_widgets),
        fields(
            old_count = old_children.len(),
            new_count = new_widgets.len()
        )
    )]
    fn update_children(
        &mut self,
        mut old_children: ChildList,
        new_widgets: &[Box<dyn DynWidget>],
    ) -> ChildList {
        if new_widgets.is_empty() {
            // All children removed - unmount old children
            if let Some(tree) = &self.tree {
                let mut tree_guard = tree.write();
                for child_id in old_children.drain(..) {
                    tree_guard.remove(child_id);
                }
            }
            return SmallVec::new();
        }

        if old_children.is_empty() {
            // All children are new - mount them all
            return self.mount_all_children(new_widgets);
        }

        // Get tree reference (needed for operations)
        let tree = match &self.tree {
            Some(t) => t.clone(),
            None => return SmallVec::new(), // No tree - can't update
        };

        let mut new_children = SmallVec::with_capacity(new_widgets.len());
        let old_len = old_children.len();
        let new_len = new_widgets.len();

        // Scan from start, update in-place while children match
        let mut old_index = 0;
        let mut new_index = 0;

        while old_index < old_len && new_index < new_len {
            let old_child_id = old_children[old_index];
            let new_widget = &new_widgets[new_index];

            // Check if we can update this child in-place
            let can_update = {
                let tree_guard = tree.read();
                if let Some(old_element) = tree_guard.get(old_child_id) {
                    Self::can_update(old_element, new_widget.as_ref())
                } else {
                    false
                }
            };

            if can_update {
                // Update in-place with IndexedSlot
                let previous_sibling = if new_index > 0 {
                    new_children.last().copied()
                } else {
                    None
                };
                let slot = Slot::with_previous_sibling(new_index, previous_sibling);
                Self::update_child(&tree, old_child_id, new_widget.as_ref(), slot);
                new_children.push(old_child_id);
                old_index += 1;
                new_index += 1;
            } else {
                break; // Mismatch - proceed to middle section
            }
        }

        // Scan from end, update in-place while children match
        let mut old_end = old_len;
        let mut new_end = new_len;

        while old_index < old_end && new_index < new_end {
            let old_child_id = old_children[old_end - 1];
            let new_widget = &new_widgets[new_end - 1];

            let can_update = {
                let tree_guard = tree.read();
                if let Some(old_element) = tree_guard.get(old_child_id) {
                    Self::can_update(old_element, new_widget.as_ref())
                } else {
                    false
                }
            };

            if can_update {
                old_end -= 1;
                new_end -= 1;
            } else {
                break; // Mismatch
            }
        }

        // Handle middle section
        if old_index < old_end || new_index < new_end {
            self.handle_middle_section(
                &old_children[old_index..old_end],
                &new_widgets[new_index..new_end],
                &mut new_children,
                &tree,
                new_index,
            );
        }

        // Process children from end scan
        for (offset, new_widget) in new_widgets.iter().skip(new_end).take(new_len - new_end).enumerate() {
            let i = new_end + offset;
            let old_idx = old_end + offset;
            let old_child_id = old_children[old_idx];

            // Create IndexedSlot with previous sibling
            let previous_sibling = if i > 0 {
                new_children.last().copied()
            } else {
                None
            };
            let slot = Slot::with_previous_sibling(i, previous_sibling);
            Self::update_child(&tree, old_child_id, new_widget.as_ref(), slot);
            new_children.push(old_child_id);
        }

        new_children
    }

    /// Check if an element can be updated with a new widget
    fn can_update(element: &dyn DynElement, widget: &dyn DynWidget) -> bool {
        // Must be same type
        if element.widget_type_id() != widget.type_id() {
            return false;
        }

        // Check key compatibility
        match (element.key(), widget.key()) {
            (None, None) => true, // Both unkeyed - OK
            (Some(k1), Some(k2)) => k1.key_eq(k2), // Both keyed with same key - OK
            _ => false, // One keyed, one not - incompatible
        }
    }

    /// Update a child element with a new widget
    ///
    /// Now accepts Slot with optional previous_sibling for efficient
    /// RenderObject child insertion.
    fn update_child(
        tree: &Arc<RwLock<ElementTree>>,
        element_id: ElementId,
        new_widget: &dyn DynWidget,
        slot: Slot,
    ) {
        let mut tree_guard = tree.write();
        if let Some(element) = tree_guard.get_mut(element_id) {
            // Update with new widget
            element.update_any(dyn_clone::clone_box(new_widget));
            // Update slot if needed (slot now contains index + previous_sibling)
            element.update_slot_for_child(element_id, slot.index());
        }
    }

    /// Mount all new children (when old list is empty)
    fn mount_all_children(&mut self, new_widgets: &[Box<dyn DynWidget>]) -> ChildList {
        let mut children = SmallVec::with_capacity(new_widgets.len());

        if let Some(tree) = &self.tree {
            for (slot, widget) in new_widgets.iter().enumerate() {
                let widget_clone = dyn_clone::clone_box(widget.as_ref());
                if let Some(child_id) = tree.write().insert_child(self.id, widget_clone, slot) {
                    children.push(child_id);
                }
            }
        }

        children
    }

    /// Handle the middle section where children don't match on both ends
    fn handle_middle_section(
        &mut self,
        old_middle: &[ElementId],
        new_middle: &[Box<dyn DynWidget>],
        new_children: &mut ChildList,
        tree: &Arc<RwLock<ElementTree>>,
        start_slot: usize,
    ) {
        use std::collections::HashMap;
        use crate::foundation::key::KeyId;

        // Build key → element map for old keyed children
        let old_keyed: HashMap<KeyId, ElementId> = {
            let tree_guard = tree.read();
            old_middle
                .iter()
                .filter_map(|&id| {
                    let element = tree_guard.get(id)?;
                    let key = element.key()?;
                    Some((key.id(), id))
                })
                .collect()
        };

        // Track which old children have been reused
        let mut used_old_children = std::collections::HashSet::new();

        // Process each new widget
        for new_widget in new_middle.iter() {
            let slot_index = start_slot + new_children.len();

            // Try to find matching old element
            let old_element_id = if let Some(key) = new_widget.key() {
                // Keyed widget - lookup by key
                old_keyed.get(&key.id()).copied()
            } else {
                // Unkeyed widget - try to find first unused matching element
                old_middle.iter().find_map(|&old_id| {
                    if used_old_children.contains(&old_id) {
                        return None;
                    }

                    let tree_guard = tree.read();
                    let old_element = tree_guard.get(old_id)?;

                    if Self::can_update(old_element, new_widget.as_ref()) {
                        Some(old_id)
                    } else {
                        None
                    }
                })
            };

            if let Some(old_id) = old_element_id {
                // Reuse existing element with IndexedSlot
                used_old_children.insert(old_id);

                let previous_sibling = if slot_index > 0 {
                    new_children.last().copied()
                } else {
                    None
                };
                let slot = Slot::with_previous_sibling(slot_index, previous_sibling);
                Self::update_child(tree, old_id, new_widget.as_ref(), slot);
                new_children.push(old_id);
            } else {
                // Create new element
                let widget_clone = dyn_clone::clone_box(new_widget.as_ref());
                if let Some(element_id) = tree.write().insert_child(self.id, widget_clone, slot_index) {
                    new_children.push(element_id);
                }
            }
        }

        // Deactivate or remove unused old children
        // If element has GlobalKey, deactivate it so it can be reactivated later
        // Otherwise, remove it immediately
        let mut tree_guard = tree.write();
        for &old_id in old_middle {
            if !used_old_children.contains(&old_id) {
                tree_guard.deactivate_child(old_id);
            }
        }
    }
}

// ========== Implement Debug for MultiChildRenderObjectElement ==========

impl<W: MultiChildRenderObjectWidget> fmt::Debug for MultiChildRenderObjectElement<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MultiChildRenderObjectElement")
            .field("id", &self.id)
            .field("widget_type", &std::any::type_name::<W>())
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("lifecycle", &self.lifecycle)
            .field("has_render_object", &self.render_object.is_some())
            .field("children_count", &self.children.len())
            .finish()
    }
}

// ========== Implement DynElement for MultiChildRenderObjectElement ==========

impl<W: MultiChildRenderObjectWidget> DynElement for MultiChildRenderObjectElement<W> {
    fn id(&self) -> ElementId {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn Key> {
        DynWidget::key(&self.widget)
    }

    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.lifecycle = ElementLifecycle::Active;
        self.initialize_render_object();
        self.dirty = true;
    }

    fn unmount(&mut self) {
        self.lifecycle = ElementLifecycle::Defunct;
        // Unmount all children first
        if let Some(tree) = &self.tree {
            let mut tree_guard = tree.write();
            for child_id in self.children.drain(..) {
                tree_guard.remove(child_id);
            }
        }
        // Then clear render object
        self.render_object = None;
    }

    fn update_any(&mut self, new_widget: Box<dyn DynWidget>) {
        if let Ok(new_widget) = new_widget.downcast::<W>() {
            self.widget = *new_widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn DynWidget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Update render object
        self.update_render_object();

        // Return all child widgets to be mounted/updated by ElementTree
        let children = self.widget.children();
        children
            .iter()
            .enumerate()
            .map(|(slot, child)| (self.id, dyn_clone::clone_box(child.as_ref()), slot))
            .collect()
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
        // Note: children stay attached but inactive
        // Will be unmounted if not reactivated before frame end
    }

    fn activate(&mut self) {
        self.lifecycle = ElementLifecycle::Active;
        // Element is being reinserted into tree (GlobalKey reparenting)
        self.dirty = true; // Mark for rebuild in new location
    }

    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.children.iter().copied())
    }

    fn set_tree_ref(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        None // Multi-child elements don't use this method
    }

    fn set_child_after_mount(&mut self, _child_id: ElementId) {
        // Multi-child elements use set_children or add_child instead
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
        // Multi-child slot updates are handled by update_children algorithm
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
    use crate::{BoxConstraints, Context, RenderObjectWidget, StatelessWidget, Widget};
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

    impl crate::render::RenderObject for MockRenderFlex {
        type ParentData = ();
        type Child = Box<dyn crate::DynRenderObject>;

        fn parent_data(&self) -> Option<&Self::ParentData> {
            None
        }

        fn parent_data_mut(&mut self) -> Option<&mut Self::ParentData> {
            None
        }
    }

    impl crate::DynRenderObject for MockRenderFlex {
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

        fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn crate::DynRenderObject)) {}

        fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn crate::DynRenderObject)) {}
    }

    // Mock child widget
    #[derive(Debug, Clone)]
    struct MockChildWidget;

    impl StatelessWidget for MockChildWidget {
        fn build(&self, _context: &Context) -> Box<dyn DynWidget> {
            Box::new(MockChildWidget)
        }
    }

    // Mock parent widget (like Column)
    #[derive(Debug, Clone)]
    struct MockColumnWidget {
        children: Vec<Box<dyn DynWidget>>,
    }

    impl Widget for MockColumnWidget {
        type Element = MultiChildRenderObjectElement<Self>;

        fn into_element(self) -> Self::Element {
            MultiChildRenderObjectElement::new(self)
        }
    }

    impl RenderObjectWidget for MockColumnWidget {
        fn create_render_object(&self) -> Box<dyn crate::DynRenderObject> {
            Box::new(MockRenderFlex::new())
        }

        fn update_render_object(&self, _render_object: &mut dyn crate::DynRenderObject) {}
    }

    impl MultiChildRenderObjectWidget for MockColumnWidget {
        fn children(&self) -> &[Box<dyn DynWidget>] {
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
        assert!(element.children().is_empty());
        assert_eq!(element.lifecycle, ElementLifecycle::Initial);
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
        assert_eq!(element.lifecycle, ElementLifecycle::Active);
    }

    #[test]
    fn test_multi_child_element_children_method() {
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

        // Test the new children() method (was children_ids)
        let ids = element.children();
        assert_eq!(ids, &[child_id1, child_id2]);
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
        assert_eq!(element.children(), &[child_id1, child_id2]);

        let taken = element.take_old_children();
        assert_eq!(taken.as_slice(), &[child_id1, child_id2]);
        assert_eq!(element.children().len(), 0);
    }

    #[test]
    fn test_multi_child_element_add_child() {
        let widget = MockColumnWidget {
            children: Vec::new(),
        };
        let mut element = MultiChildRenderObjectElement::new(widget);

        assert_eq!(element.children().len(), 0);

        let child_id = ElementId::new();
        element.add_child(child_id);

        assert_eq!(element.children(), &[child_id]);
    }

    #[test]
    fn test_multi_child_element_children_iter() {
        use itertools::Itertools;

        let widget = MockColumnWidget {
            children: vec![
                Box::new(MockChildWidget),
                Box::new(MockChildWidget),
            ],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);

        assert_eq!(element.children_iter().collect_vec(), Vec::<ElementId>::new());

        let child_id1 = ElementId::new();
        let child_id2 = ElementId::new();
        element.set_children(SmallVec::from_vec(vec![child_id1, child_id2]));

        assert_eq!(element.children_iter().collect_vec(), vec![child_id1, child_id2]);
    }

    #[test]
    fn test_multi_child_element_lifecycle_transitions() {
        let widget = MockColumnWidget {
            children: vec![Box::new(MockChildWidget)],
        };
        let mut element = MultiChildRenderObjectElement::new(widget);

        // Initial -> Active
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
        element.mount(None, 0);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        // Active -> Inactive
        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        // Inactive -> Active
        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_dirty());

        // Active -> Defunct
        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }
}