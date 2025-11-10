//! RenderElement - Performs layout and paint
//!
//! RenderElement owns a RenderObject and manages layout/paint lifecycle.
//! Per FINAL_ARCHITECTURE_V2.md, it does NOT store widget or view.

use parking_lot::RwLock;

use super::{ElementBase, ElementLifecycle};
use crate::element::{Element, ElementId};
use crate::foundation::Slot;
use crate::render::{Children, LayoutContext, PaintContext, Render, RenderState};
use crate::view::IntoElement;

/// Element for render objects (type-erased)
///
/// RenderElement owns a Render and manages its lifecycle.
/// The render object is type-erased to enable storage in the `enum Element`
/// without generic parameters.
///
/// # Architecture
///
/// ```text
/// RenderElement
///   ├─ render_object: RwLock<Box<dyn Render>> (type-erased Render)
///   ├─ render_state: RwLock<RenderState> (size, constraints, dirty flags)
///   ├─ parent_data: Option<Box<dyn ParentData>> (metadata from parent)
///   ├─ children: Vec<ElementId> (managed by Render arity)
///   └─ lifecycle state
/// ```
///
/// # Type Erasure
///
/// This version uses type erasure for the render object:
///
/// - **Render**: `Box<dyn Render>` (user-extensible, unbounded types)
/// - **Arity**: Runtime information via Render trait
///
/// # Performance
///
/// Render is Box<dyn>, but this is acceptable because:
/// - Layout/paint operations use interior mutability (RwLock)
/// - Hot path (layout) uses trait methods, not enum dispatch
/// - Element enum provides fast dispatch for element operations
///
/// # Lifecycle
///
/// 1. **create_render_object()** - View creates Render
/// 2. **mount()** - Element mounted to tree
/// 3. **update_render_object()** - View config changes
/// 4. **layout()** - Render layout pass
/// 5. **paint()** - Render paint pass
/// 6. **unmount()** - Render cleanup
pub struct RenderElement {
    /// Common element data (parent, slot, lifecycle, dirty)
    base: ElementBase,

    /// The render object created by the view (type-erased)
    ///
    /// Wrapped in RwLock to allow interior mutability during layout.
    /// RwLock allows multiple concurrent reads OR one exclusive write:
    /// - Multiple immutable borrows during read operations (safe)
    /// - Single mutable borrow during write operations
    /// - More flexible than RefCell which panics on overlapping borrows
    ///
    /// **Note**: When you have `&mut self`, prefer using `render_object_mut_direct()`
    /// to avoid lock contention during the build/mount phase.
    render_object: RwLock<Box<dyn Render>>,

    /// Render state (size, constraints, dirty flags)
    ///
    /// Uses RwLock for interior mutability during layout/paint.
    /// Atomic flags inside RenderState provide lock-free checks.
    render_state: RwLock<RenderState>,

    /// Position of this element relative to parent
    ///
    /// Set during layout phase by parent. Used for:
    /// - Hit testing (to check if pointer is within bounds)
    /// - Painting (to position child layers)
    /// - Coordinate transformation during event dispatch
    ///
    /// **Important**: This is in parent's coordinate space.
    /// Root element has offset (0, 0).
    offset: flui_types::Offset,

    /// Parent data attached by parent Render
    ///
    /// This metadata is set by the parent's layout algorithm (e.g., FlexParentData for Flex,
    /// StackParentData for Stack). Parent accesses this during layout to determine how to
    /// position and size this child.
    parent_data: Option<Box<dyn crate::render::ParentData>>,

    /// Child elements (count enforced by Render arity at runtime)
    children: Vec<ElementId>,

    /// Unmounted child elements waiting to be inserted into tree
    ///
    /// When RenderBuilder creates child elements, they are stored here.
    /// When this RenderElement is inserted into ElementTree, these children
    /// are automatically inserted and linked.
    unmounted_children: Option<Vec<Element>>,
}

// Manual Debug implementation because RwLock<Box<dyn Trait>> doesn't auto-derive
impl std::fmt::Debug for RenderElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderElement")
            .field("base", &self.base)
            .field("render_object", &"<RwLock<Box<dyn Render>>>")
            .field("render_state", &self.render_state)
            .field("offset", &self.offset)
            .field("parent_data", &self.parent_data.is_some())
            .field("children", &self.children)
            .field("unmounted_children", &self.unmounted_children.as_ref().map(|c| c.len()))
            .finish()
    }
}

