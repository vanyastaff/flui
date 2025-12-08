//! Pipeline - concrete implementations for FLUI's rendering pipeline
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner (facade)
//!   ├─ tree: Arc<RwLock<ElementTree>>
//!   ├─ coordinator: FrameCoordinator
//!   │   ├─ build: BuildPipeline
//!   │   ├─ layout: LayoutPipeline
//!   │   └─ paint: PaintPipeline
//!   └─ root_mgr: RootManager
//! ```

// =============================================================================
// Core pipeline implementations
// =============================================================================

mod build_pipeline;
mod dirty_tracker;
mod frame_coordinator;
mod layout_pipeline;
mod paint_pipeline;
mod parallel_build;
mod pipeline_builder;
mod pipeline_features;
mod pipeline_owner;
mod pipeline_trait;
mod rebuild_queue;
mod root_manager;
mod tree_coordinator; // Four-tree architecture coordinator

// =============================================================================
// Re-exports from flui-pipeline (traits + utilities)
// =============================================================================

pub use flui_pipeline::{
    current_build_context,
    has_build_context,
    with_build_context,
    BatchedExecution,
    // Phase traits
    BuildPhase,
    CancellationToken,
    CoordinatorConfig,
    // Dirty tracking
    DirtySet,
    ErrorRecovery,
    FrameResult,
    LayoutPhase,
    LockFreeDirtySet,

    PaintPhase,
    ParallelExecution,
    PhaseContext,
    PhaseResult,

    PipelineBuildContext as BuildContext,
    // Build context
    PipelineBuildContext,
    PipelineCoordinator,
    // Errors (canonical location!)
    PipelineError,
    PipelineMetrics,
    PipelinePhase,
    PipelineResult,

    RecoveryAction,

    RecoveryPolicy,
    // Utilities
    TripleBuffer,
};

// =============================================================================
// Core pipeline exports (concrete implementations)
// =============================================================================

pub use build_pipeline::BuildPipeline;
pub use frame_coordinator::FrameCoordinator;
pub use layout_pipeline::LayoutPipeline;
pub use paint_pipeline::PaintPipeline;
pub use parallel_build::{partition_subtrees, rebuild_dirty_parallel, Subtree};
pub use pipeline_builder::PipelineBuilder;
pub use pipeline_features::{HitTestCache, PipelineFeatures};
pub use pipeline_owner::PipelineOwner;
pub use pipeline_trait::Pipeline;
pub use rebuild_queue::RebuildQueue;
pub use root_manager::RootManager;
pub use tree_coordinator::TreeCoordinator; // Four-tree architecture coordinator

// Re-export ElementTree for convenience
pub use flui_element::ElementTree;
