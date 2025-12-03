//! Element enum - Unified element type for the element tree
//!
//! This module provides the `Element` enum that can represent either a
//! `ViewElement` (component views) or `RenderElement` (render views).
//!
//! # Architecture
//!
//! ```text
//! enum Element {
//!     View(ViewElement),    // Stateless, Stateful, Provider, etc.
//!     Render(RenderElement), // RenderBox, RenderSliver
//! }
//! ```
//!
//! This design follows Flutter's element hierarchy where elements can be
//! either component-based (building other widgets) or render-based
//! (participating in layout/paint).

use std::any::Any;

use flui_foundation::{ElementId, Key, Slot};
use flui_view::ViewMode;

use super::{ElementBase, ElementLifecycle, RenderElement, ViewElement};
use crate::ViewObject;

/// Element - Unified element type
///
/// An Element can be either:
/// - `View`: A component element that builds children (Stateless, Stateful, Provider, etc.)
/// - `Render`: A render element that participates in layout/paint (RenderBox, RenderSliver)
///
/// # Design
///
/// Both variants share a common API through delegation, allowing uniform
/// tree operations while preserving type-specific behavior.
///
/// # Thread Safety
///
/// Element is `Send` because both ViewElement and RenderElement are Send.
#[derive(Debug)]
pub enum Element {
    /// Component element (Stateless, Stateful, Provider, Proxy, Animated)
    View(ViewElement),

    /// Render element (RenderBox, RenderSliver)
    Render(RenderElement),
}

impl Element {
    // ========== Constructors ==========

    /// Creates a new View element with the given view object and mode.
    pub fn view<V: ViewObject>(view_object: V, mode: ViewMode) -> Self {
        Self::View(ViewElement::new(view_object, mode))
    }

    /// Creates a new Render element with render object and state.
    pub fn render<RO, RS>(render_object: RO, render_state: RS, mode: ViewMode) -> Self
    where
        RO: Any + Send + Sync + 'static,
        RS: Any + Send + Sync + 'static,
    {
        Self::Render(RenderElement::new(render_object, render_state, mode))
    }

    /// Creates an empty element (View variant).
    pub fn empty() -> Self {
        Self::View(ViewElement::empty())
    }

    /// Creates a container element with pending children.
    pub fn container(children: Vec<Element>) -> Self {
        Self::View(ViewElement::container(children))
    }

    /// Creates an element with mode (for backward compatibility).
    ///
    /// Uses View variant for component modes, but callers should prefer
    /// `Element::view()` or `Element::render()` for clarity.
    pub fn with_mode<V: ViewObject>(view_object: V, mode: ViewMode) -> Self {
        Self::View(ViewElement::new(view_object, mode))
    }

    /// Creates a new view element (backward compatibility alias).
    pub fn new<V: ViewObject>(view_object: V) -> Self {
        Self::View(ViewElement::new(view_object, ViewMode::Empty))
    }

    // ========== Variant Checks ==========

    /// Returns true if this is a View element.
    #[inline]
    #[must_use]
    pub fn is_view_element(&self) -> bool {
        matches!(self, Self::View(_))
    }

    /// Returns true if this is a Render element.
    #[inline]
    #[must_use]
    pub fn is_render_element(&self) -> bool {
        matches!(self, Self::Render(_))
    }

    /// Get as ViewElement reference.
    #[inline]
    #[must_use]
    pub fn as_view(&self) -> Option<&ViewElement> {
        match self {
            Self::View(v) => Some(v),
            Self::Render(_) => None,
        }
    }

    /// Get as ViewElement mutable reference.
    #[inline]
    #[must_use]
    pub fn as_view_mut(&mut self) -> Option<&mut ViewElement> {
        match self {
            Self::View(v) => Some(v),
            Self::Render(_) => None,
        }
    }

    /// Get as RenderElement reference.
    #[inline]
    #[must_use]
    pub fn as_render(&self) -> Option<&RenderElement> {
        match self {
            Self::View(_) => None,
            Self::Render(r) => Some(r),
        }
    }

