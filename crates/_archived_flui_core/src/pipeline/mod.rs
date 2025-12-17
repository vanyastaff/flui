//! Pipeline - concrete implementations for FLUI's rendering pipeline
//!
//! # Architecture (Consolidated)
//!
//! ```text
//! PipelineOwner (owns everything directly)
//!   ├─ tree_coord: TreeCoordinator      ← DIRECT ownership (no Arc<RwLock>)
//!   │   ├─ views: ViewTree
//!   │   ├─ elements: ElementTree
//!   │   ├─ render_objects: RenderTree
//!   │   └─ layers: LayerTree
//!   ├─ dirty_elements: Vec<(ElementId, usize)>
//!   ├─ rebuild_queue: RebuildQueue
//!   ├─ build_owner: BuildOwner
//!   └─ frame_budget: FrameBudget
//!
//! External access (in flui_app):
//!   Arc<RwLock<PipelineOwner>>  ← Thread-safe access at app level
//! ```
//!
//! # Why Consolidated?
//!
//! The previous architecture had separate BuildPipeline, LayoutPipeline,
//! PaintPipeline, and FrameCoordinator, each holding their own
//! `Arc<RwLock<TreeCoordinator>>`. This caused deadlocks when one component
//! held a write lock while another tried to acquire it.
//!
//! Now all logic is in PipelineOwner, which owns TreeCoordinator directly.
//! Methods use `&mut self`, letting Rust's borrow checker prevent deadlocks.

// =============================================================================
// Core pipeline implementations
// =============================================================================

mod pipeline_builder;
pub mod pipeline_context;
mod pipeline_features;
mod pipeline_owner;
mod pipeline_trait;
mod rebuild_queue;
mod tree_coordinator;

// =============================================================================
// Re-exports from flui-pipeline (traits + utilities)
// =============================================================================

pub use flui_pipeline::{
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
// Core pipeline exports
// =============================================================================

pub use pipeline_builder::PipelineBuilder;
pub use pipeline_context::PipelineBuildContext;
pub use pipeline_features::{HitTestCache, PipelineFeatures};
pub use pipeline_owner::PipelineOwner;
pub use pipeline_trait::Pipeline;
pub use rebuild_queue::RebuildQueue;
pub use tree_coordinator::TreeCoordinator;

// Alias for backward compatibility
pub use pipeline_context::PipelineBuildContext as BuildContext;

// Re-export ElementTree for convenience
pub use flui_element::ElementTree;
