//! TreeCoordinator - Coordinates the four separate trees in FLUI
//!
//! This module provides the TreeCoordinator which manages synchronization
//! between the four trees in FLUI's architecture:
//! - ViewTree (stores ViewObjects)
//! - ElementTree (stores Elements with ID references)
//! - RenderTree (stores RenderObjects)
//! - LayerTree (stores compositor layers)
//!
//! # Architecture
//!
//! ```text
//! TreeCoordinator
//!   ├── views: ViewTree           (ViewObjects storage)
//!   ├── elements: ElementTree     (Element storage with ID refs)
//!   ├── render_objects: RenderTree (RenderObjects storage)
//!   └── layers: LayerTree         (Compositor layers)
//! ```
//!
//! # Flutter Analogy
//!
//! This is similar to Flutter's PipelineOwner combined with the coordination
//! between Widget tree, Element tree, and RenderObject tree.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::pipeline::TreeCoordinator;
//!
//! // Create coordinator with all four trees
//! let mut coordinator = TreeCoordinator::new();
//!
//! // Insert a view object
//! let view_id = coordinator.views_mut().insert(my_view);
//!
//! // Create element referencing the view
//! let element = Element::view(Some(view_id), ViewMode::Stateless);
//! let element_id = coordinator.elements_mut().insert(element);
//!
//! // Set root
//! coordinator.set_root(Some(element_id));
//!
//! // Perform build/layout/paint cycle
//! coordinator.mark_needs_build(element_id);
//! ```

use std::collections::HashSet;

use flui_element::{Element, ElementTree};
use flui_engine::LayerTree;
use flui_foundation::{ElementId, Slot};
use flui_painting::DisplayListCore;
use flui_rendering::tree::RenderTree;
use flui_view::tree::ViewTree;
use tracing::instrument;

// ============================================================================
// TREE COORDINATOR
// ============================================================================

/// Coordinates the four separate trees in FLUI's architecture.
///
/// TreeCoordinator manages:
/// - **ViewTree**: Stores ViewObjects (immutable view definitions)
/// - **ElementTree**: Stores Elements with references to Views/RenderObjects/Layers
/// - **RenderTree**: Stores RenderObjects (layout and paint logic)
/// - **LayerTree**: Stores compositor layers (GPU-accelerated rendering)
///
/// # Dirty Tracking
///
/// Like Flutter's PipelineOwner, TreeCoordinator tracks which elements need:
/// - Build (view tree changed)
/// - Layout (constraints changed)
/// - Paint (visual properties changed)
/// - Compositing (layer structure changed)
///
/// # Thread Safety
///
/// TreeCoordinator is not thread-safe by default. For multi-threaded access,
/// wrap in `Arc<RwLock<TreeCoordinator>>` or use `parking_lot::RwLock`.
#[derive(Debug)]
pub struct TreeCoordinator {
    // ========== Four Trees ==========
    /// ViewTree - stores immutable ViewObjects
    views: ViewTree,

    /// ElementTree - stores Elements with ID references
    elements: ElementTree,

    /// RenderTree - stores RenderObjects for layout/paint
    render_objects: RenderTree,

    /// LayerTree - stores compositor layers
    layers: LayerTree,

    // ========== Dirty Tracking (Flutter PipelineOwner pattern) ==========
    /// Elements that need build (view changed)
    needs_build: HashSet<ElementId>,

    /// Elements that need layout (constraints changed)
    needs_layout: HashSet<ElementId>,

    /// Elements that need paint (visual properties changed)
    needs_paint: HashSet<ElementId>,

    /// Elements that need compositing update (layer structure changed)
    needs_compositing: HashSet<ElementId>,

    /// Root element ID
    root: Option<ElementId>,
}

// ============================================================================
// CONSTRUCTION
// ============================================================================

impl TreeCoordinator {
    /// Creates a new TreeCoordinator with empty trees.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let coordinator = TreeCoordinator::new();
    /// ```
    pub fn new() -> Self {
        Self {
            views: ViewTree::new(),
            elements: ElementTree::new(),
            render_objects: RenderTree::new(),
            layers: LayerTree::new(),
            needs_build: HashSet::new(),
            needs_layout: HashSet::new(),
            needs_paint: HashSet::new(),
            needs_compositing: HashSet::new(),
            root: None,
        }
    }

