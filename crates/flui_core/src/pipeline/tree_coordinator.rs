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
//!   ├── views: ViewTree
//!   ├── elements: ElementTree
//!   ├── render_objects: RenderTree
//!   └── layers: LayerTree
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
//! let element = ViewElement::new(Some(view_id), ViewMode::Stateless);
//! let element_id = coordinator.elements_mut().insert(element);
//!
//! // Perform build/layout/paint cycle
//! coordinator.build_dirty_elements()?;
//! coordinator.layout_dirty_render_objects()?;
//! coordinator.paint_dirty_layers()?;
//! ```

use std::collections::HashSet;

use flui_foundation::ElementId;
use flui_engine::tree::LayerTree;
use flui_rendering::tree::RenderTree;
use flui_view::tree::ViewTree;

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
pub struct TreeCoordinator<E> {
    // ========== Four Trees ==========
    /// ViewTree - stores immutable ViewObjects
    views: ViewTree,

    /// ElementTree - stores Elements (can be generic over storage type)
    elements: E,

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

impl<E> TreeCoordinator<E> {
    /// Creates a new TreeCoordinator with the given element storage.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let element_tree = ElementTree::new();
    /// let coordinator = TreeCoordinator::new(element_tree);
    /// ```
    pub fn new(elements: E) -> Self {
        Self {
            views: ViewTree::new(),
            elements,
            render_objects: RenderTree::new(),
            layers: LayerTree::new(),
            needs_build: HashSet::new(),
            needs_layout: HashSet::new(),
            needs_paint: HashSet::new(),
            needs_compositing: HashSet::new(),
            root: None,
        }
    }

    /// Creates a TreeCoordinator with pre-allocated capacity for dirty sets.
    pub fn with_capacity(elements: E, capacity: usize) -> Self {
        Self {
            views: ViewTree::with_capacity(capacity),
            elements,
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

// ============================================================================
// TREE ACCESS
// ============================================================================

impl<E> TreeCoordinator<E> {
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
    pub fn elements(&self) -> &E {
        &self.elements
    }

    /// Returns a mutable reference to the ElementTree.
    #[inline]
    pub fn elements_mut(&mut self) -> &mut E {
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
    pub fn into_trees(self) -> (ViewTree, E, RenderTree, LayerTree) {
        (self.views, self.elements, self.render_objects, self.layers)
    }
}

// ============================================================================
// ROOT MANAGEMENT
// ============================================================================

impl<E> TreeCoordinator<E> {
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
}

// ============================================================================
// DIRTY TRACKING (Flutter PipelineOwner pattern)
// ============================================================================

impl<E> TreeCoordinator<E> {
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
        // Layout changes require repaint
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
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Mock ElementTree for testing
    #[derive(Debug)]
    struct MockElementTree;

    #[test]
    fn test_coordinator_creation() {
        let coordinator = TreeCoordinator::new(MockElementTree);

        assert!(coordinator.views().is_empty());
        assert!(coordinator.render_objects().is_empty());
        assert!(coordinator.layers().is_empty());
        assert_eq!(coordinator.root(), None);
    }

    #[test]
    fn test_dirty_tracking() {
        let mut coordinator = TreeCoordinator::new(MockElementTree);

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
    fn test_root_management() {
        let mut coordinator = TreeCoordinator::new(MockElementTree);

        assert_eq!(coordinator.root(), None);

        let root_id = ElementId::new(1);
        coordinator.set_root(Some(root_id));

        assert_eq!(coordinator.root(), Some(root_id));
    }

    #[test]
    fn test_tree_access() {
        let mut coordinator = TreeCoordinator::new(MockElementTree);

        // Mutable access
        let _views = coordinator.views_mut();
        let _render_objects = coordinator.render_objects_mut();
        let _layers = coordinator.layers_mut();

        // Immutable access
        let _views = coordinator.views();
        let _render_objects = coordinator.render_objects();
        let _layers = coordinator.layers();
    }

    #[test]
    fn test_into_trees() {
        let coordinator = TreeCoordinator::new(MockElementTree);

        let (views, _elements, render_objects, layers) = coordinator.into_trees();

        assert!(views.is_empty());
        assert!(render_objects.is_empty());
        assert!(layers.is_empty());
    }
}
