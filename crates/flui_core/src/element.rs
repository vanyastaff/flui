//! Element tree - mutable state holders for widgets
//!
//! This module provides the Element trait and implementations, which form the middle
//! layer of the three-tree architecture (Widget → Element → RenderObject).

use std::any::Any;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use downcast_rs::{impl_downcast, DowncastSync};
use flui_foundation::Key;
use parking_lot::RwLock;

use crate::{BuildContext, ElementTree, RenderObject, RenderObjectWidget, StatelessWidget};

/// Unique identifier for elements in the tree
///
/// Similar to Flutter's element identity. Each element gets a unique ID when created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub u64);

impl ElementId {
    /// Generate a new unique element ID
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for ElementId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ElementId({})", self.0)
    }
}

/// Core Element trait - mutable state holder in element tree
///
/// Similar to Flutter's Element. Elements manage lifecycle, hold widget references,
/// and persist across rebuilds while widgets are recreated.
///
/// # Lifecycle
///
/// 1. **Mount**: Element is inserted into tree
/// 2. **Update**: Widget configuration changes
/// 3. **Rebuild**: Element rebuilds its subtree
/// 4. **Unmount**: Element is removed from tree
pub trait Element: DowncastSync + fmt::Debug {
    /// Mount this element into the tree
    ///
    /// Called when element is first inserted. The element should initialize itself
    /// and prepare for building.
    ///
    /// # Parameters
    /// - `parent`: Parent element ID (None for root)
    /// - `slot`: Position in parent's child list
    fn mount(&mut self, parent: Option<ElementId>, slot: usize);

    /// Unmount and clean up this element
    ///
    /// Called when element is removed from tree. Should clean up resources and
    /// unmount children.
    fn unmount(&mut self);

    /// Update this element with a new widget configuration
    ///
    /// Called when parent rebuilds with a new widget that can update this element
    /// (same type and key). Should update internal state with new configuration.
    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>);

    /// Rebuild this element's subtree
    ///
    /// Called when element is marked dirty. Should rebuild child widgets and
    /// update child elements.
    ///
    /// Returns a list of (parent_id, child_widget, slot) tuples for children
    /// that need to be mounted. The caller (ElementTree) will handle the actual
    /// mounting to avoid lock recursion.
    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn crate::Widget>, usize)>;

    /// Get the element's unique ID
    fn id(&self) -> ElementId;

    /// Get the parent element ID
    fn parent(&self) -> Option<ElementId> {
        None
    }

    /// Get the key if present
    fn key(&self) -> Option<&dyn Key> {
        None
    }

    /// Check if this element is dirty (needs rebuild)
    fn is_dirty(&self) -> bool {
        false
    }

    /// Mark this element as dirty
    fn mark_dirty(&mut self);

    /// Visit child elements (read-only)
    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn Element)) {
        // Default: no children
    }

    /// Visit child elements (mutable)
    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn Element)) {
        // Default: no children
    }

    /// Set tree reference for ComponentElements
    ///
    /// This allows ComponentElements to mount their children. Only ComponentElements
    /// need this - other element types can ignore it.
    fn set_tree_ref(&mut self, _tree: std::sync::Arc<parking_lot::RwLock<crate::ElementTree>>) {
        // Default: do nothing
    }

    /// Get widget type ID for update checks
    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<()>()
    }

    /// Get RenderObject if this element has one
    ///
    /// Only RenderObjectElements return Some. ComponentElements and StatefulElements
    /// return None.
    ///
    /// # Returns
    ///
    /// Reference to the RenderObject, or None if this element doesn't have one
    fn render_object(&self) -> Option<&dyn crate::RenderObject> {
        None
    }

    /// Get mutable RenderObject if this element has one
    ///
    /// # Returns
    ///
    /// Mutable reference to the RenderObject, or None if this element doesn't have one
    fn render_object_mut(&mut self) -> Option<&mut dyn crate::RenderObject> {
        None
    }

    /// Take old child ID before rebuild (for ComponentElement)
    ///
    /// This is used by ElementTree to unmount old children before mounting new ones.
    /// Only ComponentElement needs to implement this.
    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        None
    }

    /// Set child ID after mounting (for ComponentElement)
    ///
    /// This is used by ElementTree to update the parent's child reference after
    /// mounting a new child. Only ComponentElement needs to implement this.
    fn set_child_after_mount(&mut self, _child_id: ElementId) {
        // Default: do nothing
    }

    /// Get child element IDs without acquiring any locks
    ///
    /// This is used internally by ElementTree to traverse the tree without deadlocking.
    /// Returns a Vec of child element IDs.
    fn child_ids(&self) -> Vec<ElementId> {
        Vec::new() // Default: no children
    }
}

