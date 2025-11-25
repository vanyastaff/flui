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

pub mod dirty;
pub mod error;
pub mod layout;
pub mod paint;

pub use dirty::DirtySet;
pub use error::{PipelineError, PipelineResult};
pub use layout::LayoutPipeline;
pub use paint::PaintPipeline;

/// Crate version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