    /// Get as RenderElement mutable reference.
    #[inline]
    #[must_use]
    pub fn as_render_mut(&mut self) -> Option<&mut RenderElement> {
        match self {
            Self::View(_) => None,
            Self::Render(r) => Some(r),
        }
    }

    // ========== View Mode Queries (delegated) ==========

    /// Get the view mode.
    #[inline]
    #[must_use]
    pub fn view_mode(&self) -> ViewMode {
        match self {
            Self::View(v) => v.view_mode(),
            Self::Render(r) => r.view_mode(),
        }
    }

    /// Set the view mode.
    #[inline]
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        match self {
            Self::View(v) => v.set_view_mode(mode),
            Self::Render(r) => r.set_view_mode(mode),
        }
    }

    /// Check if this is a component view.
    #[inline]
    #[must_use]
    pub fn is_component(&self) -> bool {
        self.view_mode().is_component()
    }

    /// Check if this is a render view.
    #[inline]
    #[must_use]
    pub fn is_render(&self) -> bool {
        self.view_mode().is_render()
    }

    /// Check if this is a provider view.
    #[inline]
    #[must_use]
    pub fn is_provider(&self) -> bool {
        self.view_mode().is_provider()
    }

    // ========== Key Access ==========

    /// Get the key.
    #[inline]
    #[must_use]
    pub fn key(&self) -> Option<Key> {
        match self {
            Self::View(v) => v.key(),
            Self::Render(r) => r.key(),
        }
    }

    /// Set the key.
    #[inline]
    pub fn set_key(&mut self, key: Option<Key>) {
        match self {
            Self::View(v) => v.set_key(key),
            Self::Render(r) => r.set_key(key),
        }
    }

    /// Builder: set key.
    pub fn with_key(mut self, key: Key) -> Self {
        self.set_key(Some(key));
        self
    }

    // ========== Pending Children ==========

    /// Take pending children for processing.
    pub fn take_pending_children(&mut self) -> Option<Vec<Element>> {
        match self {
            Self::View(v) => v.take_pending_children(),
            Self::Render(r) => r.take_pending_children(),
        }
    }

    /// Check if element has pending children.
    #[inline]
    #[must_use]
    pub fn has_pending_children(&self) -> bool {
        match self {
            Self::View(v) => v.has_pending_children(),
            Self::Render(r) => r.has_pending_children(),
        }
    }

    /// Builder: set pending children.
    pub fn with_pending_children(mut self, children: Vec<Element>) -> Self {
        match &mut self {
            Self::View(v) => {
                *v = std::mem::replace(v, ViewElement::empty()).with_pending_children(children);
            }
            Self::Render(r) => {
                *r = std::mem::replace(r, RenderElement::empty()).with_pending_children(children);
            }
        }
        self
    }

    // ========== View Object Access (View variant only) ==========

    /// Returns true if this element has a view object.
    #[inline]
    #[must_use]
    pub fn has_view_object(&self) -> bool {
        match self {
            Self::View(v) => v.has_view_object(),
            Self::Render(_) => false,
        }
    }

    /// Get the view object as a reference.
    #[inline]
    #[must_use]
    pub fn view_object(&self) -> Option<&dyn ViewObject> {
        match self {
            Self::View(v) => v.view_object(),
            Self::Render(_) => None,
        }
    }

    /// Get the view object as a mutable reference.
    #[inline]
    #[must_use]
    pub fn view_object_mut(&mut self) -> Option<&mut dyn ViewObject> {
        match self {
            Self::View(v) => v.view_object_mut(),
            Self::Render(_) => None,
        }
    }

    /// Get the view object as Any for downcasting.
    #[inline]
    #[must_use]
    pub fn view_object_any(&self) -> Option<&dyn Any> {
        match self {
            Self::View(v) => v.view_object_any(),
            Self::Render(_) => None,
        }
    }

    /// Get the view object as mutable Any for downcasting.
    #[inline]
    #[must_use]
    pub fn view_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        match self {
            Self::View(v) => v.view_object_any_mut(),
            Self::Render(_) => None,
        }
    }

    /// Downcast view object to concrete type.
    #[inline]
    pub fn view_object_as<V: Any + Send + Sync + 'static>(&self) -> Option<&V> {
        match self {
            Self::View(v) => v.view_object_as::<V>(),
            Self::Render(_) => None,
        }
    }

    /// Downcast view object to concrete type (mutable).
    #[inline]
    pub fn view_object_as_mut<V: Any + Send + Sync + 'static>(&mut self) -> Option<&mut V> {
        match self {
            Self::View(v) => v.view_object_as_mut::<V>(),
            Self::Render(_) => None,
        }
    }

    /// Take the view object out.
    #[inline]
    pub fn take_view_object(&mut self) -> Option<Box<dyn ViewObject>> {
        match self {
            Self::View(v) => v.take_view_object(),
            Self::Render(_) => None,
        }
    }

    /// Set a new view object (View variant only).
    #[inline]
    pub fn set_view_object<V: ViewObject>(&mut self, view_object: V) {
        if let Self::View(v) = self {
            v.set_view_object(view_object);
        }
    }

    /// Set view object from boxed ViewObject.
    #[inline]
    pub fn set_view_object_boxed(&mut self, view_object: Box<dyn ViewObject>) {
        if let Self::View(v) = self {
            v.set_view_object_boxed(view_object);
        }
    }

    // ========== Render State Access (for RenderTreeAccess trait) ==========

    /// Returns the render state for this element.
    #[inline]
    pub fn render_state(&self) -> Option<&dyn Any> {
        match self {
            Self::View(v) => v.view_object()?.render_state(),
            Self::Render(r) => r.render_state(),
        }
    }

    /// Returns a mutable reference to the render state.
    #[inline]
    pub fn render_state_mut(&mut self) -> Option<&mut dyn Any> {
        match self {
            Self::View(v) => v.view_object_mut()?.render_state_mut(),
            Self::Render(r) => r.render_state_mut(),
        }
    }

    // ========== Lifecycle Delegation ==========

    /// Mount element to tree.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>, depth: usize) {
        match self {
            Self::View(v) => v.mount(parent, slot, depth),
            Self::Render(r) => r.mount(parent, slot, depth),
        }
    }

    /// Unmount element from tree.
    #[inline]
    pub fn unmount(&mut self) {
        match self {
            Self::View(v) => v.unmount(),
            Self::Render(r) => r.unmount(),
        }
    }

    /// Activate element.
    #[inline]
    pub fn activate(&mut self) {
        match self {
            Self::View(v) => v.activate(),
            Self::Render(r) => r.activate(),
        }
    }

    /// Deactivate element.
    #[inline]
    pub fn deactivate(&mut self) {
        match self {
            Self::View(v) => v.deactivate(),
            Self::Render(r) => r.deactivate(),
        }
    }

    /// Get current lifecycle state.
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        match self {
            Self::View(v) => v.lifecycle(),
            Self::Render(r) => r.lifecycle(),
        }
    }

    /// Get cached depth in tree.
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        match self {
            Self::View(v) => v.depth(),
            Self::Render(r) => r.depth(),
        }
    }

    /// Set cached depth.
    #[inline]
    pub fn set_depth(&self, depth: usize) {
        match self {
            Self::View(v) => v.set_depth(depth),
            Self::Render(r) => r.set_depth(depth),
        }
    }

    // ========== Parent/Slot Accessors ==========

    /// Get parent element ID.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        match self {
            Self::View(v) => v.parent(),
            Self::Render(r) => r.parent(),
        }
    }

    /// Get slot position.
    #[inline]
    #[must_use]
    pub fn slot(&self) -> Option<Slot> {
        match self {
            Self::View(v) => v.slot(),
            Self::Render(r) => r.slot(),
        }
    }

    // ========== Dirty Tracking ==========

    /// Check if element needs rebuild.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        match self {
            Self::View(v) => v.is_dirty(),
            Self::Render(r) => r.is_dirty(),
        }
    }

    /// Mark element as needing rebuild.
    #[inline]
    pub fn mark_dirty(&self) {
        match self {
            Self::View(v) => v.mark_dirty(),
            Self::Render(r) => r.mark_dirty(),
        }
    }

    /// Clear dirty flag.
    #[inline]
    pub fn clear_dirty(&self) {
        match self {
            Self::View(v) => v.clear_dirty(),
            Self::Render(r) => r.clear_dirty(),
        }
    }

    /// Check if needs layout.
    #[inline]
    #[must_use]
    pub fn needs_layout(&self) -> bool {
        match self {
            Self::View(_) => false,
            Self::Render(r) => r.needs_layout(),
        }
    }

    /// Mark needs layout.
    #[inline]
    pub fn mark_needs_layout(&self) {
        if let Self::Render(r) = self {
            r.mark_needs_layout();
        }
    }

    /// Clear needs layout.
    #[inline]
    pub fn clear_needs_layout(&self) {
        if let Self::Render(r) = self {
            r.clear_needs_layout();
        }
    }

    /// Check if needs paint.
    #[inline]
    #[must_use]
    pub fn needs_paint(&self) -> bool {
        match self {
            Self::View(_) => false,
            Self::Render(r) => r.needs_paint(),
        }
    }

    /// Mark needs paint.
    #[inline]
    pub fn mark_needs_paint(&self) {
        if let Self::Render(r) = self {
            r.mark_needs_paint();
        }
    }

    /// Clear needs paint.
    #[inline]
    pub fn clear_needs_paint(&self) {
        if let Self::Render(r) = self {
            r.clear_needs_paint();
        }
    }

    /// Check if mounted.
    #[inline]
    #[must_use]
    pub fn is_mounted(&self) -> bool {
        match self {
            Self::View(v) => v.is_mounted(),
            Self::Render(r) => r.is_mounted(),
        }
    }

    // ========== Child Management ==========

    /// Get child element IDs.
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        match self {
            Self::View(v) => v.children(),
            Self::Render(r) => r.children(),
        }
    }

    /// Get mutable child element IDs.
    #[inline]
    #[must_use]
    pub fn children_mut(&mut self) -> &mut Vec<ElementId> {
        match self {
            Self::View(v) => v.children_mut(),
            Self::Render(r) => r.children_mut(),
        }
    }

    /// Add a child element.
    #[inline]
    pub fn add_child(&mut self, child_id: ElementId) {
        match self {
            Self::View(v) => v.add_child(child_id),
            Self::Render(r) => r.add_child(child_id),
        }
    }

    /// Remove a child element.
    #[inline]
    pub fn remove_child(&mut self, child_id: ElementId) {
        match self {
            Self::View(v) => v.remove_child(child_id),
            Self::Render(r) => r.remove_child(child_id),
        }
    }

    /// Clear all children.
    #[inline]
    pub fn clear_children(&mut self) {
        match self {
            Self::View(v) => v.clear_children(),
            Self::Render(r) => r.clear_children(),
        }
    }

    /// Set children from iterator.
    #[inline]
    pub fn set_children(&mut self, children: impl IntoIterator<Item = ElementId>) {
        match self {
            Self::View(v) => v.set_children(children),
            Self::Render(r) => r.set_children(children),
        }
    }

    /// Check if element has children.
    #[inline]
    #[must_use]
    pub fn has_children(&self) -> bool {
        match self {
            Self::View(v) => v.has_children(),
            Self::Render(r) => r.has_children(),
        }
    }

    /// Get first child.
    #[inline]
    #[must_use]
    pub fn first_child(&self) -> Option<ElementId> {
        match self {
            Self::View(v) => v.first_child(),
            Self::Render(r) => r.first_child(),
        }
    }

    /// Get child count.
    #[inline]
    #[must_use]
    pub fn child_count(&self) -> usize {
        match self {
            Self::View(v) => v.child_count(),
            Self::Render(r) => r.child_count(),
        }
    }

    // ========== Debug ==========

    /// Get debug name.
    #[inline]
    #[must_use]
    pub fn debug_name(&self) -> &'static str {
        match self {
            Self::View(v) => v.debug_name(),
            Self::Render(r) => r.debug_name(),
        }
    }

    /// Builder: set debug name.
    pub fn with_debug_name(self, name: &'static str) -> Self {
        match self {
            Self::View(v) => Self::View(v.with_debug_name(name)),
            Self::Render(r) => Self::Render(r.with_debug_name(name)),
        }
    }

    /// Access the internal ElementBase.
    #[inline]
    #[must_use]
    pub fn base(&self) -> &ElementBase {
        match self {
            Self::View(v) => v.base(),
            Self::Render(r) => r.base(),
        }
    }

    /// Access the internal ElementBase mutably.
    #[inline]
    #[must_use]
    pub fn base_mut(&mut self) -> &mut ElementBase {
        match self {
            Self::View(v) => v.base_mut(),
            Self::Render(r) => r.base_mut(),
        }
    }

    // ========== Compatibility Stubs ==========

    /// Stub: Get dependents list for provider elements (always returns None).
    #[inline]
    #[must_use]
    pub fn dependents(&self) -> Option<&[ElementId]> {
        None
    }

    /// Stub: Get as component (returns Some(()) if is_component).
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
    #[inline]
    #[must_use]
    pub fn as_provider(&self) -> Option<()> {
        if self.is_provider() {
            Some(())
        } else {
            None
        }
    }

    /// Stub: Handle event (always returns false).
    #[inline]
    pub fn handle_event(&mut self, _event: &dyn Any) -> bool {
        false
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
            None
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[derive(Debug)]
    struct TestRenderObject {
        value: i32,
    }

    #[derive(Debug)]
    struct TestRenderState {
        size: (f32, f32),
    }

    #[test]
    fn test_element_view_variant() {
        let element = Element::view(TestViewObject { value: 42 }, ViewMode::Stateless);

        assert!(element.is_view_element());
        assert!(!element.is_render_element());
        assert!(element.is_component());
        assert!(!element.is_render());
        assert!(element.has_view_object());
    }

    #[test]
    fn test_element_render_variant() {
        let element = Element::render(
            TestRenderObject { value: 42 },
            TestRenderState {
                size: (100.0, 50.0),
            },
            ViewMode::RenderBox,
        );

        assert!(!element.is_view_element());
        assert!(element.is_render_element());
        assert!(!element.is_component());
        assert!(element.is_render());
        assert!(!element.has_view_object());
    }

    #[test]
    fn test_element_empty() {
        let element = Element::empty();
        assert!(element.is_view_element());
        assert!(!element.has_view_object());
    }

    #[test]
    fn test_as_view_as_render() {
        let view = Element::view(TestViewObject { value: 1 }, ViewMode::Stateless);
        assert!(view.as_view().is_some());
        assert!(view.as_render().is_none());

        let render = Element::render(
            TestRenderObject { value: 1 },
            TestRenderState { size: (10.0, 10.0) },
            ViewMode::RenderBox,
        );
        assert!(render.as_view().is_none());
        assert!(render.as_render().is_some());
    }

    #[test]
    fn test_lifecycle() {
        let mut element = Element::view(TestViewObject { value: 1 }, ViewMode::Stateless);

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
        let mut element = Element::view(TestViewObject { value: 1 }, ViewMode::Stateless);

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

    #[test]
    fn test_backward_compatibility() {
        // Test that old API still works
        let element = Element::with_mode(TestViewObject { value: 42 }, ViewMode::Stateless);
        assert!(element.has_view_object());

        let element2 = Element::new(TestViewObject { value: 42 });
        assert!(element2.has_view_object());
    }
}
