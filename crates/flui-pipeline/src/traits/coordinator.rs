//! Pipeline coordinator trait
//!
//! The coordinator orchestrates the execution of pipeline phases
//! in the correct order: build → layout → paint.
//!
//! # Design
//!
//! ```text
//! PipelineCoordinator
//!   ├─ build_phase()   → Phase<Tree, (), ()>
//!   ├─ layout_phase()  → Phase<Tree, Constraints, Size>
//!   ├─ paint_phase()   → Phase<Tree, Offset, Layer>
//!   └─ execute_frame() → orchestrates all phases
//! ```

use flui_foundation::ElementId;
use std::time::Duration;

use crate::error::PipelineResult;

/// Configuration for the coordinator
#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    /// Target frames per second
    pub target_fps: u32,

    /// Enable parallel build phase
    pub parallel_build: bool,

    /// Maximum build iterations per frame
    pub max_build_iterations: usize,

    /// Skip phases with no dirty elements
    pub skip_clean_phases: bool,

    /// Frame budget (derived from target_fps)
    pub frame_budget: Duration,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self::new(60)
    }
}

impl CoordinatorConfig {
    /// Create config with target FPS
    pub fn new(target_fps: u32) -> Self {
        Self {
            target_fps,
            parallel_build: true,
            max_build_iterations: 100,
            skip_clean_phases: true,
            frame_budget: Duration::from_secs_f64(1.0 / target_fps as f64),
        }
    }

    /// Set parallel build enabled
    pub fn with_parallel_build(mut self, enabled: bool) -> Self {
        self.parallel_build = enabled;
        self
    }

    /// Set max build iterations
    pub fn with_max_build_iterations(mut self, max: usize) -> Self {
        self.max_build_iterations = max;
        self
    }

    /// Disable skip-clean optimization
    pub fn force_all_phases(mut self) -> Self {
        self.skip_clean_phases = false;
        self
    }
}

/// Result of a complete frame execution
#[derive(Debug)]
pub struct FrameResult<Layer> {
    /// The root layer for rendering
    pub layer: Option<Layer>,

    /// Root element size after layout
    pub root_size: Option<(f32, f32)>,

    /// Frame number
    pub frame_number: u64,

    /// Build phase stats
    pub build_processed: usize,
    /// Build iterations
    pub build_iterations: usize,

    /// Layout phase stats
    pub layout_processed: usize,

    /// Paint phase stats
    pub paint_processed: usize,

    /// Total frame time
    pub frame_time: Duration,

    /// Whether frame was over budget
    pub over_budget: bool,
}

impl<Layer> Default for FrameResult<Layer> {
    fn default() -> Self {
        Self {
            layer: None,
            root_size: None,
            frame_number: 0,
            build_processed: 0,
            build_iterations: 0,
            layout_processed: 0,
            paint_processed: 0,
            frame_time: Duration::ZERO,
            over_budget: false,
        }
    }
}

/// Pipeline coordinator trait
///
/// Coordinates the three phases of the rendering pipeline.
/// Implementations handle the concrete phase types.
///
/// # Responsibilities
///
/// 1. Execute phases in correct order (build → layout → paint)
/// 2. Pass outputs between phases (layout marks elements for paint)
/// 3. Track frame timing and budget
/// 4. Handle phase errors with recovery
///
/// # Example
///
/// ```rust,ignore
/// impl PipelineCoordinator for MyCoordinator {
///     type Tree = ElementTree;
///     type Constraints = BoxConstraints;
///     type Size = Size;
///     type Layer = CanvasLayer;
///
///     fn execute_frame(
///         &mut self,
///         tree: &mut Self::Tree,
///         constraints: Self::Constraints,
///     ) -> PipelineResult<FrameResult<Self::Layer>> {
///         // 1. Build phase
///         self.flush_build(tree)?;
///
///         // 2. Layout phase
///         let size = self.flush_layout(tree, constraints)?;
///
///         // 3. Paint phase
///         let layer = self.flush_paint(tree)?;
///
///         Ok(FrameResult { layer, size, .. })
///     }
/// }
/// ```
pub trait PipelineCoordinator: Send {
    /// Tree type
    type Tree;

    /// Constraints type for layout
    type Constraints;

    /// Size type from layout
    type Size;

    /// Layer type from paint
    type Layer;

    // =========================================================================
    // Configuration
    // =========================================================================

    /// Get coordinator configuration
    fn config(&self) -> &CoordinatorConfig;

    /// Set coordinator configuration
    fn set_config(&mut self, config: CoordinatorConfig);

    /// Get current frame number
    fn frame_number(&self) -> u64;

    // =========================================================================
    // Phase Access
    // =========================================================================

    /// Check if build phase has dirty elements
    fn has_dirty_build(&self) -> bool;

    /// Check if layout phase has dirty elements
    fn has_dirty_layout(&self) -> bool;

    /// Check if paint phase has dirty elements
    fn has_dirty_paint(&self) -> bool;

    /// Check if any phase has dirty elements
    fn has_any_dirty(&self) -> bool {
        self.has_dirty_build() || self.has_dirty_layout() || self.has_dirty_paint()
    }

    // =========================================================================
    // Dirty Marking
    // =========================================================================

    /// Schedule an element for build (rebuild)
    fn schedule_build(&mut self, id: ElementId, depth: usize);

    /// Mark element for layout
    fn mark_needs_layout(&mut self, id: ElementId);

    /// Mark element for paint
    fn mark_needs_paint(&mut self, id: ElementId);

    // =========================================================================
    // Phase Execution
    // =========================================================================

    /// Flush the build phase
    ///
    /// Rebuilds all dirty widgets. May run multiple iterations.
    fn flush_build(&mut self, tree: &mut Self::Tree) -> PipelineResult<usize>;

    /// Flush the layout phase
    ///
    /// Computes sizes for all dirty render objects.
    fn flush_layout(
        &mut self,
        tree: &mut Self::Tree,
        constraints: Self::Constraints,
    ) -> PipelineResult<Option<Self::Size>>;

    /// Flush the paint phase
    ///
    /// Generates layers for all dirty render objects.
    fn flush_paint(&mut self, tree: &mut Self::Tree) -> PipelineResult<Option<Self::Layer>>;

    // =========================================================================
    // Frame Execution
    // =========================================================================

    /// Execute a complete frame
    ///
    /// Runs all three phases in order and returns the result.
    fn execute_frame(
        &mut self,
        tree: &mut Self::Tree,
        constraints: Self::Constraints,
    ) -> PipelineResult<FrameResult<Self::Layer>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_config_default() {
        let config = CoordinatorConfig::default();
        assert_eq!(config.target_fps, 60);
        assert!(config.parallel_build);
        assert_eq!(config.max_build_iterations, 100);
    }

    #[test]
    fn test_coordinator_config_builder() {
        let config = CoordinatorConfig::new(120)
            .with_parallel_build(false)
            .with_max_build_iterations(50)
            .force_all_phases();

        assert_eq!(config.target_fps, 120);
        assert!(!config.parallel_build);
        assert_eq!(config.max_build_iterations, 50);
        assert!(!config.skip_clean_phases);
    }

    #[test]
    fn test_frame_budget_calculation() {
        let config = CoordinatorConfig::new(60);
        // 60 FPS = ~16.67ms per frame
        assert!(config.frame_budget.as_millis() >= 16);
        assert!(config.frame_budget.as_millis() <= 17);

        let config_120 = CoordinatorConfig::new(120);
        // 120 FPS = ~8.33ms per frame
        assert!(config_120.frame_budget.as_millis() >= 8);
        assert!(config_120.frame_budget.as_millis() <= 9);
    }
}
