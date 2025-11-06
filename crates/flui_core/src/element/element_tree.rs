//! ElementTree - Slab-based tree for managing Element instances
//!
//! Provides efficient O(1) access to elements via slab allocation.

use flui_types::constraints::BoxConstraints;
use slab::Slab;

use crate::element::{Element, ElementId};
use crate::render::RenderState;

/// Maximum layout recursion depth before panic.
///
/// This limit prevents infinite recursion in layout calculations,
/// which typically indicates a circular dependency in the render tree.
///
/// # When to Adjust
///
/// The default of 1000 should be sufficient for most UIs. If you have
/// an extremely deep component hierarchy (unusual), you may need to increase this.
///
/// To modify, change this constant and recompile flui_core.
///
/// # Performance Note
///
/// This check only runs in debug builds (`cfg(debug_assertions)`).
/// Release builds have no depth checking for maximum performance.
#[cfg(debug_assertions)]
pub const MAX_LAYOUT_DEPTH: usize = 1000;

/// Element tree managing Element instances with efficient slab allocation
///
/// # New Architecture
///
/// ElementTree now stores heterogeneous Elements (ComponentElement, InheritedElement,
/// RenderElement) instead of Renders directly. This provides:
/// - Unified tree structure for all element types
/// - View lifecycle management (build, rebuild, mount, unmount)
/// - Dependency tracking for InheritedElements
/// - RenderState is now inside RenderElement
///
/// # Memory Layout
///
/// ```text
/// ElementTree {
///     nodes: Slab<ElementNode>  ← Contiguous memory for cache efficiency
/// }
///
/// ElementNode {
///     element: Element  ← Enum-based heterogeneous storage (3-4x faster!)
///         ├─ Element::Component(ComponentElement)
///         ├─ Element::Provider(InheritedElement)
///         └─ Element::Render(RenderElement)
/// }
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use flui_core::{ElementTree, RenderElement};
///
/// let mut tree = ElementTree::new();
///
/// // Insert root element (now stores Element, not Render)
/// let root_element = RenderElement::new(render_object);
/// let root_id = tree.insert(Box::new(root_element));
///
/// // Access element
/// let element = tree.get(root_id).unwrap();
/// ```
#[derive(Debug)]
pub struct ElementTree {
    /// Slab-based arena for element nodes
    ///
    /// Each ElementNode contains:
    /// - Render (boxed trait object)
    /// - RenderState (size, constraints, flags)
    /// - Parent/children relationships
    pub(super) nodes: Slab<ElementNode>,

    /// Depth guard for layout to prevent infinite recursion
    /// Tracks the current layout depth
    #[cfg(debug_assertions)]
    layout_depth: std::cell::Cell<usize>,
}

/// Internal node in the element tree
///
/// Contains an Element enum variant (Component, Provider, Render).
/// The Element enum contains all necessary data including:
/// - View configuration (for ComponentElement)
/// - Provider data (for InheritedElement)
/// - Render + RenderState (for RenderElement)
/// - Lifecycle state
/// - Children management
#[derive(Debug)]
pub(super) struct ElementNode {
    /// The Element for this node
    ///
    /// Stored as `Element` enum for heterogeneous storage with compile-time dispatch.
    /// This is 3-4x faster than Box<dyn> thanks to match-based dispatch.
    pub(super) element: Element,
}

// ========== RAII Guards for Thread-Local Stacks ==========

/// RAII guard that automatically pops element from layout stack on drop.
///
/// This ensures the stack is cleaned up even if layout panics.
struct LayoutGuard {
    element_id: ElementId,
}

impl LayoutGuard {
    fn new(element_id: ElementId) -> Self {
        ElementTree::LAYOUT_STACK.with(|stack| {
            stack.borrow_mut().push(element_id);
        });
        Self { element_id }
    }
}

