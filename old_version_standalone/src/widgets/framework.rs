//! Core widget framework - Element, BuildContext, and widget lifecycle
//!
//! This module provides the Flutter-like three-tree architecture:
//! - Widget tree (immutable, declarative)
//! - Element tree (mutable state holder)
//! - Render tree (layout and paint)

use std::any::{Any, TypeId};
use std::fmt;
use std::sync::{Arc, RwLock, Weak};
use std::collections::HashMap;
use crate::core::Key;
use crate::widgets::RenderConstraints;
use crate::types::core::Size;

/// Trait for widgets that can build child widgets
///
/// This is an internal trait that allows ComponentElement to call build()
/// on StatelessWidgets without knowing their concrete type.
pub trait BuildableWidget: Any + fmt::Debug {
    /// Build this widget's child widget tree
    fn build(&self, context: &BuildContext) -> Box<dyn Any>;

    /// Get the key if present
    fn key(&self) -> Option<&dyn Key> {
        None
    }
}

// ============================================================================
// Widget Traits (Flutter-like)
// ============================================================================

/// StatelessWidget - immutable widget that builds once
///
/// Similar to Flutter's StatelessWidget. Build method creates child widget tree.
pub trait StatelessWidget: fmt::Debug + 'static {
    /// Build this widget's child widget tree
    fn build(&self, context: &BuildContext) -> Box<dyn Any>;

    /// Optional key for widget identification
    fn key(&self) -> Option<&dyn Key> {
        None
    }

    /// Create element for this widget
    fn create_element(&self) -> Box<dyn Element>
    where
        Self: Clone + Sized,
    {
        Box::new(ComponentElement::new(self.clone()))
    }
}

/// Automatically implement BuildableWidget for all StatelessWidgets
impl<T: StatelessWidget> BuildableWidget for T {
    fn build(&self, context: &BuildContext) -> Box<dyn Any> {
        StatelessWidget::build(self, context)
    }

    fn key(&self) -> Option<&dyn Key> {
        StatelessWidget::key(self)
    }
}

/// StatefulWidget - widget with mutable state
///
/// Similar to Flutter's StatefulWidget. Creates a State object that persists across rebuilds.
pub trait StatefulWidget: fmt::Debug + 'static {
    /// Associated State type
    type State: State;

    /// Create the state object
    fn create_state(&self) -> Self::State;

    /// Optional key for widget identification
    fn key(&self) -> Option<&dyn Key> {
        None
    }

    /// Create element for this widget
    fn create_element(&self) -> Box<dyn Element>
    where
        Self: Clone + Sized,
    {
        Box::new(StatefulElement::new(Box::new(self.clone())))
    }
}

/// State - mutable state for StatefulWidget
///
/// Similar to Flutter's State. Holds mutable state and builds widget tree.
pub trait State: Any + fmt::Debug {
    /// Build the widget tree
    fn build(&mut self, context: &BuildContext) -> Box<dyn Any>;

    /// Called when state is first created
    fn init_state(&mut self) {}

    /// Called when widget configuration changes
    fn did_update_widget(&mut self, _old_widget: &dyn Any) {}

    /// Called when removed from tree
    fn dispose(&mut self) {}

    /// Internal: get build context
    fn get_context(&self) -> Option<BuildContext> {
        None
    }

    /// Internal: set build context
    fn set_context(&mut self, _context: BuildContext) {}

    /// Request rebuild (calls setState in Flutter)
    fn mark_needs_build(&mut self) {
        if let Some(ctx) = self.get_context() {
            ctx.mark_needs_build();
        }
    }
}

/// Unique identifier for elements in the tree
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub u64);

impl ElementId {
    /// Generate a new unique element ID
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for ElementId {
    fn default() -> Self {
        Self::new()
    }
}

/// BuildContext provides access to the element tree and services
///
/// Similar to Flutter's BuildContext. Passed to build() methods.
#[derive(Clone)]
pub struct BuildContext {
    /// The element ID this context belongs to
    pub element_id: ElementId,

    /// Weak reference to the element tree (to avoid cycles)
    tree: Weak<RwLock<ElementTree>>,
}

impl BuildContext {
    /// Create a new build context
    pub fn new(element_id: ElementId, tree: Weak<RwLock<ElementTree>>) -> Self {
        Self { element_id, tree }
    }