// Enable downcasting for Element trait objects
impl_downcast!(sync Element);

/// ComponentElement - for StatelessWidget
///
/// Manages lifecycle of stateless widgets. Calls build() to create child widget tree.
pub struct ComponentElement<W: StatelessWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    /// Child element created by build()
    child: Option<ElementId>,
    /// Reference to element tree for building children
    tree: Option<std::sync::Arc<parking_lot::RwLock<crate::ElementTree>>>,
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
    fn perform_rebuild(&mut self) -> Vec<(ElementId, Box<dyn crate::Widget>, usize)> {
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
        let context = BuildContext::new(tree.clone(), self.id);

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

impl<W: StatelessWidget> Element for ComponentElement<W> {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Unmount child if exists
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                let mut tree_guard = tree.write();
                tree_guard.unmount_element(child_id);
            }
        }
    }

    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            self.widget = *widget;
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn crate::Widget>, usize)> {
        self.perform_rebuild()
    }

    fn id(&self) -> ElementId {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn Key> {
        self.widget.key()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn Element)) {
        if let Some(child_id) = self.child {
            if let Some(tree) = &self.tree {
                let tree_guard = tree.read();
                if let Some(child_element) = tree_guard.get_element(child_id) {
                    visitor(child_element);
                }
            }
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn Element)) {
        if let Some(child_id) = self.child {
            if let Some(tree) = &self.tree {
                let mut tree_guard = tree.write();
                if let Some(child_element) = tree_guard.get_element_mut(child_id) {
                    visitor(child_element);
                }
            }
        }
    }

    fn set_tree_ref(&mut self, tree: std::sync::Arc<parking_lot::RwLock<crate::ElementTree>>) {
        self.tree = Some(tree);
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        self.take_old_child()
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.set_child(child_id)
    }

    fn child_ids(&self) -> Vec<ElementId> {
        if let Some(child_id) = self.child {
            vec![child_id]
        } else {
            Vec::new()
        }
    }
}

/// StatefulElement - for StatefulWidget
///
/// Manages lifecycle of stateful widgets. Holds State object that persists across rebuilds.
pub struct StatefulElement {
    id: ElementId,
    parent: Option<ElementId>,
    dirty: bool,
    /// The widget that created this element
    widget: Option<Box<dyn Any + Send + Sync>>,
    /// The state object
    state: Option<Box<dyn crate::State>>,
    /// Child element ID
    child: Option<ElementId>,
    /// Tree reference for mounting children
    tree: Option<Arc<RwLock<ElementTree>>>,
}

impl StatefulElement {
    /// Create new stateful element with widget and state
    pub fn new<W: crate::StatefulWidget>(widget: W) -> Self {
        let state = widget.create_state();
        Self {
            id: ElementId::new(),
            parent: None,
            dirty: true,
            widget: Some(Box::new(widget)),
            state: Some(Box::new(state)),
            child: None,
            tree: None,
        }
    }
}


impl fmt::Debug for StatefulElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatefulElement")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("has_widget", &self.widget.is_some())
            .field("has_state", &self.state.is_some())
            .field("child", &self.child)
            .finish()
    }
}

impl Element for StatefulElement {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;

