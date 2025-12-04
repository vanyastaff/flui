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
//!   ├─ id: Option<ElementId> (assigned during mount)
//!   ├─ parent: Option<ElementId>
//!   ├─ children: Vec<ElementId>
//!   ├─ depth: usize
//!   ├─ lifecycle: ViewLifecycle
//!   ├─ flags: AtomicViewFlags
//!   ├─ view_object: Option<Box<dyn ViewObject>>
//!   ├─ view_mode: ViewMode
//!   ├─ key: Option<Key>
//!   └─ pending_children: Option<PendingChildren>
//! ```
//!
//! # Differences from RenderElement
//!
//! - No render object or render state
//! - No layout/paint flags (only DIRTY for rebuild)
//! - Simpler lifecycle (no LaidOut/Painted states)
//! - Stores ViewObject instead of RenderObject

use std::any::Any;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::{ElementId, Key, Slot};

use super::flags::{AtomicViewFlags, ViewFlags};
use super::lifecycle::ViewLifecycle;
use crate::view_mode::ViewMode;
use crate::view_object::ViewObject;

/// Type-erased pending children storage.
///
/// This allows ViewElement to store pending children without depending
/// on the concrete Element type from flui-element.
pub type PendingChildren = Vec<Box<dyn Any + Send>>;

/// ViewElement - Element for component views.
///
/// Represents views that build child elements (Stateless, Stateful,
/// Provider, Proxy, Animated). Does NOT directly participate in
/// layout/paint - that's handled by RenderElement.
///
/// # Thread Safety
///
/// ViewElement is `Send` because:
/// - ViewObject requires `Send`
/// - AtomicViewFlags is lock-free
/// - All other fields are Send
pub struct ViewElement {
    // ========== Identity ==========
    /// This element's ID (set during mount).
    id: Option<ElementId>,

    /// Parent element ID (None for root).
    parent: Option<ElementId>,

    /// Slot position in parent's child list.
    slot: Option<Slot>,

    /// Child element IDs (after mount).
    children: Vec<ElementId>,

    /// Cached depth in tree (0 = root).
    depth: AtomicUsize,

    // ========== Lifecycle ==========
    /// Current lifecycle state.
    lifecycle: ViewLifecycle,

    /// Atomic flags for lock-free dirty tracking.
    flags: AtomicViewFlags,

    // ========== View ==========
    /// Type-erased view object storage.
    view_object: Option<Box<dyn ViewObject>>,

    /// View mode - categorizes the view type.
    view_mode: ViewMode,

    /// Optional key for reconciliation.
    key: Option<Key>,

    /// Pending child elements (before mount, processed by BuildPipeline).
    /// Type-erased to avoid dependency on flui-element::Element.
    pending_children: Option<PendingChildren>,

    /// Debug name for diagnostics.
    debug_name: Option<&'static str>,
}

