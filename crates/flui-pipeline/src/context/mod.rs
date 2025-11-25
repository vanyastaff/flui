//! Build context module
//!
//! Provides `PipelineBuildContext` - the concrete implementation of
//! `BuildContext` trait from flui-view.

mod pipeline_context;
mod thread_local;

pub use pipeline_context::PipelineBuildContext;
pub use thread_local::{
    current_build_context, has_build_context, try_current_build_context, with_build_context,
    BuildContextGuard,
};
