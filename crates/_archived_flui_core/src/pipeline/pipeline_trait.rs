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

use flui_element::Element;
use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_pipeline::PipelineError;
use flui_types::{constraints::BoxConstraints, Size};
use parking_lot::RwLock;
use std::sync::Arc;

use super::RebuildQueue;

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
/// - **Tree Management**: `root_element_id()`
/// - **Widget Lifecycle**: `set_root()`
/// - **Rebuild Scheduling**: `rebuild_queue()`, `schedule_build_for()`
/// - **Pipeline Phases**: `flush_build()`, `flush_layout()`, `flush_paint()`
/// - **Complete Frame**: `build_frame()` (all phases)
/// - **Dirty Tracking**: `request_layout()`, `request_paint()`
pub trait Pipeline: Send + Sync {
    // =========================================================================
    // Tree & Root Management
    // =========================================================================

    /// Get the root element ID (if any)
    fn root_element_id(&self) -> Option<ElementId>;

    /// Mount an element as the root of the tree
    ///
    /// Returns the ElementId assigned to the root.
    /// Automatically schedules the root for initial build.
    fn set_root(&self, root_element: Element) -> ElementId;

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
    /// List of (ElementId, Size) pairs for laid out elements
    fn flush_layout(&self, constraints: BoxConstraints) -> Vec<(ElementId, Size)>;

    /// Flush the paint phase
    ///
    /// Generates canvas with drawing commands for all dirty RenderElements.
    ///
    /// # Returns
    ///
    /// - `Some(canvas)`: Paint succeeded, returns root canvas
    /// - `None`: No root element or tree is empty
    fn flush_paint(&self) -> Option<Canvas>;

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
    /// - `Ok(Some(canvas))`: Frame rendered successfully
    /// - `Ok(None)`: No root element or tree is empty
    /// - `Err(e)`: Pipeline error occurred
    fn build_frame(&self, constraints: BoxConstraints) -> Result<Option<Canvas>, PipelineError>;

    // =========================================================================
    // Dirty Tracking
    // =========================================================================

    /// Mark a RenderElement as needing layout
    fn request_layout(&self, node_id: ElementId);

    /// Mark a RenderElement as needing paint
    fn request_paint(&self, node_id: ElementId);

    // =========================================================================
    // Introspection (Optional)
    // =========================================================================

    /// Get number of dirty elements waiting for rebuild
    fn dirty_count(&self) -> usize;

    /// Get current frame number
    fn frame_number(&self) -> u64;
}

// =============================================================================
// Implementation for Arc<RwLock<PipelineOwner>>
// =============================================================================

use super::PipelineOwner;

impl Pipeline for Arc<RwLock<PipelineOwner>> {
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

    fn flush_layout(&self, constraints: BoxConstraints) -> Vec<(ElementId, Size)> {
        self.write().flush_layout(constraints)
    }

    fn flush_paint(&self) -> Option<Canvas> {
        self.write().flush_paint()
    }

    fn build_frame(&self, constraints: BoxConstraints) -> Result<Option<Canvas>, PipelineError> {
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
}