impl Drop for LayoutGuard {
    fn drop(&mut self) {
        ElementTree::LAYOUT_STACK.with(|stack| {
            let popped = stack.borrow_mut().pop();
            debug_assert_eq!(
                popped,
                Some(self.element_id),
                "Layout stack corruption: expected {:?}, got {:?}",
                self.element_id,
                popped
            );
        });
    }
}

/// RAII guard that automatically pops element from paint stack on drop.
///
/// This ensures the stack is cleaned up even if paint panics.
struct PaintGuard {
    element_id: ElementId,
}

impl PaintGuard {
    fn new(element_id: ElementId) -> Self {
        ElementTree::PAINT_STACK.with(|stack| {
            stack.borrow_mut().push(element_id);
        });
        Self { element_id }
    }
}

impl Drop for PaintGuard {
    fn drop(&mut self) {
        ElementTree::PAINT_STACK.with(|stack| {
            let popped = stack.borrow_mut().pop();
            debug_assert_eq!(
                popped,
                Some(self.element_id),
                "Paint stack corruption: expected {:?}, got {:?}",
                self.element_id,
                popped
            );
        });
    }
}

impl ElementTree {
    /// Create a new empty element tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tree = ElementTree::new();
    /// ```
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            #[cfg(debug_assertions)]
            layout_depth: std::cell::Cell::new(0),
        }
    }

    /// Create an element tree with pre-allocated capacity
    ///
    /// # Arguments
    ///
    /// - `capacity`: Initial capacity for the slab
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Pre-allocate for 1000 elements
    /// let tree = ElementTree::with_capacity(1000);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            #[cfg(debug_assertions)]
            layout_depth: std::cell::Cell::new(0),
        }
    }

    // ========== Element Insertion/Removal ==========

    /// Insert a new element into the tree
    ///
    /// # Arguments
    ///
    /// - `element`: The Element enum (Component, Provider, or Render)
    ///
    /// # Returns
    ///
    /// The ElementId for the newly inserted element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::{Element, RenderElement};
    ///
    /// let render_elem = RenderElement::new(render_object);
    /// let root_id = tree.insert(Element::Render(render_elem));
    /// ```
    pub fn insert(&mut self, element: Element) -> ElementId {
        // Create the node
        let node = ElementNode { element };

        // Insert into slab and get ID (convert usize to ElementId)
        // Add 1 because ElementId uses NonZeroUsize (0 is invalid)
        let id = self.nodes.insert(node);
        ElementId::new(id + 1)
    }

    /// Remove an element and all its descendants from the tree
    ///
    /// # Arguments
    ///
    /// - `element_id`: The element to remove
    ///
    /// # Returns
    ///
    /// `true` if the element was removed, `false` if it didn't exist
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.remove(element_id);
    /// ```
    pub fn remove(&mut self, element_id: ElementId) -> bool {
        // Get element to call unmount
        // Subtract 1 to convert ElementId (1-based) to slab index (0-based)
        if let Some(node) = self.nodes.get_mut(element_id.get() - 1) {
            // Call unmount lifecycle
            node.element.unmount();
        }

        // Get children from element (before removing)
        let children: Vec<ElementId> = if let Some(node) = self.nodes.get(element_id.get() - 1) {
            node.element.children().collect()
        } else {
            Vec::new()
        };

        // Remove all children recursively
        for child_id in children {
            self.remove(child_id);
        }

        // Remove from parent's children list
        if let Some(parent_id) = self.get(element_id).and_then(|e| e.parent()) {
            if let Some(parent_node) = self.nodes.get_mut(parent_id.get() - 1) {
                parent_node.element.forget_child(element_id);
            }
        }

        // Remove the node itself
        // Subtract 1 to convert ElementId (1-based) to slab index (0-based)
        self.nodes.try_remove(element_id.get() - 1).is_some()
    }

    // ========== Tree Traversal ==========

    /// Check if an element exists in the tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if tree.contains(element_id) {
    ///     // Element exists
    /// }
    /// ```
    #[inline]
    pub fn contains(&self, element_id: ElementId) -> bool {
        self.nodes.contains(element_id.get())
    }

    /// Get a reference to an element
    ///
    /// # Returns
    ///
    /// `Some(&Element)` if the element exists, `None` otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(element) = tree.get(element_id) {
    ///     println!("Element has {} children", element.children().count());
    /// }
    /// ```
    #[inline]
    pub fn get(&self, element_id: ElementId) -> Option<&Element> {
        // Subtract 1 to convert ElementId (1-based) to slab index (0-based)
        self.nodes
            .get(element_id.get() - 1)
            .map(|node| &node.element)
    }

    /// Get a mutable reference to an element
    ///
    /// # Returns
    ///
    /// `Some(&mut Element)` if the element exists, `None` otherwise
    #[inline]
    pub fn get_mut(&mut self, element_id: ElementId) -> Option<&mut Element> {
        // Subtract 1 to convert ElementId (1-based) to slab index (0-based)
        self.nodes
            .get_mut(element_id.get() - 1)
            .map(|node| &mut node.element)
    }

    /// Get the parent of an element
    ///
    /// # Returns
    ///
    /// `Some(parent_id)` if the element has a parent, `None` if it's root or doesn't exist
    #[inline]
    pub fn parent(&self, element_id: ElementId) -> Option<ElementId> {
        self.get(element_id).and_then(|element| element.parent())
    }

    /// Get the children of an element as a Vec
    ///
    /// # Returns
    ///
    /// A Vec of child ElementIds, or empty Vec if element has no children or doesn't exist
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for child_id in tree.children(parent_id) {
    ///     println!("Child: {}", child_id);
    /// }
    /// ```
    #[inline]
    pub fn children(&self, element_id: ElementId) -> Vec<ElementId> {
        self.get(element_id)
            .map(|element| element.children().collect())
            .unwrap_or_default()
    }

    /// Get the number of children for an element
    #[inline]
    pub fn child_count(&self, element_id: ElementId) -> usize {
        self.get(element_id)
            .map(|element| element.children().count())
            .unwrap_or(0)
    }

    // ========== Render Access ==========

    // Note: render_object() and render_object_mut() methods removed
    // because they cannot work with RefCell guards (lifetime issues).
    // Instead, use: tree.get(element_id)?.render_object()?
    // or: tree.get(element_id)?.render_object_mut()?

    // ========== RenderState Access ==========

    // Track which elements are currently being laid out (to prevent re-entrant layout)
    //
    // This is stored in thread-local storage since layout is single-threaded.
    thread_local! {
        static LAYOUT_STACK: std::cell::RefCell<Vec<ElementId>> = const { std::cell::RefCell::new(Vec::new()) };
        static PAINT_STACK: std::cell::RefCell<Vec<ElementId>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Get a read guard to the RenderState for an element
    ///
    /// # Returns
    ///
    /// `Some(RwLockReadGuard<RenderState>)` if the element is a RenderElement
    ///
    /// # Note
    ///
    /// Only RenderElements have RenderState. ComponentElements and StatefulElements
    /// will return None.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(state) = tree.render_state(element_id) {
    ///     if state.needs_layout() {  // Lock-free atomic check!
    ///         // Layout needed
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn render_state(
        &self,
        element_id: ElementId,
    ) -> Option<parking_lot::RwLockReadGuard<'_, RenderState>> {
        self.get(element_id)
            .and_then(|element| element.as_render())
            .map(|render| render.render_state().read())
    }

    /// Get a write guard to the RenderState for an element
    ///
    /// # Returns
    ///
    /// `Some(RwLockWriteGuard<RenderState>)` if the element is a RenderElement
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(mut state) = tree.render_state_mut(element_id) {
    ///     state.set_size(Size::new(100.0, 50.0));
    ///     state.clear_needs_layout();
    /// }
    /// ```
    #[inline]
    pub fn render_state_mut(
        &self,
        element_id: ElementId,
    ) -> Option<parking_lot::RwLockWriteGuard<'_, RenderState>> {
        self.get(element_id)
            .and_then(|element| element.as_render())
            .map(|render| render.render_state().write())
    }

    // ========== Layout & Paint Helpers ==========

    /// Perform layout on a Render
    ///
    /// Uses RefCell-based interior mutability for safe access to render objects.
    /// This is sound because layout is single-threaded and RefCell provides
    /// runtime borrow checking.
    ///
    /// # Arguments
    ///
    /// - `element_id`: The element to layout
    /// - `constraints`: Layout constraints
    ///
    /// # Returns
    ///
    /// The size computed by the Render, or None if element is not a RenderElement
    ///
    /// # Panics
    ///
    /// Panics if the render object is already borrowed mutably (indicates a layout cycle).
    pub fn layout_render_object(
        &self,
        element_id: ElementId,
        constraints: BoxConstraints,
    ) -> Option<flui_types::Size> {
        // SAFETY: Re-fetch element references after each scope to avoid use-after-free
        // if the tree is modified during layout (Issue #3)

        // Scope 1: Check cache (read-only, safe)
        {
            let element = self.get(element_id)?;
            let render_element = element.as_render()?;

            // **Optimization**: Check RenderState cache before computing layout
            // This avoids expensive dyn_layout() calls when constraints haven't changed
            let render_state = render_element.render_state();

            // Try to use cached size if constraints match and no relayout needed
            let state = render_state.read();
            if state.has_size() && !state.needs_layout() {
                if let Some(cached_constraints) = state.constraints() {
                    if cached_constraints == constraints {
                        // Cache hit! Return cached size without layout computation
                        return state.size();
                    }
                }
            }
        } // All borrows dropped here - safe to proceed

        // Cache miss or needs relayout - compute layout

        // Depth guard to prevent infinite recursion
        #[cfg(debug_assertions)]
        {
            let current_depth = self.layout_depth.get();
            if current_depth > MAX_LAYOUT_DEPTH {
                tracing::error!(
                    element_id = ?element_id,
                    depth = current_depth,
                    max_depth = MAX_LAYOUT_DEPTH,
                    "Layout depth exceeded! Infinite recursion detected. \
                     This usually means a render object is calling layout on itself or a circular dependency exists."
                );
                panic!("Layout depth limit exceeded - infinite recursion");
            }
            self.layout_depth.set(current_depth + 1);
        }

        // Check for re-entrant layout (element trying to layout itself)
        let is_reentrant = Self::LAYOUT_STACK.with(|stack| stack.borrow().contains(&element_id));

        if is_reentrant {
            tracing::error!(
                element_id = ?element_id,
                "Re-entrant layout detected! Element is trying to layout itself - this is a render object bug."
            );

            #[cfg(debug_assertions)]
            self.layout_depth
                .set(self.layout_depth.get().saturating_sub(1));

            // Re-fetch to get cached size safely (Issue #3)
            let element = self.get(element_id)?;
            let render_element = element.as_render()?;
            let render_state = render_element.render_state();
            return render_state.read().size();
        }

        // Push element onto layout stack with RAII guard
        // The guard will automatically pop the element even if layout panics
        let _guard = LayoutGuard::new(element_id);

        // Scope 2: Perform layout (re-fetch element to avoid use-after-free - Issue #3)
        let size = {
            let element = self.get(element_id)?;
            let render_element = element.as_render()?;
            render_element.render_object_mut().layout(self, constraints)
        }; // Drop all borrows before guard cleanup (which happens automatically)

        // Decrement depth
        #[cfg(debug_assertions)]
        self.layout_depth
            .set(self.layout_depth.get().saturating_sub(1));

        // Scope 3: Update state (re-fetch again to be safe - Issue #3)
        {
            let element = self.get(element_id)?;
            let render_element = element.as_render()?;
            let render_state = render_element.render_state();
            let state = render_state.write();
            state.set_size(size);
            state.set_constraints(constraints);
            state.clear_needs_layout();
        }

        Some(size)
    }

    /// Perform paint on a Render
    ///
    /// This is a helper method that safely handles access to the render object
    /// and tree for painting.
    ///
    /// # Arguments
    ///
    /// - `element_id`: The element to paint
    /// - `offset`: Painting offset
    ///
    /// # Returns
    ///
    /// The layer tree, or None if element is not a RenderElement
    pub fn paint_render_object(
        &self,
        element_id: ElementId,
        offset: crate::Offset,
    ) -> Option<crate::BoxedLayer> {
        // Check for re-entrant paint (element trying to paint itself)
        let is_reentrant = Self::PAINT_STACK.with(|stack| stack.borrow().contains(&element_id));

        if is_reentrant {
            tracing::error!(
                element_id = ?element_id,
                "Re-entrant paint detected! Element is trying to paint itself - this is a render object bug."
            );
            // Return None to avoid infinite recursion
            return None;
        }

        // Push element onto paint stack with RAII guard
        // The guard will automatically pop the element even if paint panics
        let _guard = PaintGuard::new(element_id);

        // Get render element
        let element = self.get(element_id)?;
        let render_element = element.as_render()?;

        // Borrow the render object through RwLock - the guard must live until after the call
        let render_object_guard = render_element.render_object();

        // Call paint on RenderNode
        let layer = render_object_guard.paint(self, offset);

        // Guards dropped here (render_object_guard, then _guard automatically)

        // Note: Overflow indicators are now painted by each RenderObject itself
        // (e.g., RenderFlex paints its own overflow indicators in debug mode).
        // This is more architecturally correct than wrapping at the ElementTree level.

        Some(layer)
    }

    // ========== Debug-Only Overflow Reporting ==========

    /// Set overflow for the currently-being-laid-out element (debug only)
    ///
    /// This allows RenderObjects to report overflow during layout without
    /// needing to know their own element_id. Uses the layout stack to determine
    /// which element is currently being laid out.
    ///
    /// # Arguments
    /// * `axis` - The axis on which overflow occurred
    /// * `pixels` - Number of pixels that overflow (>= 0.0)
    ///
    /// # Example
    /// ```rust,ignore
    /// // In RenderFlex::layout()
    /// let overflow = (content_size - container_size).max(0.0);
    /// tree.set_current_overflow(Axis::Horizontal, overflow);
    /// ```
    #[cfg(debug_assertions)]
    pub fn set_current_overflow(&self, axis: flui_types::Axis, pixels: f32) {
        // Get current element from layout stack
        let current_element = Self::LAYOUT_STACK.with(|stack| stack.borrow().last().copied());

        if let Some(element_id) = current_element {
            if let Some(state) = self.render_state(element_id) {
                state.set_overflow(axis, pixels);
            }
        }
    }

    // ========== Convenience Aliases for Render Traits ==========

    /// Alias for `layout_render_object` - used by SingleRender/MultiRender traits
    #[inline]
    pub fn layout_child(
        &self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> flui_types::Size {
        // Bounds checking: verify child_id exists in tree
        if !self.contains(child_id) {
            #[cfg(debug_assertions)]
            {
                tracing::error!(
                    child_id = ?child_id,
                    "layout_child called with invalid child_id - element not in tree"
                );
                panic!("Invalid child_id in layout_child: {:?}", child_id);
            }

            #[cfg(not(debug_assertions))]
            {
                tracing::error!(
                    child_id = ?child_id,
                    "layout_child called with invalid child_id, returning Size::ZERO"
                );
                return flui_types::Size::ZERO;
            }
        }

        // Walk down through ComponentElements to find the first RenderElement
        let mut current_id = child_id;
        let render_id = loop {
            if let Some(element) = self.get(current_id) {
                match element {
                    crate::element::Element::Render(_) => {
                        break Some(current_id);
                    }
                    crate::element::Element::Component(comp) => {
                        if let Some(comp_child_id) = comp.child() {
                            current_id = comp_child_id;
                        } else {
                            break None;
                        }
                    }
                    _ => {
                        break None;
                    }
                }
            } else {
                break None;
            }
        };

        if let Some(render_id) = render_id {
            match self.layout_render_object(render_id, constraints) {
                Some(size) => size,
                None => {
                    tracing::error!(
                        child_id = ?child_id,
                        render_id = ?render_id,
                        "Failed to layout render object. Returning Size::ZERO."
                    );
                    flui_types::Size::ZERO
                }
            }
        } else {
            tracing::warn!(
                child_id = ?child_id,
                "Could not find RenderElement for child. Element may be Component without child or Provider. Returning Size::ZERO."
            );
            flui_types::Size::ZERO
        }
    }

    /// Alias for `paint_render_object` - used by SingleRender/MultiRender traits
    #[inline]
    pub fn paint_child(&self, child_id: ElementId, offset: crate::Offset) -> crate::BoxedLayer {
        #[cfg(debug_assertions)]
        tracing::debug!("paint_child: called with child_id={:?}", child_id);

        // Walk down through ComponentElements to find the first RenderElement
        // (same logic as layout_child)
        let mut current_id = child_id;
        let render_id = loop {
            if let Some(element) = self.get(current_id) {
                match element {
                    crate::element::Element::Render(_) => {
                        #[cfg(debug_assertions)]
                        tracing::debug!("paint_child: found RenderElement at {:?}", current_id);
                        break Some(current_id);
                    }
                    crate::element::Element::Component(comp) => {
                        if let Some(comp_child_id) = comp.child() {
                            current_id = comp_child_id;
                        } else {
                            #[cfg(debug_assertions)]
                            tracing::warn!("paint_child: ComponentElement has no child");
                            break None;
                        }
                    }
                    _ => {
                        #[cfg(debug_assertions)]
                        tracing::warn!("paint_child: unexpected element type");
                        break None;
                    }
                }
            } else {
                #[cfg(debug_assertions)]
                tracing::error!("paint_child: element {:?} not found in tree!", current_id);
                break None;
            }
        };

        if let Some(render_id) = render_id {
            self.paint_render_object(render_id, offset)
                .unwrap_or_else(|| Box::new(flui_engine::ContainerLayer::new()))
        } else {
            #[cfg(debug_assertions)]
            tracing::warn!("paint_child: returning empty ContainerLayer (no render_id)");
            Box::new(flui_engine::ContainerLayer::new())
        }
    }

    // ========== Tree Information ==========

    /// Get the total number of elements in the tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// println!("Tree has {} elements", tree.len());
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the tree is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the current capacity of the slab
    #[inline]
    pub fn capacity(&self) -> usize {
        self.nodes.capacity()
    }

    /// Clear the entire tree
    ///
    /// Removes all elements and frees memory.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    // ========== Iteration ==========

    /// Visit all RenderElements in the tree
    ///
    /// This only visits elements that have Renders (RenderElement).
    /// ComponentElements and StatefulElements are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_render_objects(|element_id, render_obj, state| {
    ///     println!("Element {}: arity = {:?}", element_id, render_obj.arity());
    /// });
    /// ```
    /// Visit all RenderElements in the tree
    ///
    /// This only visits elements that have Renders (RenderElement).
    /// ComponentElements and StatefulElements are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_render_objects(|element_id, render_obj, state| {
    ///     println!("Element {}: arity = {:?}", element_id, render_obj.arity());
    /// });
    /// ```
    pub fn visit_all_render_objects<F>(&self, mut visitor: F)
    where
        F: FnMut(ElementId, &crate::RenderNode, parking_lot::RwLockReadGuard<RenderState>),
    {
        for (element_id, node) in &self.nodes {
            // Only visit elements with Renders
            let render_elem = match node.element.as_render() {
                Some(re) => re,
                None => continue,
            };

            // Borrow render object and state through RwLock guards
            let render_obj_guard = render_elem.render_object();
            let state = render_elem.render_state().read();

            // Call visitor with references
            // The guards live for the duration of the visitor call
            // Add 1 to convert slab index (0-based) to ElementId (1-based)
            visitor(ElementId::new(element_id + 1), &render_obj_guard, state);
            // Guards are dropped here
        }
    }

    /// Visit all elements in the tree
    ///
    /// This visits all elements (Component, Provider, and Render).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_elements(|element_id, element| {
    ///     println!("Element {} has {} children", element_id, element.children().count());
    /// });
    /// ```
    pub fn visit_all_elements<F>(&self, mut visitor: F)
    where
        F: FnMut(ElementId, &Element),
    {
        for (element_id, node) in &self.nodes {
            // Add 1 to convert slab index (0-based) to ElementId (1-based)
            visitor(ElementId::new(element_id + 1), &node.element);
        }
    }

    /// Visit all elements in a subtree (depth-first, pre-order)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_subtree(root_id, |element_id, element| {
    ///     println!("Visiting {}", element_id);
    /// });
    /// ```
    pub fn visit_subtree<F>(&self, element_id: ElementId, visitor: &mut F)
    where
        F: FnMut(ElementId, &Element),
    {
        if let Some(element) = self.get(element_id) {
            visitor(element_id, element);

            // Visit children
            let children: Vec<ElementId> = element.children().collect();
            for child_id in children {
                self.visit_subtree(child_id, visitor);
            }
        }
    }

    // ========== Dependency Tracking ==========

    /// Register a dependent element on an InheritedElement
    ///
    /// This is called by BuildContext when a descendant element accesses
    /// inherited data via `context.depend_on::<T>()`.
    ///
    /// # Arguments
    ///
    /// - `inherited_id`: The ElementId of the InheritedElement being depended upon
    /// - `dependent_id`: The ElementId of the element that depends on the inherited data
    ///
    /// # Returns
    ///
    /// `true` if the dependency was registered successfully, `false` if the inherited_id
    /// doesn't exist or isn't an InheritedElement.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Called internally by BuildContext::depend_on()
    /// tree.add_dependent(theme_element_id, widget_element_id);
    /// ```
    pub fn add_dependent(&mut self, inherited_id: ElementId, dependent_id: ElementId) -> bool {
        if let Some(Element::Provider(inherited)) = self.get_mut(inherited_id) {
            inherited.add_dependent(dependent_id);
            return true;
        }
        false
    }

    /// Remove a dependent element from an InheritedElement
    ///
    /// This is called when a dependent element is unmounted or no longer needs
    /// to track the inherited data.
    ///
    /// # Arguments
    ///
    /// - `inherited_id`: The ElementId of the InheritedElement
    /// - `dependent_id`: The ElementId of the element to remove from dependents
    ///
    /// # Returns
    ///
    /// `true` if the dependency was removed successfully, `false` if the inherited_id
    /// doesn't exist or isn't an InheritedElement.
    pub fn remove_dependent(&mut self, inherited_id: ElementId, dependent_id: ElementId) -> bool {
        if let Some(Element::Provider(inherited)) = self.get_mut(inherited_id) {
            inherited.remove_dependent(dependent_id);
            return true;
        }
        false
    }

    /// Get all dependents of an InheritedElement
    ///
    /// Returns the set of ElementIds that have registered a dependency on
    /// the specified InheritedElement.
    ///
    /// # Returns
    ///
    /// `Some(&HashSet<ElementId>)` if the element exists and is an InheritedElement,
    /// `None` otherwise.
    pub fn get_dependents(
        &self,
        inherited_id: ElementId,
    ) -> Option<&std::collections::HashSet<ElementId>> {
        if let Some(Element::Provider(inherited)) = self.get(inherited_id) {
            Some(inherited.dependents())
        } else {
            None
        }
    }
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

// Tests removed - need to be rewritten with View API