        // Call init_state() on first mount
        if let Some(state) = &mut self.state {
            state.init_state();
        }
    }

    fn unmount(&mut self) {
        // Unmount child first
        if let Some(child_id) = self.child.take() {
            if let Some(tree) = &self.tree {
                tree.write().unmount_element(child_id);
            }
        }

        // Call dispose() on state
        if let Some(state) = &mut self.state {
            state.dispose();
        }
    }

    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>) {
        // Store old widget for did_update_widget
        let old_widget = self.widget.take();
        self.widget = Some(new_widget);

        // Call did_update_widget() on state
        if let Some(state) = &mut self.state {
            if let Some(old) = old_widget.as_ref() {
                state.did_update_widget(old.as_ref());
            }
        }

        self.dirty = true;
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn crate::Widget>, usize)> {
        if !self.dirty {
            return Vec::new();
        }
        self.dirty = false;

        // Call build() on state
        if let Some(state) = &mut self.state {
            if let Some(tree) = &self.tree {
                let context = crate::BuildContext::new(tree.clone(), self.id);
                let child_widget = state.build(&context);

                // Mark old child for unmounting
                self.child = None;

                // Return child to mount
                return vec![(self.id, child_widget, 0)];
            }
        }

        Vec::new()
    }

    fn id(&self) -> ElementId {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn set_tree_ref(&mut self, tree: std::sync::Arc<parking_lot::RwLock<crate::ElementTree>>) {
        self.tree = Some(tree);
    }

    fn take_old_child_for_rebuild(&mut self) -> Option<ElementId> {
        self.child.take()
    }

    fn set_child_after_mount(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    fn child_ids(&self) -> Vec<ElementId> {
        if let Some(child_id) = self.child {
            vec![child_id]
        } else {
            Vec::new()
        }
    }
}

impl StatefulElement {
    /// Set tree reference (called by ElementTree after mounting)
    pub(crate) fn set_tree(&mut self, tree: Arc<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    /// Set child element ID
    pub(crate) fn set_child(&mut self, child_id: ElementId) {
        self.child = Some(child_id);
    }

    /// Get child element ID
    pub(crate) fn child(&self) -> Option<ElementId> {
        self.child
    }
}

/// RenderObjectElement - for RenderObjectWidget
///
/// Manages lifecycle of render object widgets. Holds the RenderObject that performs
/// layout and painting.
///
/// Similar to Flutter's RenderObjectElement.
pub struct RenderObjectElement<W: RenderObjectWidget> {
    id: ElementId,
    widget: W,
    parent: Option<ElementId>,
    dirty: bool,
    render_object: Option<Box<dyn RenderObject>>,
}

impl<W: RenderObjectWidget> RenderObjectElement<W> {
    /// Create new render object element from a widget
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
            render_object: None,
        }
    }

    /// Get reference to the render object
    pub fn render_object(&self) -> Option<&dyn RenderObject> {
        self.render_object.as_ref().map(|r| r.as_ref())
    }

    /// Get mutable reference to the render object
    pub fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
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
            .field("id", &self.id)
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .field("has_render_object", &self.render_object.is_some())
            .finish()
    }
}