    /// Mark this element as needing rebuild
    pub fn mark_needs_build(&self) {
        if let Some(tree) = self.tree.upgrade() {
            if let Ok(mut tree) = tree.write() {
                tree.mark_dirty(self.element_id);
            }
        }
    }

    /// Get the element's size (after layout)
    pub fn size(&self) -> Option<Size> {
        if let Some(tree) = self.tree.upgrade() {
            if let Ok(tree) = tree.read() {
                return tree.get_element_size(self.element_id);
            }
        }
        None
    }
}

impl fmt::Debug for BuildContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildContext")
            .field("element_id", &self.element_id)
            .finish()
    }
}

/// Core Element trait - mutable state holder in element tree
///
/// Similar to Flutter's Element. Elements manage lifecycle and hold widget references.
pub trait Element: Any + fmt::Debug {
    /// Mount this element into the tree
    fn mount(&mut self, parent: Option<ElementId>, slot: usize);

    /// Unmount and clean up this element
    fn unmount(&mut self);

    /// Update this element with a new widget configuration
    fn update(&mut self, new_widget: &dyn Any);

    /// Rebuild this element's subtree
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

    /// Visit child elements
    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn Element)) {
        // Default: no children
    }

    /// Visit child elements mutably
    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn Element)) {
        // Default: no children
    }
}

/// Check if two widgets can update in place
///
/// Two widgets can update if they have the same type and compatible keys:
/// - Both have no key: can update
/// - One has key, other doesn't: cannot update
/// - Both have keys: can update only if keys are equal
pub fn can_update_widget(old_widget: &dyn Any, new_widget: &dyn Any) -> bool {
    // Same type required
    if old_widget.type_id() != new_widget.type_id() {
        return false;
    }

    // Try to get keys from BuildableWidget trait
    let old_key = old_widget
        .downcast_ref::<&dyn BuildableWidget>()
        .and_then(|w| w.key());

    let new_key = new_widget
        .downcast_ref::<&dyn BuildableWidget>()
        .and_then(|w| w.key());

    // Check key compatibility
    match (old_key, new_key) {
        (None, None) => true,           // Both have no key
        (Some(_), None) => false,       // One has key, other doesn't
        (None, Some(_)) => false,       // One has key, other doesn't
        (Some(k1), Some(k2)) => {
            // Both have keys - check equality
            k1.type_id() == k2.type_id() && k1.equals(k2)
        }
    }
}

/// Helper function to create an element from a widget descriptor
///
/// This is used during rebuild to create elements for new child widgets.
/// The widget_descriptor should be a boxed widget type that implements create_element().
fn create_element_from_widget(widget_descriptor: Box<dyn Any>) -> Option<Box<dyn Element>> {
    // For now, we can't easily create elements without knowing the widget type
    // This is a placeholder - in a real implementation, we'd need a registry
    // or the Widget trait would need to be object-safe with create_element()
    None
}

/// Element tree manager
///
/// Manages the element tree lifecycle, dirty tracking, and rebuilds
pub struct ElementTree {
    /// Root element
    root: Option<Box<dyn Element>>,

    /// Dirty elements that need rebuild
    dirty_elements: Vec<ElementId>,

    /// All elements by ID (for lookup)
    elements: std::collections::HashMap<ElementId, *mut dyn Element>,
}

impl ElementTree {
    /// Create a new empty element tree
    pub fn new() -> Self {
        Self {
            root: None,
            dirty_elements: Vec::new(),
            elements: std::collections::HashMap::new(),
        }
    }

    /// Mount a widget as the root of the tree
    pub fn mount_root<W: Any>(&mut self, _widget: W)
    where
        W: 'static,
    {
        // For now, we need to know if this is a Widget trait object
        // In real implementation, W would implement Widget trait
        // This is a simplified version
        // TODO: Implement proper widget mounting
    }

    /// Set the root element (low-level method)
    pub fn set_root(&mut self, mut element: Box<dyn Element>) {
        let id = element.id();
        element.mount(None, 0);

        // Register in elements map
        let ptr = &mut *element as *mut dyn Element;
        self.elements.insert(id, ptr);

        self.root = Some(element);
    }

