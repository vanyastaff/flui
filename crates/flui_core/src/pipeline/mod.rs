//! Pipeline architecture
//!
//! The pipeline module orchestrates the three phases of frame rendering:
//! build, layout, and paint. Each phase is implemented as a separate pipeline
//! with clear responsibilities.
//!
//! # Architecture
//!
//! ```text
//! PipelineOwner (orchestrator)
//! ├─ ElementTree (owns elements)
//! ├─ BuildPipeline (widget rebuild)
//! ├─ LayoutPipeline (size computation)
//! └─ PaintPipeline (layer generation)
//! ```
//!
//! # Design Principles
//!
//! 1. **Single Responsibility**: Each pipeline does one thing
//! 2. **Clear Ownership**: ElementTree owned by PipelineOwner
//! 3. **Composability**: Each pipeline independently testable
//! 4. **Multi-threading Ready**: Parallel layout from day one
//!
//! # Example
//!
//! ```rust,ignore
//! let mut owner = PipelineOwner::new();
//! owner.set_root(my_widget);
//!
//! // Build complete frame
//! let layer = owner.build_frame(constraints);
//! ```
//!
//! # Production Features
//!
//! See [`PipelineOwner`] for optional production features:
//! - Metrics (performance monitoring)
//! - Cancellation (timeout support)
//! - Error recovery (graceful degradation)

pub mod build_pipeline;
pub mod cancellation;
pub mod dirty_tracking;
pub mod element_tree;
pub mod error;
pub mod layout_pipeline;
pub mod metrics;
pub mod paint_pipeline;
pub mod pipeline_builder;
pub mod pipeline_owner;
pub mod recovery;
pub mod triple_buffer;







pub use build_pipeline::BuildPipeline;
pub use cancellation::CancellationToken;
pub use dirty_tracking::LockFreeDirtySet;
pub use element_tree::ElementTree;
pub use error::{PipelineError, PipelinePhase};
pub use layout_pipeline::LayoutPipeline;
pub use metrics::PipelineMetrics;
pub use paint_pipeline::PaintPipeline;
pub use pipeline_builder::PipelineBuilder;
pub use pipeline_owner::PipelineOwner;
pub use recovery::{ErrorRecovery, RecoveryAction, RecoveryPolicy};
pub use triple_buffer::TripleBuffer;






