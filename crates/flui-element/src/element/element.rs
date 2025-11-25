//! Element struct - Unified element with type-erased view object
//!
//! This module provides the unified `Element` struct that can hold any view type
//! through type erasure using `Box<dyn Any + Send + Sync>`.
//!
//! # Architecture
//!
//! ```text
//! View (Config)
//!   ↓ wrap in ViewObject
//! Element (Lifecycle + type-erased ViewObject)
//!   ├─ base: ElementBase (lifecycle, flags, parent/slot)
//!   ├─ view_object: Box<dyn Any + Send + Sync> (type-erased!)
//!   └─ children: Vec<ElementId>
//! ```
//!
//! # Type Erasure
//!
//! Element stores `Box<dyn Any + Send + Sync>` instead of `Box<dyn ViewObject>`.
//! This breaks the dependency on ViewObject trait, allowing flui-element
//! to be independent of flui-view.
//!
//! The actual ViewObject is stored inside and can be accessed via downcasting:
//!
//! ```rust,ignore
//! // In flui-view, after downcasting:
//! let view_object: &dyn ViewObject = element.view_object_as::<StatelessViewWrapper<MyView>>()?;
//! ```

use std::any::Any;
use std::fmt;

use flui_foundation::{ElementId, Slot};

use super::{ElementBase, ElementLifecycle};

/// Element - Unified element struct with type-erased view object
///
/// This struct represents any View instance in the element tree.
/// The view-specific behavior is stored in a type-erased `Box<dyn Any + Send + Sync>`.
///
/// # Design Principles
///
/// - `base`: Common lifecycle fields (parent, slot, lifecycle, flags)
/// - `view_object`: Type-erased view object (Any + Send + Sync)
/// - `children`: Child element IDs
///
/// # Thread Safety
///
/// Element is `Send + Sync` because all internal fields are Send + Sync.
pub struct Element {
    /// Common lifecycle fields
    base: ElementBase,

    /// Type-erased view object storage
    ///
    /// Contains the actual ViewObject wrapper (StatelessViewWrapper, etc.)
    /// but stored as `dyn Any + Send + Sync` to break dependency on ViewObject trait.
    view_object: Option<Box<dyn Any + Send + Sync>>,

    /// Child element IDs
    children: Vec<ElementId>,

    /// Debug name for diagnostics
    debug_name: Option<&'static str>,
}

// Element is Send + Sync because:
// - ElementBase is Send + Sync (contains only Send + Sync types)
// - Box<dyn Any + Send + Sync> is Send + Sync by definition
// - Vec<ElementId> is Send + Sync (ElementId is Copy)
unsafe impl Send for Element {}
unsafe impl Sync for Element {}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Element")
            .field("parent", &self.base.parent())
            .field("lifecycle", &self.base.lifecycle())
            .field("children_count", &self.children.len())
            .field("has_view_object", &self.view_object.is_some())
            .field("debug_name", &self.debug_name)
            .finish()
    }
}