    /// Mark an element as dirty (needs rebuild)
    pub fn mark_dirty(&mut self, element_id: ElementId) {
        if !self.dirty_elements.contains(&element_id) {
            self.dirty_elements.push(element_id);
        }
    }

    /// Rebuild all dirty elements
    pub fn rebuild_dirty(&mut self) {
        if self.dirty_elements.is_empty() {
            return;
        }

        // Sort by depth (rebuild parents before children)
        // This ensures parent rebuilds happen before child rebuilds
        // We need to collect depths first to avoid borrow checker issues
        let mut depths: Vec<(ElementId, usize)> = self
            .dirty_elements
            .iter()
            .map(|id| (*id, self.depth_of(*id)))
            .collect();

        depths.sort_by_key(|(_, depth)| *depth);

        let dirty: Vec<ElementId> = depths.into_iter().map(|(id, _)| id).collect();
        self.dirty_elements.clear();

        for element_id in dirty {
            // Find and rebuild the element
            self.rebuild_element(element_id);
        }
    }

    /// Rebuild a specific element
    fn rebuild_element(&mut self, element_id: ElementId) {
        // Simplified: only handles root for now
        if let Some(root) = &mut self.root {
            if root.id() == element_id {
                root.rebuild();
            } else {
                // TODO: Traverse tree to find element
                // For now, use visitor pattern
                let mut found = false;
                root.visit_children_mut(&mut |child| {
                    if child.id() == element_id {
                        child.rebuild();
                        found = true;
                    }
                });

                // If not found in immediate children, search deeper
                // This is simplified - real implementation would do full DFS
            }
        }
    }

    /// Calculate depth of element in tree
    pub fn depth_of(&self, element_id: ElementId) -> usize {
        // Start from element and walk up to root counting steps
        let mut depth = 0;
        let mut current_id = element_id;

        // Walk up the tree
        loop {
            // Find element
            let parent = if let Some(root) = &self.root {
                if root.id() == current_id {
                    break; // Reached root
                }

                // Search for element in tree
                let mut found_parent: Option<ElementId> = None;
                root.visit_children(&mut |child| {
                    if child.id() == current_id {
                        found_parent = child.parent();
                    }
                });
                found_parent
            } else {
                None
            };

            if let Some(parent_id) = parent {
                depth += 1;
                current_id = parent_id;
            } else {
                break;
            }
        }

        depth
    }

    /// Check if there are dirty elements
    pub fn has_dirty_elements(&self) -> bool {
        !self.dirty_elements.is_empty()
    }

    /// Get element size (after layout)
    pub fn get_element_size(&self, _element_id: ElementId) -> Option<Size> {
        // TODO: Implement when render objects are added
        None
    }

    /// Get root element (for debugging/inspection)
    pub fn root(&self) -> Option<&dyn Element> {
        self.root.as_ref().map(|e| e.as_ref())
    }
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for ElementTree {}
unsafe impl Sync for ElementTree {}

// ============================================================================
// Element Implementations
// ============================================================================

/// ComponentElement - for StatelessWidget
///
/// Manages lifecycle of stateless widgets. Builds child widget on mount/rebuild.
pub struct ComponentElement {
    id: ElementId,
    widget: Box<dyn BuildableWidget>,
    child: Option<Box<dyn Element>>,
    parent: Option<ElementId>,
    dirty: bool,
    tree: Option<Weak<RwLock<ElementTree>>>,
}

impl fmt::Debug for ComponentElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentElement")
            .field("id", &self.id)
            .field("widget", &self.widget)
            .field("has_child", &self.child.is_some())
            .field("parent", &self.parent)
            .field("dirty", &self.dirty)
            .finish()
    }
}

