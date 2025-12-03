//! RenderElement - Element that participates in layout and painting.
//!
//! This is the core type for elements that have render objects and
//! participate in the rendering pipeline (layout → paint → hit test).
//!
//! # Architecture
//!
//! ```text
//! RenderElement
//!   ├─ Identity: ElementId, parent, children
//!   ├─ Render: Box<dyn RenderObject>, protocol, arity
//!   ├─ Layout cache: size, offset, constraints
//!   ├─ Dirty flags: needs_layout, needs_paint
//!   └─ Lifecycle: RenderLifecycle
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderElement, ProtocolId, BoxRenderWrapper};
//! use flui_tree::arity::Leaf;
//!
//! // Create from a render object
//! let render_obj = BoxRenderWrapper::<Leaf>::new(MyRenderBox::new());
//! let element = RenderElement::new(render_obj, ProtocolId::Box);
//! ```

use std::any::Any;
use std::fmt;

use flui_foundation::ElementId;
use flui_tree::RuntimeArity;
use flui_types::{Offset, Size};

use super::protocol::ProtocolId;
use super::render_lifecycle::RenderLifecycle;
use super::render_object::RenderObject;
use super::BoxConstraints;

// ============================================================================
// RENDER ELEMENT
// ============================================================================

/// Element that participates in layout and painting.
///
/// `RenderElement` holds a render object and manages its lifecycle
/// through the rendering pipeline. It caches layout results and
/// tracks dirty state for efficient incremental updates.
///
/// # Thread Safety
///
/// `RenderElement` is `Send` because all fields are `Send`.
pub struct RenderElement {
    // ========== Identity ==========
    /// Parent element ID (None for root).
    parent: Option<ElementId>,

    /// Child element IDs.
    children: Vec<ElementId>,

    // ========== Render Object ==========
    /// Type-erased render object.
    render_object: Box<dyn RenderObject>,

    /// Protocol (Box or Sliver).
    protocol: ProtocolId,

    /// Runtime arity (how many children).
    arity: RuntimeArity,

    // ========== Layout Cache ==========
    /// Computed size from layout.
    size: Size,

    /// Offset relative to parent.
    offset: Offset,

    /// Last constraints used for layout.
    constraints: Option<BoxConstraints>,

    // ========== Dirty Flags ==========
    /// Needs layout pass.
    needs_layout: bool,

    /// Needs paint pass.
    needs_paint: bool,

    // ========== Lifecycle ==========
    /// Current lifecycle state.
    lifecycle: RenderLifecycle,

    // ========== Debug ==========
    /// Debug name for diagnostics.
    debug_name: Option<&'static str>,
}

impl fmt::Debug for RenderElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderElement")
            .field("parent", &self.parent)
            .field("children_count", &self.children.len())
            .field("protocol", &self.protocol)
            .field("arity", &self.arity)
            .field("size", &self.size)
            .field("offset", &self.offset)
            .field("needs_layout", &self.needs_layout)
            .field("needs_paint", &self.needs_paint)
            .field("lifecycle", &self.lifecycle)
            .field(
                "debug_name",
                &self.debug_name.unwrap_or(self.render_object.debug_name()),
            )
            .finish()
    }
}

impl RenderElement {
    // ========== Constructors ==========