    /// Creates a TreeCoordinator with pre-allocated capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Initial capacity for trees and dirty sets
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            views: ViewTree::with_capacity(capacity),
            elements: ElementTree::with_capacity(capacity),
            render_objects: RenderTree::with_capacity(capacity),
            layers: LayerTree::with_capacity(capacity),
            needs_build: HashSet::with_capacity(capacity),
            needs_layout: HashSet::with_capacity(capacity),
            needs_paint: HashSet::with_capacity(capacity),
            needs_compositing: HashSet::with_capacity(capacity),
            root: None,
        }
    }
}

impl Default for TreeCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TREE ACCESS
// ============================================================================

impl TreeCoordinator {
    /// Returns a reference to the ViewTree.
    #[inline]
    pub fn views(&self) -> &ViewTree {
        &self.views
    }

    /// Returns a mutable reference to the ViewTree.
    #[inline]
    pub fn views_mut(&mut self) -> &mut ViewTree {
        &mut self.views
    }

    /// Returns a reference to the ElementTree.
    #[inline]
    pub fn elements(&self) -> &ElementTree {
        &self.elements
    }

    /// Returns a mutable reference to the ElementTree.
    #[inline]
    pub fn elements_mut(&mut self) -> &mut ElementTree {
        &mut self.elements
    }

    /// Returns a reference to the RenderTree.
    #[inline]
    pub fn render_objects(&self) -> &RenderTree {
        &self.render_objects
    }

    /// Returns a mutable reference to the RenderTree.
    #[inline]
    pub fn render_objects_mut(&mut self) -> &mut RenderTree {
        &mut self.render_objects
    }

    /// Returns a reference to the LayerTree.
    #[inline]
    pub fn layers(&self) -> &LayerTree {
        &self.layers
    }

    /// Returns a mutable reference to the LayerTree.
    #[inline]
    pub fn layers_mut(&mut self) -> &mut LayerTree {
        &mut self.layers
    }

    /// Unwraps the coordinator, returning all four trees.
    ///
    /// Returns: (views, elements, render_objects, layers)
    pub fn into_trees(self) -> (ViewTree, ElementTree, RenderTree, LayerTree) {
        (self.views, self.elements, self.render_objects, self.layers)
    }
}

// ============================================================================
// ROOT MANAGEMENT
// ============================================================================

