//! # FLUI Pipeline
//!
//! Pipeline orchestration for FLUI using trait-based tree abstraction.
//!
//! This crate provides the build, layout, and paint pipeline phases that
//! work with any tree implementation that satisfies the `flui-tree` traits.
//!
//! ## Architecture
//!
//! ```text
//! flui-foundation (ElementId, Slot)
//!        │
//! flui-tree (TreeRead, TreeNav, RenderTreeAccess, DirtyTracking)
//!        │
//! flui-pipeline (BuildPipeline, LayoutPipeline, PaintPipeline)
//!        │
//! flui-core (ElementTree implements traits)
//! ```
//!
//! ## Key Benefits
//!
//! - **No circular dependencies**: Pipeline depends on traits, not concrete types
//! - **Testable**: Use mock trees for unit testing
//! - **Extensible**: Works with any tree implementation
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_pipeline::{LayoutPipeline, PaintPipeline};
//! use flui_tree::prelude::*;
//!
//! // Works with any tree that implements the traits
//! fn process_frame<T>(tree: &mut T, root: ElementId, constraints: BoxConstraints)
//! where
//!     T: RenderTreeAccess + DirtyTracking + TreeNav,
//! {
//!     let mut layout = LayoutPipeline::new();
//!     layout.compute_layout(tree, root, constraints).unwrap();
//!
//!     let mut paint = PaintPipeline::new();
//!     paint.generate_layers(tree, root).unwrap();
//! }
//! ```

#![warn(missing_docs)]

pub mod cancellation;
pub mod dirty;
pub mod error;
pub mod layout;
pub mod metrics;
pub mod paint;
pub mod recovery;
pub mod triple_buffer;

pub use cancellation::CancellationToken;
pub use dirty::{DirtySet, LockFreeDirtySet};
pub use error::{PipelineError, PipelinePhase, PipelineResult};
pub use layout::LayoutPipeline;
pub use metrics::PipelineMetrics;
pub use paint::PaintPipeline;
pub use recovery::{ErrorRecovery, RecoveryAction, RecoveryPolicy};
pub use triple_buffer::TripleBuffer;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
