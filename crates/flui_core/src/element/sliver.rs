//! SliverElement - Performs sliver layout and paint for scrollable content
//!
//! SliverElement owns a RenderSliver and manages its sliver-specific layout/paint lifecycle.

use parking_lot::RwLock;

use super::{ElementBase, ElementLifecycle};
use crate::element::{Element, ElementId};
use crate::foundation::Slot;
use crate::render::{RenderSliver, RenderSliverState, SliverLayoutContext, SliverPaintContext};

/// Element for sliver render objects (type-erased)
///
/// SliverElement owns a RenderSliver and manages its lifecycle.
/// Unlike RenderElement which uses BoxConstraints → Size,
/// SliverElement uses SliverConstraints → SliverGeometry.
///
/// # Architecture
///
/// ```text
/// SliverElement
///   ├─ render_object: RwLock<Box<dyn RenderSliver>> (type-erased RenderSliver)
///   ├─ render_state: RwLock<RenderSliverState> (geometry, constraints, dirty flags)
///   ├─ offset: Offset (position relative to viewport)
///   ├─ parent_data: Option<Box<dyn ParentData>> (metadata from parent)
///   ├─ children: Vec<ElementId> (managed by RenderSliver arity)
///   └─ lifecycle state
/// ```
///
/// # Type Erasure
///
/// Uses type erasure for the render object:
///
/// - **RenderSliver**: `Box<dyn RenderSliver>` (user-extensible, unbounded types)
/// - **Arity**: Runtime information via RenderSliver trait
///
/// # Performance
///
/// RenderSliver is `Box<dyn>`, but this is acceptable because:
/// - Layout/paint operations use interior mutability (RwLock)
/// - Hot path (layout) uses trait methods, not enum dispatch
/// - Element enum provides fast dispatch for element operations
///
/// # Lifecycle
///
/// 1. **create_render_object()** - View creates RenderSliver
/// 2. **mount()** - Element mounted to tree
/// 3. **update_render_object()** - View config changes
/// 4. **layout()** - RenderSliver layout pass (produces SliverGeometry)
/// 5. **paint()** - RenderSliver paint pass
/// 6. **unmount()** - RenderSliver cleanup
pub struct SliverElement {
    /// Common element data (parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// The sliver render object created by the view (type-erased)
    ///
    /// Wrapped in RwLock to allow interior mutability during layout.
    /// RwLock allows multiple concurrent reads OR one exclusive write:
    /// - Multiple immutable borrows during read operations (safe)
    /// - Single mutable borrow during write operations
    /// - More flexible than RefCell which panics on overlapping borrows
    ///
    /// **Note**: When you have `&mut self`, prefer using `render_object_mut_direct()`
    /// to avoid lock contention during the build/mount phase.
    render_object: RwLock<Box<dyn RenderSliver>>,

    /// Render sliver state (geometry, constraints, dirty flags)
    ///
    /// Uses RwLock for interior mutability during layout/paint.
    /// Atomic flags inside RenderSliverState provide lock-free checks.
    render_state: RwLock<RenderSliverState>,

    /// Position of this sliver element relative to viewport
    ///
    /// Set during layout phase by viewport. Used for:
    /// - Determining visible region
    /// - Painting (to position child layers)
    /// - Coordinate transformation during event dispatch
    ///
    /// **Important**: This is in viewport's coordinate space.
    offset: flui_types::Offset,

    /// Parent data attached by parent RenderSliver
    ///
    /// This metadata is set by the parent's layout algorithm (e.g., SliverPadding for padded slivers).
    /// Parent accesses this during layout to determine how to position and size this child.
    parent_data: Option<Box<dyn crate::render::ParentData>>,

    /// Child elements (count enforced by RenderSliver arity at runtime)
    children: Vec<ElementId>,

    /// Unmounted child elements waiting to be inserted into tree
    ///
    /// When SliverBuilder creates child elements, they are stored here.
    /// When this SliverElement is inserted into ElementTree, these children
    /// are automatically inserted and linked.
    unmounted_children: Option<Vec<Element>>,
}

// Manual Debug implementation because RwLock<Box<dyn Trait>> doesn't auto-derive
impl std::fmt::Debug for SliverElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SliverElement")
            .field("base", &self.base)
            .field("render_object", &"<RwLock<Box<dyn RenderSliver>>>")
            .field("render_state", &self.render_state)
            .field("offset", &self.offset)
            .field("parent_data", &self.parent_data.is_some())
            .field("children", &self.children)
            .field(
                "unmounted_children",
                &self.unmounted_children.as_ref().map(|c| c.len()),
            )
            .finish()
    }
}