impl TreeCoordinator {
    /// Gets the root element ID.
    #[inline]
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Sets the root element ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<ElementId>) {
        self.root = root;
    }

    /// Mount an element as the root of the tree.
    ///
    /// This is a convenience method that:
    /// 1. Mounts the element (sets parent=None, slot=0, depth=0)
    /// 2. Inserts it into the ElementTree
    /// 3. Sets it as the root
    ///
    /// # Arguments
    ///
    /// * `element` - The element to mount as root
    ///
    /// # Returns
    ///
    /// The ElementId of the mounted root element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut coordinator = TreeCoordinator::new();
    /// let element = Element::new(Box::new(wrapper));
    /// let root_id = coordinator.mount_root(element);
    /// ```
    pub fn mount_root(&mut self, mut element: Element) -> ElementId {
        // Mount the element (no parent, slot 0, depth 0 for root)
        element.mount(None, Some(Slot::new(0)), 0);

        // Handle pending ViewObject (four-tree architecture)
        if let Some(view_elem) = element.as_view_mut() {
            if let Some(view_object) = view_elem.take_pending_view_object() {
                use flui_view::tree::ViewNode;
                let mode = view_object.mode();
                let node = ViewNode::from_boxed(view_object, mode);
                let view_id = self.views.insert(node);
                view_elem.set_view_id(Some(view_id));
                tracing::debug!(?view_id, ?mode, "Root ViewObject registered");
            }
        }

        // Handle pending RenderObject (four-tree architecture)
        if let Some(render_elem) = element.as_render_mut() {
            if let Some(render_object) = render_elem.take_pending_render_object() {
                use flui_rendering::tree::RenderNode;
                let node = RenderNode::from_boxed(render_object);
                let render_id = self.render_objects.insert(node);
                render_elem.set_render_id(Some(render_id));
                tracing::debug!(?render_id, "Root RenderObject registered");
            }
        }

        // Insert into ElementTree
        let id = self.elements.insert(element);

        // Handle pending children (must be done after parent is inserted)
        if let Some(element) = self.elements.get_mut(id) {
            let pending_children = match element {
                Element::View(v) => v.take_pending_children(),
                Element::Render(r) => r.take_pending_children(),
            };

            if let Some(children) = pending_children {
                for child_box in children {
                    // Downcast to Element
                    if let Ok(child_element) = child_box.downcast::<Element>() {
                        let child_id = self.mount_child(*child_element, id);
                        // Add child to parent's children list
                        if let Some(parent) = self.elements.get_mut(id) {
                            parent.add_child(child_id);
                        }
                    }
                }
            }
        }

        // Set as root
        self.root = Some(id);

        id
    }

    /// Mounts a child element with the given parent.
    ///
    /// This handles:
    /// 1. Mounting the element with parent reference
    /// 2. Registering pending ViewObject/RenderObject
    /// 3. Linking children in RenderTree (for layout)
    /// 4. Recursively mounting pending children
    fn mount_child(&mut self, mut element: Element, parent_id: ElementId) -> ElementId {
        // Get parent depth
        let parent_depth = self.elements.get(parent_id).map(|e| e.depth()).unwrap_or(0);

        // Get next slot index based on parent's current children count
        let slot_index = self
            .elements
            .get(parent_id)
            .map(|e| e.children().len())
            .unwrap_or(0);

        // Mount the element
        element.mount(
            Some(parent_id),
            Some(Slot::new(slot_index)),
            parent_depth + 1,
        );

        // Handle pending ViewObject
        if let Some(view_elem) = element.as_view_mut() {
            if let Some(view_object) = view_elem.take_pending_view_object() {
                use flui_view::tree::ViewNode;
                let mode = view_object.mode();
                let node = ViewNode::from_boxed(view_object, mode);
                let view_id = self.views.insert(node);
                view_elem.set_view_id(Some(view_id));
                tracing::debug!(?view_id, ?mode, "Child ViewObject registered");
            }
        }

        // Handle pending RenderObject and link in RenderTree
        let child_render_id = if let Some(render_elem) = element.as_render_mut() {
            if let Some(render_object) = render_elem.take_pending_render_object() {
                use flui_rendering::tree::RenderNode;
                let node = RenderNode::from_boxed(render_object);
                let render_id = self.render_objects.insert(node);
                render_elem.set_render_id(Some(render_id));
                tracing::debug!(?render_id, "Child RenderObject registered");
                Some(render_id)
            } else {
                render_elem.render_id()
            }
        } else {
            None
        };

        // Link child RenderObject to parent RenderObject in RenderTree
        if let Some(child_rid) = child_render_id {
            // Get parent's render_id
            let parent_render_id = self
                .elements
                .get(parent_id)
                .and_then(|e| e.as_render())
                .and_then(|r| r.render_id());

            if let Some(parent_rid) = parent_render_id {
                self.render_objects.add_child(parent_rid, child_rid);
                tracing::debug!(
                    parent = ?parent_rid,
                    child = ?child_rid,
                    "RenderTree child linked"
                );
            }
        }

        // Insert into ElementTree
        let id = self.elements.insert(element);

        // Handle pending children recursively
        if let Some(element) = self.elements.get_mut(id) {
            let pending_children = match element {
                Element::View(v) => v.take_pending_children(),
                Element::Render(r) => r.take_pending_children(),
            };

            if let Some(children) = pending_children {
                for child_box in children {
                    if let Ok(child_element) = child_box.downcast::<Element>() {
                        let child_id = self.mount_child(*child_element, id);
                        if let Some(parent) = self.elements.get_mut(id) {
                            parent.add_child(child_id);
                        }
                    }
                }
            }
        }

        id
    }
}

// ============================================================================
// DIRTY TRACKING (Flutter PipelineOwner pattern)
// ============================================================================

impl TreeCoordinator {
    /// Marks an element as needing build.
    ///
    /// This is called when a view's dependencies change or when
    /// a stateful view's state is updated.
    pub fn mark_needs_build(&mut self, id: ElementId) {
        self.needs_build.insert(id);
    }

    /// Marks an element as needing layout.
    ///
    /// This is called when constraints change or when a render object's
    /// intrinsic dimensions change.
    pub fn mark_needs_layout(&mut self, id: ElementId) {
        self.needs_layout.insert(id);
        // Layout changes require repaint (Flutter pattern)
        self.mark_needs_paint(id);
    }

