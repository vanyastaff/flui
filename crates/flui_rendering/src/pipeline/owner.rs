//! PipelineOwner manages the rendering pipeline.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::traits::RenderObject;

// ============================================================================
// PipelineOwner
// ============================================================================

/// Manages the rendering pipeline for a tree of render objects.
///
/// The pipeline owner:
/// - Stores the root render object
/// - Tracks dirty nodes needing layout/paint/semantics
/// - Coordinates flush operations for each phase
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `PipelineOwner` class in
/// `rendering/object.dart`.
///
/// # Pipeline Phases
///
/// Call these methods in order during each frame:
///
/// 1. [`flush_layout`](Self::flush_layout) - Update layout
/// 2. [`flush_compositing_bits`](Self::flush_compositing_bits) - Update layer needs
/// 3. [`flush_paint`](Self::flush_paint) - Generate paint commands
/// 4. [`flush_semantics`](Self::flush_semantics) - Update accessibility tree
pub struct PipelineOwner {
    /// The root render object of this pipeline.
    root_node: Option<Arc<RwLock<dyn RenderObject>>>,

    /// Callback when visual update is needed.
    #[allow(clippy::type_complexity)]
    on_need_visual_update: Option<Box<dyn Fn() + Send + Sync>>,

    /// Nodes needing layout.
    nodes_needing_layout: Vec<usize>,

    /// Nodes needing compositing bits update.
    nodes_needing_compositing_bits_update: Vec<usize>,

    /// Nodes needing paint.
    nodes_needing_paint: Vec<usize>,

    /// Nodes needing semantics update.
    nodes_needing_semantics: Vec<usize>,

    /// Whether we're currently doing layout.
    debug_doing_layout: bool,

    /// Whether we're currently doing paint.
    debug_doing_paint: bool,
}

impl std::fmt::Debug for PipelineOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineOwner")
            .field("root_node", &self.root_node.is_some())
            .field("nodes_needing_layout", &self.nodes_needing_layout.len())
            .field("nodes_needing_paint", &self.nodes_needing_paint.len())
            .field("debug_doing_layout", &self.debug_doing_layout)
            .field("debug_doing_paint", &self.debug_doing_paint)
            .finish()
    }
}

impl Default for PipelineOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineOwner {
    /// Creates a new pipeline owner.
    pub fn new() -> Self {
        Self {
            root_node: None,
            on_need_visual_update: None,
            nodes_needing_layout: Vec::new(),
            nodes_needing_compositing_bits_update: Vec::new(),
            nodes_needing_paint: Vec::new(),
            nodes_needing_semantics: Vec::new(),
            debug_doing_layout: false,
            debug_doing_paint: false,
        }
    }

    /// Sets the callback for when a visual update is needed.
    pub fn set_on_need_visual_update<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_need_visual_update = Some(Box::new(callback));
    }

    /// Requests a visual update.
    ///
    /// Called by render objects when they need to be re-rendered.
    pub fn request_visual_update(&self) {
        if let Some(callback) = &self.on_need_visual_update {
            callback();
        }
    }

    /// Returns the root render object.
    pub fn root_node(&self) -> Option<&Arc<RwLock<dyn RenderObject>>> {
        self.root_node.as_ref()
    }

    /// Sets the root render object.
    pub fn set_root_node(&mut self, node: Option<Arc<RwLock<dyn RenderObject>>>) {
        self.root_node = node;
    }

    // ========================================================================
    // Layout Phase
    // ========================================================================

    /// Updates layout for all dirty render objects.
    ///
    /// This is phase 1 of the rendering pipeline. During layout:
    /// - Sizes and positions are calculated
    /// - Objects may dirty paint or compositing
    pub fn flush_layout(&mut self) {
        tracing::debug!("flush_layout: {} nodes", self.nodes_needing_layout.len());

        self.debug_doing_layout = true;

        // Sort by depth (shallow first)
        self.nodes_needing_layout.sort_unstable();

        // Process dirty nodes
        // TODO: Implement actual layout flushing
        self.nodes_needing_layout.clear();

        self.debug_doing_layout = false;
    }

    // ========================================================================
    // Compositing Bits Phase
    // ========================================================================

    /// Updates compositing bits for all dirty render objects.
    ///
    /// This is phase 2 of the rendering pipeline. During this phase:
    /// - Each object determines if it needs a compositing layer
    /// - This information is used during paint
    pub fn flush_compositing_bits(&mut self) {
        tracing::debug!(
            "flush_compositing_bits: {} nodes",
            self.nodes_needing_compositing_bits_update.len()
        );

        // Sort by depth (deep first for bottom-up propagation)
        self.nodes_needing_compositing_bits_update
            .sort_unstable_by(|a, b| b.cmp(a));

        // TODO: Implement actual compositing bits flushing
        self.nodes_needing_compositing_bits_update.clear();
    }

    // ========================================================================
    // Paint Phase
    // ========================================================================

    /// Paints all dirty render objects.
    ///
    /// This is phase 3 of the rendering pipeline. During paint:
    /// - Render objects record paint commands
    /// - Compositing layers are built
    pub fn flush_paint(&mut self) {
        tracing::debug!("flush_paint: {} nodes", self.nodes_needing_paint.len());

        self.debug_doing_paint = true;

        // TODO: Implement actual paint flushing
        self.nodes_needing_paint.clear();

        self.debug_doing_paint = false;
    }

    // ========================================================================
    // Semantics Phase
    // ========================================================================

    /// Updates semantics for all dirty render objects.
    ///
    /// This is phase 4 of the rendering pipeline. During semantics:
    /// - Accessibility information is gathered
    /// - Semantics tree is updated
    pub fn flush_semantics(&mut self) {
        tracing::debug!(
            "flush_semantics: {} nodes",
            self.nodes_needing_semantics.len()
        );

        // TODO: Implement actual semantics flushing
        self.nodes_needing_semantics.clear();
    }

    // ========================================================================
    // Debug
    // ========================================================================

    /// Returns whether layout is currently being performed.
    pub fn debug_doing_layout(&self) -> bool {
        self.debug_doing_layout
    }

    /// Returns whether paint is currently being performed.
    pub fn debug_doing_paint(&self) -> bool {
        self.debug_doing_paint
    }
}
