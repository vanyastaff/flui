//! # FLUI Pipeline
//!
//! Abstract pipeline traits and utilities for FLUI's rendering system.
//!
//! This crate provides:
//! - **Abstract traits** for pipeline phases and coordination
//! - **Dirty tracking** utilities (bitmap and hashset based)
//! - **Buffer management** (triple buffer for lock-free frame exchange)
//! - **Metrics and monitoring** for performance analysis
//! - **Error recovery** strategies
//! - **Cancellation** support for long-running operations
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        flui-pipeline                            │
//! │  (Abstract traits + utilities)                                  │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  traits/                                                        │
//! │    ├─ BuildPhase                    - Widget rebuild phase      │
//! │    ├─ LayoutPhase                   - Size computation phase    │
//! │    ├─ PaintPhase                    - Layer generation phase    │
//! │    ├─ PipelineCoordinator           - Phase orchestration       │
//! │    └─ SchedulerIntegration          - Scheduler bridge          │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  utilities/                                                     │
//! │    ├─ dirty (DirtySet, LockFreeDirtySet)                       │
//! │    ├─ buffer (TripleBuffer)                                     │
//! │    ├─ metrics (PipelineMetrics)                                │
//! │    ├─ recovery (ErrorRecovery)                                  │
//! │    └─ cancellation (CancellationToken)                         │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    flui_core/pipeline                           │
//! │  (Concrete implementations)                                     │
//! │    ├─ BuildPipeline   : impl BuildPhase                         │
//! │    ├─ LayoutPipeline  : impl LayoutPhase                        │
//! │    ├─ PaintPipeline   : impl PaintPhase                         │
//! │    └─ FrameCoordinator: impl PipelineCoordinator               │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Phase Traits
//!
//! ### BuildPhase
//!
//! Rebuilds dirty widgets with depth-aware scheduling:
//!
//! ```rust,ignore
//! pub trait BuildPhase: Send {
//!     type Tree;
//!     fn schedule(&mut self, element_id: ElementId, depth: usize);
//!     fn rebuild_dirty(&mut self, tree: &Self::Tree) -> usize;
//! }
//! ```
//!
//! ### LayoutPhase
//!
//! Computes sizes with constraint propagation:
//!
//! ```rust,ignore
//! pub trait LayoutPhase: Send {
//!     type Tree;
//!     type Constraints;
//!     type Size;
//!     fn compute_layout(&mut self, tree: &mut Self::Tree, constraints: Self::Constraints)
//!         -> PipelineResult<Vec<ElementId>>;
//! }
//! ```
//!
//! ### PaintPhase
//!
//! Generates paint layers:
//!
//! ```rust,ignore
//! pub trait PaintPhase: Send {
//!     type Tree;
//!     fn generate_layers(&mut self, tree: &mut Self::Tree) -> PipelineResult<usize>;
//! }
//! ```
//!
//! ## Utilities
//!
//! ### Dirty Tracking
//!
//! Two implementations for different use cases:
//!
//! - `LockFreeDirtySet` - Atomic bitmap, O(1) operations, fixed capacity
//! - `DirtySet` - HashSet based, dynamic size, simple API
//!
//! ### Triple Buffer
//!
//! Lock-free frame exchange between producer (pipeline) and consumer (renderer):
//!
//! ```rust,ignore
//! let buffer = TripleBuffer::new(frame1, frame2, frame3);
//! buffer.write(new_frame);  // Producer
//! let frame = buffer.read(); // Consumer
//! ```
//!
//! ## Feature Flags
//!
//! - `parallel` - Enable rayon-based parallel phase execution
//! - `serde` - Serialization support for metrics and errors
//! - `full` - Enable all features

#![warn(missing_docs)]

// =============================================================================
// Modules
// =============================================================================

// Abstract traits
pub mod traits;

// Utilities
pub mod cancellation;
pub mod dirty;
pub mod error;
pub mod metrics;
pub mod recovery;
pub mod triple_buffer;

// Legacy modules (kept for backward compatibility)
pub mod build;

// =============================================================================
// Re-exports: Traits
// =============================================================================

pub use traits::{
    // Visitor traits (from flui-tree)
    hit_test_with_callback,
    layout_with_callback,
    paint_with_callback,
    // Execution strategies
    BatchedExecution,
    // Phase traits
    BuildPhase,
    // Coordinator
    CoordinatorConfig,
    // Tree access traits (from flui-tree)
    DirtyTracking,
    DirtyTrackingExt,
    FrameResult,
    // Scheduler integration (from flui-scheduler)
    FrameTiming,
    HitTestVisitable,
    HitTestVisitableExt,
    LayoutPhase,
    LayoutVisitable,
    LayoutVisitableExt,
    NoopScheduler,
    PaintPhase,
    PaintVisitable,
    PaintVisitableExt,
    ParallelExecution,
    // Common types
    PhaseContext,
    PhaseResult,
    PipelineCoordinator,
    PipelineVisitor,
    Priority,
    RecordingScheduler,
    RenderTreeAccess,
    SchedulerIntegration,
    SimpleTreeVisitor,
    TreeNav,
    TreeOperation,
    TreeRead,
};

// =============================================================================
// Re-exports: Utilities
// =============================================================================

pub use build::{BuildBatcher, BuildPipeline};
pub use cancellation::CancellationToken;
pub use dirty::{DirtySet, LockFreeDirtySet};
pub use error::{PipelineError, PipelinePhase, PipelineResult};
pub use metrics::PipelineMetrics;
pub use recovery::{ErrorRecovery, RecoveryAction, RecoveryPolicy};
pub use triple_buffer::TripleBuffer;

// =============================================================================
// Prelude
// =============================================================================

/// Commonly used types for convenient importing
pub mod prelude {
    // Phase traits
    pub use crate::traits::{
        BatchedExecution, BuildPhase, LayoutPhase, PaintPhase, ParallelExecution, PhaseContext,
        PhaseResult,
    };

    // Coordinator
    pub use crate::traits::{CoordinatorConfig, FrameResult, PipelineCoordinator};

    // Scheduler
    pub use crate::traits::{FrameTiming, Priority, SchedulerIntegration};

    // Utilities
    pub use crate::cancellation::CancellationToken;
    pub use crate::dirty::{DirtySet, LockFreeDirtySet};
    pub use crate::error::{PipelineError, PipelinePhase, PipelineResult};
    pub use crate::metrics::PipelineMetrics;
    pub use crate::recovery::{ErrorRecovery, RecoveryAction, RecoveryPolicy};
    pub use crate::triple_buffer::TripleBuffer;
}

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_prelude_imports() {
        use prelude::*;

        // Verify types are accessible
        let _: PipelinePhase = PipelinePhase::Build;
        let _: RecoveryPolicy = RecoveryPolicy::SkipFrame;
        let _: Priority = Priority::Build;
    }

    #[test]
    fn test_trait_imports() {
        use crate::traits::*;

        // Verify common types are accessible
        let ctx = PhaseContext::default();
        assert!(ctx.root_id.is_none());

        let config = CoordinatorConfig::default();
        assert_eq!(config.target_fps, 60);
    }
}