    /// Marks an element as needing paint.
    ///
    /// This is called when visual properties change (color, opacity, etc.)
    /// but layout remains the same.
    pub fn mark_needs_paint(&mut self, id: ElementId) {
        self.needs_paint.insert(id);
    }

    /// Marks an element as needing compositing update.
    ///
    /// This is called when layer properties change or when elements
    /// are added/removed from the compositor.
    pub fn mark_needs_compositing(&mut self, id: ElementId) {
        self.needs_compositing.insert(id);
    }

    /// Returns the set of elements that need build.
    #[inline]
    pub fn needs_build(&self) -> &HashSet<ElementId> {
        &self.needs_build
    }

    /// Returns the set of elements that need layout.
    #[inline]
    pub fn needs_layout(&self) -> &HashSet<ElementId> {
        &self.needs_layout
    }

    /// Returns the set of elements that need paint.
    #[inline]
    pub fn needs_paint(&self) -> &HashSet<ElementId> {
        &self.needs_paint
    }

    /// Returns the set of elements that need compositing.
    #[inline]
    pub fn needs_compositing(&self) -> &HashSet<ElementId> {
        &self.needs_compositing
    }

    /// Returns and clears elements needing build.
    ///
    /// This is useful for processing dirty elements in a frame.
    pub fn take_needs_build(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_build)
    }

    /// Returns and clears elements needing layout.
    pub fn take_needs_layout(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_layout)
    }

    /// Returns and clears elements needing paint.
    pub fn take_needs_paint(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_paint)
    }

    /// Returns and clears elements needing compositing.
    pub fn take_needs_compositing(&mut self) -> HashSet<ElementId> {
        std::mem::take(&mut self.needs_compositing)
    }

    /// Clears all dirty sets.
    ///
    /// This is typically called after a complete frame has been rendered.
    pub fn clear_dirty(&mut self) {
        self.needs_build.clear();
        self.needs_layout.clear();
        self.needs_paint.clear();
        self.needs_compositing.clear();
    }

    /// Returns true if there are any dirty elements.
    pub fn has_dirty_elements(&self) -> bool {
        !self.needs_build.is_empty()
            || !self.needs_layout.is_empty()
            || !self.needs_paint.is_empty()
            || !self.needs_compositing.is_empty()
    }

    /// Returns true if any element needs build.
    #[inline]
    pub fn has_needs_build(&self) -> bool {
        !self.needs_build.is_empty()
    }

    /// Returns true if any element needs layout.
    #[inline]
    pub fn has_needs_layout(&self) -> bool {
        !self.needs_layout.is_empty()
    }

    /// Returns true if any element needs paint.
    #[inline]
    pub fn has_needs_paint(&self) -> bool {
        !self.needs_paint.is_empty()
    }
}

// ============================================================================
// LAYOUT
// ============================================================================

impl TreeCoordinator {
    /// Layout a single render element via its RenderTree entry.
    ///
    /// This method implements the four-tree layout flow:
    /// 1. Get Element from ElementTree using element_id
    /// 2. Get RenderId from RenderElement
    /// 3. Get RenderNode from RenderTree
    /// 4. Check arity and dispatch to appropriate layout method
    /// 5. Cache the computed size in RenderNode
    ///
    /// # Arguments
    ///
    /// * `element_id` - The ElementId of a RenderElement
    /// * `constraints` - Box constraints for layout
    ///
    /// # Returns
    ///
    /// - `Some(Size)` if layout succeeded
    /// - `None` if element not found, not a RenderElement, or has no render_id
    #[instrument(level = "trace", skip(self, constraints), fields(element = ?element_id))]
    pub fn layout_element(
        &mut self,
        element_id: flui_foundation::ElementId,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        use flui_rendering::core::RuntimeArity;

        // Step 1: Get Element from ElementTree
        let element = self.elements.get(element_id)?;

        // Step 2: Element must be a RenderElement with a render_id
        let render_elem = element.as_render()?;
        let render_id = render_elem.render_id()?;
        let arity = render_elem.arity();

        // Step 3: Verify RenderNode exists in RenderTree
        if self.render_objects.get(render_id).is_none() {
            tracing::warn!(?render_id, "RenderNode not found in tree");
            return None;
        }

        // Step 4: Dispatch based on arity
        let size = match arity {
            RuntimeArity::Exact(0) => {
                // Leaf - no children
                self.layout_leaf_render_object(render_id, constraints)?
            }
            RuntimeArity::Exact(1) => {
                // Single - exactly one child
                self.layout_single_render_object(render_id, constraints)?
            }
            RuntimeArity::Exact(n) => {
                // Exact<N> - fixed number of children
                self.layout_variable_render_object(render_id, constraints, Some(n))?
            }
            RuntimeArity::Optional => {
                // Optional - 0 or 1 child
                self.layout_optional_render_object(render_id, constraints)?
            }
            RuntimeArity::Variable => {
                // Variable - any number of children
                self.layout_variable_render_object(render_id, constraints, None)?
            }
            RuntimeArity::AtLeast(min) => {
                // AtLeast<N> - minimum N children
                self.layout_variable_render_object(render_id, constraints, Some(min))?
            }
            RuntimeArity::Range(_, _) => {
                // Range<MIN, MAX> - between MIN and MAX children
                self.layout_variable_render_object(render_id, constraints, None)?
            }
            RuntimeArity::Never => {
                // Never - impossible, should not happen
                tracing::error!(?render_id, "Never arity should not occur");
                return None;
            }
        };

        tracing::trace!(?element_id, ?size, "Layout computed");

        Some(size)
    }

