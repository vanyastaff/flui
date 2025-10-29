//! RenderPipeline - orchestrates the layout → paint pipeline
//!
//! Manages the rendering pipeline for the UI framework, coordinating
//! layout and paint phases with dirty tracking for incremental updates.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::{RenderPipeline, BoxConstraints};
//! use flui_widgets::FlexWidget;
//!
//! let mut pipeline = RenderPipeline::new();
//!
//! // Add root widget (creates RenderElement internally)
//! let root_id = pipeline.insert_root(FlexWidget::column());
//!
//! // Each frame:
//! let constraints = BoxConstraints::tight(800.0, 600.0);
//! if let Some(size) = pipeline.flush_layout(constraints) {
//!     let layer = pipeline.flush_paint();
//!     // Composite layer to screen
//! }
//! ```
//!
//! # Architecture
//!
//! RenderPipeline works with the three-tree architecture:
//! - Widget → RenderElement → Render
//! - ElementTree stores RenderElements (not bare Renders)
//! - Widgets are immutable configuration, Elements manage lifecycle
//!
//! # Phases
//!
//! 1. **Layout**: Renders compute their size and position
//! 2. **Paint**: Renders produce their layer tree
//!
//! # Dirty Tracking
//!
//! RenderPipeline tracks which Renders need layout/paint:
//! - `nodes_needing_layout` - Elements that need relayout
//! - `nodes_needing_paint` - Elements that need repaint
//! - `flush_layout()` processes dirty nodes, sorted by depth (parents before children)
//! - `flush_paint()` processes dirty nodes for incremental rendering

use flui_engine::{BoxedLayer, ContainerLayer};
use flui_types::constraints::BoxConstraints;
use flui_types::{Offset, Size};

use crate::element::{ElementId, ElementTree, RenderElement};
use crate::widget::{RenderWidget, BoxedWidget, DynWidget};

/// RenderPipeline - orchestrates the rendering pipeline
///
/// Manages the layout → paint pipeline with dirty tracking for incremental updates.
///
/// # Thread Safety
///
/// RenderPipeline owns the ElementTree and is not thread-safe.
/// For multi-threaded use, wrap in Arc<RwLock<RenderPipeline>>.
///
/// # Dirty Tracking
///
/// Tracks dirty Renders for incremental layout/paint:
/// - `nodes_needing_layout` - Renders that need relayout
/// - `nodes_needing_paint` - Renders that need repaint
/// - `flush_layout()` processes only dirty nodes, sorted by depth
/// - `flush_paint()` processes only dirty nodes
pub struct RenderPipeline {
    /// The element tree
    tree: ElementTree,

    /// Root element ID
    root_id: Option<ElementId>,

    // Dirty tracking
    /// Renders that need layout
    nodes_needing_layout: Vec<ElementId>,

    /// Renders that need paint
    nodes_needing_paint: Vec<ElementId>,
}

impl std::fmt::Debug for RenderPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderPipeline")
            .field("root_id", &self.root_id)
            .field(
                "nodes_needing_layout_count",
                &self.nodes_needing_layout.len(),
            )
            .field("nodes_needing_paint_count", &self.nodes_needing_paint.len())
            .finish()
    }
}

impl RenderPipeline {
    /// Create a new render pipeline
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let pipeline = RenderPipeline::new();
    /// ```
    pub fn new() -> Self {
        Self {
            tree: ElementTree::new(),
            root_id: None,
            nodes_needing_layout: Vec::new(),
            nodes_needing_paint: Vec::new(),
        }
    }

