//! ElementTree - Slab-based tree for managing Element instances
//!
//! Provides efficient O(1) access to elements via slab allocation.

use flui_types::constraints::BoxConstraints;
use slab::Slab;

use crate::element::{Element, ElementId};
use crate::render::RenderState;

/// Element tree managing Element instances with efficient slab allocation
///
/// # New Architecture
///
/// ElementTree now stores heterogeneous Elements (ComponentElement, StatefulElement,
/// RenderElement) instead of Renders directly. This provides:
/// - Unified tree structure for all element types
/// - Widget lifecycle management (build, rebuild, mount, unmount)
/// - State management for StatefulElements
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
///         ├─ Element::Component(StatefulElement)
///         ├─ Element::Provider(InheritedElement)
///         ├─ Element::Render(RenderElement)
///         └─ Element::ParentData(ParentDataElement)
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
/// let root_element = RenderElement::new(FlexWidget::column());
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
/// Contains an Element enum variant (Component, Stateful, Inherited, Render, ParentData).
/// The Element enum contains all necessary data including:
/// - Widget configuration
/// - State (for StatefulElement)
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
    /// - `element`: The Element enum (Component, Stateful, Inherited, Render, or ParentData)
    ///
    /// # Returns
    ///
    /// The ElementId for the newly inserted element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::{Element, RenderElement, FlexWidget};
    ///
    /// let render_elem = RenderElement::new(FlexWidget::column());
    /// let root_id = tree.insert(Element::Render(render_elem));
    /// ```
    pub fn insert(&mut self, element: Element) -> ElementId {
        // Create the node
        let node = ElementNode { element };

        // Insert into slab and get ID (convert usize to ElementId)
        let id = self.nodes.insert(node);
        ElementId::new(id)
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
        if let Some(node) = self.nodes.get_mut(element_id.get()) {
            // Call unmount lifecycle
            node.element.unmount();
        }

        // Get children from element (before removing)
        let children: Vec<ElementId> = if let Some(node) = self.nodes.get(element_id.get()) {
            node.element.children().collect()
        } else {
            Vec::new()
        };

        // Remove all children recursively
        for child_id in children {
            self.remove(child_id);
        }

        // Remove from parent's children list
        if let Some(parent_id) = self.get(element_id).and_then(|e| e.parent())
            && let Some(parent_node) = self.nodes.get_mut(parent_id.get())
        {
            parent_node.element.forget_child(element_id);
        }

        // Remove the node itself
        self.nodes.try_remove(element_id.get()).is_some()
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
        self.nodes.get(element_id.get()).map(|node| &node.element)
    }

    /// Get a mutable reference to an element
    ///
    /// # Returns
    ///
    /// `Some(&mut Element)` if the element exists, `None` otherwise
    #[inline]
    pub fn get_mut(&mut self, element_id: ElementId) -> Option<&mut Element> {
        self.nodes.get_mut(element_id.get()).map(|node| &mut node.element)
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

    /// Get a reference to the Render for an element
    ///
    /// # Returns
    ///
    /// `Some(&RenderNode)` if the element is a RenderElement, `None` otherwise
    // Note: render_object() and render_object_mut() methods removed
    // because they cannot work with RefCell guards (lifetime issues).
    // Instead, use: tree.get(element_id)?.render_object()?
    // or: tree.get(element_id)?.render_object_mut()?

    // ========== RenderState Access ==========

    /// Track which elements are currently being laid out (to prevent re-entrant layout)
    ///
    /// This is stored in thread-local storage since layout is single-threaded.
    thread_local! {
        static LAYOUT_STACK: std::cell::RefCell<Vec<ElementId>> = std::cell::RefCell::new(Vec::new());
        static PAINT_STACK: std::cell::RefCell<Vec<ElementId>> = std::cell::RefCell::new(Vec::new());
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
    ///
    /// # Safety
    ///
    /// This method uses unsafe to dereference a raw pointer. It is sound because:
    ///
    /// 1. **Pointer Validity**: The pointer comes from `render_state_ptr()` which returns
    ///    `*const RwLock<RenderState>` that points to data owned by a RenderElement
    ///    stored in the slab.
    ///
    /// 2. **Lifetime Guarantees**: We hold a reference `&self` to the ElementTree for the
    ///    entire duration of this function. The slab owns the ElementNode which owns the
    ///    Element which owns the RenderElement which owns the RwLock<RenderState>.
    ///    The returned guard is tied to lifetime `'_` (same as `&self`), preventing
    ///    the tree from being mutated while the guard exists.
    ///
    /// 3. **No Concurrent Removal**: The element cannot be removed from the slab while
    ///    we hold a reference to it via `get()`. The slab is behind `self` which is
    ///    borrowed immutably.
    ///
    /// 4. **RwLock Safety**: `RwLock::read()` is always safe to call on a valid RwLock
    ///    reference, and provides interior mutability correctly.
    ///
    /// 5. **Initialization**: The RenderState is created along with the RenderElement
    ///    and is always properly initialized before any pointer is handed out.
    ///
    /// **INVARIANTS REQUIRED**:
    /// - Element must exist in tree (checked by `get()`)
    /// - Element must not be removed while reference exists (enforced by Rust borrow checker)
    /// - RenderState must be valid for the lifetime of RenderElement (true by construction)
    #[inline]
    pub fn render_state(
        &self,
        element_id: ElementId,
    ) -> Option<parking_lot::RwLockReadGuard<'_, RenderState>> {
        self.get(element_id).and_then(|element| {
            element.render_state_ptr().map(|ptr| unsafe {
                // SAFETY: See extensive safety documentation above
                (*ptr).read()
            })
        })
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
    ///
    /// # Safety
    ///
    /// This method uses unsafe to dereference a raw pointer. It is sound for the same
    /// reasons as `render_state()` (see that method's safety documentation).
    ///
    /// Additional considerations for write access:
    /// - `RwLock::write()` provides exclusive access via the write guard
    /// - No other readers or writers can access the RenderState while the guard exists
    /// - The lifetime of the guard prevents the element from being removed
    #[inline]
    pub fn render_state_mut(
        &self,
        element_id: ElementId,
    ) -> Option<parking_lot::RwLockWriteGuard<'_, RenderState>> {
        self.get(element_id).and_then(|element| {
            element.render_state_ptr().map(|ptr| unsafe {
                // SAFETY: Same invariants as render_state(), see that method's documentation
                (*ptr).write()
            })
        })
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
        // Check element exists and is a render element
        let element = self.get(element_id)?;
        let render_element = element.as_render()?;

        // **Optimization**: Check RenderState cache before computing layout
        // This avoids expensive dyn_layout() calls when constraints haven't changed
        let render_state = render_element.render_state();

        // Try to use cached size if constraints match and no relayout needed
        {
            let state = render_state.read();
            if state.has_size()
                && !state.needs_layout()
                && let Some(cached_constraints) = state.constraints()
                && cached_constraints == constraints
            {
                // Cache hit! Return cached size without layout computation
                return state.size();
            }
        } // Release read lock

        // Cache miss or needs relayout - compute layout

        // Depth guard to prevent infinite recursion
        #[cfg(debug_assertions)]
        {
            const MAX_LAYOUT_DEPTH: usize = 1000; // Increased for complex UIs
            let current_depth = self.layout_depth.get();
            if current_depth > MAX_LAYOUT_DEPTH {
                eprintln!("ERROR: Layout depth exceeded {}! Infinite recursion detected.", MAX_LAYOUT_DEPTH);
                eprintln!("  Element ID: {}", element_id);
                eprintln!("  This usually means a render object is calling layout on itself or a circular dependency exists.");
                panic!("Layout depth limit exceeded - infinite recursion");
            }
            self.layout_depth.set(current_depth + 1);
        }

        // Check for re-entrant layout (element trying to layout itself)
        let is_reentrant = Self::LAYOUT_STACK.with(|stack| {
            stack.borrow().contains(&element_id)
        });

        if is_reentrant {
            // Get element info for debugging
            let element_debug = if let Some(_elem) = self.get(element_id) {
                // Note: widget() method removed during Widget → View migration
                format!("Element #{}", element_id)
            } else {
                "Unknown".to_string()
            };
            eprintln!("WARNING: Re-entrant layout detected!");
            eprintln!("  Element ID: {}", element_id);
            eprintln!("  Element: {}", element_debug);
            eprintln!("  This element is trying to layout itself - this is a bug in the render object implementation!");
            #[cfg(debug_assertions)]
            self.layout_depth.set(self.layout_depth.get().saturating_sub(1));
            // Return cached size if available, otherwise zero size
            return render_state.read().size();
        }

        // Push element onto layout stack
        Self::LAYOUT_STACK.with(|stack| {
            stack.borrow_mut().push(element_id);
        });

        let size = render_element.render_object_mut().layout(self, constraints);

        // Pop element from layout stack
        Self::LAYOUT_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });

        // Decrement depth
        #[cfg(debug_assertions)]
        self.layout_depth.set(self.layout_depth.get().saturating_sub(1));

        // Update RenderState with new size and constraints
        {
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
        let is_reentrant = Self::PAINT_STACK.with(|stack| {
            stack.borrow().contains(&element_id)
        });

        if is_reentrant {
            // Get element info for debugging
            let element_debug = if let Some(_elem) = self.get(element_id) {
                // Note: widget() method removed during Widget → View migration
                format!("Element #{}", element_id)
            } else {
                "Unknown".to_string()
            };
            eprintln!("WARNING: Re-entrant paint detected!");
            eprintln!("  Element ID: {}", element_id);
            eprintln!("  Element: {}", element_debug);
            eprintln!("  This element is trying to paint itself - this is a bug in the render object implementation!");
            // Return None to avoid infinite recursion
            return None;
        }

        // Push element onto paint stack
        Self::PAINT_STACK.with(|stack| {
            stack.borrow_mut().push(element_id);
        });

        // Get render element
        let element = self.get(element_id)?;
        let render_element = element.as_render()?;

            // Borrow the render object through RwLock - the guard must live until after the call
            let render_object_guard = render_element.render_object();

            // Call paint on RenderNode
            let layer = render_object_guard.paint(self, offset);

        // Pop element from paint stack
        Self::PAINT_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });

        // Guard dropped here (render_object_guard)

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
        let current_element = Self::LAYOUT_STACK.with(|stack| {
            stack.borrow().last().copied()
        });

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
        // TODO(2025-01): Add bounds checking for child_id to ensure it exists in the tree

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
                    crate::element::Element::Component(stateful) => {
                        if let Some(stateful_child_id) = stateful.child() {
                            current_id = stateful_child_id;
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
            self.layout_render_object(render_id, constraints)
                .unwrap_or(flui_types::Size::ZERO)
        } else {
            flui_types::Size::ZERO
        }
    }

    /// Alias for `paint_render_object` - used by SingleRender/MultiRender traits
    #[inline]
    pub fn paint_child(&self, child_id: ElementId, offset: crate::Offset) -> crate::BoxedLayer {
        // TODO(2025-01): Add bounds checking for child_id to ensure it exists in the tree

        // Walk down through ComponentElements to find the first RenderElement
        // (same logic as layout_child)
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
                    crate::element::Element::Component(stateful) => {
                        if let Some(stateful_child_id) = stateful.child() {
                            current_id = stateful_child_id;
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
            self.paint_render_object(render_id, offset)
                .unwrap_or_else(|| Box::new(flui_engine::ContainerLayer::new()))
        } else {
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
    ///
    /// # Safety
    ///
    /// This method uses unsafe to dereference a raw pointer to RenderState.
    /// It is sound because:
    /// 1. We iterate over `&self.nodes` which keeps the slab borrowed immutably
    /// 2. Each element exists for the duration of the loop iteration
    /// 3. The state pointer points to valid RenderState owned by the RenderElement
    /// 4. The state guard's lifetime is contained within the visitor call
    /// 5. No elements can be removed during iteration (immutable borrow of tree)
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

            // Borrow render object through RwLock guard
            let render_obj_guard = render_elem.render_object();
            let state_ptr: *const parking_lot::RwLock<RenderState> = render_elem.render_state();
            
            // SAFETY: 
            // - We hold immutable reference to tree via &self
            // - The slab owns ElementNode owns Element owns RenderElement owns RwLock<RenderState>
            // - state_ptr points to valid RenderState for the element's lifetime
            // - The element cannot be removed while we iterate (borrow checker guarantees)
            // - RwLock::read() is safe to call on a valid RwLock
            let state = unsafe { (*state_ptr).read() };

            // Call visitor with references
            // The guards live for the duration of the visitor call
            visitor(ElementId::new(element_id), &render_obj_guard, state);
            // Guards are dropped here
        }
    }

    /// Visit all elements in the tree
    ///
    /// This visits all elements (Component, Stateful, Inherited, Render, and ParentData).
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
            visitor(ElementId::new(element_id), &node.element);
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
        if let Some(element) = self.get_mut(inherited_id)
            && let Element::Provider(inherited) = element
        {
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
        if let Some(element) = self.get_mut(inherited_id)
            && let Element::Provider(inherited) = element
        {
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

// Tests disabled - need to be updated for new Element enum API
#[cfg(all(test, not(feature = "intentionally_disabled")))]
mod tests {
    use super::*;
    use crate::{LayoutCx, LeafArity, PaintCx, SingleArity};
    use crate::{Render, RenderWidget, Widget};
    use flui_engine::{BoxedLayer, ContainerLayer};
    use flui_types::Size;

    // Test Widgets and Renders
    #[derive(Debug, Clone)]
    struct TestLeafWidget;

    impl Widget for TestLeafWidget {}
    impl Widget for TestLeafWidget {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl RenderWidget for TestLeafWidget {
        type Render = TestLeafRender;
        type Arity = LeafArity;

        fn create_render_object(&self) -> Self::Render {
            TestLeafRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct TestLeafRender;

    impl Render for TestLeafRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::new(100.0, 50.0))
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug, Clone)]
    struct TestSingleWidget;

    impl Widget for TestSingleWidget {}
    impl Widget for TestSingleWidget {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    impl RenderWidget for TestSingleWidget {
        type Render = TestSingleRender;
        type Arity = SingleArity;

        fn create_render_object(&self) -> Self::Render {
            TestSingleRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct TestSingleRender;

    impl Render for TestSingleRender {
        type Arity = SingleArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::new(200.0, 100.0))
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_element_tree_creation() {
        let tree = ElementTree::new();
        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_element_tree_with_capacity() {
        let tree = ElementTree::with_capacity(100);
        assert!(tree.capacity() >= 100);
    }

    #[test]
    fn test_insert_root() {
        let mut tree = ElementTree::new();
        let element = RenderElement::new(TestLeafWidget);
        let root_id = tree.insert(Box::new(element));

        assert_eq!(tree.len(), 1);
        assert!(tree.contains(root_id));
        assert_eq!(tree.parent(root_id), None);
    }

    #[test]
    fn test_remove_element() {
        let mut tree = ElementTree::new();
        let element = RenderElement::new(TestLeafWidget);
        let root_id = tree.insert(Box::new(element));

        assert!(tree.remove(root_id));
        assert!(!tree.contains(root_id));
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_render_object_access() {
        let mut tree = ElementTree::new();
        let element = RenderElement::new(TestLeafWidget);
        let element_id = tree.insert(Box::new(element));

        // Immutable access
        let render_obj = tree.render_object(element_id).unwrap();
        assert_eq!(render_obj.arity(), Some(0));

        // Mutable access
        let render_obj_mut = tree.render_object_mut(element_id).unwrap();
        assert_eq!(render_obj_mut.arity(), Some(0));
    }

    #[test]
    fn test_render_state_access() {
        let mut tree = ElementTree::new();
        let element = RenderElement::new(TestLeafWidget);
        let element_id = tree.insert(Box::new(element));

        // Read access
        {
            let state = tree.render_state(element_id).unwrap();
            assert!(!state.has_size());
        }

        // Write access
        {
            let state = tree.render_state_mut(element_id).unwrap();
            state.set_size(Size::new(100.0, 50.0));
        }

        // Verify
        {
            let state = tree.render_state(element_id).unwrap();
            assert!(state.has_size());
            assert_eq!(state.size(), Some(Size::new(100.0, 50.0)));
        }
    }

    #[test]
    fn test_visit_all_elements() {
        let mut tree = ElementTree::new();
        tree.insert(Box::new(RenderElement::new(TestLeafWidget)));
        tree.insert(Box::new(RenderElement::new(TestLeafWidget)));

        let mut count = 0;
        tree.visit_all_elements(|_id, _element| {
            count += 1;
        });

        assert_eq!(count, 2);
    }

    #[test]
    fn test_clear() {
        let mut tree = ElementTree::new();
        tree.insert(Box::new(RenderElement::new(TestLeafWidget)));
        tree.insert(Box::new(RenderElement::new(TestLeafWidget)));

        assert_eq!(tree.len(), 2);

        tree.clear();

        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
    }
}