    /// Layout a Leaf RenderObject (no children).
    ///
    /// This handles RenderObjects with Leaf arity like RenderParagraph.
    fn layout_leaf_render_object(
        &mut self,
        render_id: flui_rendering::tree::RenderId,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        use flui_objects::RenderParagraph;

        // Get the RenderNode
        let render_node = self.render_objects.get_mut(render_id)?;

        // Try to downcast to RenderParagraph (the common Leaf case for Text)
        let render_object = render_node.render_object_mut();

        // Use Any to downcast
        if let Some(paragraph) =
            (render_object as &mut dyn std::any::Any).downcast_mut::<RenderParagraph>()
        {
            // Inline the RenderParagraph layout logic since Leaf doesn't need children
            let data = paragraph.data();

            // Calculate text size (simplified estimation)
            let char_width = data.font_size * 0.6;
            let line_height = data.font_size * 1.2;
            let text_len = data.text.len() as f32;
            let max_width = constraints.max_width;

            // Text wrapping simulation
            let chars_per_line = if data.soft_wrap && max_width.is_finite() {
                (max_width / char_width).max(1.0) as usize
            } else {
                data.text.len()
            };

            let num_lines = if chars_per_line > 0 {
                ((text_len / chars_per_line as f32).ceil() as usize).max(1)
            } else {
                1
            };

            // Apply max_lines constraint
            let actual_lines = if let Some(max_lines) = data.max_lines {
                num_lines.min(max_lines)
            } else {
                num_lines
            };

            // Calculate actual text width (intrinsic size)
            let actual_text_width = (text_len * char_width).min(max_width);

            let width = if data.soft_wrap && max_width.is_finite() && actual_text_width > max_width
            {
                max_width
            } else {
                actual_text_width
            };

            let height = actual_lines as f32 * line_height;

            let size = constraints.constrain(flui_types::Size::new(width, height));

            tracing::trace!(?render_id, ?size, "RenderParagraph layout");

            // Cache the size in RenderNode
            if let Some(node) = self.render_objects.get_mut(render_id) {
                node.set_cached_size(Some(size));
            }

            return Some(size);
        }

        // For other Leaf types, return a default size
        tracing::warn!(?render_id, "Unknown Leaf type, using default 100x100 size");
        Some(flui_types::Size::new(100.0, 100.0))
    }

