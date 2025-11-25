//! Pipeline architecture
//!
//! The pipeline module orchestrates the three phases of frame rendering:
//! build, layout, and paint. Each phase is implemented as a separate pipeline
//! with clear responsibilities.
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner (thin facade)
//!   ├─ tree: Arc<RwLock<ElementTree>>  // Element storage
//!   ├─ coordinator: FrameCoordinator   // Phase orchestration
//!   │   ├─ build: BuildPipeline        // View rebuild (with parallel support)
//!   │   ├─ layout: LayoutPipeline      // Size computation
//!   │   ├─ paint: PaintPipeline        // Layer generation
//!   │   └─ budget: FrameBudget         // Frame timing (from flui-scheduler)
//!   ├─ root_mgr: RootManager           // Root element tracking
//!   └─ Optional features (from flui-pipeline):
//!       ├─ metrics: PipelineMetrics
//!       ├─ recovery: ErrorRecovery
//!       ├─ cancellation: CancellationToken
//!       └─ frame_buffer: TripleBuffer
//! ```
//!
//! # Design Principles (SOLID)
//!
//! 1. **Single Responsibility**: Each component has ONE clear purpose
//! 2. **Open/Closed**: Easy to extend without modifying core
//! 3. **Liskov Substitution**: Components can be tested/mocked independently
//! 4. **Interface Segregation**: Focused, minimal interfaces
//! 5. **Dependency Inversion**: Depend on abstractions (traits)
//!
//! # Example
//!
//! ```rust,ignore
//! let mut owner = PipelineOwner::new();
//! owner.set_root(my_element);
//!
//! // Build complete frame
//! let layer = owner.build_frame(constraints)?;
//! ```

// =============================================================================
// Core pipeline modules (flui-core specific)
// =============================================================================

pub mod build_pipeline;
pub mod error;
pub mod frame_coordinator;
pub mod layout_pipeline;
pub mod paint_pipeline;
pub mod parallel_build;
pub mod pipeline_builder;
pub mod pipeline_features;
pub mod pipeline_owner;
pub mod pipeline_trait;
pub mod rebuild_queue;
pub mod root_manager;

// =============================================================================
// Re-exports from flui-pipeline (generic utilities)
// =============================================================================

pub use flui_pipeline::{
    // Cancellation
    CancellationToken,
    // Dirty tracking
    DirtySet,
    // Error recovery
    ErrorRecovery,
    LockFreeDirtySet,
    // Metrics
    PipelineMetrics,
    RecoveryAction,
    RecoveryPolicy,
    // Triple buffer
    TripleBuffer,
};

// =============================================================================
// Core pipeline exports
// =============================================================================

pub use crate::element::ElementTree;
pub use build_pipeline::BuildPipeline;
pub use error::{InvalidDuration, InvalidError, PipelineError, PipelinePhase, TimeoutDuration};
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
