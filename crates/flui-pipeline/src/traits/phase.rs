//! Pipeline phase traits
//!
//! Defines abstract traits for the three pipeline phases:
//! - **BuildPhase**: Rebuilds dirty widgets
//! - **LayoutPhase**: Computes sizes and positions
//! - **PaintPhase**: Generates paint layers
//!
//! # Design Principles
//!
//! 1. **Single Responsibility**: Each phase does ONE thing
//! 2. **Open/Closed**: New implementations can be added without modifying traits
//! 3. **Interface Segregation**: Separate traits for separate concerns
//!
//! # Why Three Separate Traits?
//!
//! Each phase has different:
//! - Input types (constraints for layout, nothing for build/paint)
//! - Output types (count for build, size for layout, layer for paint)
//! - Dirty tracking (depth-aware for build, simple set for layout/paint)
//!
//! A single generic trait would require complex type gymnastics.
//! Three focused traits are simpler and more idiomatic.

use flui_foundation::ElementId;
use std::fmt::Debug;

use crate::error::PipelineResult;

// =============================================================================
// Common Types
// =============================================================================

/// Context passed to phase execution
#[derive(Debug, Clone)]
pub struct PhaseContext {
    /// Root element ID (if any)
    pub root_id: Option<ElementId>,

    /// Current frame number
    pub frame_number: u64,

    /// Maximum iterations for iterative phases (like build)
    pub max_iterations: usize,
}

impl Default for PhaseContext {
    fn default() -> Self {
        Self {
            root_id: None,
            frame_number: 0,
            max_iterations: 100,
        }
    }
}

impl PhaseContext {
    /// Create a new context with root ID
    pub fn new(root_id: Option<ElementId>) -> Self {
        Self {
            root_id,
            ..Default::default()
        }
    }

    /// Set the frame number
    pub fn with_frame(mut self, frame: u64) -> Self {
        self.frame_number = frame;
        self
    }

    /// Set max iterations
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }
}

/// Result of phase execution
#[derive(Debug, Clone)]
pub struct PhaseResult<O> {
    /// Phase output value
    pub output: O,

    /// Number of elements processed
    pub processed_count: usize,

    /// Number of iterations (for iterative phases)
    pub iterations: usize,

    /// Whether the phase was skipped (no dirty elements)
    pub skipped: bool,
}

impl<O: Default> Default for PhaseResult<O> {
    fn default() -> Self {
        Self {
            output: O::default(),
            processed_count: 0,
            iterations: 0,
            skipped: true,
        }
    }
}

impl<O> PhaseResult<O> {
    /// Create a new result
    pub fn new(output: O, processed_count: usize) -> Self {
        Self {
            output,
            processed_count,
            iterations: 1,
            skipped: false,
        }
    }

    /// Create a skipped result
    pub fn skipped(output: O) -> Self {
        Self {
            output,
            processed_count: 0,
            iterations: 0,
            skipped: true,
        }
    }

    /// Set iterations count
    pub fn with_iterations(mut self, iterations: usize) -> Self {
        self.iterations = iterations;
        self
    }

    /// Map the output value
    pub fn map<U, F: FnOnce(O) -> U>(self, f: F) -> PhaseResult<U> {
        PhaseResult {
            output: f(self.output),
            processed_count: self.processed_count,
            iterations: self.iterations,
            skipped: self.skipped,
        }
    }
}

// =============================================================================
// Build Phase Trait
// =============================================================================

/// Build phase - rebuilds dirty widgets
///
/// The build phase is responsible for:
/// - Rebuilding dirty ComponentElements
/// - Updating element tree structure
/// - Reconciling old and new view trees
///
/// # Depth Tracking
///
/// Build phase tracks elements with their depths because:
/// - Parents must build before children
/// - Building a parent may invalidate children
///
/// # Iterative Nature
///
/// Build may require multiple iterations because:
/// - Rebuilding widget A may mark widget B as dirty
/// - Signals during build may schedule more rebuilds
pub trait BuildPhase: Send {
    /// The tree type (usually `Arc<RwLock<ElementTree>>`)
    type Tree;

    /// Schedule an element for rebuild with its depth
    fn schedule(&mut self, element_id: ElementId, depth: usize);

    /// Check if any elements are dirty
    fn has_dirty(&self) -> bool;

    /// Get count of dirty elements
    fn dirty_count(&self) -> usize;

    /// Clear all scheduled rebuilds
    fn clear_dirty(&mut self);

    /// Execute rebuild for all dirty elements
    ///
    /// Returns number of elements rebuilt.
    fn rebuild_dirty(&mut self, tree: &Self::Tree) -> usize;