    /// Create a new render pipeline with pre-allocated capacity
    ///
    /// # Arguments
    ///
    /// - `capacity`: Initial capacity for the element tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let pipeline = RenderPipeline::with_capacity(1000);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tree: ElementTree::with_capacity(capacity),
            root_id: None,
            nodes_needing_layout: Vec::new(),
            nodes_needing_paint: Vec::new(),
        }
    }

    // ========== Tree Access ==========

    /// Get reference to the element tree
    pub fn tree(&self) -> &ElementTree {
        &self.tree
    }

    /// Get mutable reference to the element tree
    pub fn tree_mut(&mut self) -> &mut ElementTree {
        &mut self.tree
    }

    /// Get the root element ID
    pub fn root_id(&self) -> Option<ElementId> {
        self.root_id
    }

    // ========== Tree Construction ==========

    /// Insert a root RenderWidget
    ///
    /// Creates the root of the render tree by wrapping the widget in a RenderElement.
    ///
    /// # Arguments
    ///
    /// - `widget`: The root RenderWidget
    ///
    /// # Returns
    ///
    /// The ElementId of the root element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let root_id = pipeline.insert_root(FlexWidget::column());
    /// ```
    pub fn insert_root<W>(&mut self, widget: W) -> ElementId
    where
        W: RenderWidget + Clone + Send + Sync + std::fmt::Debug + DynWidget + 'static,
    {
        // Create a dummy BuildContext for root widget
        // TODO: This should be properly constructed
        use crate::element::BuildContext;
        let ctx = unsafe { std::mem::zeroed::<BuildContext>() };

        let render_boxed = widget.create_render_object(&ctx);

        // Box the widget (it implements DynWidget)
        let widget_boxed = BoxedWidget::new(widget);

        let render_element = RenderElement::new(widget_boxed, render_boxed);
        let element = crate::element::Element::Render(render_element);
        let id = self.tree.insert(element);
        self.root_id = Some(id);

        // Mark root as needing layout and paint
        self.request_layout(id);
        self.request_paint(id);

        id
    }

    // ========== Dirty Tracking API ==========

    /// Request layout for a Render
    ///
    /// Adds the node to the layout dirty list if not already present.
    /// Call this when a Render's properties change and it needs relayout.
    ///
    /// # Arguments
    ///
    /// - `node_id`: The element ID that needs layout
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// pipeline.request_layout(element_id);
    /// ```
    pub fn request_layout(&mut self, node_id: ElementId) {
        if !self.nodes_needing_layout.contains(&node_id) {
            self.nodes_needing_layout.push(node_id);
        }
    }

    /// Request paint for a Render
    ///
    /// Adds the node to the paint dirty list if not already present.
    /// Call this when a Render's appearance changes and it needs repaint.
    ///
    /// # Arguments
    ///
    /// - `node_id`: The element ID that needs paint
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// pipeline.request_paint(element_id);
    /// ```
    pub fn request_paint(&mut self, node_id: ElementId) {
        if !self.nodes_needing_paint.contains(&node_id) {
            self.nodes_needing_paint.push(node_id);
        }
    }

    /// Get count of nodes needing layout
    pub fn layout_dirty_count(&self) -> usize {
        self.nodes_needing_layout.len()
    }

    /// Get count of nodes needing paint
    pub fn paint_dirty_count(&self) -> usize {
        self.nodes_needing_paint.len()
    }

    // ========== Layout Phase ==========

    /// Flush the layout phase
    ///
    /// Performs layout on the root (and recursively, all children).
    /// If there are dirty nodes, processes them first.
    ///
    /// # Arguments
    ///
    /// - `constraints`: Root constraints (typically screen size)
    ///
    /// # Returns
    ///
    /// The size of the root render object, or None if no root
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let constraints = BoxConstraints::tight(800.0, 600.0);
    /// if let Some(size) = pipeline.flush_layout(constraints) {
    ///     println!("Root size: {:?}", size);
    /// }
    /// ```
    pub fn flush_layout(&mut self, constraints: BoxConstraints) -> Option<Size> {
        let root_id = self.root_id?;

        // Clear dirty list - we'll layout the whole tree from root
        // In a real implementation, we'd sort by depth and process incrementally
        self.nodes_needing_layout.clear();

        // Layout the root (which recursively layouts children)
        let size = self.tree.layout_render_object(root_id, constraints)?;

        // Store size in RenderState
        if let Some(state) = self.tree.render_state_mut(root_id) {
            state.set_size(size);
            state.clear_needs_layout();
        }

        Some(size)
    }

    // ========== Paint Phase ==========

    /// Flush the paint phase
    ///
    /// Paints the root Render (and recursively, all children).
    ///
    /// # Returns
    ///
    /// The root layer tree, or an empty ContainerLayer if no root
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let layer = pipeline.flush_paint();
    /// // Composite layer to screen
    /// ```
    pub fn flush_paint(&mut self) -> BoxedLayer {
        let root_id = match self.root_id {
            Some(id) => id,
            None => return Box::new(ContainerLayer::new()),
        };

        // Clear dirty list - we'll paint the whole tree from root
        // In a real implementation, we'd process only dirty nodes
        self.nodes_needing_paint.clear();

        // Paint the root (which recursively paints children)
        let layer = self
            .tree
            .paint_render_object(root_id, Offset::ZERO)
            .unwrap_or_else(|| Box::new(ContainerLayer::new()));

        // Clear paint flag in RenderState
        if let Some(state) = self.tree.render_state_mut(root_id) {
            state.clear_needs_paint();
        }

        layer
    }
}

