//! Testing utilities for Pipeline trait
//!
//! This module provides mock implementations and testing helpers for the Pipeline trait.
//! These are only available when compiled with `#[cfg(test)]` or when explicitly needed
//! for integration tests.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::testing::MockPipeline;
//! use flui_core::pipeline::Pipeline;
//! use std::sync::Arc;
//!
//! // Create mock for testing
//! let mock = Arc::new(MockPipeline::new());
//! let pipeline: Arc<dyn Pipeline> = mock.clone();
//!
//! // Use in tests
//! assert_eq!(pipeline.frame_number(), 0);
//! ```

use crate::element::{Element, ElementId};
use crate::pipeline::{ElementTree, PipelineError, RebuildQueue};
use flui_engine::CanvasLayer;
use flui_types::{constraints::BoxConstraints, Size};
use parking_lot::RwLock;
use std::sync::Arc;

/// Mock Pipeline for testing
///
/// This mock implementation provides:
/// - Predictable behavior (fixed sizes, frame counting)
/// - No side effects (no file I/O, no network)
/// - Fast execution (no heavy initialization)
/// - Easy verification (frame counter, dirty flag tracking)
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::testing::MockPipeline;
/// use flui_core::pipeline::Pipeline;
///
/// let mock = Arc::new(MockPipeline::new());
/// let pipeline: Arc<dyn Pipeline> = mock.clone();
///
/// // Build frames
/// let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
/// pipeline.build_frame(constraints).unwrap();
///
/// // Verify
/// assert_eq!(pipeline.frame_number(), 1);
/// ```
#[derive(Debug)]
pub struct MockPipeline {
    /// Element tree (empty by default)
    tree: Arc<RwLock<ElementTree>>,

    /// Root element ID (if any)
    root_id: parking_lot::Mutex<Option<ElementId>>,

    /// Frame counter (incremented on each build_frame)
    frame_count: parking_lot::Mutex<u64>,

    /// Dirty element counter (for testing rebuild scheduling)
    dirty_count: parking_lot::Mutex<usize>,
}

impl MockPipeline {
    /// Create a new MockPipeline with empty state
    pub fn new() -> Self {
        Self {
            tree: Arc::new(RwLock::new(ElementTree::new())),
            root_id: parking_lot::Mutex::new(None),
            frame_count: parking_lot::Mutex::new(0),
            dirty_count: parking_lot::Mutex::new(0),
        }
    }

    /// Get current frame count (useful for verification)
    pub fn frame_count(&self) -> u64 {
        *self.frame_count.lock()
    }

    /// Get dirty element count (useful for verification)
    pub fn dirty_element_count(&self) -> usize {
        *self.dirty_count.lock()
    }

    /// Reset mock state (useful between tests)
    pub fn reset(&self) {
        *self.root_id.lock() = None;
        *self.frame_count.lock() = 0;
        *self.dirty_count.lock() = 0;
    }
}

impl Default for MockPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::pipeline::Pipeline for MockPipeline {
    fn tree(&self) -> Arc<RwLock<ElementTree>> {
        self.tree.clone()
    }

    fn root_element_id(&self) -> Option<ElementId> {
        *self.root_id.lock()
    }

    fn set_root(&self, _root_element: Element) -> ElementId {
        let id = ElementId::new(1);
        *self.root_id.lock() = Some(id);
        id
    }

    fn rebuild_queue(&self) -> Arc<RebuildQueue> {
        Arc::new(RebuildQueue::new())
    }

    fn schedule_build_for(&self, _element_id: ElementId, _depth: usize) {
        // Mock: increment dirty counter
        *self.dirty_count.lock() += 1;
    }

    fn flush_build(&self) {
        // Mock: reset dirty counter
        *self.dirty_count.lock() = 0;
    }

    fn flush_layout(
        &self,
        _constraints: BoxConstraints,
    ) -> Result<Option<Size>, PipelineError> {
        // Mock: return fixed size (800x600)
        Ok(Some(Size::new(800.0, 600.0)))
    }

    fn flush_paint(&self) -> Result<Option<Box<CanvasLayer>>, PipelineError> {
        // Mock: return empty layer
        Ok(Some(Box::new(CanvasLayer::new())))
    }

    fn build_frame(
        &self,
        _constraints: BoxConstraints,
    ) -> Result<Option<Box<CanvasLayer>>, PipelineError> {
        // Increment frame counter
        *self.frame_count.lock() += 1;

        // Reset dirty counter (flush_build was called)
        *self.dirty_count.lock() = 0;

        // Return empty layer
        Ok(Some(Box::new(CanvasLayer::new())))
    }

    fn request_layout(&self, _node_id: ElementId) {
        // Mock: increment dirty counter
        *self.dirty_count.lock() += 1;
    }

    fn request_paint(&self, _node_id: ElementId) {
        // Mock: increment dirty counter
        *self.dirty_count.lock() += 1;
    }

    fn dirty_count(&self) -> usize {
        *self.dirty_count.lock()
    }

    fn frame_number(&self) -> u64 {
        *self.frame_count.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::Pipeline;

    #[test]
    fn test_mock_pipeline_creation() {
        let mock = MockPipeline::new();
        assert!(mock.root_element_id().is_none());
        assert_eq!(mock.frame_number(), 0);
        assert_eq!(mock.dirty_count(), 0);
    }

    #[test]
    fn test_mock_pipeline_as_trait_object() {
        let mock = Arc::new(MockPipeline::new());
        let pipeline: Arc<dyn Pipeline> = mock.clone();

        // Should work through trait
        assert!(pipeline.root_element_id().is_none());
        assert_eq!(pipeline.frame_number(), 0);
    }

    #[test]
    fn test_mock_frame_counting() {
        let mock = Arc::new(MockPipeline::new());
        let pipeline: Arc<dyn Pipeline> = mock.clone();

        assert_eq!(pipeline.frame_number(), 0);

        // Build frames
        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let _ = pipeline.build_frame(constraints);
        assert_eq!(pipeline.frame_number(), 1);

        let _ = pipeline.build_frame(constraints);
        assert_eq!(pipeline.frame_number(), 2);
    }

    #[test]
    fn test_mock_dirty_tracking() {
        let mock = Arc::new(MockPipeline::new());
        let pipeline: Arc<dyn Pipeline> = mock.clone();

        assert_eq!(pipeline.dirty_count(), 0);

        // Schedule some rebuilds
        let id = ElementId::new(1);
        pipeline.schedule_build_for(id, 0);
        pipeline.schedule_build_for(id, 1);
        assert_eq!(pipeline.dirty_count(), 2);

        // Flush build resets counter
        pipeline.flush_build();
        assert_eq!(pipeline.dirty_count(), 0);
    }

    #[test]
    fn test_mock_reset() {
        let mock = MockPipeline::new();

        // Build some frames
        let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
        let _ = mock.build_frame(constraints);
        assert_eq!(mock.frame_number(), 1);

        // Reset
        mock.reset();
        assert_eq!(mock.frame_number(), 0);
        assert!(mock.root_element_id().is_none());
    }

    #[test]
    fn test_mock_layout_returns_fixed_size() {
        let mock = Arc::new(MockPipeline::new());
        let pipeline: Arc<dyn Pipeline> = mock.clone();

        let constraints = BoxConstraints::tight(Size::new(400.0, 300.0));
        let size = pipeline.flush_layout(constraints).unwrap();

        // Mock always returns 800x600 regardless of constraints
        assert_eq!(size, Some(Size::new(800.0, 600.0)));
    }
}
