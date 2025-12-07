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
use flui_tree::RuntimeArity;
use flui_view::{PendingChildren, ViewLifecycle, ViewMode};

use flui_rendering::{ProtocolId, RenderElement, RenderLifecycle, RenderObject};

use super::{ElementLifecycle, ViewElement};
use crate::ViewObject;

// ============================================================================
// LIFECYCLE CONVERSION HELPERS
// ============================================================================

/// Converts ViewLifecycle to ElementLifecycle.
fn view_to_element_lifecycle(lifecycle: ViewLifecycle) -> ElementLifecycle {
    match lifecycle {
        ViewLifecycle::Initial => ElementLifecycle::Initial,
        ViewLifecycle::Active => ElementLifecycle::Active,
        ViewLifecycle::Inactive => ElementLifecycle::Inactive,
        ViewLifecycle::Defunct => ElementLifecycle::Defunct,
    }
}

/// Converts RenderLifecycle to ElementLifecycle.
fn render_to_element_lifecycle(lifecycle: RenderLifecycle) -> ElementLifecycle {
    match lifecycle {
        RenderLifecycle::Detached => ElementLifecycle::Initial,
        RenderLifecycle::Attached
        | RenderLifecycle::NeedsLayout
        | RenderLifecycle::LaidOut
        | RenderLifecycle::NeedsPaint
        | RenderLifecycle::Painted => ElementLifecycle::Active,
    }
}

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

    /// Creates a new View element with the given view ID and mode.
    ///
    /// Note: In the four-tree architecture, view objects are stored in ViewTree.
    /// This constructor creates an element that references a view by ID.
    pub fn view(view_id: Option<flui_view::ViewId>, mode: ViewMode) -> Self {
        Self::View(ViewElement::new(view_id, mode))
    }

    /// Creates a new Render element with render ID and protocol.
    ///
    /// Note: In the four-tree architecture, render objects are stored in RenderTree.
    /// This constructor creates an element that references a render object by ID.
    pub fn render(render_id: Option<flui_rendering::RenderId>, protocol: ProtocolId) -> Self {
        Self::Render(RenderElement::new(render_id, protocol, RuntimeArity::Variable))
    }

    /// Creates a new Render element with render ID, protocol, and arity.
    pub fn render_with_arity(
        render_id: Option<flui_rendering::RenderId>,
        protocol: ProtocolId,
        arity: RuntimeArity,
    ) -> Self {
        Self::Render(RenderElement::new(render_id, protocol, arity))
    }

    /// Creates an empty element (View variant).
    pub fn empty() -> Self {
        Self::View(ViewElement::empty())
    }

    /// Creates a container element with pending children.
    ///
    /// Note: Children are type-erased to `PendingChildren` (Vec<Box<dyn Any + Send + Sync>>).
    /// Use `Element::boxed_children()` to convert `Vec<Element>` to `PendingChildren`.
    pub fn container(children: PendingChildren) -> Self {
        Self::View(ViewElement::container(children))
    }

    /// Converts a Vec<Element> to PendingChildren for use with container().
    pub fn boxed_children(children: Vec<Element>) -> PendingChildren {
        children
            .into_iter()
            .map(|e| Box::new(e) as Box<dyn Any + Send + Sync>)
            .collect()
    }

    /// Creates an element with view ID and mode (for backward compatibility).
    ///
    /// Note: API changed - now takes ViewId instead of ViewObject.
    pub fn with_mode(view_id: Option<flui_view::ViewId>, mode: ViewMode) -> Self {
        Self::View(ViewElement::new(view_id, mode))
    }

    /// Creates a new view element with optional view ID (backward compatibility alias).
    ///
    /// Note: API changed - now takes ViewId instead of ViewObject.
    pub fn new(view_id: Option<flui_view::ViewId>) -> Self {
        Self::View(ViewElement::new(view_id, ViewMode::Empty))
    }

    // ========== Element ID ==========

    /// Get the element's unique ID.
    ///
    /// Returns `None` if the element hasn't been mounted yet.
    #[inline]
    #[must_use]
    pub fn id(&self) -> Option<ElementId> {
        match self {
            Self::View(v) => v.id(),
            Self::Render(r) => r.id(),
        }
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
            Self::Render(r) => {
                // RenderElement uses ProtocolId, convert to ViewMode
                match r.protocol() {
                    ProtocolId::Box => ViewMode::RenderBox,
                    ProtocolId::Sliver => ViewMode::RenderSliver,
                }
            }
        }
    }

    /// Set the view mode.
    ///
    /// Note: For `Render` elements, view mode is derived from protocol and cannot be changed.
    #[inline]
    pub fn set_view_mode(&mut self, mode: ViewMode) {
        match self {
            Self::View(v) => v.set_view_mode(mode),
            Self::Render(_) => {
                // RenderElement's view_mode is derived from protocol, ignore
            }
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
    ///
    /// Note: RenderElement doesn't support keys, returns None.
    #[inline]
    #[must_use]
    pub fn key(&self) -> Option<Key> {
        match self {
            Self::View(v) => v.key(),
            Self::Render(_) => None, // RenderElement doesn't have key
        }
    }

    /// Set the key.
    ///
    /// Note: Has no effect on RenderElement.
    #[inline]
    pub fn set_key(&mut self, key: Option<Key>) {
        match self {
            Self::View(v) => v.set_key(key),
            Self::Render(_) => {} // RenderElement doesn't support keys
        }
    }

    /// Builder: set key.
    pub fn with_key(mut self, key: Key) -> Self {
        self.set_key(Some(key));
        self
    }

    // ========== Pending Children ==========

    /// Take pending children for processing.
    ///
    /// Returns type-erased children. Downcast each to `Element` using:
    /// ```ignore
    /// let elements: Vec<Element> = pending
    ///     .into_iter()
    ///     .filter_map(|b| b.downcast::<Element>().ok().map(|b| *b))
    ///     .collect();
    /// ```
    pub fn take_pending_children(&mut self) -> Option<PendingChildren> {
        match self {
            Self::View(v) => v.take_pending_children(),
            Self::Render(_) => None, // RenderElement doesn't support pending children
        }
    }

    /// Take pending children and downcast to Vec<Element>.
    pub fn take_pending_children_as_elements(&mut self) -> Option<Vec<Element>> {
        self.take_pending_children().map(|pending| {
            pending
                .into_iter()
                .filter_map(|b| b.downcast::<Element>().ok().map(|b| *b))
                .collect()
        })
    }

    /// Check if element has pending children.
    #[inline]
    #[must_use]
    pub fn has_pending_children(&self) -> bool {
        match self {
            Self::View(v) => v.has_pending_children(),
            Self::Render(_) => false, // RenderElement doesn't support pending children
        }
    }

    /// Builder: set pending children (type-erased).
    ///
    /// Note: Only works for View elements. RenderElement doesn't support pending children.
    pub fn with_pending_children(mut self, children: PendingChildren) -> Self {
        if let Self::View(v) = &mut self {
            *v = std::mem::replace(v, ViewElement::empty()).with_pending_children(children);
        }
        // RenderElement doesn't support pending children - ignore silently
        self
    }

    /// Builder: set pending children from Vec<Element>.
    pub fn with_element_children(self, children: Vec<Element>) -> Self {
        self.with_pending_children(Self::boxed_children(children))
    }

    // ========== View Object Access (DEPRECATED - objects now in ViewTree) ==========
    //
    // Note: In the four-tree architecture, ViewObjects are stored in ViewTree, not in ViewElement.
    // ViewElement only holds a ViewId reference. To access view objects, use ViewTree.get(view_id).
    //
    // These methods are kept for API compatibility but always return None/false.

    /// Returns true if this element has a view object.
    ///
    /// **DEPRECATED**: Always returns false. View objects are stored in ViewTree.
    /// Use `ViewElement::view_id()` to get the ID, then `ViewTree::get(id)` to access the object.
    #[inline]
    #[must_use]
    #[deprecated(note = "View objects are now stored in ViewTree. Use ViewElement::view_id() and ViewTree::get()")]
    pub fn has_view_object(&self) -> bool {
        false // ViewElement no longer stores objects
    }

    /// Get the view object as a reference.
    ///
    /// **DEPRECATED**: Always returns None. View objects are stored in ViewTree.
    #[inline]
    #[must_use]
    #[deprecated(note = "View objects are now stored in ViewTree. Use ViewElement::view_id() and ViewTree::get()")]
    pub fn view_object(&self) -> Option<&dyn ViewObject> {
        None // ViewElement no longer stores objects
    }

    /// Get the view object as a mutable reference.
    ///
    /// **DEPRECATED**: Always returns None. View objects are stored in ViewTree.
    #[inline]
    #[must_use]
    #[deprecated(note = "View objects are now stored in ViewTree. Use ViewElement::view_id() and ViewTree::get_mut()")]
    pub fn view_object_mut(&mut self) -> Option<&mut dyn ViewObject> {
        None // ViewElement no longer stores objects
    }

    /// Get the view object as Any for downcasting.
    ///
    /// **DEPRECATED**: Always returns None. View objects are stored in ViewTree.
    #[inline]
    #[must_use]
    #[deprecated(note = "View objects are now stored in ViewTree")]
    pub fn view_object_any(&self) -> Option<&dyn Any> {
        None
    }

    /// Get the view object as mutable Any for downcasting.
    ///
    /// **DEPRECATED**: Always returns None. View objects are stored in ViewTree.
    #[inline]
    #[must_use]
    #[deprecated(note = "View objects are now stored in ViewTree")]
    pub fn view_object_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    /// Downcast view object to concrete type.
    ///
    /// **DEPRECATED**: Always returns None. View objects are stored in ViewTree.
    #[inline]
    #[deprecated(note = "View objects are now stored in ViewTree")]
    pub fn view_object_as<V: Any + Send + Sync + 'static>(&self) -> Option<&V> {
        None
    }

    /// Downcast view object to concrete type (mutable).
    ///
    /// **DEPRECATED**: Always returns None. View objects are stored in ViewTree.
    #[inline]
    #[deprecated(note = "View objects are now stored in ViewTree")]
    pub fn view_object_as_mut<V: Any + Send + Sync + 'static>(&mut self) -> Option<&mut V> {
        None
    }

    /// Take the view object out.
    ///
    /// **DEPRECATED**: Always returns None. View objects are stored in ViewTree.
    #[inline]
    #[deprecated(note = "View objects are now stored in ViewTree")]
    pub fn take_view_object(&mut self) -> Option<Box<dyn ViewObject>> {
        None
    }

    /// Set a new view object (View variant only).
    ///
    /// **DEPRECATED**: No-op. View objects are stored in ViewTree.
    #[inline]
    #[deprecated(note = "View objects are now stored in ViewTree. Use ViewTree::insert() or ViewTree::update()")]
    pub fn set_view_object<V: ViewObject>(&mut self, _view_object: V) {
        // No-op - ViewElement no longer stores objects
    }

    /// Set view object from boxed ViewObject.
    ///
    /// **DEPRECATED**: No-op. View objects are stored in ViewTree.
    #[inline]
    #[deprecated(note = "View objects are now stored in ViewTree. Use ViewTree::insert() or ViewTree::update()")]
    pub fn set_view_object_boxed(&mut self, _view_object: Box<dyn ViewObject>) {
        // No-op - ViewElement no longer stores objects
    }

    // ========== Render State Access (for RenderTreeAccess trait) ==========

    /// Returns the render state for this element.
    ///
    /// **DEPRECATED**: Always returns None. View objects are in ViewTree, render state in RenderTree.
    #[inline]
    #[deprecated(note = "State is now accessed via ViewTree and RenderTree")]
    pub fn render_state(&self) -> Option<&dyn Any> {
        None // State access requires tree access
    }

    /// Returns a mutable reference to the render state.
    ///
    /// **DEPRECATED**: Always returns None. View objects are in ViewTree, render state in RenderTree.
    #[inline]
    #[deprecated(note = "State is now accessed via ViewTree and RenderTree")]
    pub fn render_state_mut(&mut self) -> Option<&mut dyn Any> {
        None // State access requires tree access
    }

    // ========== Render Object Access (for RenderTreeAccess trait) ==========

    /// Returns the render object for this element.
    ///
    /// **DEPRECATED**: Always returns None. Render objects are stored in RenderTree.
    /// Use `RenderElement::render_id()` to get the ID, then `RenderTree::get(id)` to access the object.
    #[inline]
    #[deprecated(note = "Render objects are now stored in RenderTree. Use RenderElement::render_id() and RenderTree::get()")]
    pub fn render_object(&self) -> Option<&dyn Any> {
        None // RenderElement no longer stores objects
    }

    /// Returns a mutable reference to the render object.
    ///
    /// **DEPRECATED**: Always returns None. Render objects are stored in RenderTree.
    /// Use `RenderElement::render_id()` to get the ID, then `RenderTree::get_mut(id)` to access the object.
    #[inline]
    #[deprecated(note = "Render objects are now stored in RenderTree. Use RenderElement::render_id() and RenderTree::get_mut()")]
    pub fn render_object_mut(&mut self) -> Option<&mut dyn Any> {
        None // RenderElement no longer stores objects
    }

    // ========== Lifecycle Delegation ==========

    /// Mount element to tree.
    ///
    /// For View elements, an ElementId must be provided externally.
    /// Use `mount_with_id` for View elements or call this after the ID is set.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>, depth: usize) {
        match self {
            Self::View(v) => {
                // ViewElement needs an ID - generate a placeholder if not set
                // In practice, the tree should assign IDs before mounting
                let id = v.id().unwrap_or_else(|| ElementId::new(1));
                v.mount(id, parent, slot, depth);
            }
            Self::Render(r) => {
                // RenderElement takes (id, parent) - generate placeholder ID
                let id = r.id().unwrap_or_else(|| ElementId::new(1));
                r.mount(id, parent);
                r.set_depth(depth);
            }
        }
    }

    /// Mount element to tree with explicit ID (for View elements).
    #[inline]
    pub fn mount_with_id(
        &mut self,
        id: ElementId,
        parent: Option<ElementId>,
        slot: Option<Slot>,
        depth: usize,
    ) {
        match self {
            Self::View(v) => v.mount(id, parent, slot, depth),
            Self::Render(r) => {
                r.mount(id, parent);
                r.set_depth(depth);
            }
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
    ///
    /// Note: Both ViewElement and RenderElement use their own lifecycle enums,
    /// which are converted to ElementLifecycle for a unified API.
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        match self {
            Self::View(v) => view_to_element_lifecycle(v.lifecycle()),
            Self::Render(r) => render_to_element_lifecycle(r.lifecycle()),
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
    pub fn set_depth(&mut self, depth: usize) {
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
    ///
    /// Note: RenderElement doesn't track slot position.
    #[inline]
    #[must_use]
    pub fn slot(&self) -> Option<Slot> {
        match self {
            Self::View(v) => v.slot(),
            Self::Render(_) => None, // RenderElement doesn't track slot
        }
    }

    /// Set parent element ID.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<ElementId>) {
        match self {
            Self::View(v) => v.set_parent(parent),
            Self::Render(r) => r.set_parent(parent),
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
    ///
    /// Note: For RenderElement, this marks as needing layout.
    #[inline]
    pub fn mark_dirty(&mut self) {
        match self {
            Self::View(v) => v.mark_dirty(),
            Self::Render(r) => r.mark_needs_layout(), // RenderElement uses layout flags
        }
    }

    /// Clear dirty flag.
    ///
    /// Note: For RenderElement, dirty flags are cleared during pipeline phases.
    #[inline]
    pub fn clear_dirty(&mut self) {
        match self {
            Self::View(v) => v.clear_dirty(),
            Self::Render(r) => {
                // RenderElement clears dirty via pipeline phases
                r.clear_needs_layout();
                r.clear_needs_paint();
            }
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
    pub fn mark_needs_layout(&mut self) {
        if let Self::Render(r) = self {
            r.mark_needs_layout();
        }
    }

    /// Clear needs layout.
    #[inline]
    pub fn clear_needs_layout(&mut self) {
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
    pub fn mark_needs_paint(&mut self) {
        if let Self::Render(r) = self {
            r.mark_needs_paint();
        }
    }

    /// Clear needs paint.
    #[inline]
    pub fn clear_needs_paint(&mut self) {
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
            Self::Render(r) => r.is_attached(), // RenderElement uses is_attached()
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
            Self::Render(r) => r.children_mut().clear(), // RenderElement doesn't have clear_children()
        }
    }

    /// Set children from iterator.
    #[inline]
    pub fn set_children(&mut self, children: impl IntoIterator<Item = ElementId>) {
        match self {
            Self::View(v) => v.set_children(children),
            Self::Render(r) => {
                // RenderElement doesn't have set_children(), use children_mut()
                let children_vec = r.children_mut();
                children_vec.clear();
                children_vec.extend(children);
            }
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
            Self::Render(r) => r.children().first().copied(),
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

    // Note: ElementBase accessors removed - RenderElement manages its own state internally.
    // Use Element's delegate methods (parent(), depth(), lifecycle(), etc.) instead.

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
// BUILD CONTEXT IMPLEMENTATION
// ============================================================================

use flui_view::context::BuildContext;
use std::any::TypeId;
use std::sync::Arc;

/// Implements BuildContext for Element.
///
/// This provides a basic BuildContext implementation where:
/// - Element identity methods work fully (element_id, depth, parent_id)
/// - mark_dirty() works via Element's dirty flag
/// - Tree-walking methods (visit_ancestors, depend_on_raw) return empty results
///   because Element doesn't have direct tree access
///
/// For full BuildContext functionality with tree access, use PipelineBuildContext
/// from the flui-pipeline crate.
impl BuildContext for Element {
    fn element_id(&self) -> ElementId {
        self.id().expect("Element not mounted - no ID assigned")
    }

    fn depth(&self) -> usize {
        self.depth()
    }

    fn parent_id(&self) -> Option<ElementId> {
        self.parent()
    }

    fn mark_dirty(&self) {
        // Note: Element.mark_dirty() requires &mut self, but BuildContext requires &self.
        // This is a limitation - we can only read the dirty state, not set it through trait.
        // Real dirty marking should go through the mutable Element reference or BuildOwner.
        tracing::trace!(
            "BuildContext::mark_dirty called on Element - use mutable access for actual marking"
        );
    }

    fn schedule_rebuild(&self, element_id: ElementId) {
        // Element alone doesn't have access to BuildOwner or dirty set.
        // This needs to be handled by a higher-level context (PipelineBuildContext).
        tracing::trace!(
            "BuildContext::schedule_rebuild({:?}) - requires BuildOwner",
            element_id
        );
    }

    fn create_rebuild_callback(&self) -> Box<dyn Fn() + Send + Sync> {
        // Intentional stub: Element doesn't have access to dirty set or scheduling.
        // This is by design - Element is a data holder, not a scheduler.
        //
        // Real implementation is in PipelineBuildContext which has access to
        // the dirty set and can properly schedule rebuilds.
        //
        // This no-op callback is safe because:
        // - Element is rarely used directly as BuildContext in production
        // - PipelineBuildContext is the primary BuildContext implementation
        // - Tests use MockBuildContext which also has a no-op implementation
        tracing::trace!(
            "BuildContext::create_rebuild_callback() called on Element - \
             this is a stub. Use PipelineBuildContext for real rebuild scheduling."
        );
        Box::new(|| {})
    }

    fn depend_on_raw(&self, _type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        // Element doesn't have tree access to walk ancestors and find providers.
        // This needs PipelineBuildContext which has tree access.
        None
    }

    fn find_ancestor_widget(&self, _type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        // Element doesn't have tree access.
        None
    }

    fn visit_ancestors(&self, _visitor: &mut dyn FnMut(ElementId) -> bool) {
        // Element only knows parent_id, not the actual parent Element.
        // Full ancestor walking requires tree access (PipelineBuildContext).
    }

    fn as_any(&self) -> &dyn Any {
        self
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

    impl RenderObject for TestRenderObject {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn debug_name(&self) -> &'static str {
            "TestRenderObject"
        }
    }

    #[test]
    fn test_element_view_variant() {
        // In four-tree architecture, create element with ViewId (None = not in tree yet)
        let element = Element::view(None, ViewMode::Stateless);

        assert!(element.is_view_element());
        assert!(!element.is_render_element());
        assert!(element.is_component());
        assert!(!element.is_render());
        #[allow(deprecated)]
        {
            assert!(!element.has_view_object()); // Always false - objects in ViewTree
        }
    }

    #[test]
    fn test_element_render_variant() {
        // In four-tree architecture, create element with RenderId (None = not in tree yet)
        let element = Element::render(None, ProtocolId::Box);

        assert!(!element.is_view_element());
        assert!(element.is_render_element());
        assert!(!element.is_component());
        assert!(element.is_render());
        #[allow(deprecated)]
        {
            assert!(!element.has_view_object()); // Always false - not a view element
        }
    }

    #[test]
    fn test_element_empty() {
        let element = Element::empty();
        assert!(element.is_view_element());
        #[allow(deprecated)]
        {
            assert!(!element.has_view_object()); // Always false - objects in ViewTree
        }
    }

    #[test]
    fn test_as_view_as_render() {
        let view = Element::view(None, ViewMode::Stateless);
        assert!(view.as_view().is_some());
        assert!(view.as_render().is_none());

        let render = Element::render(None, ProtocolId::Box);
        assert!(render.as_view().is_none());
        assert!(render.as_render().is_some());
    }

    #[test]
    fn test_lifecycle() {
        let mut element = Element::view(None, ViewMode::Stateless);

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
        let mut element = Element::view(None, ViewMode::Stateless);

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
        // Test that API works with ViewId (None = not in tree yet)
        let element = Element::with_mode(None, ViewMode::Stateless);
        #[allow(deprecated)]
        {
            assert!(!element.has_view_object()); // Always false - objects in ViewTree
        }

        let element2 = Element::new(None);
        #[allow(deprecated)]
        {
            assert!(!element2.has_view_object()); // Always false - objects in ViewTree
        }
    }
}