impl fmt::Debug for ViewElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewElement")
            .field("id", &self.id)
            .field("parent", &self.parent)
            .field("lifecycle", &self.lifecycle)
            .field("view_mode", &self.view_mode)
            .field("children_count", &self.children.len())
            .field("has_view_object", &self.view_object.is_some())
            .field("debug_name", &self.debug_name)
            .finish()
    }
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl ViewElement {
    /// Creates a new ViewElement with the given view object and mode.
    pub fn new<V: ViewObject>(view_object: V, mode: ViewMode) -> Self {
        debug_assert!(
            mode.is_component() || mode.is_empty(),
            "ViewElement should only be used for component views, got {mode:?}"
        );

        let flags = AtomicViewFlags::new();
        flags.insert(ViewFlags::DIRTY); // Needs initial build

        Self {
            id: None,
            parent: None,
            slot: None,
            children: Vec::new(),
            depth: AtomicUsize::new(0),
            lifecycle: ViewLifecycle::Initial,
            flags,
            view_object: Some(Box::new(view_object)),
            view_mode: mode,
            key: None,
            pending_children: None,
            debug_name: None,
        }
    }

    /// Creates an empty ViewElement (no view object).
    pub fn empty() -> Self {
        Self {
            id: None,
            parent: None,
            slot: None,
            children: Vec::new(),
            depth: AtomicUsize::new(0),
            lifecycle: ViewLifecycle::Initial,
            flags: AtomicViewFlags::new(),
            view_object: None,
            view_mode: ViewMode::Empty,
            key: None,
            pending_children: None,
            debug_name: Some("Empty"),
        }
    }

    /// Creates a container ViewElement with pending children.
    pub fn container(children: PendingChildren) -> Self {
        let child_count = children.len();
        Self {
            id: None,
            parent: None,
            slot: None,
            children: Vec::with_capacity(child_count),
            depth: AtomicUsize::new(0),
            lifecycle: ViewLifecycle::Initial,
            flags: AtomicViewFlags::new(),
            view_object: None,
            view_mode: ViewMode::Empty,
            key: None,
            pending_children: Some(children),
            debug_name: Some("Container"),
        }
    }

    /// Builder: set debug name.
    #[must_use]
    pub fn with_debug_name(mut self, name: &'static str) -> Self {
        self.debug_name = Some(name);
        self
    }

    /// Builder: set key.
    #[must_use]
    pub fn with_key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    /// Builder: set pending children.
    #[must_use]
    pub fn with_pending_children(mut self, children: PendingChildren) -> Self {
        self.pending_children = Some(children);
        self
    }
}

// ============================================================================
// IDENTITY & TREE NAVIGATION
// ============================================================================

impl ViewElement {
    /// Get element ID.
    #[inline]
    #[must_use]
    pub fn id(&self) -> Option<ElementId> {
        self.id
    }

    /// Get parent element ID.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.parent
    }

    /// Set parent element ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        self.parent = parent;
    }

    /// Get slot position.
    #[inline]
    #[must_use]
    pub fn slot(&self) -> Option<Slot> {
        self.slot
    }

    /// Get cached depth in tree.
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth.load(Ordering::Relaxed)
    }

    /// Set cached depth.
    #[inline]
    pub fn set_depth(&self, depth: usize) {
        self.depth.store(depth, Ordering::Relaxed);
    }
}

// ============================================================================
// LIFECYCLE
// ============================================================================

impl ViewElement {
    /// Mount element to tree.
    ///
    /// # Arguments
    ///
    /// * `id` - ElementId assigned by the tree
    /// * `parent` - Parent element ID (None for root)
    /// * `slot` - Slot position in parent
    /// * `depth` - Depth in tree (0 for root)
    pub fn mount(
        &mut self,
        id: ElementId,
        parent: Option<ElementId>,
        slot: Option<Slot>,
        depth: usize,
    ) {
        self.id = Some(id);
        self.parent = parent;
        self.slot = slot;
        self.depth.store(depth, Ordering::Relaxed);
        self.lifecycle.mount();
        self.flags
            .insert(ViewFlags::DIRTY | ViewFlags::MOUNTED | ViewFlags::ACTIVE);
    }

    /// Unmount element from tree.
    pub fn unmount(&mut self) {
        self.lifecycle.unmount();
        self.flags.remove(ViewFlags::MOUNTED | ViewFlags::ACTIVE);
        self.id = None;
    }

    /// Activate element.
    pub fn activate(&mut self) {
        self.lifecycle.activate();
        self.flags.insert(ViewFlags::ACTIVE | ViewFlags::DIRTY);
    }

    /// Deactivate element.
    pub fn deactivate(&mut self) {
        self.lifecycle.deactivate();
        self.flags.remove(ViewFlags::ACTIVE);
    }

    /// Get current lifecycle state.
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ViewLifecycle {
        self.lifecycle
    }