impl ComponentElement {
    /// Create new component element from a buildable widget
    pub fn new<W: BuildableWidget + 'static>(widget: W) -> Self {
        Self {
            id: ElementId::new(),
            widget: Box::new(widget),
            child: None,
            parent: None,
            dirty: true,
            tree: None,
        }
    }

    /// Create from boxed buildable widget
    pub fn from_boxed(widget: Box<dyn BuildableWidget>) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            child: None,
            parent: None,
            dirty: true,
            tree: None,
        }
    }

    /// Set tree reference
    pub fn set_tree(&mut self, tree: Weak<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    /// Perform build
    fn perform_rebuild(&mut self) {
        if !self.dirty {
            return;
        }

        self.dirty = false;

        // Create build context
        let context = BuildContext::new(
            self.id,
            self.tree.clone().unwrap_or_else(|| Weak::new()),
        );

        // Call build() on the BuildableWidget
        // This returns Box<dyn Any> which represents the child widget descriptor
        let new_child_widget_descriptor = self.widget.build(&context);

        // Handle the child element update/creation
        match &mut self.child {
            Some(existing_child) => {
                // Check if we can update the existing child
                if can_update_widget(existing_child.widget_any(), new_child_widget_descriptor.as_ref()) {
                    // Update in place
                    existing_child.update(new_child_widget_descriptor.as_ref());
                    existing_child.rebuild();
                } else {
                    // Different widget type or key - need to replace
                    existing_child.unmount();

                    // Try to create new element from widget descriptor
                    if let Some(new_element) = create_element_from_widget(new_child_widget_descriptor) {
                        let mut element = new_element;
                        element.mount(Some(self.id), 0);
                        self.child = Some(element);
                    } else {
                        // Could not create element - clear child
                        self.child = None;
                    }
                }
            }
            None => {
                // No existing child - create new one
                if let Some(new_element) = create_element_from_widget(new_child_widget_descriptor) {
                    let mut element = new_element;
                    element.mount(Some(self.id), 0);
                    self.child = Some(element);
                }
            }
        }
    }
}

impl Element for ComponentElement {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
    }

    fn update(&mut self, new_widget: &dyn Any) {
        // Check if can update
        if can_update_widget(self.widget.as_ref() as &dyn Any, new_widget) {
            // Update widget reference (need to clone/copy the new widget)
            // This is tricky without Clone trait on Any
            // For now, mark as dirty
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) {
        self.perform_rebuild();
    }

    fn id(&self) -> ElementId {
        self.id
    }

    fn widget_any(&self) -> &dyn Any {
        self.widget.as_ref() as &dyn Any
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
        if let Some(child) = &self.child {
            visitor(child.as_ref());
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn Element)) {
        if let Some(child) = &mut self.child {
            visitor(child.as_mut());
        }
    }
}

/// StatefulElement - for StatefulWidget
///
/// Manages lifecycle of stateful widgets. Holds State object that persists across rebuilds.
#[derive(Debug)]
pub struct StatefulElement {
    id: ElementId,
    widget: Box<dyn Any>,
    state: Option<Box<dyn Any>>, // Holds the State
    child: Option<Box<dyn Element>>,
    parent: Option<ElementId>,
    dirty: bool,
    tree: Option<Weak<RwLock<ElementTree>>>,
}

impl StatefulElement {
    /// Create new stateful element
    pub fn new(widget: Box<dyn Any>) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            state: None,
            child: None,
            parent: None,
            dirty: true,
            tree: None,
        }
    }

    /// Set tree reference
    pub fn set_tree(&mut self, tree: Weak<RwLock<ElementTree>>) {
        self.tree = Some(tree);
    }

    /// Perform build
    fn perform_rebuild(&mut self) {
        if !self.dirty {
            return;
        }

        self.dirty = false;

        // Create build context
        let context = BuildContext::new(
            self.id,
            self.tree.clone().unwrap_or_else(|| Weak::new()),
        );

        // Call build on state
        // TODO: Implement proper state building
    }
}

impl Element for StatefulElement {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;

        // Create state if not exists
        // TODO: Call create_state on widget

        // Call init_state on state
        // TODO: Implement

