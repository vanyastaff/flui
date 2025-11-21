//! Pipeline trait - abstraction for dependency inversion
//!
//! This trait provides an abstract interface to the rendering pipeline,
//! enabling dependency inversion, mock testing, and alternative implementations.
//!
//! # Design Rationale
//!
//! Following SOLID principles, high-level modules (bindings, embedders) should
//! depend on abstractions (traits), not concrete implementations. This trait
//! allows:
//!
//! - **Testing**: Mock implementations for unit tests
//! - **Flexibility**: Multiple pipeline implementations (recording, debugging, etc.)
//! - **Decoupling**: Bindings don't depend on PipelineOwner directly
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::pipeline::{Pipeline, PipelineOwner};
//! use parking_lot::RwLock;
//! use std::sync::Arc;
//!
//! // Production: Use real PipelineOwner
//! let owner = Arc::new(RwLock::new(PipelineOwner::new()));
//! let pipeline: Arc<dyn Pipeline> = owner;
//!
//! // Testing: Use MockPipeline from testing module
//! #[cfg(test)]
//! use flui_core::testing::MockPipeline;
//! let mock: Arc<dyn Pipeline> = Arc::new(MockPipeline::new());
//! ```

use super::{ElementTree, PipelineError, RebuildQueue};
use crate::element::{Element, ElementId};
use flui_engine::CanvasLayer;
use flui_types::{constraints::BoxConstraints, Size};
use parking_lot::RwLock;
use std::sync::Arc;

/// Pipeline trait - abstract interface for rendering pipeline
///
/// # Thread-Safety
///
/// All methods take `&self` (not `&mut self`) because the trait is typically
/// used behind `Arc<dyn Pipeline>`. Implementations handle interior mutability
/// as needed (e.g., via RwLock).
///
/// # Core Methods
///
/// The trait exposes only the **essential** pipeline operations needed by
/// bindings and embedders:
///
/// - **Tree Management**: `tree()`, `root_element_id()`
/// - **Widget Lifecycle**: `attach()`, `set_root()`
/// - **Rebuild Scheduling**: `rebuild_queue()`, `schedule_build_for()`
/// - **Pipeline Phases**: `flush_build()`, `flush_layout()`, `flush_paint()`
/// - **Complete Frame**: `build_frame()` (all phases)
/// - **Dirty Tracking**: `request_layout()`, `request_paint()`
///
/// # Optional Features
///
/// Optional production features (metrics, recovery, caching) are accessed
/// via `as_pipeline_owner()` for backward compatibility.
pub trait Pipeline: Send + Sync {
    // =========================================================================
    // Tree & Root Management
    // =========================================================================

    /// Get shared reference to element tree
    ///
    /// The tree is wrapped in `Arc<RwLock<>>` for thread-safe access.
    fn tree(&self) -> Arc<RwLock<ElementTree>>;

    /// Get the root element ID (if any)
    fn root_element_id(&self) -> Option<ElementId>;

    /// Mount an element as the root of the tree
    ///
    /// Returns the ElementId assigned to the root.
    /// Automatically schedules the root for initial build.
    fn set_root(&self, root_element: Element) -> ElementId;

    // Note: attach<V: View>() is NOT in the trait because generic methods
    // make traits non-dyn-compatible. Use the concrete method on PipelineOwner
    // or convert View → Element first, then call set_root().

    // =========================================================================
    // Rebuild Scheduling
    // =========================================================================

    /// Get reference to rebuild queue
    ///
    /// Used by signals and hooks to schedule component rebuilds.
    fn rebuild_queue(&self) -> Arc<RebuildQueue>;

    /// Schedule an element for rebuild
    ///
    /// # Parameters
    ///
    /// - `element_id`: Element to rebuild
    /// - `depth`: Tree depth (for ordering)
    fn schedule_build_for(&self, element_id: ElementId, depth: usize);

    // =========================================================================
    // Pipeline Phases (Individual)
    // =========================================================================

    /// Flush the build phase
    ///
    /// Rebuilds all dirty ComponentElements by calling `View::build()`.
    fn flush_build(&self);

    /// Flush the layout phase
    ///
    /// Computes sizes and positions for all dirty RenderElements.
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root layout constraints (typically tight constraints matching window size)
    ///
    /// # Returns
    ///
    /// - `Ok(Some(size))`: Layout succeeded, returns root size
    /// - `Ok(None)`: No root element or tree is empty
    /// - `Err(e)`: Layout error occurred
    fn flush_layout(&self, constraints: BoxConstraints) -> Result<Option<Size>, PipelineError>;

    /// Flush the paint phase
    ///
    /// Generates layer tree for all dirty RenderElements.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(layer))`: Paint succeeded, returns root layer
    /// - `Ok(None)`: No root element or tree is empty
    /// - `Err(e)`: Paint error occurred
    fn flush_paint(&self) -> Result<Option<Box<CanvasLayer>>, PipelineError>;