    /// Check if mounted.
    #[inline]
    #[must_use]
    pub fn is_mounted(&self) -> bool {
        self.flags.is_mounted()
    }

    /// Check if active.
    #[inline]
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.flags.is_active()
    }
}

// ============================================================================
// DIRTY TRACKING
// ============================================================================

impl ViewElement {
    /// Check if element needs rebuild.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.flags.is_dirty()
    }

    /// Mark element as needing rebuild.
    #[inline]
    pub fn mark_dirty(&self) {
        self.flags.mark_dirty();
    }

    /// Clear dirty flag.
    #[inline]
    pub fn clear_dirty(&self) {
        self.flags.clear_dirty();
    }

    /// Get the flags.
    #[inline]
    #[must_use]
    pub fn flags(&self) -> &AtomicViewFlags {
        &self.flags
    }
}

// ============================================================================
// VIEW MODE & KEY
// ============================================================================

impl ViewElement {
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
}

// ============================================================================
// PENDING CHILDREN
// ============================================================================

impl ViewElement {
    /// Take pending children for processing.
    pub fn take_pending_children(&mut self) -> Option<PendingChildren> {
        self.pending_children.take()
    }

    /// Check if element has pending children.
    #[inline]
    #[must_use]
    pub fn has_pending_children(&self) -> bool {
        self.pending_children.is_some()
    }

    /// Set pending children.
    pub fn set_pending_children(&mut self, children: PendingChildren) {
        self.pending_children = Some(children);
    }
}

// ============================================================================
// VIEW OBJECT ACCESS
// ============================================================================

impl ViewElement {
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
}

// ============================================================================
// CHILD MANAGEMENT
// ============================================================================

impl ViewElement {
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
}

// ============================================================================
// DEBUG
// ============================================================================

impl ViewElement {
    /// Get debug name.
    #[inline]
    #[must_use]
    pub fn debug_name(&self) -> &'static str {
        self.debug_name.unwrap_or("ViewElement")
    }

    /// Set debug name.
    #[inline]
    pub fn set_debug_name(&mut self, name: &'static str) {
        self.debug_name = Some(name);
    }

    /// Get debug description.
    #[must_use]
    pub fn debug_description(&self) -> String {
        format!(
            "{}#{:?} ({}, {:?})",
            self.debug_name(),
            self.id,
            self.lifecycle,
            self.view_mode
        )
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

    #[test]
    fn test_view_element_creation() {
        let element = ViewElement::new(TestViewObject { value: 42 }, ViewMode::Stateless);
        assert!(element.has_view_object());
        assert_eq!(element.view_mode(), ViewMode::Stateless);
        assert!(element.is_component());
        assert!(!element.is_provider());
        assert!(element.is_dirty()); // Initially dirty
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

        assert_eq!(element.lifecycle(), ViewLifecycle::Initial);

        element.mount(
            ElementId::new(1),
            Some(ElementId::new(100)), // Parent ID (must be non-zero)
            Some(Slot::new(0)),
            1,
        );
        assert_eq!(element.lifecycle(), ViewLifecycle::Active);
        assert!(element.is_mounted());
        assert!(element.is_active());

        element.deactivate();
        assert_eq!(element.lifecycle(), ViewLifecycle::Inactive);
        assert!(!element.is_active());

        element.activate();
        assert_eq!(element.lifecycle(), ViewLifecycle::Active);
        assert!(element.is_active());

        element.unmount();
        assert_eq!(element.lifecycle(), ViewLifecycle::Defunct);
        assert!(!element.is_mounted());
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

    #[test]
    fn test_dirty_tracking() {
        let element = ViewElement::new(TestViewObject { value: 1 }, ViewMode::Stateless);

        // Initially dirty
        assert!(element.is_dirty());

        // Clear
        element.clear_dirty();
        assert!(!element.is_dirty());

        // Mark dirty
        element.mark_dirty();
        assert!(element.is_dirty());
    }
}