    /// Creates a new RenderElement with a render object.
    ///
    /// # Arguments
    ///
    /// * `render_object` - The render object (must implement RenderObject)
    /// * `protocol` - ProtocolId::Box or ProtocolId::Sliver
    pub fn new<R: RenderObject>(render_object: R, protocol: ProtocolId) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            render_object: Box::new(render_object),
            protocol,
            arity: RuntimeArity::Exact(0),
            size: Size::ZERO,
            offset: Offset::ZERO,
            constraints: None,
            needs_layout: true,
            needs_paint: true,
            lifecycle: RenderLifecycle::Detached,
            debug_name: None,
        }
    }

    /// Creates a new RenderElement with specified arity.
    pub fn with_arity<R: RenderObject>(
        render_object: R,
        protocol: ProtocolId,
        arity: RuntimeArity,
    ) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            render_object: Box::new(render_object),
            protocol,
            arity,
            size: Size::ZERO,
            offset: Offset::ZERO,
            constraints: None,
            needs_layout: true,
            needs_paint: true,
            lifecycle: RenderLifecycle::Detached,
            debug_name: None,
        }
    }

    /// Creates from a boxed render object.
    pub fn from_boxed(
        render_object: Box<dyn RenderObject>,
        protocol: ProtocolId,
        arity: RuntimeArity,
    ) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            render_object,
            protocol,
            arity,
            size: Size::ZERO,
            offset: Offset::ZERO,
            constraints: None,
            needs_layout: true,
            needs_paint: true,
            lifecycle: RenderLifecycle::Detached,
            debug_name: None,
        }
    }

    // ========== Builder Methods ==========

    /// Set debug name.
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    /// Set parent.
    pub fn with_parent(mut self, parent: ElementId) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Set children.
    pub fn with_children(mut self, children: Vec<ElementId>) -> Self {
        self.children = children;
        self
    }

    // ========== Identity ==========

    /// Get parent element ID.
    #[inline]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Set parent element ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    /// Get children element IDs.
    #[inline]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get mutable children.
    #[inline]
    pub fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Add a child.
    #[inline]
    pub fn add_child(&mut self, child: ElementId) {
        self.children.push(child);
    }

    /// Remove a child.
    #[inline]
    pub fn remove_child(&mut self, child: ElementId) {
        self.children.retain(|&id| id != child);
    }

    /// Check if has children.
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Get child count.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    // ========== Render Object ==========

    /// Get render object reference.
    #[inline]
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    /// Get mutable render object reference.
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut *self.render_object
    }

    /// Downcast render object to concrete type.
    #[inline]
    pub fn render_object_as<T: RenderObject>(&self) -> Option<&T> {
        self.render_object.as_any().downcast_ref::<T>()
    }

    /// Downcast render object to concrete type (mutable).
    #[inline]
    pub fn render_object_as_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.render_object.as_any_mut().downcast_mut::<T>()
    }

    /// Get render object as Any.
    #[inline]
    pub fn render_object_any(&self) -> &dyn Any {
        self.render_object.as_any()
    }

    /// Get mutable render object as Any.
    #[inline]
    pub fn render_object_any_mut(&mut self) -> &mut dyn Any {
        self.render_object.as_any_mut()
    }

    // ========== Protocol & Arity ==========

    /// Get protocol (Box or Sliver).
    #[inline]
    pub fn protocol(&self) -> ProtocolId {
        self.protocol
    }

    /// Check if box protocol.
    #[inline]
    pub fn is_box(&self) -> bool {
        self.protocol == ProtocolId::Box
    }

    /// Check if sliver protocol.
    #[inline]
    pub fn is_sliver(&self) -> bool {
        self.protocol == ProtocolId::Sliver
    }

    /// Get runtime arity.
    #[inline]
    pub fn arity(&self) -> RuntimeArity {
        self.arity
    }

    /// Set runtime arity.
    #[inline]
    pub fn set_arity(&mut self, arity: RuntimeArity) {
        self.arity = arity;
    }

    // ========== Layout Cache ==========

    /// Get computed size.
    #[inline]
    pub fn size(&self) -> Size {
        self.size
    }

    /// Set computed size.
    #[inline]
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// Get offset relative to parent.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Set offset relative to parent.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Get last constraints.
    #[inline]
    pub fn constraints(&self) -> Option<&BoxConstraints> {
        self.constraints.as_ref()
    }

    /// Set constraints.
    #[inline]
    pub fn set_constraints(&mut self, constraints: BoxConstraints) {
        self.constraints = Some(constraints);
    }

    // ========== Dirty Flags ==========

    /// Check if needs layout.
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    /// Mark as needing layout.
    #[inline]
    pub fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
        self.needs_paint = true;
        self.lifecycle.invalidate_layout();
    }

    /// Clear needs layout flag.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
        self.needs_layout = false;
        if self.lifecycle.is_attached() && !self.lifecycle.is_laid_out() {
            self.lifecycle.mark_laid_out();
        }
    }

    /// Check if needs paint.
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.needs_paint
    }

    /// Mark as needing paint.
    #[inline]
    pub fn mark_needs_paint(&mut self) {
        self.needs_paint = true;
        self.lifecycle.invalidate_paint();
    }

    /// Clear needs paint flag.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
        self.needs_paint = false;
        if self.lifecycle == RenderLifecycle::LaidOut {
            self.lifecycle.mark_painted();
        }
    }

    // ========== Lifecycle ==========

    /// Get current lifecycle state.
    #[inline]
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    /// Check if attached to tree.
    #[inline]
    pub fn is_attached(&self) -> bool {
        self.lifecycle.is_attached()
    }

    /// Attach to tree.
    pub fn attach(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
        self.lifecycle.attach();
        self.needs_layout = true;
        self.needs_paint = true;
    }

    /// Detach from tree.
    pub fn detach(&mut self) {
        self.parent = None;
        self.lifecycle.detach();
        self.needs_layout = true;
        self.needs_paint = true;
    }

    // ========== Debug ==========

    /// Get debug name.
    #[inline]
    pub fn debug_name(&self) -> &str {
        self.debug_name
            .unwrap_or_else(|| self.render_object.debug_name())
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

    impl RenderObject for TestRenderObject {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_render_element_creation() {
        let element = RenderElement::new(TestRenderObject { value: 42 }, ProtocolId::Box);

        assert!(element.is_box());
        assert!(!element.is_sliver());
        assert!(element.needs_layout());
        assert!(element.needs_paint());
        assert_eq!(element.lifecycle(), RenderLifecycle::Detached);
    }

    #[test]
    fn test_lifecycle() {
        let mut element = RenderElement::new(TestRenderObject { value: 1 }, ProtocolId::Box);

        assert_eq!(element.lifecycle(), RenderLifecycle::Detached);
        assert!(!element.is_attached());

        element.attach(Some(ElementId::new(1)));
        assert_eq!(element.lifecycle(), RenderLifecycle::Attached);
        assert!(element.is_attached());

        element.clear_needs_layout();
        assert_eq!(element.lifecycle(), RenderLifecycle::LaidOut);

        element.clear_needs_paint();
        assert_eq!(element.lifecycle(), RenderLifecycle::Painted);

        element.detach();
        assert_eq!(element.lifecycle(), RenderLifecycle::Detached);
    }

    #[test]
    fn test_dirty_flags() {
        let mut element = RenderElement::new(TestRenderObject { value: 1 }, ProtocolId::Box);

        element.attach(None);
        element.clear_needs_layout();
        element.clear_needs_paint();

        assert!(!element.needs_layout());
        assert!(!element.needs_paint());

        element.mark_needs_layout();
        assert!(element.needs_layout());
        assert!(element.needs_paint()); // Layout implies paint

        element.clear_needs_layout();
        element.clear_needs_paint();

        element.mark_needs_paint();
        assert!(!element.needs_layout());
        assert!(element.needs_paint());
    }

    #[test]
    fn test_layout_cache() {
        let mut element = RenderElement::new(TestRenderObject { value: 1 }, ProtocolId::Box);

        element.set_size(Size::new(100.0, 50.0));
        element.set_offset(Offset::new(10.0, 20.0));

        assert_eq!(element.size(), Size::new(100.0, 50.0));
        assert_eq!(element.offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_children() {
        let mut element = RenderElement::new(TestRenderObject { value: 1 }, ProtocolId::Box);

        assert!(!element.has_children());

        element.add_child(ElementId::new(10));
        element.add_child(ElementId::new(20));

        assert!(element.has_children());
        assert_eq!(element.child_count(), 2);

        element.remove_child(ElementId::new(10));
        assert_eq!(element.child_count(), 1);
    }

    #[test]
    fn test_downcast() {
        let element = RenderElement::new(TestRenderObject { value: 42 }, ProtocolId::Box);

        let ro = element.render_object_as::<TestRenderObject>();
        assert!(ro.is_some());
        assert_eq!(ro.unwrap().value, 42);
    }
}