impl RenderElement {
    /// Create a new RenderElement from a render object
    ///
    /// # Parameters
    ///
    /// - `render_object` - The RenderObject for this element
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let render = Box::new(RenderPadding::new(EdgeInsets::all(10.0)));
    /// let element = RenderElement::new(render);
    /// ```
    pub fn new(render_object: Box<dyn Render>) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(render_object),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            parent_data: None,
            children: Vec::new(),
            unmounted_children: None,
        }
    }

    /// Create a new RenderElement with unmounted children
    ///
    /// This is used by RenderBuilder to create elements with children
    /// that will be mounted when this element is inserted into the tree.
    pub fn new_with_children(render_object: Box<dyn Render>, children: Vec<Element>) -> Self {
        Self {
            base: ElementBase::new(),
            render_object: RwLock::new(render_object),
            render_state: RwLock::new(RenderState::new()),
            offset: flui_types::Offset::ZERO,
            parent_data: None,
            children: Vec::new(),
            unmounted_children: Some(children),
        }
    }

    // Note: widget() method removed - RenderElement no longer stores widget
    // Per FINAL_ARCHITECTURE_V2.md, elements don't store widgets/views

    /// Get reference to the render object
    ///
    /// Returns a read guard that allows multiple concurrent immutable borrows.
    /// This uses parking_lot::RwLock which is more flexible than RefCell.
    #[inline]
    pub fn render_object(&self) -> parking_lot::RwLockReadGuard<'_, Box<dyn Render>> {
        self.render_object.read()
    }

    /// Get mutable reference to the render object (through RwLock)
    ///
    /// Returns a write guard that allows exclusive mutable access.
    /// This blocks if there are any active read or write guards.
    #[inline]
    pub fn render_object_mut(&self) -> parking_lot::RwLockWriteGuard<'_, Box<dyn Render>> {
        self.render_object.write()
    }

    /// Get direct mutable reference to the render object
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
    fn render_object_mut_direct(&mut self) -> &mut Box<dyn Render> {
        self.render_object.get_mut()
    }

    /// Get children element IDs
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get reference to render state
    ///
    /// RenderState contains size, constraints, and dirty flags.
    /// Uses RwLock for interior mutability during layout/paint.
    #[inline]
    #[must_use]
    pub fn render_state(&self) -> &RwLock<RenderState> {
        &self.render_state
    }

    /// Get the offset (position relative to parent)
    #[inline]
    #[must_use]
    pub fn offset(&self) -> flui_types::Offset {
        self.offset
    }

    /// Set the offset (called by parent during layout)
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
    /// Called by parent Render during setup or when parent changes.
    pub fn set_parent_data(&mut self, parent_data: Box<dyn crate::render::ParentData>) {
        self.parent_data = Some(parent_data);
    }

    /// Clear parent data
    pub fn clear_parent_data(&mut self) {
        self.parent_data = None;
    }

    /// Perform layout on this render object
    ///
    /// Creates a LayoutContext and calls the render object's layout() method.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `constraints`: Layout constraints from parent
    ///
    /// # Returns
    ///
    /// The computed size
    pub(crate) fn layout_render(
        &self,
        tree: &crate::element::ElementTree,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> flui_types::Size {
        let mut render = self.render_object.write();
        let children = Children::from_slice(&self.children);
        let ctx = LayoutContext::new(tree, &children, constraints);
        render.layout(&ctx)
    }

    /// Perform paint on this render object
    ///
    /// Creates a PaintContext and calls the render object's paint() method.
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `offset`: Paint offset in parent's coordinate space
    ///
    /// # Returns
    ///
    /// A boxed layer containing the painted content
    pub(crate) fn paint_render(
        &self,
        tree: &crate::element::ElementTree,
        offset: flui_types::Offset,
    ) -> flui_engine::BoxedLayer {
        let render = self.render_object.read();
        let children = Children::from_slice(&self.children);
        let ctx = PaintContext::new(tree, &children, offset);
        render.paint(&ctx)
    }

    // Note: update() method removed - will be replaced with View-based update
    // Note: Implement View::rebuild() integration

    /// Set children (enforces arity constraints at runtime)
    #[allow(dead_code)]
    pub(crate) fn set_children(&mut self, children: Vec<ElementId>) {
        // Enforce arity constraints
        let render = self.render_object_mut_direct();
        let arity = render.arity();

        match arity {
            crate::render::Arity::Exact(0) => {
                // Leaf render - no children allowed
                if !children.is_empty() {
                    panic!(
                        "Leaf render object (arity=0) cannot have children, got {} children",
                        children.len()
                    );
                }
            }
            crate::render::Arity::Exact(1) => {
                // Single render - exactly one child required
                if children.len() != 1 {
                    panic!(
                        "Single render object (arity=1) must have exactly 1 child, got {}",
                        children.len()
                    );
                }
            }
            crate::render::Arity::Exact(n) => {
                // Fixed arity > 1 - exactly n children required
                if children.len() != n {
                    panic!(
                        "Fixed arity render object (arity={}) must have exactly {} children, got {}",
                        n, n, children.len()
                    );
                }
            }
            crate::render::Arity::Variable => {
                // Multi render - any number of children allowed
            }
        }

        self.children = children;
    }

    /// Add a child (for render objects with arity Exact(1) or Variable)
    ///
    /// NOTE: This should only be called with RenderElement children!
    /// ComponentElements should not be added as children to RenderElements.
    /// The element tree building should ensure that RenderElements only have
    /// RenderElement children (by walking down through ComponentElements).
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
    /// RenderElements typically don't need to handle events directly,
    /// as they focus on layout and painting. However, this can be overridden
    /// for specific render objects that need to react to events.
    ///
    /// **Possible Use Cases:**
    /// - **RenderImage**: Reload textures on `Event::Window(WindowEvent::ScaleChanged)`
    /// - **RenderVideo**: Pause playback on `Event::Window(WindowEvent::VisibilityChanged)`
    /// - **RenderAnimated**: Stop animations on focus loss
    ///
    /// Default implementation: does not handle events (returns false)
    ///
    /// # Returns
    ///
    /// `true` if the event was handled, `false` otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// match event {
    ///     Event::Window(WindowEvent::ScaleChanged { scale }) => {
    ///         self.reload_textures_at_scale(*scale);
    ///         true // Handled
    ///     }
    ///     _ => false // Ignore other events
    /// }
    /// ```
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false // RenderElements don't handle events by default
    }
}

// ========== ViewElement Implementation ==========

use crate::view::view::ViewElement;
use std::any::Any;

impl ViewElement for RenderElement {
    fn into_element(self: Box<Self>) -> crate::element::Element {
        crate::element::Element::Render(*self)
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

impl IntoElement for RenderElement {
    fn into_element(self) -> Element {
        Element::Render(self)
    }
}
