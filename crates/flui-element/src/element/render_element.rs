//! RenderElement - Element for render views (RenderBox, RenderSliver)
//!
//! RenderElement holds render objects that participate in layout and painting.
//! Unlike ViewElement which builds children, RenderElement directly handles
//! layout constraints and painting operations.
//!
//! # Architecture
//!
//! ```text
//! RenderElement
//!   ├─ base: ElementBase (lifecycle, flags, parent/slot)
//!   ├─ render_object: Box<dyn RenderObject> (type-erased render object)
//!   ├─ render_state: Box<dyn Any + Send + Sync> (type-erased RenderState<P>)
//!   ├─ view_mode: ViewMode (RenderBox or RenderSliver)
//!   ├─ key: Option<Key> (for reconciliation)
//!   └─ children: Vec<ElementId> (render children)
//! ```
//!
//! # Type Erasure
//!
//! RenderElement stores `Box<dyn Any + Send + Sync>` for the render state
//! because `RenderState<P>` is generic over the protocol. The actual protocol
//! (Box or Sliver) is indicated by `view_mode`.

use std::any::Any;
use std::fmt;

use flui_foundation::{ElementId, Key, Slot};
use flui_view::ViewMode;

use super::{ElementBase, ElementLifecycle};

/// Trait for type-erased render objects.
///
/// This trait mirrors `flui_rendering::RenderObject` but is defined here
/// to avoid circular dependencies. The actual RenderObject from flui_rendering
/// will implement this trait.
pub trait RenderObjectTrait: Send + Sync + fmt::Debug + 'static {
    /// For downcasting to concrete type.
    fn as_any(&self) -> &dyn Any;

    /// For mutable downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Human-readable debug name.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// RenderElement - Element for render views
///
/// Represents views that directly participate in layout and painting
/// (RenderBox, RenderSliver). Stores a type-erased render object and
/// render state.
///
/// # Thread Safety
///
/// RenderElement is `Send` because all fields are Send.
pub struct RenderElement {
    /// Common lifecycle fields
    base: ElementBase,

    /// Type-erased render object
    ///
    /// Stored as `Box<dyn Any + Send + Sync>` to avoid dependency on
    /// `flui_rendering::RenderObject` trait. Downcast to concrete type
    /// when needed.
    render_object: Option<Box<dyn Any + Send + Sync>>,

    /// Type-erased render state
    ///
    /// Contains `RenderState<BoxProtocol>` or `RenderState<SliverProtocol>`
    /// depending on `view_mode`. Stored as Any for type erasure.
    render_state: Option<Box<dyn Any + Send + Sync>>,

    /// View mode - RenderBox or RenderSliver
    view_mode: ViewMode,

    /// Optional key for reconciliation
    key: Option<Key>,

    /// Child element IDs
    children: Vec<ElementId>,

    /// Pending child elements (before mount)
    pending_children: Option<Vec<super::Element>>,

    /// Debug name for diagnostics
    debug_name: Option<&'static str>,
}

impl fmt::Debug for RenderElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderElement")
            .field("parent", &self.base.parent())
            .field("lifecycle", &self.base.lifecycle())
            .field("view_mode", &self.view_mode)
            .field("children_count", &self.children.len())
            .field("has_render_object", &self.render_object.is_some())
            .field("has_render_state", &self.render_state.is_some())
            .field("debug_name", &self.debug_name)
            .finish()
    }
}

impl RenderElement {
    /// Creates a new RenderElement with render object and state.
    ///
    /// # Arguments
    ///
    /// * `render_object` - Type-erased render object
    /// * `render_state` - Type-erased render state
    /// * `mode` - ViewMode::RenderBox or ViewMode::RenderSliver
    pub fn new<RO, RS>(render_object: RO, render_state: RS, mode: ViewMode) -> Self
    where
        RO: Any + Send + Sync + 'static,
        RS: Any + Send + Sync + 'static,
    {
        debug_assert!(
            mode.is_render(),
            "RenderElement should only be used for render views, got {:?}",
            mode
        );

        Self {
            base: ElementBase::new(),
            render_object: Some(Box::new(render_object)),
            render_state: Some(Box::new(render_state)),
            view_mode: mode,
            key: None,
            children: Vec::new(),
            pending_children: None,
            debug_name: None,
        }
    }

    /// Creates a RenderElement with only render object (state created separately).
    pub fn with_render_object<RO>(render_object: RO, mode: ViewMode) -> Self
    where
        RO: Any + Send + Sync + 'static,
    {
        debug_assert!(
            mode.is_render(),
            "RenderElement should only be used for render views, got {:?}",
            mode
        );

        Self {
            base: ElementBase::new(),
            render_object: Some(Box::new(render_object)),
            render_state: None,
            view_mode: mode,
            key: None,
            children: Vec::new(),
            pending_children: None,
            debug_name: None,
        }
    }

