//! Element struct - Unified element with type-erased view object
//!
//! This module provides the unified `Element` struct that can hold any view type
//! through type erasure using `Box<dyn ViewObject>`.
//!
//! # Architecture
//!
//! ```text
//! View (Config)
//!   ↓ wrap in ViewObject
//! Element (Lifecycle + type-erased ViewObject)
//!   ├─ base: ElementBase (lifecycle, flags, parent/slot)
//!   ├─ view_object: Box<dyn ViewObject> (type-erased!)
//!   └─ children: Vec<ElementId>
//! ```
//!
//! # Type Erasure
//!
//! Element stores `Box<dyn ViewObject>` which provides:
//! - `as_any()` for downcasting to concrete wrapper types
//! - `as_any_mut()` for mutable downcasting
//! - `build()`, `init()`, `dispose()` lifecycle methods
//!
//! The actual ViewObject is stored inside and can be accessed via downcasting:
//!
//! ```rust,ignore
//! // In flui-view, after downcasting:
//! let wrapper = element.view_object_as::<StatelessViewWrapper<MyView>>()?;
//! ```

use std::any::Any;
use std::fmt;

use flui_foundation::{ElementId, Key, Slot, ViewMode};

use super::{ElementBase, ElementLifecycle};
use crate::ViewObject;

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
    /// stored as `dyn ViewObject` for type erasure.
    view_object: Option<Box<dyn ViewObject>>,

    /// View mode - categorizes the view type (Stateless, Stateful, RenderBox, etc.)
    ///
    /// Stored separately to allow querying without downcasting.
    view_mode: ViewMode,

    /// Optional key for element identity and reconciliation
    ///
    /// Used during reconciliation to determine if an element can be reused.
    /// Elements with matching ViewMode and Key can be reused instead of recreated.
    key: Option<Key>,

    /// Child element IDs
    children: Vec<ElementId>,

    /// Debug name for diagnostics
    debug_name: Option<&'static str>,
}