    /// Layout a Single RenderObject (exactly one child).
    ///
    /// This handles RenderObjects with Single arity like RenderPadding.
    fn layout_single_render_object(
        &mut self,
        render_id: flui_rendering::tree::RenderId,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        use flui_objects::RenderPadding;
        use flui_types::{Offset, Size};

        // Get child RenderId from RenderTree
        let child_render_id = {
            let render_node = self.render_objects.get(render_id)?;
            let children = render_node.children();
            if children.is_empty() {
                tracing::warn!(?render_id, "Single arity RenderObject has no children");
                return None;
            }
            children[0]
        };

        // Try to downcast to RenderPadding
        let render_node = self.render_objects.get_mut(render_id)?;
        let render_object = render_node.render_object_mut();

        if let Some(padding) =
            (render_object as &mut dyn std::any::Any).downcast_mut::<RenderPadding>()
        {
            let edge_insets = padding.padding;

            // Deflate constraints by padding
            let child_constraints = constraints.deflate(&edge_insets);

            // Layout child recursively
            let child_size =
                self.layout_render_object_recursive(child_render_id, child_constraints)?;

            // Trace child offset (padding.left, padding.top)
            // TODO: Store offset for paint phase
            tracing::trace!(
                child = ?child_render_id,
                offset = ?Offset::new(edge_insets.left, edge_insets.top),
                "Child offset set"
            );

            // Add padding to child size
            let size = Size::new(
                child_size.width + edge_insets.horizontal_total(),
                child_size.height + edge_insets.vertical_total(),
            );

            tracing::trace!(?render_id, ?size, "RenderPadding layout");

            // Cache the size in RenderNode
            if let Some(node) = self.render_objects.get_mut(render_id) {
                node.set_cached_size(Some(size));
            }

            return Some(size);
        }

        // Unknown Single arity type - layout child and return its size
        tracing::warn!(
            ?render_id,
            "Unknown Single arity type, passing through to child"
        );
        let child_size = self.layout_render_object_recursive(child_render_id, constraints)?;

        if let Some(node) = self.render_objects.get_mut(render_id) {
            node.set_cached_size(Some(child_size));
        }

        Some(child_size)
    }

    /// Layout an Optional RenderObject (0 or 1 child).
    fn layout_optional_render_object(
        &mut self,
        render_id: flui_rendering::tree::RenderId,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        // Get child RenderId from RenderTree (if exists)
        let child_render_id = {
            let render_node = self.render_objects.get(render_id)?;
            let children = render_node.children();
            children.first().copied()
        };

        if let Some(child_id) = child_render_id {
            // Has child - layout it
            let child_size = self.layout_render_object_recursive(child_id, constraints)?;

            if let Some(node) = self.render_objects.get_mut(render_id) {
                node.set_cached_size(Some(child_size));
            }

            Some(child_size)
        } else {
            // No child - return minimum size
            let size = flui_types::Size::ZERO;

            if let Some(node) = self.render_objects.get_mut(render_id) {
                node.set_cached_size(Some(size));
            }

            Some(size)
        }
    }

    /// Layout a Variable RenderObject (any number of children).
    fn layout_variable_render_object(
        &mut self,
        render_id: flui_rendering::tree::RenderId,
        constraints: flui_types::constraints::BoxConstraints,
        _expected_count: Option<usize>,
    ) -> Option<flui_types::Size> {
        // Get all child RenderIds from RenderTree
        let child_render_ids: Vec<_> = {
            let render_node = self.render_objects.get(render_id)?;
            render_node.children().to_vec()
        };

        // Layout all children and accumulate size
        let mut total_width = 0.0f32;
        let mut max_height = 0.0f32;

        for child_id in child_render_ids {
            if let Some(child_size) = self.layout_render_object_recursive(child_id, constraints) {
                total_width += child_size.width;
                max_height = max_height.max(child_size.height);
            }
        }

        let size = flui_types::Size::new(total_width, max_height);

        if let Some(node) = self.render_objects.get_mut(render_id) {
            node.set_cached_size(Some(size));
        }

        Some(size)
    }

    /// Recursively layout a RenderObject by its RenderId.
    ///
    /// This is the core recursive layout method that dispatches based on
    /// the RenderObject type.
    fn layout_render_object_recursive(
        &mut self,
        render_id: flui_rendering::tree::RenderId,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        use flui_objects::{RenderPadding, RenderParagraph};

        // Get the RenderNode to determine type
        let render_node = self.render_objects.get(render_id)?;
        let children_count = render_node.children().len();
        let render_object = render_node.render_object();

        // Determine arity based on type
        let is_paragraph = (render_object as &dyn std::any::Any)
            .downcast_ref::<RenderParagraph>()
            .is_some();
        let is_padding = (render_object as &dyn std::any::Any)
            .downcast_ref::<RenderPadding>()
            .is_some();

        if is_paragraph {
            // Leaf - no children
            self.layout_leaf_render_object(render_id, constraints)
        } else if is_padding {
            // Single - exactly one child
            self.layout_single_render_object(render_id, constraints)
        } else if children_count == 0 {
            // Unknown leaf type
            self.layout_leaf_render_object(render_id, constraints)
        } else if children_count == 1 {
            // Unknown single child type
            self.layout_single_render_object(render_id, constraints)
        } else {
            // Variable children
            self.layout_variable_render_object(render_id, constraints, None)
        }
    }
}

