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
    /// Tracks the current layout depth (thread-safe with AtomicUsize)
    #[cfg(debug_assertions)]
    layout_depth: std::sync::atomic::AtomicUsize,
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
            layout_depth: std::sync::atomic::AtomicUsize::new(0),
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
            layout_depth: std::sync::atomic::AtomicUsize::new(0),
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
    /// Insert an element into the tree (raw insertion without mounting children)
    ///
    /// This is an internal method. Use `insert()` which handles unmounted children automatically.
    ///
    /// # Arguments
    ///
    /// - `element`: The element to insert
    ///
    /// # Returns
    ///
    /// The ElementId of the inserted element
    fn insert_raw(&mut self, element: Element) -> ElementId {
        // Create the node
        let node = ElementNode { element };

        // Insert into slab and get ID (convert usize to ElementId)
        // Add 1 because ElementId uses NonZeroUsize (0 is invalid)
        let id = self.nodes.insert(node);
        ElementId::new(id + 1)
    }

    /// Insert an element into the tree
    ///
    /// Automatically handles mounting of unmounted children if present.
    /// This is the main entry point for inserting elements created by Views.
    ///
    /// # Arguments
    ///
    /// - `element`: The element to insert
    ///
    /// # Returns
    ///
    /// The ElementId of the inserted element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let render_elem = RenderElement::new(render_object);
    /// let root_id = tree.insert(Element::Render(render_elem));
    /// ```
    pub fn insert(&mut self, mut element: Element) -> ElementId {
        // First, check if there are unmounted children and mount them
        let child_ids = match &mut element {
            Element::Render(render_elem) => {
                if let Some(unmounted) = render_elem.take_unmounted_children() {
                    // Recursively insert each unmounted child
                    let mut ids = Vec::with_capacity(unmounted.len());
                    for child in unmounted {
                        let child_id = self.insert(child); // Recursive call
                        ids.push(child_id);
                    }
                    Some(ids)
                } else {
                    None
                }
            }
            Element::Component(_comp_elem) => {
                // ComponentElement children are managed by build pipeline
                None
            }
            Element::Provider(_) => None,
        };

        // Insert the parent element (using raw insertion to avoid recursion)
        let parent_id = self.insert_raw(element);

        // Link children to parent
        if let Some(child_ids) = child_ids {
            // Access the element we just inserted
            if let Some(node) = self.nodes.get_mut(parent_id.get() - 1) {
                match &mut node.element {
                    Element::Render(render_elem) => {
                        render_elem.set_children(child_ids.clone());
                    }
                    _ => {}
                }
            }

            // Set parent for each child
            for child_id in child_ids {
                if let Some(child_node) = self.nodes.get_mut(child_id.get() - 1) {
                    child_node.element.mount(Some(parent_id), None);
                }
            }
        }

        parent_id
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
            let current_depth = self.layout_depth.load(std::sync::atomic::Ordering::Relaxed);
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
            self.layout_depth
                .store(current_depth + 1, std::sync::atomic::Ordering::Relaxed);
        }

        // Check for re-entrant layout (element trying to layout itself)
        let is_reentrant = Self::LAYOUT_STACK.with(|stack| stack.borrow().contains(&element_id));

        if is_reentrant {
            tracing::error!(
                element_id = ?element_id,
                "Re-entrant layout detected! Element is trying to layout itself - this is a render object bug."
            );

            #[cfg(debug_assertions)]
            self.layout_depth.store(
                self.layout_depth
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .saturating_sub(1),
                std::sync::atomic::Ordering::Relaxed,
            );

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
            render_element.layout_render(self, constraints)
        }; // Drop all borrows before guard cleanup (which happens automatically)

        // Decrement depth
        #[cfg(debug_assertions)]
        self.layout_depth.store(
            self.layout_depth
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_sub(1),
            std::sync::atomic::Ordering::Relaxed,
        );

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

        // Call paint on render object
        let layer = render_element.paint_render(self, offset);

        // Guards dropped here (_guard automatically)

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

    // ========== Helper Methods ==========

    /// Find the first RenderElement by walking down through ComponentElements
    ///
    /// This helper is used by both `layout_child` and `paint_child` to find
    /// the actual RenderElement to operate on.
    ///
    /// # Arguments
    /// * `start_id` - Starting element ID (may be Component or Render)
    ///
    /// # Returns
    /// * `Some(ElementId)` - ID of the first RenderElement found
    /// * `None` - If no RenderElement found or tree walk failed
    fn find_render_element(&self, start_id: ElementId) -> Option<ElementId> {
        let mut current_id = start_id;
        loop {
            if let Some(element) = self.get(current_id) {
                match element {
                    crate::element::Element::Render(_) => {
                        return Some(current_id);
                    }
                    crate::element::Element::Component(comp) => {
                        if let Some(comp_child_id) = comp.child() {
                            current_id = comp_child_id;
                        } else {
                            return None;
                        }
                    }
                    _ => {
                        return None;
                    }
                }
            } else {
                return None;
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
        let render_id = self.find_render_element(child_id);

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
        let render_id = self.find_render_element(child_id);

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
        F: FnMut(ElementId, &Box<dyn crate::render::Render>, parking_lot::RwLockReadGuard<RenderState>),
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

    /// Visit all elements with mutable access
    ///
    /// Allows dispatching events or performing mutations on all elements.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_elements_mut(|element_id, element| {
    ///     element.handle_window_event(&event);
    /// });
    /// ```
    pub fn visit_all_elements_mut<F>(&mut self, mut visitor: F)
    where
        F: FnMut(ElementId, &mut Element),
    {
        for (element_id, node) in &mut self.nodes {
            // Add 1 to convert slab index (0-based) to ElementId (1-based)
            visitor(ElementId::new(element_id + 1), &mut node.element);
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

impl ElementTree {
    // ========== Hit Testing ==========

    /// Perform hit testing on the element tree
    ///
    /// Tests whether the given position hits any elements, starting from root.
    /// Returns ElementHitTestResult with all hit elements in depth-first order
    /// (children before parents).
    ///
    /// Following Flutter's `RenderObject.hitTest()` pattern:
    /// 1. Check if position is within element bounds
    /// 2. Recursively test children (front to back)
    /// 3. Add self to result if hit
    ///
    /// # Arguments
    ///
    /// * `root_id` - The root element to start testing from
    /// * `position` - Global position to test (in window coordinates)
    ///
    /// # Returns
    ///
    /// ElementHitTestResult containing all hit elements
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let result = tree.hit_test(root_id, Offset::new(100.0, 50.0));
    /// for entry in result.iter() {
    ///     println!("Hit element: {:?} at local position: {:?}",
    ///              entry.element_id, entry.local_position);
    /// }
    /// ```
    pub fn hit_test(&self, root_id: ElementId, position: flui_types::Offset) -> crate::element::ElementHitTestResult {
        use crate::element::ElementHitTestResult;

        let mut result = ElementHitTestResult::new();
        self.hit_test_recursive(root_id, position, &mut result);
        result
    }

    /// Recursive hit testing helper
    ///
    /// Returns true if the element (or any of its children) was hit.
    fn hit_test_recursive(
        &self,
        element_id: ElementId,
        position: flui_types::Offset,
        result: &mut crate::element::ElementHitTestResult,
    ) -> bool {
        let element = match self.get(element_id) {
            Some(e) => e,
            None => return false,
        };

        match element {
            Element::Render(render_elem) => {
                self.hit_test_render(element_id, render_elem, position, result)
            }
            Element::Component(comp_elem) => {
                // ComponentElement delegates to child
                if let Some(child_id) = comp_elem.child() {
                    self.hit_test_recursive(child_id, position, result)
                } else {
                    false
                }
            }
            Element::Provider(prov_elem) => {
                // ProviderElement delegates to child
                if let Some(child_id) = prov_elem.child() {
                    self.hit_test_recursive(child_id, position, result)
                } else {
                    false
                }
            }
        }
    }

    /// Hit test for RenderElement
    ///
    /// Checks if position is within element bounds and recursively tests children.
    /// Adds hit elements to result in depth-first order (children before parents).
    fn hit_test_render(
        &self,
        element_id: ElementId,
        render_elem: &crate::element::RenderElement,
        position: flui_types::Offset,
        result: &mut crate::element::ElementHitTestResult,
    ) -> bool {
        // Get size from render state
        let render_state = render_elem.render_state().read();
        let size = match render_state.size() {
            Some(s) => s,
            None => return false, // No layout yet
        };
        drop(render_state);

        // Get offset (position relative to parent)
        let offset = render_elem.offset();

        // Transform position to local coordinates
        let local_position = position - offset;

        // Check if position is within bounds
        if local_position.dx < 0.0
            || local_position.dy < 0.0
            || local_position.dx > size.width
            || local_position.dy > size.height
        {
            return false; // Outside bounds
        }

        // Test children first (front to back)
        // Continue testing all children even after finding a hit,
        // since overlapping elements should all register hits
        for &child_id in render_elem.children() {
            self.hit_test_recursive(child_id, position, result);
        }

        // Add self to result (even if child was hit)
        // This maintains depth-first order: children added before parents
        result.add_element(element_id, local_position);

        true
    }
}

// SAFETY: ElementTree is thread-safe for multi-threaded UI:
// - Slab<ElementNode> is Send+Sync (contains only owned data)
// - AtomicUsize is Send+Sync (atomic operations)
// - Element enum variants are designed to be Send (though not all are Sync due to interior mutability)
// - Access is controlled by parking_lot::RwLock which provides thread-safe interior mutability
unsafe impl Send for ElementTree {}
unsafe impl Sync for ElementTree {}

// Tests removed - need to be rewritten with View API
