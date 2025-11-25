//! BuildContext module
//!
//! Provides the `BuildContext` trait - an abstraction for accessing
//! framework services during view building.
//!
//! The concrete implementation `PipelineBuildContext` is in `flui-pipeline`.

mod build_context;

pub use build_context::BuildContext;
