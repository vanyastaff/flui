//! Pipeline architecture
//!
//! The pipeline module orchestrates the three phases of frame rendering:
//! build, layout, and paint. Each phase is implemented as a separate pipeline
//! with clear responsibilities.
//!
//! # Architecture (After Refactoring)
//!
//! ```text
//! PipelineOwner (thin facade)
//!   ├─ tree: Arc<RwLock<ElementTree>>  // Element storage
//!   ├─ coordinator: FrameCoordinator   // Phase orchestration
//!   │   ├─ build: BuildPipeline        // View rebuild (with parallel support)
//!   │   ├─ layout: LayoutPipeline      // Size computation
//!   │   ├─ paint: PaintPipeline        // Layer generation
//!   │   └─ scheduler: FrameScheduler   // Frame timing & budget
//!   ├─ root_mgr: RootManager           // Root element tracking
//!   └─ Optional features:
//!       ├─ metrics: PipelineMetrics
//!       ├─ recovery: ErrorRecovery
//!       ├─ cancellation: CancellationToken
//!       └─ frame_buffer: TripleBuffer
//! ```
//!
//! # Design Principles (SOLID)
//!
//! 1. **Single Responsibility**: Each component has ONE clear purpose
//!    - `FrameCoordinator`: Orchestrates pipeline phases
//!    - `RootManager`: Manages root element
//!    - `ElementTree`: Stores elements
//!    - `BuildPipeline`, `LayoutPipeline`, `PaintPipeline`: Phase-specific logic
//!    - `FrameScheduler`: Manages frame timing and budget
//!    - `parallel_build`: Parallel execution of independent subtrees
//!
//! 2. **Open/Closed**: Easy to extend with new features without modifying core
//!
//! 3. **Liskov Substitution**: Components can be tested/mocked independently
//!
//! 4. **Interface Segregation**: Focused, minimal interfaces
//!
//! 5. **Dependency Inversion**: Depend on abstractions (traits), not implementations
//!
//! # Benefits
//!
//! - **Maintainability**: Changes localized to specific components
//! - **Testability**: Each component testable in isolation
//! - **Clarity**: Clear separation of concerns
//! - **Extensibility**: New features don't bloat PipelineOwner
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
//!
//! # Production Features
//!
//! See [`PipelineOwner`] for optional production features:
//! - Metrics (performance monitoring)
//! - Cancellation (timeout support)
//! - Error recovery (graceful degradation)
//! - Frame buffer (lock-free frame exchange)
//! - Parallel build (multi-threaded widget rebuilds)
//! - Frame scheduling (frame budget management)

pub mod build_pipeline;
pub mod cancellation;
pub mod dirty_tracking;
pub mod error;
pub mod frame_coordinator;
// frame_coordinator_tests is included in frame_coordinator.rs via #[cfg(test)]
pub mod frame_scheduler;
pub mod layout_pipeline;
pub mod metrics;
pub mod paint_pipeline;
pub mod parallel_build;
pub mod pipeline_builder;
pub mod pipeline_owner;
pub mod rebuild_queue;
pub mod recovery;
pub mod root_manager;
pub mod triple_buffer;

pub use build_pipeline::BuildPipeline;
pub use cancellation::CancellationToken;
pub use dirty_tracking::LockFreeDirtySet;
// ElementTree moved to element module (breaking circular dependency)
pub use crate::element::ElementTree;
pub use error::{InvalidDuration, InvalidError, PipelineError, PipelinePhase, TimeoutDuration};
pub use frame_coordinator::FrameCoordinator;
pub use frame_scheduler::{FrameScheduler, FrameSkipPolicy};
pub use layout_pipeline::LayoutPipeline;
pub use metrics::PipelineMetrics;
pub use paint_pipeline::PaintPipeline;
pub use pipeline_builder::PipelineBuilder;
pub use pipeline_owner::PipelineOwner;
pub use rebuild_queue::RebuildQueue;
pub use recovery::{ErrorRecovery, RecoveryAction, RecoveryPolicy};
pub use root_manager::RootManager;
pub use triple_buffer::TripleBuffer;