    /// Creates an empty RenderElement (for placeholder use).
    pub fn empty() -> Self {
        Self {
            base: ElementBase::new(),
            render_object: None,
            render_state: None,
            view_mode: ViewMode::RenderBox,
            key: None,
            children: Vec::new(),
            pending_children: None,
            debug_name: Some("EmptyRender"),
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

    /// Builder: set render state.
    pub fn with_render_state<RS>(mut self, render_state: RS) -> Self
    where
        RS: Any + Send + Sync + 'static,
    {
        self.render_state = Some(Box::new(render_state));
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

    /// Check if this is a render view.
    #[inline]
    #[must_use]
    pub fn is_render(&self) -> bool {
        self.view_mode.is_render()
    }

    /// Check if this is a box render.
    #[inline]
    #[must_use]
    pub fn is_box(&self) -> bool {
        self.view_mode == ViewMode::RenderBox
    }

    /// Check if this is a sliver render.
    #[inline]
    #[must_use]
    pub fn is_sliver(&self) -> bool {
        self.view_mode == ViewMode::RenderSliver
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

    // ========== Render Object Access ==========

    /// Returns true if this element has a render object.
    #[inline]
    #[must_use]
    pub fn has_render_object(&self) -> bool {
        self.render_object.is_some()
    }

    /// Get the render object as Any for downcasting.
    #[inline]
    #[must_use]
    pub fn render_object_any(&self) -> Option<&dyn Any> {
        self.render_object.as_ref().map(|b| b.as_ref() as &dyn Any)
    }

    /// Get the render object as mutable Any for downcasting.
    #[inline]
    #[must_use]
    pub fn render_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        self.render_object
            .as_mut()
            .map(|b| b.as_mut() as &mut dyn Any)
    }

    /// Downcast render object to concrete type.
    #[inline]
    pub fn render_object_as<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.render_object.as_ref()?.downcast_ref::<T>()
    }

    /// Downcast render object to concrete type (mutable).
    #[inline]
    pub fn render_object_as_mut<T: Any + Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.render_object.as_mut()?.downcast_mut::<T>()
    }

    /// Take the render object out.
    #[inline]
    pub fn take_render_object(&mut self) -> Option<Box<dyn Any + Send + Sync>> {
        self.render_object.take()
    }

    /// Set a new render object.
    #[inline]
    pub fn set_render_object<T: Any + Send + Sync + 'static>(&mut self, render_object: T) {
        self.render_object = Some(Box::new(render_object));
    }

    // ========== Render State Access ==========

    /// Returns true if this element has a render state.
    #[inline]
    #[must_use]
    pub fn has_render_state(&self) -> bool {
        self.render_state.is_some()
    }

    /// Get the render state as Any for downcasting.
    #[inline]
    #[must_use]
    pub fn render_state(&self) -> Option<&dyn Any> {
        self.render_state.as_ref().map(|b| b.as_ref() as &dyn Any)
    }

    /// Get the render state as mutable Any for downcasting.
    #[inline]
    #[must_use]
    pub fn render_state_mut(&mut self) -> Option<&mut dyn Any> {
        self.render_state
            .as_mut()
            .map(|b| b.as_mut() as &mut dyn Any)
    }

    /// Downcast render state to concrete type.
    #[inline]
    pub fn render_state_as<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.render_state.as_ref()?.downcast_ref::<T>()
    }

    /// Downcast render state to concrete type (mutable).
    #[inline]
    pub fn render_state_as_mut<T: Any + Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.render_state.as_mut()?.downcast_mut::<T>()
    }

    /// Take the render state out.
    #[inline]
    pub fn take_render_state(&mut self) -> Option<Box<dyn Any + Send + Sync>> {
        self.render_state.take()
    }

    /// Set a new render state.
    #[inline]
    pub fn set_render_state<T: Any + Send + Sync + 'static>(&mut self, render_state: T) {
        self.render_state = Some(Box::new(render_state));
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
        self.debug_name.unwrap_or("RenderElement")
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

    #[derive(Debug)]
    struct TestRenderObject {
        value: i32,
    }

    #[derive(Debug)]
    struct TestRenderState {
        size: (f32, f32),
    }

    #[test]
    fn test_render_element_creation() {
        let element = RenderElement::new(
            TestRenderObject { value: 42 },
            TestRenderState {
                size: (100.0, 50.0),
            },
            ViewMode::RenderBox,
        );

        assert!(element.has_render_object());
        assert!(element.has_render_state());
        assert_eq!(element.view_mode(), ViewMode::RenderBox);
        assert!(element.is_render());
        assert!(element.is_box());
        assert!(!element.is_sliver());
    }

    #[test]
    fn test_render_element_empty() {
        let element = RenderElement::empty();
        assert!(!element.has_render_object());
        assert!(!element.has_render_state());
    }

    #[test]
    fn test_render_object_downcast() {
        let element = RenderElement::new(
            TestRenderObject { value: 42 },
            TestRenderState {
                size: (100.0, 50.0),
            },
            ViewMode::RenderBox,
        );

        let ro = element.render_object_as::<TestRenderObject>();
        assert!(ro.is_some());
        assert_eq!(ro.unwrap().value, 42);

        let rs = element.render_state_as::<TestRenderState>();
        assert!(rs.is_some());
        assert_eq!(rs.unwrap().size, (100.0, 50.0));
    }

    #[test]
    fn test_lifecycle() {
        let mut element = RenderElement::new(
            TestRenderObject { value: 1 },
            TestRenderState { size: (10.0, 10.0) },
            ViewMode::RenderBox,
        );

        assert_eq!(element.lifecycle(), ElementLifecycle::Initial);

        element.mount(Some(ElementId::new(1)), Some(Slot::new(0)), 1);
        assert_eq!(element.lifecycle(), ElementLifecycle::Active);
        assert!(element.is_mounted());

        element.unmount();
        assert_eq!(element.lifecycle(), ElementLifecycle::Defunct);
    }

    #[test]
    fn test_children_management() {
        let mut element = RenderElement::new(
            TestRenderObject { value: 1 },
            TestRenderState { size: (10.0, 10.0) },
            ViewMode::RenderBox,
        );

        assert!(!element.has_children());

        element.add_child(ElementId::new(10));
        element.add_child(ElementId::new(20));

        assert!(element.has_children());
        assert_eq!(element.child_count(), 2);

        element.remove_child(ElementId::new(10));
        assert_eq!(element.child_count(), 1);
    }
}