    /// Flush any queued rebuilds (from signals, batching, etc.)
    fn flush_queues(&mut self);

    /// Phase name for logging
    fn name(&self) -> &'static str {
        "build"
    }
}

// =============================================================================
// Layout Phase Trait
// =============================================================================

/// Layout phase - computes sizes and positions
///
/// The layout phase is responsible for:
/// - Computing sizes for RenderElements
/// - Positioning children within parents
/// - Caching layout results
///
/// # Constraints
///
/// Layout receives constraints from parent and must compute
/// a size that satisfies those constraints.
pub trait LayoutPhase: Send {
    /// The tree type (usually `&mut ElementTree`)
    type Tree;

    /// The constraints type (usually `BoxConstraints`)
    type Constraints;

    /// The size type (usually `Size`)
    type Size;

    /// Mark an element as needing layout
    fn mark_dirty(&self, element_id: ElementId);

    /// Check if any elements need layout
    fn has_dirty(&self) -> bool;

    /// Get count of elements needing layout
    fn dirty_count(&self) -> usize;

    /// Check if specific element needs layout
    fn is_dirty(&self, element_id: ElementId) -> bool;

    /// Clear all dirty flags
    fn clear_dirty(&mut self);

    /// Compute layout for all dirty elements
    ///
    /// Returns list of elements that were laid out.
    fn compute_layout(
        &mut self,
        tree: &mut Self::Tree,
        constraints: Self::Constraints,
    ) -> PipelineResult<Vec<ElementId>>;

    /// Phase name for logging
    fn name(&self) -> &'static str {
        "layout"
    }
}

// =============================================================================
// Paint Phase Trait
// =============================================================================

/// Paint phase - generates paint layers
///
/// The paint phase is responsible for:
/// - Generating paint commands for RenderElements
/// - Building layer tree for compositor
/// - Optimizing paint operations
pub trait PaintPhase: Send {
    /// The tree type (usually `&mut ElementTree`)
    type Tree;

    /// Mark an element as needing repaint
    fn mark_dirty(&self, element_id: ElementId);

    /// Check if any elements need repaint
    fn has_dirty(&self) -> bool;

    /// Get count of elements needing repaint
    fn dirty_count(&self) -> usize;

    /// Check if specific element needs repaint
    fn is_dirty(&self, element_id: ElementId) -> bool;

    /// Clear all dirty flags
    fn clear_dirty(&mut self);

    /// Generate paint layers for all dirty elements
    ///
    /// Returns count of elements painted.
    fn generate_layers(&mut self, tree: &mut Self::Tree) -> PipelineResult<usize>;

    /// Phase name for logging
    fn name(&self) -> &'static str {
        "paint"
    }
}

// =============================================================================
// Extension Traits
// =============================================================================

/// Extension for phases that support parallel execution
pub trait ParallelExecution {
    /// Enable/disable parallel execution
    fn set_parallel(&mut self, enabled: bool);

    /// Check if parallel execution is enabled
    fn is_parallel(&self) -> bool;

    /// Minimum elements to trigger parallel execution
    fn min_parallel_threshold(&self) -> usize {
        50
    }
}

/// Extension for phases that support batching
pub trait BatchedExecution {
    /// Enable batching with given duration
    fn enable_batching(&mut self, duration: std::time::Duration);

    /// Disable batching
    fn disable_batching(&mut self);

    /// Check if batching is enabled
    fn is_batching_enabled(&self) -> bool;

    /// Flush current batch
    fn flush_batch(&mut self);

    /// Check if batch is ready to flush
    fn should_flush_batch(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_context_default() {
        let ctx = PhaseContext::default();
        assert!(ctx.root_id.is_none());
        assert_eq!(ctx.frame_number, 0);
        assert_eq!(ctx.max_iterations, 100);
    }

    #[test]
    fn test_phase_context_builder() {
        let root = ElementId::new(42);
        let ctx = PhaseContext::new(Some(root))
            .with_frame(100)
            .with_max_iterations(50);

        assert_eq!(ctx.root_id, Some(root));
        assert_eq!(ctx.frame_number, 100);
        assert_eq!(ctx.max_iterations, 50);
    }

    #[test]
    fn test_phase_result_map() {
        let result = PhaseResult::new(42, 10);
        let mapped = result.map(|x| x * 2);

        assert_eq!(mapped.output, 84);
        assert_eq!(mapped.processed_count, 10);
    }

    #[test]
    fn test_phase_result_skipped() {
        let result: PhaseResult<i32> = PhaseResult::skipped(0);

        assert!(result.skipped);
        assert_eq!(result.processed_count, 0);
        assert_eq!(result.iterations, 0);
    }
}