    // =========================================================================
    // Complete Frame (All Phases)
    // =========================================================================

    /// Execute complete rendering pipeline
    ///
    /// This is the **main entry point** for frame rendering. It executes all
    /// three phases in order:
    ///
    /// 1. **Build**: Rebuild dirty components (flush_build)
    /// 2. **Layout**: Compute sizes (flush_layout)
    /// 3. **Paint**: Generate layers (flush_paint)
    ///
    /// # Parameters
    ///
    /// - `constraints`: Root layout constraints
    ///
    /// # Returns
    ///
    /// - `Ok(Some(layer))`: Frame rendered successfully
    /// - `Ok(None)`: No root element or tree is empty
    /// - `Err(e)`: Pipeline error occurred
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
    /// match pipeline.build_frame(constraints) {
    ///     Ok(Some(layer)) => {
    ///         // Render layer to GPU
    ///         renderer.render(&layer);
    ///     }
    ///     Ok(None) => {
    ///         // Empty tree, nothing to render
    ///     }
    ///     Err(e) => {
    ///         eprintln!("Pipeline error: {:?}", e);
    ///     }
    /// }
    /// ```
    fn build_frame(
        &self,
        constraints: BoxConstraints,
    ) -> Result<Option<Box<CanvasLayer>>, PipelineError>;

    // =========================================================================
    // Dirty Tracking
    // =========================================================================

    /// Mark a RenderElement as needing layout
    ///
    /// # Parameters
    ///
    /// - `node_id`: RenderElement to mark dirty
    fn request_layout(&self, node_id: ElementId);

    /// Mark a RenderElement as needing paint
    ///
    /// # Parameters
    ///
    /// - `node_id`: RenderElement to mark dirty
    fn request_paint(&self, node_id: ElementId);

    // =========================================================================
    // Introspection (Optional)
    // =========================================================================

    /// Get number of dirty elements waiting for rebuild
    ///
    /// Useful for debugging and testing.
    fn dirty_count(&self) -> usize;

    /// Get current frame number
    ///
    /// Increments with each `build_frame()` call.
    fn frame_number(&self) -> u64;
}

// =============================================================================
// Implementation for Arc<RwLock<PipelineOwner>>
// =============================================================================

use super::PipelineOwner;

impl Pipeline for Arc<RwLock<PipelineOwner>> {
    fn tree(&self) -> Arc<RwLock<ElementTree>> {
        self.read().tree().clone()
    }

    fn root_element_id(&self) -> Option<ElementId> {
        self.read().root_element_id()
    }

    fn set_root(&self, root_element: Element) -> ElementId {
        self.write().set_root(root_element)
    }

    fn rebuild_queue(&self) -> Arc<RebuildQueue> {
        Arc::new(self.read().rebuild_queue().clone())
    }

    fn schedule_build_for(&self, element_id: ElementId, depth: usize) {
        self.write().schedule_build_for(element_id, depth);
    }

    fn flush_build(&self) {
        self.write().flush_build();
    }

    fn flush_layout(&self, constraints: BoxConstraints) -> Result<Option<Size>, PipelineError> {
        self.write().flush_layout(constraints)
    }

    fn flush_paint(&self) -> Result<Option<Box<CanvasLayer>>, PipelineError> {
        self.write().flush_paint()
    }

    fn build_frame(
        &self,
        constraints: BoxConstraints,
    ) -> Result<Option<Box<CanvasLayer>>, PipelineError> {
        self.write().build_frame(constraints)
    }

    fn request_layout(&self, node_id: ElementId) {
        self.write().request_layout(node_id);
    }

    fn request_paint(&self, node_id: ElementId) {
        self.write().request_paint(node_id);
    }

    fn dirty_count(&self) -> usize {
        self.read().dirty_count()
    }

    fn frame_number(&self) -> u64 {
        self.read().frame_number()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_trait_object_size() {
        // Verify trait object is pointer-sized (important for performance)
        use std::mem::size_of;
        assert_eq!(size_of::<Arc<dyn Pipeline>>(), size_of::<usize>() * 2); // Fat pointer
    }

    #[test]
    fn test_arc_rwlock_implements_pipeline() {
        let owner = Arc::new(RwLock::new(PipelineOwner::new()));
        // Arc<RwLock<PipelineOwner>> implements Pipeline
        assert!(owner.root_element_id().is_none());
        // Should compile and run without error
    }

    #[test]
    fn test_mock_pipeline_integration() {
        // Integration test using MockPipeline from testing module
        use crate::testing::MockPipeline;

        let mock = Arc::new(MockPipeline::new());
        let pipeline: Arc<dyn Pipeline> = mock.clone();

        // Test basic operations
        assert!(pipeline.root_element_id().is_none());
        assert_eq!(pipeline.frame_number(), 0);

        // Build frames
        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let _ = pipeline.build_frame(constraints);
        let _ = pipeline.build_frame(constraints);

        // Verify mock tracked frames
        assert_eq!(pipeline.frame_number(), 2);
    }
}