// Element is Send because ViewObject requires Send
// Sync is not implemented - use Arc<RwLock<Element>> for shared access

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
    /// Creates a new Element with the given view object and mode.
    ///
    /// # Arguments
    ///
    /// * `view_object` - Any type that implements `ViewObject`
    /// * `mode` - The ViewMode categorizing this element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let wrapper = StatelessViewWrapper::new(my_view);
    /// let element = Element::with_mode(wrapper, ViewMode::Stateless);
    /// ```
    pub fn with_mode<V: ViewObject>(view_object: V, mode: ViewMode) -> Self {
        Self {
            base: ElementBase::new(),
            view_object: Some(Box::new(view_object)),
            view_mode: mode,
            key: None,
            children: Vec::new(),
            debug_name: None,
        }
    }

    /// Creates a new Element with the given view object (defaults to Empty mode).
    ///
    /// Prefer `with_mode()` when the mode is known.
    ///
    /// # Arguments
    ///
    /// * `view_object` - Any type that implements `ViewObject`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let wrapper = StatelessViewWrapper::new(my_view);
    /// let element = Element::new(wrapper);
    /// ```
    pub fn new<V: ViewObject>(view_object: V) -> Self {
        Self {
            base: ElementBase::new(),
            view_object: Some(Box::new(view_object)),
            view_mode: ViewMode::Empty,
            key: None,
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
            view_mode: ViewMode::Empty,
            key: None,
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
            view_mode: ViewMode::Empty,
            key: None,
            children: Vec::with_capacity(child_count),
            debug_name: Some("Container"),
        }
    }

    /// Creates an element with a debug name.
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    /// Sets the view mode.
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        self.view_mode = mode;
    }

    // ========== View Mode Queries ==========

    /// Get the view mode of this element.
    #[inline]
    #[must_use]
    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    /// Check if this element is a component view (Stateless, Stateful, Proxy, Animated, Provider).
    #[inline]
    #[must_use]
    pub fn is_component(&self) -> bool {
        self.view_mode.is_component()
    }

    /// Check if this element is a render view (RenderBox, RenderSliver).
    #[inline]
    #[must_use]
    pub fn is_render(&self) -> bool {
        self.view_mode.is_render()
    }

    /// Check if this element is a provider view.
    #[inline]
    #[must_use]
    pub fn is_provider(&self) -> bool {
        self.view_mode.is_provider()
    }

    // ========== Key Access ==========

    /// Get the key of this element (if any).
    ///
    /// Keys are used during reconciliation to determine if elements can be reused.
    #[inline]
    #[must_use]
    pub fn key(&self) -> Option<Key> {
        self.key
    }

    /// Set the key of this element.
    ///
    /// Used during element construction or updates.
    #[inline]
    pub fn set_key(&mut self, key: Option<Key>) {
        self.key = key;
    }

    /// Create element with a key (builder pattern).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let element = Element::with_mode(view_object, ViewMode::Stateless)
    ///     .with_key(Key::from_str("my_element"));
    /// ```
    pub fn with_key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    // ========== View Object Access ==========

    /// Returns true if this element has a view object.
    #[inline]
    #[must_use]
    pub fn has_view_object(&self) -> bool {
        self.view_object.is_some()
    }

    /// Get the view object as a reference to ViewObject.
    #[inline]
    #[must_use]
    pub fn view_object(&self) -> Option<&dyn ViewObject> {
        self.view_object.as_ref().map(|b| b.as_ref())
    }

    /// Get the view object as a mutable reference to ViewObject.
    #[inline]
    #[must_use]
    pub fn view_object_mut(&mut self) -> Option<&mut dyn ViewObject> {
        self.view_object.as_mut().map(|b| b.as_mut())
    }

    /// Get the view object as a reference to Any (via ViewObject::as_any).
    #[inline]
    #[must_use]
    pub fn view_object_any(&self) -> Option<&dyn Any> {
        self.view_object.as_ref().map(|b| b.as_any())
    }

    /// Get the view object as a mutable reference to Any (via ViewObject::as_any_mut).
    #[inline]
    #[must_use]
    pub fn view_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        self.view_object.as_mut().map(|b| b.as_any_mut())
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
        self.view_object.as_ref()?.as_any().downcast_ref::<V>()
    }

    /// Downcast view object to concrete type (mutable).
    #[inline]
    pub fn view_object_as_mut<V: Any + Send + Sync + 'static>(&mut self) -> Option<&mut V> {
        self.view_object.as_mut()?.as_any_mut().downcast_mut::<V>()
    }

    /// Take the view object out of the element.
    ///
    /// Returns the boxed view object, leaving None in its place.
    #[inline]
    pub fn take_view_object(&mut self) -> Option<Box<dyn ViewObject>> {
        self.view_object.take()
    }

    /// Set a new view object.
    ///
    /// The view object must implement `ViewObject`.
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
    ///
    /// # Parameters
    ///
    /// - `parent`: Parent element ID (None for root)
    /// - `slot`: Slot position in parent
    /// - `depth`: Depth in tree (0 for root, parent.depth() + 1 for children)
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

    /// Get cached depth in tree (0 = root).
    ///
    /// **Lock-free and thread-safe!** O(1) lookup.
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        self.base.depth()
    }

    /// Set cached depth in tree.
    ///
    /// **Lock-free and thread-safe!**
    ///
    /// Called when element is mounted or reparented.
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


    // ========== Compatibility Stubs ==========
    // These methods provide API compatibility with the old element module.

    /// Stub: Get dependents list for provider elements (always returns None).
    ///
    /// Provider dependents are managed by ProviderViewWrapper in flui-view.
    #[inline]
    #[must_use]
    pub fn dependents(&self) -> Option<&[ElementId]> {
        None
    }

    /// Stub: Get as component (returns Some(()) if is_component).
    ///
    /// For actual component data, downcast the view_object.
    #[inline]
    #[must_use]
    pub fn as_component(&self) -> Option<()> {
        if self.is_component() {
            Some(())
        } else {
            None
        }
    }

    /// Stub: Get as component mut (returns Some(()) if is_component).
    #[inline]
    #[must_use]
    pub fn as_component_mut(&mut self) -> Option<()> {
        if self.is_component() {
            Some(())
        } else {
            None
        }
    }

    /// Stub: Get as provider (returns Some(()) if is_provider).
    ///
    /// For actual provider data, downcast the view_object.
    #[inline]
    #[must_use]
    pub fn as_provider(&self) -> Option<()> {
        if self.is_provider() {
            Some(())
        } else {
            None
        }
    }

    /// Stub: Handle event (always returns false - not handled).
    ///
    /// Event handling should be implemented in the gesture/interaction layer.
    #[inline]
    pub fn handle_event(&mut self, _event: &dyn std::any::Any) -> bool {
        false
    }

    // ========== Render State Access (for RenderTreeAccess trait) ==========

    /// Returns the render state for this element, if it's a render element.
    ///
    /// # Returns
    ///
    /// Reference to the RenderState as `dyn Any`, or `None` if not a render element.
    ///
    /// # Note
    ///
    /// This is a stub implementation. Full render state management is in progress.
    #[inline]
    pub fn render_state(&self) -> Option<&dyn std::any::Any> {
        // TODO: Implement render state access via ViewObject
        None
    }

    /// Returns a mutable reference to the render state, if it's a render element.
    ///
    /// # Returns
    ///
    /// Mutable reference to the RenderState as `dyn Any`, or `None` if not a render element.
    ///
    /// # Note
    ///
    /// This is a stub implementation. Full render state management is in progress.
    #[inline]
    pub fn render_state_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        // TODO: Implement render state access via ViewObject
        None
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

    impl ViewObject for TestViewObject {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
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

        // Can downcast the taken value via as_any()
        let boxed = taken.unwrap();
        let downcasted = boxed.as_any().downcast_ref::<TestViewObject>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);
    }
}