impl<W: RenderObjectWidget> Element for RenderObjectElement<W> {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.initialize_render_object();
        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Clean up render object
        self.render_object = None;
    }

    fn update(&mut self, new_widget: Box<dyn Any + Send + Sync>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            self.widget = *widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) -> Vec<(ElementId, Box<dyn crate::Widget>, usize)> {
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

    fn id(&self) -> ElementId {
        self.id
    }

    fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    fn key(&self) -> Option<&dyn Key> {
        self.widget.key()
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn widget_type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<W>()
    }

    fn render_object(&self) -> Option<&dyn crate::RenderObject> {
        self.render_object.as_ref().map(|ro| ro.as_ref())
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn crate::RenderObject> {
        self.render_object.as_mut().map(|ro| ro.as_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_id_unique() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_element_id_display() {
        let id = ElementId(42);
        assert_eq!(format!("{}", id), "ElementId(42)");
    }

    // Test helper for StatefulWidget
    #[derive(Debug, Clone)]
    struct TestStatefulWidget {
        value: i32,
    }

    #[derive(Debug)]
    struct TestState {
        count: i32,
    }

    impl crate::StatefulWidget for TestStatefulWidget {
        type State = TestState;

        fn create_state(&self) -> Self::State {
            TestState { count: self.value }
        }
    }

    impl crate::State for TestState {
        fn build(&mut self, _context: &crate::BuildContext) -> Box<dyn crate::Widget> {
            // Return a simple widget for testing
            Box::new(TestStatefulWidget { value: self.count })
        }
    }

    // Manual Widget impl (no blanket impl for StatefulWidget)
    impl crate::Widget for TestStatefulWidget {
        fn create_element(&self) -> Box<dyn Element> {
            Box::new(StatefulElement::new(self.clone()))
        }
    }

    #[test]
    fn test_stateful_element_creation() {
        let widget = TestStatefulWidget { value: 42 };
        let element = StatefulElement::new(widget);
        assert!(element.is_dirty());
        assert_eq!(element.parent(), None);
    }

    #[test]
    fn test_stateful_element_mount() {
        let widget = TestStatefulWidget { value: 42 };
        let mut element = StatefulElement::new(widget);
        let parent_id = ElementId(100);

        element.mount(Some(parent_id), 0);

        assert_eq!(element.parent(), Some(parent_id));
        assert!(element.is_dirty());
    }

    #[test]
    fn test_stateful_element_mark_dirty() {
        let widget = TestStatefulWidget { value: 42 };
        let mut element = StatefulElement::new(widget);
        element.dirty = false;

        assert!(!element.is_dirty());

        element.mark_dirty();
        assert!(element.is_dirty());
    }

    #[test]
    fn test_element_downcast() {
        let widget = TestStatefulWidget { value: 42 };
        let element = StatefulElement::new(widget);
        let boxed: Box<dyn Element> = Box::new(element);

        // Test is() check
        assert!(boxed.is::<StatefulElement>());

        // Test downcast_ref
        let downcasted = boxed.downcast_ref::<StatefulElement>().unwrap();
        assert!(downcasted.is_dirty());
    }

    #[test]
    fn test_element_downcast_mut() {
        let widget = TestStatefulWidget { value: 42 };
        let element = StatefulElement::new(widget);
        let mut boxed: Box<dyn Element> = Box::new(element);

        boxed.downcast_mut::<StatefulElement>().unwrap().dirty = false;

        assert!(!boxed.downcast_ref::<StatefulElement>().unwrap().is_dirty());
    }

    #[test]
    fn test_element_downcast_owned() {
        let widget = TestStatefulWidget { value: 42 };
        let element = StatefulElement::new(widget);
        let id = element.id();
        let boxed: Box<dyn Element> = Box::new(element);

        // Consume and downcast
        let owned: Box<StatefulElement> = boxed.downcast().ok().unwrap();
        assert_eq!(owned.id(), id);
    }

    // RenderObjectElement tests

    use crate::{BoxConstraints, LeafRenderObjectWidget, Offset, RenderObject, RenderObjectWidget, Size, Widget};

    // Test render object
    #[derive(Debug)]
    struct TestRenderBox {
        size: Size,
        needs_layout_flag: bool,
        needs_paint_flag: bool,
        update_count: usize,
    }

    impl TestRenderBox {
        fn new() -> Self {
            Self {
                size: Size::zero(),
                needs_layout_flag: true,
                needs_paint_flag: true,
                update_count: 0,
            }
        }
    }

    impl RenderObject for TestRenderBox {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.size = constraints.biggest();
            self.needs_layout_flag = false;
            self.size
        }

        fn paint(&self, _painter: &egui::Painter, _offset: Offset) {
            // Test implementation
        }

        fn size(&self) -> Size {
            self.size
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
    }

    // Test widget
    #[derive(Debug, Clone)]
    struct TestRenderWidget {
        width: f32,
        height: f32,
    }

    impl Widget for TestRenderWidget {
        fn create_element(&self) -> Box<dyn Element> {
            Box::new(RenderObjectElement::new(self.clone()))
        }
    }

    impl RenderObjectWidget for TestRenderWidget {
        fn create_render_object(&self) -> Box<dyn RenderObject> {
            Box::new(TestRenderBox::new())
        }

        fn update_render_object(&self, render_object: &mut dyn RenderObject) {
            if let Some(render_box) = render_object.downcast_mut::<TestRenderBox>() {
                render_box.update_count += 1;
                render_box.mark_needs_layout();
            }
        }
    }

    impl LeafRenderObjectWidget for TestRenderWidget {}

    #[test]
    fn test_render_object_element_creation() {
        let widget = TestRenderWidget {
            width: 100.0,
            height: 50.0,
        };
        let element = RenderObjectElement::new(widget);

        assert!(element.is_dirty());
        assert_eq!(element.parent(), None);
        assert!(element.render_object().is_none());
    }

    #[test]
    fn test_render_object_element_mount() {
        let widget = TestRenderWidget {
            width: 100.0,
            height: 50.0,
        };
        let mut element = RenderObjectElement::new(widget);
        let parent_id = ElementId(100);

        element.mount(Some(parent_id), 0);

        assert_eq!(element.parent(), Some(parent_id));
        assert!(element.is_dirty());
        assert!(element.render_object().is_some());
    }

    #[test]
    fn test_render_object_element_render_object_access() {
        let widget = TestRenderWidget {
            width: 100.0,
            height: 50.0,
        };
        let mut element = RenderObjectElement::new(widget);

        element.mount(None, 0);

        // Test immutable access
        let render_obj = element.render_object().unwrap();
        assert!(render_obj.is::<TestRenderBox>());

        // Test mutable access
        let render_obj_mut = element.render_object_mut().unwrap();
        render_obj_mut.downcast_mut::<TestRenderBox>().unwrap().update_count = 42;

        // Verify change
        let render_obj = element.render_object().unwrap();
        let test_box = render_obj.downcast_ref::<TestRenderBox>().unwrap();
        assert_eq!(test_box.update_count, 42);
    }

    #[test]
    fn test_render_object_element_update() {
        let widget = TestRenderWidget {
            width: 100.0,
            height: 50.0,
        };
        let mut element = RenderObjectElement::new(widget);

        element.mount(None, 0);
        element.rebuild();

        // Get initial update count
        let render_obj = element.render_object().unwrap();
        let initial_count = render_obj.downcast_ref::<TestRenderBox>().unwrap().update_count;

        // Update with new widget
        let new_widget = TestRenderWidget {
            width: 200.0,
            height: 100.0,
        };
        element.update(Box::new(new_widget));

        // Verify update_render_object was called
        let render_obj = element.render_object().unwrap();
        let new_count = render_obj.downcast_ref::<TestRenderBox>().unwrap().update_count;
        assert_eq!(new_count, initial_count + 1);
        assert!(element.is_dirty());
    }

    #[test]
    fn test_render_object_element_unmount() {
        let widget = TestRenderWidget {
            width: 100.0,
            height: 50.0,
        };
        let mut element = RenderObjectElement::new(widget);

        element.mount(None, 0);
        assert!(element.render_object().is_some());

        element.unmount();
        assert!(element.render_object().is_none());
    }

    #[test]
    fn test_render_object_element_rebuild() {
        let widget = TestRenderWidget {
            width: 100.0,
            height: 50.0,
        };
        let mut element = RenderObjectElement::new(widget);

        element.mount(None, 0);
        assert!(element.is_dirty());

        element.rebuild();
        assert!(!element.is_dirty());

        element.mark_dirty();
        assert!(element.is_dirty());

        element.rebuild();
        assert!(!element.is_dirty());
    }
}
