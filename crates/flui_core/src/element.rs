//! Element tree - mutable state holders for widgets
//!
//! This module provides the Element trait and implementations, which form the middle
//! layer of the three-tree architecture (Widget → Element → RenderObject).

use std::any::Any;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use downcast_rs::{impl_downcast, DowncastSync};
use flui_foundation::Key;

use crate::{BuildContext, RenderObject, RenderObjectWidget, StatelessWidget};

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
    fn update(&mut self, new_widget: Box<dyn Any>);

    /// Rebuild this element's subtree
    ///
    /// Called when element is marked dirty. Should rebuild child widgets and
    /// update child elements.
    fn rebuild(&mut self);

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
}

impl<W: StatelessWidget> ComponentElement<W> {
    /// Create new component element from a widget
    pub fn new(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            parent: None,
            dirty: true,
        }
    }

    /// Perform rebuild
    fn perform_rebuild(&mut self) {
        if !self.dirty {
            return;
        }

        self.dirty = false;

        // Create build context
        let context = BuildContext::new();

        // Call build() on the widget
        // In a full implementation, this would create/update child elements
        let _child_widget = self.widget.build(&context);

        // TODO: Handle child element creation/update
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
        // TODO: Unmount children
    }

    fn update(&mut self, _new_widget: Box<dyn Any>) {
        // TODO: Update widget and mark dirty
        self.dirty = true;
    }

    fn rebuild(&mut self) {
        self.perform_rebuild();
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
}

/// StatefulElement - for StatefulWidget
///
/// Manages lifecycle of stateful widgets. Holds State object that persists across rebuilds.
pub struct StatefulElement {
    id: ElementId,
    parent: Option<ElementId>,
    dirty: bool,
    // TODO: Add state field when StatefulWidget is implemented
}

impl StatefulElement {
    /// Create new stateful element
    pub fn new() -> Self {
        Self {
            id: ElementId::new(),
            parent: None,
            dirty: true,
        }
    }
}

impl Default for StatefulElement {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for StatefulElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatefulElement")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .finish()
    }
}

impl Element for StatefulElement {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
        // TODO: Create state and call init_state()
    }

    fn unmount(&mut self) {
        // TODO: Call dispose() on state and unmount children
    }

    fn update(&mut self, _new_widget: Box<dyn Any>) {
        // TODO: Call did_update_widget() on state
        self.dirty = true;
    }

    fn rebuild(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        // TODO: Call build() on state
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

    fn update(&mut self, new_widget: Box<dyn Any>) {
        // Try to downcast to our widget type
        if let Ok(widget) = new_widget.downcast::<W>() {
            self.widget = *widget;
            self.update_render_object();
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;

        // Update render object if needed
        self.update_render_object();

        // RenderObjectElement typically doesn't have child elements
        // (those are managed by specific subclasses)
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

    #[test]
    fn test_stateful_element_creation() {
        let element = StatefulElement::new();
        assert!(element.is_dirty());
        assert_eq!(element.parent(), None);
    }

    #[test]
    fn test_stateful_element_mount() {
        let mut element = StatefulElement::new();
        let parent_id = ElementId(100);

        element.mount(Some(parent_id), 0);

        assert_eq!(element.parent(), Some(parent_id));
        assert!(element.is_dirty());
    }

    #[test]
    fn test_stateful_element_mark_dirty() {
        let mut element = StatefulElement::new();
        element.dirty = false;

        assert!(!element.is_dirty());

        element.mark_dirty();
        assert!(element.is_dirty());
    }

    #[test]
    fn test_element_downcast() {
        let element = StatefulElement::new();
        let boxed: Box<dyn Element> = Box::new(element);

        // Test is() check
        assert!(boxed.is::<StatefulElement>());

        // Test downcast_ref
        let downcasted = boxed.downcast_ref::<StatefulElement>().unwrap();
        assert!(downcasted.is_dirty());
    }

    #[test]
    fn test_element_downcast_mut() {
        let element = StatefulElement::new();
        let mut boxed: Box<dyn Element> = Box::new(element);

        boxed.downcast_mut::<StatefulElement>().unwrap().dirty = false;

        assert!(!boxed.downcast_ref::<StatefulElement>().unwrap().is_dirty());
    }

    #[test]
    fn test_element_downcast_owned() {
        let element = StatefulElement::new();
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
