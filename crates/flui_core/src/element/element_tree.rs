//! Slab-based storage for managing Element instances.
//!
//! Provides O(1) access to elements via arena allocation.
//!
//! # Slab Offset Pattern
//!
//! ElementTree uses a +1/-1 offset pattern between ElementId and Slab indices:
//!
//! - **ElementId**: 1-based (uses `NonZeroUsize`, where 0 is invalid)
//! - **Slab indices**: 0-based (standard Vec-like indexing)
//!
//! ```text
//! ElementId (user-facing)  →  Slab index (internal)
//! ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//! ElementId(1)             →  nodes[0]
//! ElementId(2)             →  nodes[1]
//! ElementId(3)             →  nodes[2]
//! ```
//!
//! This offset exists because ElementId uses `NonZeroUsize` for niche optimization,
//! enabling `Option<ElementId>` to occupy only 8 bytes. Since NonZeroUsize cannot
//! represent 0, all IDs are offset by 1.
//!
//! Conversion pattern:
//!
//! ```rust,ignore
//! // Insert: Slab index → ElementId (+1)
//! let slab_index = self.nodes.insert(node);  // Returns 0, 1, 2, ...
//! ElementId::new(slab_index + 1)              // Returns 1, 2, 3, ...
//!
//! // Get: ElementId → Slab index (-1)
//! let element_id = ElementId::new(5);         // User has ID 5
//! self.nodes.get(element_id.get() - 1)        // Access nodes[4]
//! ```

use flui_types::constraints::BoxConstraints;
use slab::Slab;

use crate::element::{Element, ElementId};

use flui_rendering::core::RenderState;

/// Maximum layout recursion depth.
///
/// Prevents infinite recursion in layout calculations, which typically
/// indicates a circular dependency in the render tree.
///
/// Default value of 1000 is sufficient for most UI hierarchies. Modify
/// this constant and recompile if deeper nesting is required.
///
/// This check runs only in debug builds. Release builds omit depth
/// checking for performance.
#[cfg(debug_assertions)]
pub const MAX_LAYOUT_DEPTH: usize = 1000;

/// Central storage for all elements in the UI tree.
///
/// Provides O(1) insertion, removal, and access using slab-based arena allocation.
///
/// # Three-Tree Architecture
///
/// ElementTree is the middle layer in FLUI's three-tree architecture:
///
/// ```text
/// View Tree (immutable)  →  Element Tree (mutable)  →  Render Tree (layout/paint)
///     Configuration             State Management          Visual Output
/// ```
///
/// Responsibilities:
/// - Stores all elements (Component, Provider, Render)
/// - Manages element lifecycle (mount, unmount, dirty tracking)
/// - Provides parent-child relationships
/// - Enables efficient tree traversal and updates
///
/// # Memory Layout
///
/// ```text
/// ElementTree {
///     nodes: Slab<ElementNode>  ← Contiguous memory for cache efficiency
/// }
///
/// ElementNode {
///     element: Element  ← Unified struct with ViewObject delegation
///         ├─ StatelessViewWrapper  - Composable widgets
///         ├─ ProviderViewWrapper   - Inherited data
///         └─ RenderViewWrapper     - Layout & paint
/// }
/// ```
///
/// Slab allocator characteristics:
/// - O(1) insertion and removal
/// - Stable indices (ElementIds remain valid until explicit removal)
/// - Contiguous memory layout for improved cache locality
/// - Automatic slot reuse
///
/// # Slab Offset Pattern (CRITICAL)
///
/// See module-level documentation for the +1/-1 offset pattern between
/// ElementId (1-based) and Slab indices (0-based).
///
/// # Thread Safety
///
/// ElementTree is `Send + Sync` (marked with `unsafe impl`):
/// - Slab storage is thread-safe (owned data only)
/// - Interior mutability handled by parking_lot::RwLock at higher levels
/// - Layout operations use thread-local stacks for re-entrancy detection
///
/// # Usage
///
/// ```rust,ignore
/// use flui_core::{ElementTree, Element};
/// use flui_core::view::RenderViewWrapper;
///
/// let mut tree = ElementTree::new();
///
/// // Insert root element
/// let root_element = Element::new(Box::new(RenderViewWrapper::new(render_object)));
/// let root_id = tree.insert(root_element);
///
/// // Access element (remember: ElementId is 1-based!)
/// if let Some(element) = tree.get(root_id) {
///     println!("Element has {} children", element.children().len());
/// }
///
/// // Layout and paint
/// let size = tree.layout_render_object(root_id, constraints);
/// let layer = tree.paint_render_object(root_id, Offset::ZERO);
/// ```
///
/// # Performance Characteristics
///
/// | Operation | Complexity | Notes |
/// |-----------|------------|-------|
/// | Insert | O(1) | Slab amortized insertion |
/// | Remove | O(k) | k = number of descendants |
/// | Get | O(1) | Direct slab indexing |
/// | Layout/Paint | O(n) | n = subtree size |
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
/// - Render + RenderState (for RenderViewWrapper)
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
            let inserted = stack.borrow_mut().insert(element_id);
            debug_assert!(
                inserted,
                "PaintGuard::new called twice for same element {:?}",
                element_id
            );
        });
        Self { element_id }
    }
}

