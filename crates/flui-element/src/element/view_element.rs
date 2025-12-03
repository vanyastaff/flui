//! ViewElement - Element for component views (Stateless, Stateful, Provider, etc.)
//!
//! ViewElement holds views that build child elements but don't directly
//! participate in layout/paint. The ViewObject handles lifecycle and
//! building logic.
//!
//! # Architecture
//!
//! ```text
//! ViewElement
//!   ├─ base: ElementBase (lifecycle, flags, parent/slot)
//!   ├─ view_object: Box<dyn ViewObject> (type-erased view wrapper)
//!   ├─ view_mode: ViewMode (Stateless, Stateful, Provider, etc.)
//!   ├─ key: Option<Key> (for reconciliation)
//!   ├─ children: Vec<ElementId> (child element IDs)
//!   └─ pending_children: Option<Vec<Element>> (before mount)
//! ```

use std::any::Any;
use std::fmt;

use flui_foundation::{ElementId, Key, Slot};
use flui_view::ViewMode;

use super::{ElementBase, ElementLifecycle};
use crate::ViewObject;

/// ViewElement - Element for component views
///
/// Represents views that build child elements (Stateless, Stateful,
/// Provider, Proxy, Animated). Does NOT directly participate in
/// layout/paint - that's handled by RenderElement.
///
/// # Thread Safety
///
/// ViewElement is `Send` because ViewObject requires `Send`.
pub struct ViewElement {
    /// Common lifecycle fields
    base: ElementBase,

    /// Type-erased view object storage
    view_object: Option<Box<dyn ViewObject>>,

    /// View mode - categorizes the view type
    view_mode: ViewMode,

    /// Optional key for reconciliation
    key: Option<Key>,

    /// Child element IDs (after mount)
    children: Vec<ElementId>,

    /// Pending child elements (before mount, processed by BuildPipeline)
    pending_children: Option<Vec<super::Element>>,

    /// Debug name for diagnostics
    debug_name: Option<&'static str>,
}

impl fmt::Debug for ViewElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewElement")
            .field("parent", &self.base.parent())
            .field("lifecycle", &self.base.lifecycle())
            .field("view_mode", &self.view_mode)
            .field("children_count", &self.children.len())
            .field("has_view_object", &self.view_object.is_some())
            .field("debug_name", &self.debug_name)
            .finish()
    }
}

impl ViewElement {
    /// Creates a new ViewElement with the given view object and mode.
    pub fn new<V: ViewObject>(view_object: V, mode: ViewMode) -> Self {
        debug_assert!(
            mode.is_component() || mode.is_empty(),
            "ViewElement should only be used for component views, got {mode:?}"
        );

        Self {
            base: ElementBase::new(),
            view_object: Some(Box::new(view_object)),
            view_mode: mode,
            key: None,
            children: Vec::new(),
            pending_children: None,
            debug_name: None,
        }
    }

    /// Creates an empty ViewElement (no view object).
    pub fn empty() -> Self {
        Self {
            base: ElementBase::new(),
            view_object: None,
            view_mode: ViewMode::Empty,
            key: None,
            children: Vec::new(),
            pending_children: None,
            debug_name: Some("Empty"),
        }
    }

    /// Creates a container ViewElement with pending children.
    pub fn container(children: Vec<super::Element>) -> Self {
        let child_count = children.len();
        Self {
            base: ElementBase::new(),
            view_object: None,
            view_mode: ViewMode::Empty,
            key: None,
            children: Vec::with_capacity(child_count),
            pending_children: Some(children),
            debug_name: Some("Container"),
        }
    }

    /// Builder: set debug name.
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    /// Builder: set key.
    pub fn with_key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    /// Builder: set pending children.
    pub fn with_pending_children(mut self, children: Vec<super::Element>) -> Self {
        self.pending_children = Some(children);
        self
    }

    // ========== View Mode Queries ==========