// ============================================================================
// PAINTING
// ============================================================================

impl TreeCoordinator {
    /// Paints the root element to a new canvas and returns it.
    ///
    /// This method traverses from the root Element to its RenderObject in RenderTree
    /// and calls the paint method to generate a Canvas with draw commands.
    ///
    /// # Returns
    ///
    /// - `Some(Canvas)` if the root element exists and was painted successfully
    /// - `None` if there's no root element or the element has no render object
    ///
    /// # Architecture
    ///
    /// This implements the four-tree paint flow:
    /// 1. Get root ElementId from TreeCoordinator
    /// 2. Get Element from ElementTree
    /// 3. Get RenderId from RenderElement
    /// 4. Get RenderNode from RenderTree
    /// 5. Call paint on the RenderObject
    #[instrument(level = "trace", skip(self))]
    pub fn paint_root(&mut self) -> Option<flui_painting::Canvas> {
        // Get root element
        let root_id = self.root?;
        let element = self.elements.get(root_id)?;

        // Element must be a RenderElement with a render_id
        let render_elem = element.as_render()?;
        let render_id = render_elem.render_id()?;

        // Get RenderNode from RenderTree
        let render_node = self.render_objects.get(render_id)?;

        // Get size from layout (use cached size or default)
        let size = render_node
            .cached_size()
            .unwrap_or(flui_types::Size::new(800.0, 600.0));

        // Create canvas and paint
        let mut canvas = flui_painting::Canvas::new();

        // For now, directly paint using the RenderObject
        // This uses a simplified paint approach for Leaf elements
        self.paint_render_object_to_canvas(render_id, flui_types::Offset::ZERO, size, &mut canvas);

        tracing::trace!(
            ?render_id,
            commands = canvas.display_list().len(),
            "Paint complete"
        );

        Some(canvas)
    }

    /// Paints a single RenderObject to a canvas recursively.
    ///
    /// This method handles both Leaf elements (like RenderParagraph) and
    /// container elements (like RenderPadding) by recursively painting children.
    #[instrument(level = "trace", skip(self, canvas, _size), fields(render = ?render_id))]
    fn paint_render_object_to_canvas(
        &mut self,
        render_id: flui_rendering::tree::RenderId,
        offset: flui_types::Offset,
        _size: flui_types::Size,
        canvas: &mut flui_painting::Canvas,
    ) {
        use flui_objects::{RenderPadding, RenderParagraph};
        use flui_painting::Paint;
        use flui_types::typography::TextStyle;

        // Get the RenderNode
        let Some(render_node) = self.render_objects.get(render_id) else {
            tracing::warn!(?render_id, "RenderNode not found during paint");
            return;
        };

        // Get children before borrowing render_object
        let children: Vec<_> = render_node.children().to_vec();
        let render_object = render_node.render_object();

        // Try RenderParagraph first (Leaf - no children)
        if let Some(paragraph) =
            (render_object as &dyn std::any::Any).downcast_ref::<RenderParagraph>()
        {
            let data = paragraph.data();
            let paint = Paint {
                color: data.color,
                ..Default::default()
            };

            let text_style = TextStyle::default()
                .with_font_size(data.font_size as f64)
                .with_color(data.color);

            canvas.draw_text(&data.text, offset, &text_style, &paint);
            tracing::trace!(text = %data.text, "Text painted");
            return;
        }

        // Try RenderPadding (Single child)
        if let Some(padding) = (render_object as &dyn std::any::Any).downcast_ref::<RenderPadding>()
        {
            let edge_insets = padding.padding;

            // Paint child with offset adjusted by padding
            if let Some(&child_id) = children.first() {
                let child_offset =
                    offset + flui_types::Offset::new(edge_insets.left, edge_insets.top);
                let child_size = self
                    .render_objects
                    .get(child_id)
                    .and_then(|n| n.cached_size())
                    .unwrap_or(flui_types::Size::ZERO);

                self.paint_render_object_to_canvas(child_id, child_offset, child_size, canvas);
            }

            tracing::trace!(?render_id, "RenderPadding painted");
            return;
        }

        // Unknown type - try to paint children anyway
        if !children.is_empty() {
            tracing::debug!(
                ?render_id,
                children_count = children.len(),
                "Painting unknown container type"
            );
            for child_id in children {
                let child_size = self
                    .render_objects
                    .get(child_id)
                    .and_then(|n| n.cached_size())
                    .unwrap_or(flui_types::Size::ZERO);
                self.paint_render_object_to_canvas(child_id, offset, child_size, canvas);
            }
        } else {
            tracing::warn!(?render_id, "Unknown RenderObject type, skipping paint");
        }
    }
}