impl Drop for PaintGuard {
    fn drop(&mut self) {
        ElementTree::PAINT_STACK.with(|stack| {
            let removed = stack.borrow_mut().remove(&self.element_id);
            debug_assert!(
                removed,
                "Paint stack corruption: element {:?} not in stack",
                self.element_id
            );
        });
    }
}

impl ElementTree {
    /// Creates a new empty element tree.
    ///
    /// # Examples
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

    /// Creates an element tree with pre-allocated capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Initial slab capacity
    ///
    /// # Examples
    ///
    /// ```rust,ignore
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

    /// Inserts an element into the tree (raw insertion without mounting children).
    ///
    /// Internal method. Use `insert()` for automatic child mounting.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset when creating ElementIds:
    ///
    /// ```rust,ignore
    /// let slab_index = self.nodes.insert(node);  // Returns 0, 1, 2, ...
    /// ElementId::new(slab_index + 1)              // Returns 1, 2, 3, ...
    /// ```
    ///
    /// ElementId uses NonZeroUsize, making 0 an invalid ID.
    ///
    /// # Returns
    ///
    /// ElementId of the inserted element (1-based)
    fn insert_raw(&mut self, element: Element) -> ElementId {
        // Create the node
        let node = ElementNode { element };

        // Insert into slab and get ID (convert usize to ElementId)
        // CRITICAL: Add 1 because ElementId uses NonZeroUsize (0 is invalid)
        let slab_index = self.nodes.insert(node);
        ElementId::new(slab_index + 1) // Slab index (0-based) → ElementId (1-based)
    }