        self.dirty = true;
    }

    fn unmount(&mut self) {
        // Call dispose on state
        // TODO: Implement

        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
        self.state = None;
    }

    fn update(&mut self, new_widget: &dyn Any) {
        if can_update_widget(self.widget.as_ref(), new_widget) {
            // Call did_update_widget on state
            // TODO: Implement
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) {
        self.perform_rebuild();
    }

    fn id(&self) -> ElementId {
        self.id
    }

    fn widget_any(&self) -> &dyn Any {
        self.widget.as_ref()
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

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn Element)) {
        if let Some(child) = &self.child {
            visitor(child.as_ref());
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn Element)) {
        if let Some(child) = &mut self.child {
            visitor(child.as_mut());
        }
    }
}

/// SingleChildElement - for widgets with one child (Container, Padding, etc.)
///
/// Similar to Flutter's SingleChildRenderObjectElement
#[derive(Debug)]
pub struct SingleChildElement {
    id: ElementId,
    widget: Box<dyn Any>,
    child: Option<Box<dyn Element>>,
    parent: Option<ElementId>,
    dirty: bool,
}

impl SingleChildElement {
    /// Create new single child element
    pub fn new(widget: Box<dyn Any>) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            child: None,
            parent: None,
            dirty: true,
        }
    }
}

impl Element for SingleChildElement {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
    }

    fn update(&mut self, new_widget: &dyn Any) {
        if can_update_widget(self.widget.as_ref(), new_widget) {
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;

        // Update child if needed
        // TODO: Get child widget from widget and mount/update
    }

    fn id(&self) -> ElementId {
        self.id
    }

    fn widget_any(&self) -> &dyn Any {
        self.widget.as_ref()
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

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn Element)) {
        if let Some(child) = &self.child {
            visitor(child.as_ref());
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn Element)) {
        if let Some(child) = &mut self.child {
            visitor(child.as_mut());
        }
    }
}

/// MultiChildElement - for widgets with multiple children (Row, Column, etc.)
///
/// Similar to Flutter's MultiChildRenderObjectElement
#[derive(Debug)]
pub struct MultiChildElement {
    id: ElementId,
    widget: Box<dyn Any>,
    children: Vec<Box<dyn Element>>,
    parent: Option<ElementId>,
    dirty: bool,
}

impl MultiChildElement {
    /// Create new multi child element
    pub fn new(widget: Box<dyn Any>) -> Self {
        Self {
            id: ElementId::new(),
            widget,
            children: Vec::new(),
            parent: None,
            dirty: true,
        }
    }
}

impl Element for MultiChildElement {
    fn mount(&mut self, parent: Option<ElementId>, _slot: usize) {
        self.parent = parent;
        self.dirty = true;
    }

    fn unmount(&mut self) {
        for child in &mut self.children {
            child.unmount();
        }
        self.children.clear();
    }

    fn update(&mut self, new_widget: &dyn Any) {
        if can_update_widget(self.widget.as_ref(), new_widget) {
            self.dirty = true;
        }
    }

    fn rebuild(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;

        // Update children if needed
        // TODO: Get children widgets from widget and mount/update
    }

    fn id(&self) -> ElementId {
        self.id
    }

    fn widget_any(&self) -> &dyn Any {
        self.widget.as_ref()
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

    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn Element)) {
        for child in &self.children {
            visitor(child.as_ref());
        }
    }

    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn Element)) {
        for child in &mut self.children {
            visitor(child.as_mut());
        }
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
    fn test_element_tree_creation() {
        let tree = ElementTree::new();
        assert!(!tree.has_dirty_elements());
        assert!(tree.root.is_none());
    }

    #[test]
    fn test_element_tree_mark_dirty() {
        let mut tree = ElementTree::new();
        let id = ElementId::new();

        assert!(!tree.has_dirty_elements());

        tree.mark_dirty(id);
        assert!(tree.has_dirty_elements());
        assert_eq!(tree.dirty_elements.len(), 1);

        // Mark same element again - should not duplicate
        tree.mark_dirty(id);
        assert_eq!(tree.dirty_elements.len(), 1);
    }

    #[test]
    fn test_can_update_widget_same_type() {
        let widget1 = 42i32;
        let widget2 = 100i32;

        assert!(can_update_widget(&widget1, &widget2));
    }

    #[test]
    fn test_can_update_widget_different_type() {
        let widget1 = 42i32;
        let widget2 = "string";

        assert!(!can_update_widget(&widget1, &widget2));
    }
}