impl Element {
    /// Creates a new Element with the given view object.
    ///
    /// # Arguments
    ///
    /// * `view_object` - Any type that implements `Any + Send + Sync + 'static`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let wrapper = StatelessViewWrapper::new(my_view);
    /// let element = Element::new(wrapper);
    /// ```
    pub fn new<V: Any + Send + Sync + 'static>(view_object: V) -> Self {
        Self {
            base: ElementBase::new(),
            view_object: Some(Box::new(view_object)),
            children: Vec::new(),
            debug_name: None,
        }
    }

    /// Creates an empty element (no view object).
    ///
    /// Useful for placeholder elements or unit type `()` conversions.
    pub fn empty() -> Self {
        Self {
            base: ElementBase::new(),
            view_object: None,
            children: Vec::new(),
            debug_name: Some("Empty"),
        }
    }

    /// Creates an element with multiple children (container).
    ///
    /// Used for `Vec<T>` and tuple conversions.
    pub fn container(children: Vec<Element>) -> Self {
        // For now, container just holds children
        // In practice, these get flattened during tree insertion
        let child_count = children.len();
        Self {
            base: ElementBase::new(),
            view_object: None,
            children: Vec::with_capacity(child_count),
            debug_name: Some("Container"),
        }
    }

    /// Creates an element with a debug name.
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    // ========== View Object Access ==========

    /// Returns true if this element has a view object.
    #[inline]
    #[must_use]
    pub fn has_view_object(&self) -> bool {
        self.view_object.is_some()
    }

    /// Get the view object as a reference to Any.
    #[inline]
    #[must_use]
    pub fn view_object_any(&self) -> Option<&(dyn Any + Send + Sync)> {
        self.view_object.as_ref().map(|b| b.as_ref())
    }

    /// Get the view object as a mutable reference to Any.
    #[inline]
    #[must_use]
    pub fn view_object_any_mut(&mut self) -> Option<&mut (dyn Any + Send + Sync)> {
        self.view_object.as_mut().map(|b| b.as_mut())
    }

    /// Downcast view object to concrete type.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(wrapper) = element.view_object_as::<StatelessViewWrapper<MyView>>() {
    ///     // Use wrapper
    /// }
    /// ```
    #[inline]
    pub fn view_object_as<V: Any + Send + Sync + 'static>(&self) -> Option<&V> {
        self.view_object.as_ref()?.downcast_ref::<V>()
    }

    /// Downcast view object to concrete type (mutable).
    #[inline]
    pub fn view_object_as_mut<V: Any + Send + Sync + 'static>(&mut self) -> Option<&mut V> {
        self.view_object.as_mut()?.downcast_mut::<V>()
    }

    /// Take the view object out of the element.
    ///
    /// Returns the boxed view object, leaving None in its place.
    #[inline]
    pub fn take_view_object(&mut self) -> Option<Box<dyn Any + Send + Sync>> {
        self.view_object.take()
    }

    /// Set a new view object.
    #[inline]
    pub fn set_view_object<V: Any + Send + Sync + 'static>(&mut self, view_object: V) {
        self.view_object = Some(Box::new(view_object));
    }

    /// Set view object from boxed Any.
    #[inline]
    pub fn set_view_object_boxed(&mut self, view_object: Box<dyn Any + Send + Sync>) {
        self.view_object = Some(view_object);
    }

    // ========== Lifecycle Delegation ==========

    /// Mount element to tree.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.base.mount(parent, slot);
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

    /// Check if needs layout.
    #[inline]
    #[must_use]
    pub fn needs_layout(&self) -> bool {
        self.base.needs_layout()
    }

    /// Mark needs layout.
    #[inline]
    pub fn mark_needs_layout(&self) {
        self.base.mark_needs_layout();
    }

    /// Clear needs layout.
    #[inline]
    pub fn clear_needs_layout(&self) {
        self.base.clear_needs_layout();
    }

    /// Check if needs paint.
    #[inline]
    #[must_use]
    pub fn needs_paint(&self) -> bool {
        self.base.needs_paint()
    }

    /// Mark needs paint.
    #[inline]
    pub fn mark_needs_paint(&self) {
        self.base.mark_needs_paint();
    }

    /// Clear needs paint.
    #[inline]
    pub fn clear_needs_paint(&self) {
        self.base.clear_needs_paint();
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
        self.debug_name.unwrap_or("Element")
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

    // Test view object type for testing
    #[derive(Debug)]
    struct TestViewObject {
        value: i32,
    }

    #[test]
    fn test_element_creation() {
        let element = Element::new(TestViewObject { value: 42 });
        assert!(element.has_view_object());
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
    }

    #[test]
    fn test_element_empty() {
        let element = Element::empty();
        assert!(!element.has_view_object());
        assert_eq!(element.debug_name(), "Empty");
    }

    #[test]
    fn test_view_object_downcast() {
        let element = Element::new(TestViewObject { value: 42 });

        let view_object = element.view_object_as::<TestViewObject>();
        assert!(view_object.is_some());
        assert_eq!(view_object.unwrap().value, 42);

        // Wrong type returns None
        let wrong: Option<&String> = element.view_object_as::<String>();
        assert!(wrong.is_none());
    }

    #[test]
    fn test_view_object_downcast_mut() {
        let mut element = Element::new(TestViewObject { value: 42 });

        if let Some(vo) = element.view_object_as_mut::<TestViewObject>() {
            vo.value = 100;
        }

        let view_object = element.view_object_as::<TestViewObject>();
        assert_eq!(view_object.unwrap().value, 100);
    }

    #[test]
    fn test_element_lifecycle() {
        let mut element = Element::new(TestViewObject { value: 1 });

        // Initial state
        assert!(element.is_dirty());
        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

        // Mount
        element.mount(Some(ElementId::new(1)), Some(Slot::new(0)));
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_mounted());

        // Deactivate
        element.deactivate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Inactive);

        // Activate
        element.activate();
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);

        // Unmount
        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }

    #[test]
    fn test_children_management() {
        let mut element = Element::new(TestViewObject { value: 1 });

        assert!(!element.has_children());
        assert_eq!(element.child_count(), 0);

        // Add children
        element.add_child(ElementId::new(10));
        element.add_child(ElementId::new(20));

        assert!(element.has_children());
        assert_eq!(element.child_count(), 2);
        assert_eq!(element.first_child(), Some(ElementId::new(10)));

        // Remove child
        element.remove_child(ElementId::new(10));
        assert_eq!(element.child_count(), 1);
        assert_eq!(element.first_child(), Some(ElementId::new(20)));

        // Clear
        element.clear_children();
        assert!(!element.has_children());
    }

    #[test]
    fn test_take_view_object() {
        let mut element = Element::new(TestViewObject { value: 42 });
        assert!(element.has_view_object());

        let taken = element.take_view_object();
        assert!(taken.is_some());
        assert!(!element.has_view_object());

        // Can downcast the taken value
        let boxed = taken.unwrap();
        let downcasted = boxed.downcast::<TestViewObject>();
        assert!(downcasted.is_ok());
        assert_eq!(downcasted.unwrap().value, 42);
    }
}
