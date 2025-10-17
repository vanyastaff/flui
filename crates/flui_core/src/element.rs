//! Element tree - mutable state holders for widgets
//!
//! This module provides the Element trait and implementations, which form the middle
//! layer of the three-tree architecture (Widget → Element → RenderObject).

use std::any::Any;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use flui_foundation::Key;

use crate::{BuildContext, StatelessWidget};

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
pub trait Element: Any + fmt::Debug + Send + Sync {
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

    /// Get the widget as Any for downcasting
    fn widget_any(&self) -> &dyn Any;

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

    fn widget_any(&self) -> &dyn Any {
        &self.widget
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

    fn widget_any(&self) -> &dyn Any {
        &() // Placeholder
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
}