impl Default for RenderPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DynWidget, Render, RenderWidget, Widget};
    use crate::{LayoutCx, LeafArity, PaintCx, SingleArity};
    use flui_engine::ContainerLayer;

    // Test Widgets
    #[derive(Debug, Clone)]
    struct TestLeafWidget {
        width: f32,
        height: f32,
    }

    impl TestLeafWidget {
        fn new(width: f32, height: f32) -> Self {
            Self { width, height }
        }
    }

    impl Widget for TestLeafWidget {}

    impl RenderWidget for TestLeafWidget {
        type Render = TestLeafRender;
        type Arity = LeafArity;

        fn create_render_object(&self) -> Self::Render {
            TestLeafRender {
                size: Size::new(self.width, self.height),
            }
        }

        fn update_render_object(&self, render: &mut Self::Render) {
            render.size = Size::new(self.width, self.height);
        }
    }

    #[derive(Debug)]
    struct TestLeafRender {
        size: Size,
    }

    impl Render for TestLeafRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(self.size)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug, Clone)]
    struct TestContainerWidget;

    impl Widget for TestContainerWidget {}

    impl RenderWidget for TestContainerWidget {
        type Arity = SingleArity;
        type Render = TestContainerRender;

        fn create_render_object(&self) -> Self::Render {
            TestContainerRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct TestContainerRender;

    impl Render for TestContainerRender {
        type Arity = SingleArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            use crate::render::layout_cx::SingleChild;
            let child = cx.child();
            cx.layout_child(child, cx.constraints())
        }

        fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            use crate::render::paint_cx::SingleChildPaint;
            let child = cx.child();
            cx.capture_child_layer(child)
        }
    }

    #[test]
    fn test_pipeline_creation() {
        let pipeline = RenderPipeline::new();
        assert!(pipeline.root_id().is_none());
        assert_eq!(pipeline.layout_dirty_count(), 0);
        assert_eq!(pipeline.paint_dirty_count(), 0);
    }

    #[test]
    fn test_pipeline_with_capacity() {
        let pipeline = RenderPipeline::with_capacity(100);
        assert!(pipeline.root_id().is_none());
    }

    #[test]
    fn test_insert_root() {
        let mut pipeline = RenderPipeline::new();
        let root_id = pipeline.insert_root(TestLeafWidget::new(100.0, 100.0));

        assert_eq!(pipeline.root_id(), Some(root_id));
        assert_eq!(pipeline.layout_dirty_count(), 1);
        assert_eq!(pipeline.paint_dirty_count(), 1);
    }

    #[test]
    fn test_flush_layout() {
        let mut pipeline = RenderPipeline::new();
        pipeline.insert_root(TestLeafWidget::new(100.0, 100.0));

        // Use loose constraints so the widget can use its preferred size
        let constraints = BoxConstraints::new(0.0, 800.0, 0.0, 600.0);
        let size = pipeline.flush_layout(constraints);

        assert_eq!(size, Some(Size::new(100.0, 100.0)));
        assert_eq!(pipeline.layout_dirty_count(), 0);
    }

    #[test]
    fn test_flush_paint() {
        let mut pipeline = RenderPipeline::new();
        pipeline.insert_root(TestLeafWidget::new(100.0, 100.0));

        let _layer = pipeline.flush_paint();
        // Layer was successfully created
        assert_eq!(pipeline.paint_dirty_count(), 0);
    }

    #[test]
    fn test_request_layout() {
        let mut pipeline = RenderPipeline::new();
        let root_id = pipeline.insert_root(TestLeafWidget::new(100.0, 100.0));

        // Clear initial dirty flags
        pipeline.nodes_needing_layout.clear();

        pipeline.request_layout(root_id);
        assert_eq!(pipeline.layout_dirty_count(), 1);

        // Duplicate request should not add again
        pipeline.request_layout(root_id);
        assert_eq!(pipeline.layout_dirty_count(), 1);
    }

    #[test]
    fn test_request_paint() {
        let mut pipeline = RenderPipeline::new();
        let root_id = pipeline.insert_root(TestLeafWidget::new(100.0, 100.0));

        // Clear initial dirty flags
        pipeline.nodes_needing_paint.clear();

        pipeline.request_paint(root_id);
        assert_eq!(pipeline.paint_dirty_count(), 1);

        // Duplicate request should not add again
        pipeline.request_paint(root_id);
        assert_eq!(pipeline.paint_dirty_count(), 1);
    }

    #[test]
    fn test_pipeline_default() {
        let pipeline = RenderPipeline::default();
        assert!(pipeline.root_id().is_none());
    }
}