    /// Inserts an element into the tree.
    ///
    /// Automatically mounts any unmounted children. Primary entry point
    /// for inserting elements created by Views.
    ///
    /// # Returns
    ///
    /// ElementId of the inserted element
    ///
    /// # Complexity
    ///
    /// O(1) amortized for insertion, O(n) for n children
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let render_wrapper = RenderViewWrapper::new(render_object);
    /// let root_id = tree.insert(Element::new(Box::new(render_wrapper)));
    /// ```
    pub fn insert(&mut self, element: Element) -> ElementId {
        // Log element type being inserted
        let element_type = element.debug_name();
        tracing::trace!(element_type = %element_type, "ElementTree::insert()");

        // For render views with unmounted children, process them recursively
        // Component and Provider elements have children managed by build pipeline
        let child_ids: Option<Vec<ElementId>> = if element.is_render_view() {
            // Render views may have unmounted children to process
            // Note: RenderViewWrapper handles this internally
            None
        } else {
            // Component/Provider elements have children managed by build pipeline
            None
        };

        // Insert the parent element (using raw insertion to avoid recursion)
        let parent_id = self.insert_raw(element);

        // Link children to parent
        if let Some(child_ids) = child_ids {
            // Access the element we just inserted
            if let Some(node) = self.nodes.get_mut(parent_id.get() - 1) {
                // Replace children for render elements
                if node.element.is_render() {
                    node.element.children_mut().clear();
                    node.element.children_mut().extend(child_ids.clone());
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

    /// Removes an element and all its descendants from the tree.
    ///
    /// # Returns
    ///
    /// `true` if the element was removed, `false` if nonexistent
    ///
    /// # Complexity
    ///
    /// O(k) where k is the number of descendants
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let removed = tree.remove(element_id);
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
            node.element.children().to_vec()
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

    /// Checks if an element exists in the tree.
    ///
    /// # Complexity
    ///
    /// O(1)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if tree.contains(element_id) {
    ///     // Element exists
    /// }
    /// ```
    #[inline]
    pub fn contains(&self, element_id: ElementId) -> bool {
        // CRITICAL: Apply -1 offset because Slab uses 0-based indexing
        // but ElementId uses 1-based indexing (NonZeroUsize)
        self.nodes.contains(element_id.get() - 1)
    }

    /// Returns a reference to an element.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset when accessing the Slab:
    ///
    /// ```rust,ignore
    /// let element_id = ElementId::new(5);       // 1-based
    /// self.nodes.get(element_id.get() - 1)      // Access nodes[4] (0-based)
    /// ```
    ///
    /// # Complexity
    ///
    /// O(1)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Some(element) = tree.get(element_id) {
    ///     println!("Element has {} children", element.children().len());
    /// }
    /// ```
    #[inline]
    pub fn get(&self, element_id: ElementId) -> Option<&Element> {
        // CRITICAL: Subtract 1 to convert ElementId (1-based) to slab index (0-based)
        self.nodes
            .get(element_id.get() - 1) // ElementId(1) → nodes[0], ElementId(2) → nodes[1], etc.
            .map(|node| &node.element)
    }

    /// Returns a mutable reference to an element.
    ///
    /// # Complexity
    ///
    /// O(1)
    #[inline]
    pub fn get_mut(&mut self, element_id: ElementId) -> Option<&mut Element> {
        // Subtract 1 to convert ElementId (1-based) to slab index (0-based)
        self.nodes
            .get_mut(element_id.get() - 1)
            .map(|node| &mut node.element)
    }

    /// Returns the parent element ID.
    ///
    /// Returns `None` for root elements or nonexistent elements.
    #[inline]
    pub fn parent(&self, element_id: ElementId) -> Option<ElementId> {
        self.get(element_id).and_then(|element| element.parent())
    }

    /// Returns the children of an element.
    ///
    /// Returns an empty Vec if the element has no children or does not exist.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// for child_id in tree.children(parent_id) {
    ///     println!("Child: {}", child_id);
    /// }
    /// ```
    #[inline]
    pub fn children(&self, element_id: ElementId) -> Vec<ElementId> {
        self.get(element_id)
            .map(|element| element.children().to_vec())
            .unwrap_or_default()
    }

    /// Returns the number of children for an element.
    #[inline]
    pub fn child_count(&self, element_id: ElementId) -> usize {
        self.get(element_id)
            .map(|element| element.children().len())
            .unwrap_or(0)
    }

    /// Returns all element IDs in the tree.
    ///
    /// This iterates over all elements currently in the slab (including vacant slots are skipped).
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of elements in the tree.
    ///
    /// # Use Cases
    ///
    /// - LayoutManager::mark_all_dirty() - mark all elements for re-layout
    /// - Debugging - iterate all elements for validation
    /// - Metrics - count total elements
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for element_id in tree.all_element_ids() {
    ///     if let Some(element) = tree.get(element_id) {
    ///         // Process element...
    ///     }
    /// }
    /// ```
    pub fn all_element_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
        // Slab::iter() returns (index, &value) where index is 0-based
        // Convert to 1-based ElementId by adding 1
        self.nodes
            .iter()
            .map(|(index, _)| ElementId::new(index + 1))
    }

    // ========== Render Access ==========

    // Note: render_object() and render_object_mut() methods removed
    // because they cannot work with RefCell guards (lifetime issues).
    // Instead, use: tree.get(element_id)?.render_object()?
    // or: tree.get(element_id)?.render_object_mut()?

    // ========== RenderState Access ==========

    // Track which elements are currently being laid out/painted (to prevent re-entrant operations)
    //
    // This is stored in thread-local storage since layout/paint is single-threaded per-tree.
    //
    // Performance:
    // - LAYOUT_STACK: Vec (needs .last() for debug overflow tracking, O(N) contains but infrequent)
    // - PAINT_STACK: HashSet (O(1) lookups, paint is more frequent than layout)
    thread_local! {
        static LAYOUT_STACK: std::cell::RefCell<Vec<ElementId>> =
            const { std::cell::RefCell::new(Vec::new()) };
        static PAINT_STACK: std::cell::RefCell<std::collections::HashSet<ElementId>> =
            std::cell::RefCell::new(std::collections::HashSet::new());
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
    /// The size computed by the Render, or None if element is not a render element
    ///
    /// # Panics
    ///
    /// Panics if the render object is already borrowed mutably (indicates a layout cycle).
    pub fn layout_render_object(
        &self,
        element_id: ElementId,
        constraints: BoxConstraints,
    ) -> Option<flui_types::Size> {
        let element = self.get(element_id)?;
        if element.is_render() {
            let size = element
                .view_object()
                .layout_render(self, element.children(), constraints);
            Some(size)
        } else {
            None
        }
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
    /// The layer tree, or None if element is not a render element
    pub fn paint_render_object(
        &self,
        element_id: ElementId,
        offset: crate::Offset,
    ) -> Option<flui_painting::Canvas> {
        let element = self.get(element_id)?;
        if element.is_render() {
            let canvas = element
                .view_object()
                .paint_render(self, element.children(), offset);
            Some(canvas)
        } else {
            None
        }
    }

    // ========== Debug-Only Overflow Reporting ==========

    /// Set overflow for the currently-being-laid-out element (debug only)
    ///
    /// This allows renderers to report overflow during layout without
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

        if let Some(_element_id) = current_element {
            // TODO: Implement overflow tracking in RenderState
            // When implemented, this will track overflow in the current render element:
            // if let Some(element) = self.get_mut(element_id) {
            //     if let Some(render_state) = element.render_state_mut() {
            //         render_state.set_overflow(axis, pixels);
            //     }
            // }

            // For now, overflow tracking is not implemented
            #[allow(unused_variables)]
            let (_, _) = (axis, pixels);
        }
    }

    // ========== Helper Methods ==========

    /// Generic helper to walk down through ComponentElements to find an element matching a predicate
    ///
    /// This unified helper eliminates code duplication between `find_render_element`
    /// and `find_sliver_element` by using a predicate function to check element types.
    ///
    /// # Arguments
    /// * `start_id` - Starting element ID (may be Component or target type)
    /// * `predicate` - Function to check if element matches desired type
    ///
    /// # Returns
    /// * `Some(ElementId)` - ID of the first matching element found
    /// * `None` - If no matching element found or tree walk failed
    fn find_element_matching<F>(&self, start_id: ElementId, predicate: F) -> Option<ElementId>
    where
        F: Fn(&crate::element::Element) -> bool,
    {
        let mut current_id = start_id;
        loop {
            if let Some(element) = self.get(current_id) {
                // Check if this element matches the predicate
                if predicate(element) {
                    return Some(current_id);
                }

                // If it's a ComponentElement, walk down to its child
                if element.is_component() {
                    if let Some(&comp_child_id) = element.children().first() {
                        current_id = comp_child_id;
                        continue;
                    }
                }

                // Not a match and not a Component with child -> dead end
                return None;
            } else {
                return None;
            }
        }
    }

    /// Find the first render element by walking down through component elements
    ///
    /// This helper is used by both `layout_child` and `paint_child` to find
    /// the actual render element to operate on.
    ///
    /// # Arguments
    /// * `start_id` - Starting element ID (may be Component or Render)
    ///
    /// # Returns
    /// * `Some(ElementId)` - ID of the first render element found
    /// * `None` - If no render element found or tree walk failed
    fn find_render_element(&self, start_id: ElementId) -> Option<ElementId> {
        self.find_element_matching(start_id, |e| e.is_render())
    }

    /// Walk down through ComponentElements to find the first SliverElement
    ///
    /// This helper is used by both `layout_sliver_child` and `paint_sliver_child` to find
    /// the actual SliverElement to operate on.
    ///
    /// # Arguments
    /// * `start_id` - Starting element ID (may be Component or Sliver)
    ///
    /// # Returns
    /// * `Some(ElementId)` - ID of the first SliverElement found
    /// * `None` - If no SliverElement found or tree walk failed
    // TODO: Re-enable sliver support after completing box render migration
    fn find_sliver_element(&self, _start_id: ElementId) -> Option<ElementId> {
        // self.find_element_matching(start_id, |e| e.is_sliver())
        None // Slivers temporarily disabled
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

        // Walk down through component elements to find the first render element
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
                "Could not find render element for child. Element may be component without child or provider. Returning Size::ZERO."
            );
            flui_types::Size::ZERO
        }
    }

    /// Alias for `paint_render_object` - used by SingleRender/MultiRender traits
    #[inline]
    pub fn paint_child(&self, child_id: ElementId, offset: crate::Offset) -> flui_painting::Canvas {
        // Hot path - trace disabled for performance

        // Walk down through component elements to find the first render element
        let render_id = self.find_render_element(child_id);

        if let Some(render_id) = render_id {
            self.paint_render_object(render_id, offset)
                .unwrap_or_default()
        } else {
            #[cfg(debug_assertions)]
            tracing::warn!("paint_child: returning empty Canvas (no render_id)");
            flui_painting::Canvas::new()
        }
    }

    /// Layout a sliver child with sliver constraints
    ///
    /// This is a convenience method for laying out sliver children from within
    /// a RenderSliver implementation.
    ///
    /// # Parameters
    ///
    /// - `child_id`: The element ID of the sliver child to layout
    /// - `constraints`: The sliver constraints to apply
    ///
    /// # Returns
    ///
    /// The sliver geometry computed by the child's layout method.
    #[inline]
    pub fn layout_sliver_child(
        &self,
        child_id: ElementId,
        _constraints: flui_types::SliverConstraints,
    ) -> flui_types::SliverGeometry {
        // Bounds checking: verify child_id exists in tree
        if !self.contains(child_id) {
            #[cfg(debug_assertions)]
            {
                tracing::error!(
                    child_id = ?child_id,
                    "layout_sliver_child called with invalid child_id - element not in tree"
                );
                panic!("Invalid child_id in layout_sliver_child: {:?}", child_id);
            }

            #[cfg(not(debug_assertions))]
            {
                tracing::error!(
                    child_id = ?child_id,
                    "layout_sliver_child called with invalid child_id, returning default geometry"
                );
                return flui_types::SliverGeometry::default();
            }
        }

        // Walk down through ComponentElements to find the first SliverElement
        let sliver_id = self.find_sliver_element(child_id);

        if let Some(_sliver_id) = sliver_id {
            // TODO: Re-enable sliver support after completing box render migration
            // // Get the SliverElement
            // if let Some(crate::element::Element::Sliver(sliver_elem)) = self.get(sliver_id) {
            //     // Call layout_sliver on the element
            //     let geometry = sliver_elem.layout_sliver(self, constraints);
            //
            //     // Store geometry in render state (combined write guard for efficiency)
            //     {
            //         let state = sliver_elem.render_state().write();
            //         state.set_geometry(geometry);
            //         state.clear_needs_layout();
            //     }
            //
            //     geometry
            // } else {
            //     tracing::error!(
            //         child_id = ?child_id,
            //         sliver_id = ?sliver_id,
            //         "Found sliver_id but failed to get SliverElement. Returning default geometry."
            //     );
            //     flui_types::SliverGeometry::default()
            // }
            tracing::warn!("Sliver support temporarily disabled during migration");
            flui_types::SliverGeometry::default()
        } else {
            tracing::warn!(
                child_id = ?child_id,
                "Could not find SliverElement for child. Element may be Component without child or other type. Returning default geometry."
            );
            flui_types::SliverGeometry::default()
        }
    }

    /// Paint a sliver child at the given offset
    ///
    /// This is a convenience method for painting sliver children from within
    /// a RenderSliver implementation.
    ///
    /// # Parameters
    ///
    /// - `child_id`: The element ID of the sliver child to paint
    /// - `offset`: The offset at which to paint the child
    ///
    /// # Returns
    ///
    /// A Canvas containing the child's drawing commands.
    #[inline]
    pub fn paint_sliver_child(
        &self,
        child_id: ElementId,
        _offset: crate::Offset,
    ) -> flui_painting::Canvas {
        // Hot path - trace disabled for performance

        // Walk down through ComponentElements to find the first SliverElement
        let sliver_id = self.find_sliver_element(child_id);

        if let Some(_sliver_id) = sliver_id {
            // TODO: Re-enable sliver support after completing box render migration
            // // Get the SliverElement
            // if let Some(crate::element::Element::Sliver(sliver_elem)) = self.get(sliver_id) {
            //     // Call paint_sliver on the element
            //     sliver_elem.paint_sliver(self, offset)
            // } else {
            //     #[cfg(debug_assertions)]
            //     tracing::warn!("paint_sliver_child: found sliver_id but failed to get SliverElement, returning empty Canvas");
            //     flui_painting::Canvas::new()
            // }
            tracing::warn!("Sliver support temporarily disabled during migration");
            flui_painting::Canvas::new()
        } else {
            #[cfg(debug_assertions)]
            tracing::warn!("paint_sliver_child: returning empty Canvas (no sliver_id)");
            flui_painting::Canvas::new()
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

    /// Get the root element ID
    ///
    /// Returns the first element without a parent (root element).
    /// Returns `None` if the tree is empty.
    ///
    /// # Complexity
    ///
    /// O(n) where n is the number of elements (scans for parentless element)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(root_id) = tree.root_id() {
    ///     println!("Root element: {:?}", root_id);
    /// }
    /// ```
    pub fn root_id(&self) -> Option<ElementId> {
        // Find first element without a parent
        for (index, node) in &self.nodes {
            if node.element.parent().is_none() {
                return Some(ElementId::new(index + 1));
            }
        }
        None
    }

    // ========== Iteration ==========

    /// Visit all render elements in the tree
    ///
    /// This only visits elements that have render objects (RenderViewWrapper).
    /// Component and provider elements are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_render_objects(|element_id, render_obj, state| {
    ///     println!("Element {}: arity = {:?}", element_id, render_obj.arity());
    /// });
    /// ```
    /// Visit all render elements in the tree
    ///
    /// This only visits elements that have render objects (RenderViewWrapper).
    /// Component and provider elements are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_render_elements(|element_id, element| {
    ///     if let Some(protocol) = element.protocol() {
    ///         println!("Element {}: protocol = {:?}", element_id, protocol);
    ///     }
    /// });
    /// ```
    pub fn visit_all_render_elements<F>(&self, mut visitor: F)
    where
        F: FnMut(ElementId, &Element),
    {
        for (element_id, node) in &self.nodes {
            // Only visit elements with render objects
            if !node.element.is_render() {
                continue;
            }

            // Call visitor with element reference
            // Add 1 to convert slab index (0-based) to ElementId (1-based)
            visitor(ElementId::new(element_id + 1), &node.element);
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
    ///     println!("Element {} has {} children", element_id, element.children().len());
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
            let children: Vec<ElementId> = element.children().to_vec();
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
        if let Some(element) = self.get_mut(inherited_id) {
            if element.is_provider() {
                element.add_dependent(dependent_id);
                return true;
            }
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
        if let Some(element) = self.get_mut(inherited_id) {
            if element.is_provider() {
                element.remove_dependent(dependent_id);
                return true;
            }
        }
        false
    }

    /// Get all dependents of an InheritedElement
    ///
    /// Returns the slice of ElementIds that have registered a dependency on
    /// the specified InheritedElement.
    ///
    /// # Returns
    ///
    /// `Some(&[ElementId])` if the element exists and is an InheritedElement,
    /// `None` otherwise.
    pub fn get_dependents(&self, inherited_id: ElementId) -> Option<&[ElementId]> {
        if let Some(element) = self.get(inherited_id) {
            if element.is_provider() {
                return element.dependents();
            }
        }
        None
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
    /// Following Flutter's `Render.hitTest()` pattern:
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
    pub fn hit_test(
        &self,
        root_id: ElementId,
        position: flui_types::Offset,
    ) -> crate::element::ElementHitTestResult {
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

        if element.is_render() {
            self.hit_test_render(element_id, element, position, result)
        } else if element.is_component() {
            // ComponentElement delegates to child
            if let Some(&child_id) = element.children().first() {
                self.hit_test_recursive(child_id, position, result)
            } else {
                false
            }
        } else if element.is_provider() {
            // Provider element delegates to child
            if let Some(&child_id) = element.children().first() {
                self.hit_test_recursive(child_id, position, result)
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Hit test a child element from a RenderObject
    ///
    /// This method is called from `Render::hit_test_children()` to test a specific child.
    /// It bridges between the RenderObject-level hit testing (BoxHitTestResult) and
    /// the ElementTree-level hit testing (ElementHitTestResult).
    ///
    /// # Parameters
    ///
    /// - `child_id`: The child element to test
    /// - `position`: Global position to test (in window coordinates)
    /// - `box_result`: BoxHitTestResult from the RenderObject hit testing
    ///
    /// # Returns
    ///
    /// - `true` if the child or any of its descendants was hit
    /// - `false` if nothing was hit
    ///
    /// # Note
    ///
    /// This is a bridge method. In the future, we may unify ElementHitTestResult
    /// and BoxHitTestResult for a more integrated approach.
    #[inline]
    pub fn hit_test_box_child(
        &self,
        child_id: ElementId,
        position: flui_types::Offset,
        _box_result: &mut crate::element::hit_test::BoxHitTestResult,
    ) -> bool {
        // For now, use a temporary ElementHitTestResult to test the child
        // In a future refactoring, we can merge BoxHitTestResult and ElementHitTestResult
        let mut temp_result = crate::element::ElementHitTestResult::new();
        // TODO: In the future, transfer entries from temp_result to box_result
        // For now, we just return whether there was a hit
        self.hit_test_recursive(child_id, position, &mut temp_result)
    }

    /// Hit test a child element from a RenderSliver
    ///
    /// This method is called from `RenderSliver::hit_test_children()` to test a specific child.
    /// It bridges between the RenderSliver-level hit testing (SliverHitTestResult) and
    /// the ElementTree-level hit testing (ElementHitTestResult).
    ///
    /// # Parameters
    ///
    /// - `child_id`: The child element to test
    /// - `position`: Global position to test (in window coordinates)
    /// - `sliver_result`: SliverHitTestResult from the RenderSliver hit testing
    ///
    /// # Returns
    ///
    /// - `true` if the child or any of its descendants was hit
    /// - `false` if nothing was hit
    ///
    /// # Note
    ///
    /// This is a bridge method. In the future, we may unify ElementHitTestResult
    /// and SliverHitTestResult for a more integrated approach.
    #[inline]
    pub fn hit_test_sliver_child(
        &self,
        child_id: ElementId,
        position: flui_types::Offset,
        _sliver_result: &mut crate::element::hit_test::SliverHitTestResult,
    ) -> bool {
        // For now, use a temporary ElementHitTestResult to test the child
        let mut temp_result = crate::element::ElementHitTestResult::new();
        // TODO: In the future, transfer entries from temp_result to sliver_result
        self.hit_test_recursive(child_id, position, &mut temp_result)
    }

    /// Hit test for render element
    ///
    /// Checks if position is within element bounds and recursively tests children.
    /// Adds hit elements to result in depth-first order (children before parents).
    fn hit_test_render(
        &self,
        element_id: ElementId,
        element: &Element,
        position: flui_types::Offset,
        result: &mut crate::element::ElementHitTestResult,
    ) -> bool {
        // Get size from render state via ViewObject delegation
        let render_state = match element.render_state() {
            Some(state) => state,
            None => {
                tracing::warn!("Element is not a render element");
                return false;
            }
        };

        if !render_state.has_size() {
            return false; // No layout yet
        }
        let size = render_state.size();

        // For unified Element, offset is stored in render state or ViewObject
        // TODO: Implement proper offset handling in RenderViewWrapper
        let offset = flui_types::Offset::ZERO; // Placeholder until RenderViewWrapper handles offset

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
        for &child_id in element.children() {
            self.hit_test_recursive(child_id, position, result);
        }

        // Add self to result (even if child was hit)
        // This maintains depth-first order: children added before parents
        result.add_element(element_id, local_position);

        true
    }

    /// Hit test for SliverElement (TEMPORARILY DISABLED)
    ///
    /// Checks if position is within sliver bounds and recursively tests children.
    /// Slivers use geometry instead of size for bounds checking.
    // TODO: Re-enable sliver support after completing box render migration
    #[allow(dead_code)]
    fn hit_test_sliver(
        &self,
        _element_id: ElementId,
        _sliver_elem: &(), // Placeholder since SliverElement is disabled
        _position: flui_types::Offset,
        _result: &mut crate::element::ElementHitTestResult,
    ) -> bool {
        false // Slivers temporarily disabled - entire body commented out
              // // Get geometry from render state
              // let render_state = sliver_elem.render_state().read();
              // let geometry = match render_state.geometry() {
              //     Some(g) => g,
              //     None => return false, // No layout yet
              // };
              // drop(render_state);
              //
              // // Get offset (position in viewport)
              // let offset = sliver_elem.offset();
              //
              // // Transform position to local coordinates
              // let local_position = position - offset;
              //
              // // For slivers, we need to check against paint_extent
              // // (the visible portion of the sliver)
              // // TODO: This is a simplified check - proper sliver hit testing
              // // should account for scroll direction and constraints
              // if local_position.dx < 0.0
              //     || local_position.dy < 0.0
              //     || local_position.dy > geometry.paint_extent
              // {
              //     return false; // Outside visible bounds
              // }
              //
              // // Test children first (front to back)
              // for &child_id in sliver_elem.children() {
              //     self.hit_test_recursive(child_id, position, result);
              // }
              //
              // // Add self to result
              // result.add_element(element_id, local_position);
              //
              // true
    }

    // ============================================================================
    // PHASE 6: Unified RenderObject System Helper Methods
    // ============================================================================

    /// Request layout for an element (Phase 6 - unified system)
    ///
    /// This method handles both dirty set marking and RenderState flag setting atomically
    /// to prevent the "marked but not flagged" bug.
    ///
    /// # Parameters
    ///
    /// - `element_id` - The element that needs layout
    ///
    /// # Thread Safety
    ///
    /// This method is lock-free for the flag check (uses AtomicRenderFlags).
    /// The dirty set is managed by the coordinator (not part of ElementTree yet).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.request_layout(element_id);
    /// // Both dirty set AND flag are marked atomically
    /// ```
    #[tracing::instrument(skip(self), level = "debug")]
    pub fn request_layout(&mut self, element_id: ElementId) {
        tracing::debug!("Requesting layout for element {:?}", element_id);

        // Get the element node (with -1 offset for slab access)
        if let Some(node) = self.nodes.get_mut(element_id.get() - 1) {
            if let Some(render_state) = node.element.view_object_mut().render_state_mut() {
                // Mark needs layout in RenderState flags
                // This is the critical part that was missing before
                render_state.mark_needs_layout();

                // TODO: Also add to dirty_layout set (will be in coordinator)
                // For now, just marking the flag is sufficient for Phase 6
            } else {
                tracing::warn!(
                    "request_layout called on non-render element {:?}",
                    element_id
                );
            }
        } else {
            tracing::error!("request_layout: element {:?} not found", element_id);
        }
    }

    /// Request paint for an element (Phase 6 - unified system)
    ///
    /// Similar to request_layout, handles both dirty set and flag atomically.
    ///
    /// # Parameters
    ///
    /// - `element_id` - The element that needs paint
    #[tracing::instrument(skip(self), level = "debug")]
    pub fn request_paint(&mut self, element_id: ElementId) {
        tracing::debug!("Requesting paint for element {:?}", element_id);

        // Get the element node (with -1 offset for slab access)
        if let Some(node) = self.nodes.get_mut(element_id.get() - 1) {
            if let Some(render_state) = node.element.view_object_mut().render_state_mut() {
                // Mark needs paint in RenderState flags
                render_state.mark_needs_paint();

                // TODO: Also add to dirty_paint set (will be in coordinator)
            } else {
                tracing::warn!(
                    "request_paint called on non-render element {:?}",
                    element_id
                );
            }
        } else {
            tracing::error!("request_paint: element {:?} not found", element_id);
        }
    }

    // NOTE: layout_box_child, paint_box_child, and hit_test_box_child already exist
    // in this file with correct signatures. Phase 6 full implementation will enhance
    // those existing methods to use DynRenderObject::dyn_layout/dyn_paint/dyn_hit_test.
}

// SAFETY: ElementTree is thread-safe for multi-threaded UI:
// - Slab<ElementNode> is Send+Sync (contains only owned data)
// - AtomicUsize is Send+Sync (atomic operations)
// - Element enum variants are designed to be Send (though not all are Sync due to interior mutability)
// - Access is controlled by parking_lot::RwLock which provides thread-safe interior mutability
unsafe impl Send for ElementTree {}
unsafe impl Sync for ElementTree {}

// Tests removed - need to be rewritten with View API