impl SliverElement {
    /// Create a new SliverElement from a sliver renderer
    ///
    /// # Parameters
    ///
    /// - `render_object` - The sliver renderer (RenderSliver trait implementation) for this element
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let render = Box::new(RenderSliverList::new(item_extent: 50.0));
    /// let element = SliverElement::new(render);
    /// ```
    pub fn new(render_object: Box<dyn RenderSliver>) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(render_object),
            render_state: RwLock::new(RenderSliverState::new()),
            offset: flui_types::Offset::ZERO,
            parent_data: None,
            children: Vec::new(),
            unmounted_children: None,
        }
    }

    /// Create a new SliverElement with unmounted children
    ///
    /// This is used by SliverBuilder to create elements with children
    /// that will be mounted when this element is inserted into the tree.
    pub fn new_with_children(render_object: Box<dyn RenderSliver>, children: Vec<Element>) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(render_object),
            render_state: RwLock::new(RenderSliverState::new()),
            offset: flui_types::Offset::ZERO,
            parent_data: None,
            children: Vec::new(),
            unmounted_children: Some(children),
        }
    }

    /// Get reference to the sliver render object
    ///
    /// Returns a read guard that allows multiple concurrent immutable borrows.
    /// This uses parking_lot::RwLock which is more flexible than RefCell.
    #[inline]
    pub fn render_object(&self) -> parking_lot::RwLockReadGuard<'_, Box<dyn RenderSliver>> {
        self.render_object.read()
    }

    /// Get mutable reference to the sliver render object (through RwLock)
    ///
    /// Returns a write guard that allows exclusive mutable access.
    /// This blocks if there are any active read or write guards.
    #[inline]
    pub fn render_object_mut(&self) -> parking_lot::RwLockWriteGuard<'_, Box<dyn RenderSliver>> {
        self.render_object.write()
    }

    /// Get direct mutable reference to the sliver render object
    ///
    /// This method bypasses the RwLock when we have `&mut self` access.
    /// Use this instead of `render_object_mut()` when you have mutable access
    /// to avoid lock contention.
    ///
    /// # Safety
    ///
    /// This is safe because having `&mut self` guarantees exclusive access.
    #[inline]
    #[must_use]
    fn render_object_mut_direct(&mut self) -> &mut Box<dyn RenderSliver> {
        self.render_object.get_mut()
    }

    /// Get children element IDs
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get reference to render sliver state
    ///
    /// RenderSliverState contains geometry, constraints, and dirty flags.
    /// Uses RwLock for interior mutability during layout/paint.
    #[inline]
    #[must_use]
    pub fn render_state(&self) -> &RwLock<RenderSliverState> {
        &self.render_state
    }

    /// Get the offset (position relative to viewport)
    #[inline]
    #[must_use]
    pub fn offset(&self) -> flui_types::Offset {
        self.offset
    }

    /// Set the offset (called by viewport during layout)
    #[inline]
    pub fn set_offset(&mut self, offset: flui_types::Offset) {
        self.offset = offset;
    }

    /// Get parent data attached to this element
    ///
    /// Returns the ParentData trait object if parent has attached metadata,
    /// None otherwise.
    #[inline]
    #[must_use]
    pub fn parent_data(&self) -> Option<&dyn crate::render::ParentData> {
        self.parent_data
            .as_ref()
            .map(|pd| &**pd as &dyn crate::render::ParentData)
    }

    /// Get mutable parent data attached to this element
    #[inline]
    #[must_use]
    pub fn parent_data_mut(&mut self) -> Option<&mut dyn crate::render::ParentData> {
        self.parent_data
            .as_mut()
            .map(|pd| &mut **pd as &mut dyn crate::render::ParentData)
    }

    /// Set parent data for this element
    ///
    /// Called by parent RenderSliver during setup or when parent changes.
    pub fn set_parent_data(&mut self, parent_data: Box<dyn crate::render::ParentData>) {
        self.parent_data = Some(parent_data);
    }

    /// Clear parent data
    pub fn clear_parent_data(&mut self) {
        self.parent_data = None;
    }

    /// Perform layout on this sliver render object
    ///
    /// Creates a SliverLayoutContext and calls the render object's layout() method.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `constraints`: Sliver layout constraints from viewport
    ///
    /// # Returns
    ///
    /// The computed sliver geometry
    pub(crate) fn layout_sliver(
        &self,
        tree: &crate::element::ElementTree,
        constraints: flui_types::SliverConstraints,
    ) -> flui_types::SliverGeometry {
        let mut render = self.render_object.write();
        let children = crate::render::Children::from_slice(&self.children);
        let ctx = SliverLayoutContext::new(tree, &children, constraints);
        render.layout(&ctx)
    }

    /// Perform paint on this sliver render object
    ///
    /// Creates a SliverPaintContext and calls the render object's paint() method.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `offset`: Paint offset in viewport's coordinate space
    ///
    /// # Returns
    ///
    /// A canvas containing the painted content
    pub(crate) fn paint_sliver(
        &self,
        tree: &crate::element::ElementTree,
        offset: flui_types::Offset,
    ) -> flui_painting::Canvas {
        let render = self.render_object.read();
        let children = crate::render::Children::from_slice(&self.children);
        let ctx = SliverPaintContext::new(tree, &children, offset);
        render.paint(&ctx)
    }

    /// Set children (enforces arity constraints at runtime)
    #[allow(dead_code)]
    pub(crate) fn set_children(&mut self, children: Vec<ElementId>) {
        // Enforce arity constraints
        let render = self.render_object_mut_direct();
        let arity = render.arity();

        match arity {
            crate::render::Arity::Exact(0) => {
                // Leaf sliver - no children allowed
                if !children.is_empty() {
                    panic!(
                        "Leaf sliver render object (arity=0) cannot have children, got {} children",
                        children.len()
                    );
                }
            }
            crate::render::Arity::Exact(1) => {
                // Single sliver - exactly one child required
                if children.len() != 1 {
                    panic!(
                        "Single sliver render object (arity=1) must have exactly 1 child, got {}",
                        children.len()
                    );
                }
            }
            crate::render::Arity::Exact(n) => {
                // Fixed arity > 1 - exactly n children required
                if children.len() != n {
                    panic!(
                        "Fixed arity sliver render object (arity={}) must have exactly {} children, got {}",
                        n, n, children.len()
                    );
                }
            }
            crate::render::Arity::Variable => {
                // Multi sliver - any number of children allowed
            }
        }

        self.children = children;
    }

    /// Add a child (for sliver render objects with arity Exact(1) or Variable)
    ///
    /// NOTE: This should only be called with SliverElement children!
    /// ComponentElements should not be added as children to SliverElements.
    /// The element tree building should ensure that SliverElements only have
    /// SliverElement children (by walking down through ComponentElements).
    #[allow(dead_code)]
    pub(crate) fn add_child(&mut self, child_id: ElementId) {
        // Add to children list
        self.children.push(child_id);
    }

    /// Remove a child by ID
    #[allow(dead_code)]
    pub(crate) fn remove_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    /// Take unmounted children (consumes them)
    ///
    /// Returns the unmounted children and sets the field to None.
    /// This is called by ElementTree during insertion to mount the children.
    pub(crate) fn take_unmounted_children(&mut self) -> Option<Vec<Element>> {
        self.unmounted_children.take()
    }

    // ========== DynElement-like Interface ==========
    //
    // These methods match the DynElement trait and are called by Element enum.
    // Following API Guidelines: is_* for predicates, no get_* prefix.

    /// Get parent element ID
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    /// Get iterator over child element IDs
    #[inline]
    pub fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_> {
        Box::new(self.children.iter().copied())
    }

    /// Get current lifecycle state
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    /// Mount element to tree
    ///
    /// Sets parent, slot, and transitions to Active lifecycle state.
    /// Marks element as dirty to trigger initial build.
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmount element from tree
    ///
    /// Transitions to Defunct lifecycle state and clears children.
    /// The children will be unmounted by ElementTree separately.
    pub fn unmount(&mut self) {
        self.base.unmount();
        // Children will be unmounted by ElementTree
        self.children.clear();
    }

    /// Deactivate element
    ///
    /// Called when element is temporarily deactivated (e.g., moved to cache).
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Activate element
    ///
    /// Called when element is reactivated. Marks dirty to trigger rebuild.
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Check if element needs rebuild
    ///
    /// Following API Guidelines: is_* prefix for boolean predicates.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as needing rebuild
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Forget child element
    ///
    /// Called by ElementTree when child is being removed.
    pub(crate) fn forget_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    /// Update slot for child
    ///
    /// Slot is managed by parent, children don't need to track it.
    pub(crate) fn update_slot_for_child(&mut self, _child_id: ElementId, _new_slot: usize) {
        // Slot is managed by parent
    }

    /// Handle an event
    ///
    /// SliverElements typically don't need to handle events directly,
    /// as they focus on layout and painting. However, this can be overridden
    /// for specific sliver render objects that need to react to events.
    ///
    /// **Possible Use Cases:**
    /// - **RenderSliverAppBar**: React to scroll events for collapse/expand
    /// - **RenderSliverPersistentHeader**: Handle visibility changes
    ///
    /// Default implementation: does not handle events (returns false)
    ///
    /// # Returns
    ///
    /// `true` if the event was handled, `false` otherwise
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false // SliverElements don't handle events by default
    }
}

// ========== ViewElement Implementation ==========

use crate::view::view::ViewElement;
use std::any::Any;

impl ViewElement for SliverElement {
    fn into_element(self: Box<Self>) -> crate::element::Element {
        crate::element::Element::Sliver(*self)
    }

    fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