    /// Get the view mode.
    #[inline]
    #[must_use]
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    /// Set the view mode.
    #[inline]
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
    }

    /// Check if this is a component view.
    #[inline]
    #[must_use]
    pub fn is_component(&self) -> bool {
        self.view_mode.is_component()
    }

    /// Check if this is a provider view.
    #[inline]
    #[must_use]
    pub fn is_provider(&self) -> bool {
        self.view_mode.is_provider()
    }

    // ========== Key Access ==========

    /// Get the key.
    #[inline]
    #[must_use]
    pub fn key(&self) -> Option<Key> {
        self.key
    }

    /// Set the key.
    #[inline]
    pub fn set_key(&mut self, key: Option<Key>) {
        self.key = key;
    }

    // ========== Pending Children ==========

    /// Take pending children for processing.
    pub fn take_pending_children(&mut self) -> Option<Vec<super::Element>> {
        self.pending_children.take()
    }

    /// Check if element has pending children.
    #[inline]
    #[must_use]
    pub fn has_pending_children(&self) -> bool {
        self.pending_children.is_some()
    }

    // ========== View Object Access ==========

    /// Returns true if this element has a view object.
    #[inline]
    #[must_use]
    pub fn has_view_object(&self) -> bool {
        self.view_object.is_some()
    }

    /// Get the view object as a reference.
    #[inline]
    #[must_use]
    pub fn view_object(&self) -> Option<&dyn ViewObject> {
        self.view_object.as_ref().map(|b| b.as_ref())
    }

    /// Get the view object as a mutable reference.
    #[inline]
    #[must_use]
    pub fn view_object_mut(&mut self) -> Option<&mut dyn ViewObject> {
        self.view_object.as_mut().map(|b| b.as_mut())
    }

    /// Get the view object as Any for downcasting.
    #[inline]
    #[must_use]
    pub fn view_object_any(&self) -> Option<&dyn Any> {
        self.view_object.as_ref().map(|b| b.as_any())
    }

    /// Get the view object as mutable Any for downcasting.
    #[inline]
    #[must_use]
    pub fn view_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        self.view_object.as_mut().map(|b| b.as_any_mut())
    }

    /// Downcast view object to concrete type.
    #[inline]
    pub fn view_object_as<V: Any + Send + Sync + 'static>(&self) -> Option<&V> {
        self.view_object.as_ref()?.as_any().downcast_ref::<V>()
    }

    /// Downcast view object to concrete type (mutable).
    #[inline]
    pub fn view_object_as_mut<V: Any + Send + Sync + 'static>(&mut self) -> Option<&mut V> {
        self.view_object.as_mut()?.as_any_mut().downcast_mut::<V>()
    }

    /// Take the view object out.
    #[inline]
    pub fn take_view_object(&mut self) -> Option<Box<dyn ViewObject>> {
        self.view_object.take()
    }

    /// Set a new view object.
    #[inline]
    pub fn set_view_object<V: ViewObject>(&mut self, view_object: V) {
        self.view_object = Some(Box::new(view_object));
    }

    /// Set view object from boxed ViewObject.
    #[inline]
    pub fn set_view_object_boxed(&mut self, view_object: Box<dyn ViewObject>) {
        self.view_object = Some(view_object);
    }

    // ========== Lifecycle Delegation ==========

    /// Mount element to tree.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>, depth: usize) {
        self.base.mount(parent, slot, depth);
    }

    /// Unmount element from tree.
    #[inline]
    pub fn unmount(&mut self) {
        self.base.unmount();
    }

    /// Activate element.
    #[inline]
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Deactivate element.
    #[inline]
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Get current lifecycle state.
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    /// Get cached depth in tree.
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        self.base.depth()
    }

    /// Set cached depth.
    #[inline]
    pub fn set_depth(&self, depth: usize) {
        self.base.set_depth(depth);
    }

    // ========== Parent/Slot Accessors ==========

    /// Get parent element ID.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Get slot position.
    #[inline]
    #[must_use]
    pub fn slot(&self) -> Option<Slot> {
        self.base.slot()
    }

    // ========== Dirty Tracking ==========

    /// Check if element needs rebuild.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as needing rebuild.
    #[inline]
    pub fn mark_dirty(&self) {
        self.base.mark_dirty();
    }

    /// Clear dirty flag.
    #[inline]
    pub fn clear_dirty(&self) {
        self.base.clear_dirty();
    }

    /// Check if mounted.
    #[inline]
    #[must_use]
    pub fn is_mounted(&self) -> bool {
        self.base.is_mounted()
    }

    // ========== Child Management ==========

    /// Get child element IDs.
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get mutable child element IDs.
    #[inline]
    #[must_use]
    pub fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Add a child element.
    #[inline]
    pub fn add_child(&mut self, child_id: ElementId) {
        self.children.push(child_id);
    }

    /// Remove a child element.
    #[inline]
    pub fn remove_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    /// Clear all children.
    #[inline]
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    /// Set children from iterator.
    #[inline]
    pub fn set_children(&mut self, children: impl IntoIterator<Item = ElementId>) {
        self.children = children.into_iter().collect();
    }

    /// Check if element has children.
    #[inline]
    #[must_use]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Get first child.
    #[inline]
    #[must_use]
    pub fn first_child(&self) -> Option<ElementId> {
        self.children.first().copied()
    }

    /// Get child count.
    #[inline]
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    // ========== Debug ==========

    /// Get debug name.
    #[inline]
    #[must_use]
    pub fn debug_name(&self) -> &'static str {
        self.debug_name.unwrap_or("ViewElement")
    }

    /// Access the internal ElementBase.
    #[inline]
    #[must_use]
    pub fn base(&self) -> &ElementBase {
        &self.base
    }

    /// Access the internal ElementBase mutably.
    #[inline]
    #[must_use]
    pub fn base_mut(&mut self) -> &mut ElementBase {
        &mut self.base
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuildContext;

    // Test view object type
    #[derive(Debug)]
    struct TestViewObject {
        value: i32,
    }

    impl ViewObject for TestViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
            None // Empty build for test
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_view_element_creation() {
        let element = ViewElement::new(TestViewObject { value: 42 }, ViewMode::Stateless);
        assert!(element.has_view_object());
        assert_eq!(element.view_mode(), ViewMode::Stateless);
        assert!(element.is_component());
        assert!(!element.is_provider());
    }

    #[test]
    fn test_view_element_empty() {
        let element = ViewElement::empty();
        assert!(!element.has_view_object());
        assert_eq!(element.view_mode(), ViewMode::Empty);
        assert_eq!(element.debug_name(), "Empty");
    }

    #[test]
    fn test_view_object_downcast() {
        let element = ViewElement::new(TestViewObject { value: 42 }, ViewMode::Stateless);

        let view_object = element.view_object_as::<TestViewObject>();
        assert!(view_object.is_some());
        assert_eq!(view_object.unwrap().value, 42);
    }

    #[test]
    fn test_lifecycle() {
        let mut element = ViewElement::new(TestViewObject { value: 1 }, ViewMode::Stateless);

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

        element.mount(Some(ElementId::new(1)), Some(Slot::new(0)), 1);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_mounted());

        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }

    #[test]
    fn test_children_management() {
        let mut element = ViewElement::new(TestViewObject { value: 1 }, ViewMode::Stateless);

        assert!(!element.has_children());

        element.add_child(ElementId::new(10));
        element.add_child(ElementId::new(20));

        assert!(element.has_children());
        assert_eq!(element.child_count(), 2);
        assert_eq!(element.first_child(), Some(ElementId::new(10)));

        element.remove_child(ElementId::new(10));
        assert_eq!(element.child_count(), 1);

        element.clear_children();
        assert!(!element.has_children());
    }
}