// ============================================================================
// STATISTICS
// ============================================================================

impl TreeCoordinator {
    /// Returns the number of elements in the tree.
    #[inline]
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Returns the number of view objects in the tree.
    #[inline]
    pub fn view_count(&self) -> usize {
        self.views.len()
    }

    /// Returns the number of render objects in the tree.
    #[inline]
    pub fn render_object_count(&self) -> usize {
        self.render_objects.len()
    }

    /// Returns the number of layers in the tree.
    #[inline]
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_creation() {
        let coordinator = TreeCoordinator::new();

        assert!(coordinator.views().is_empty());
        assert!(coordinator.elements().is_empty());
        assert!(coordinator.render_objects().is_empty());
        assert!(coordinator.layers().is_empty());
        assert_eq!(coordinator.root(), None);
    }

    #[test]
    fn test_with_capacity() {
        let coordinator = TreeCoordinator::with_capacity(100);

        assert!(coordinator.views().is_empty());
        assert_eq!(coordinator.root(), None);
    }

    #[test]
    fn test_dirty_tracking() {
        let mut coordinator = TreeCoordinator::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);

        // Mark elements dirty
        coordinator.mark_needs_build(id1);
        coordinator.mark_needs_layout(id2);

        assert!(coordinator.needs_build().contains(&id1));
        assert!(coordinator.needs_layout().contains(&id2));
        assert!(coordinator.needs_paint().contains(&id2)); // layout implies paint
        assert!(coordinator.has_dirty_elements());

        // Clear dirty
        coordinator.clear_dirty();
        assert!(!coordinator.has_dirty_elements());
    }

    #[test]
    fn test_take_needs_build() {
        let mut coordinator = TreeCoordinator::new();

        let id1 = ElementId::new(1);
        let id2 = ElementId::new(2);

        coordinator.mark_needs_build(id1);
        coordinator.mark_needs_build(id2);

        assert!(coordinator.has_needs_build());

        let dirty = coordinator.take_needs_build();
        assert_eq!(dirty.len(), 2);
        assert!(dirty.contains(&id1));
        assert!(dirty.contains(&id2));

        // After take, set should be empty
        assert!(!coordinator.has_needs_build());
    }

    #[test]
    fn test_root_management() {
        let mut coordinator = TreeCoordinator::new();

        assert_eq!(coordinator.root(), None);

        let root_id = ElementId::new(1);
        coordinator.set_root(Some(root_id));

        assert_eq!(coordinator.root(), Some(root_id));
    }

    #[test]
    fn test_tree_access() {
        let mut coordinator = TreeCoordinator::new();

        // Mutable access
        let _views = coordinator.views_mut();
        let _elements = coordinator.elements_mut();
        let _render_objects = coordinator.render_objects_mut();
        let _layers = coordinator.layers_mut();

        // Immutable access
        let _views = coordinator.views();
        let _elements = coordinator.elements();
        let _render_objects = coordinator.render_objects();
        let _layers = coordinator.layers();
    }

    #[test]
    fn test_into_trees() {
        let coordinator = TreeCoordinator::new();

        let (views, elements, render_objects, layers) = coordinator.into_trees();

        assert!(views.is_empty());
        assert!(elements.is_empty());
        assert!(render_objects.is_empty());
        assert!(layers.is_empty());
    }

    #[test]
    fn test_statistics() {
        let coordinator = TreeCoordinator::new();

        assert_eq!(coordinator.element_count(), 0);
        assert_eq!(coordinator.view_count(), 0);
        assert_eq!(coordinator.render_object_count(), 0);
        assert_eq!(coordinator.layer_count(), 0);
    }

    #[test]
    fn test_default() {
        let coordinator = TreeCoordinator::default();
        assert!(coordinator.views().is_empty());
    }
}
