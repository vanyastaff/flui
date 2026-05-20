//! Retained-layer tracking for [].
//!
//! Extracted from `compositor.rs` in Mythos Step 10. The compositor
//! tracks retained layer subtrees across frames -- separate concern
//! from `SceneBuilder` construction logic.

use flui_foundation::LayerId;

use crate::tree::LayerTree;

// ============================================================================
// SCENE COMPOSITOR
// ============================================================================

/// High-level compositor for managing multiple scenes.
///
/// SceneCompositor provides utilities for compositing multiple layer trees,
/// managing retained layers, and optimizing layer reuse.
#[derive(Debug, Default)]
pub struct SceneCompositor {
    /// Retained layer roots from previous frames
    retained: Vec<LayerId>,

    /// Statistics for debugging
    stats: CompositorStats,
}

/// Statistics about compositor operations.
#[derive(Debug, Default, Clone, Copy)]
pub struct CompositorStats {
    /// Number of layers created this frame
    pub layers_created: usize,

    /// Number of retained layers reused
    pub layers_reused: usize,

    /// Number of layers removed
    pub layers_removed: usize,

    /// Current total layer count
    pub total_layers: usize,
}

impl SceneCompositor {
    /// Creates a new SceneCompositor.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns current compositor statistics.
    pub fn stats(&self) -> CompositorStats {
        self.stats
    }

    /// Resets statistics for a new frame.
    pub fn reset_stats(&mut self) {
        self.stats = CompositorStats::default();
    }

    /// Marks a layer subtree for retention.
    ///
    /// Retained layers can be reused across frames without rebuilding.
    pub fn retain(&mut self, layer_id: LayerId) {
        if !self.retained.contains(&layer_id) {
            self.retained.push(layer_id);
        }
    }

    /// Returns all retained layer IDs.
    pub fn retained_layers(&self) -> &[LayerId] {
        &self.retained
    }

    /// Checks if a layer is retained.
    pub fn is_retained(&self, layer_id: LayerId) -> bool {
        self.retained.contains(&layer_id)
    }

    /// Clears all retained layers.
    pub fn clear_retained(&mut self) {
        self.retained.clear();
    }

    /// Removes a layer from retention.
    pub fn release(&mut self, layer_id: LayerId) {
        self.retained.retain(|&id| id != layer_id);
    }

    /// Updates statistics after frame composition.
    pub fn update_stats(&mut self, tree: &LayerTree) {
        self.stats.total_layers = tree.len();
    }
}

