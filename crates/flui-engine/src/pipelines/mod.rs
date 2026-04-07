//! GPU pipeline management
//!
//! Creates and caches wgpu render pipelines for each primitive type.
//! Pipelines are lazily initialized and shared across frames.

#[cfg(feature = "wgpu-backend")]
pub mod blur_pipeline;
#[cfg(feature = "wgpu-backend")]
pub mod gradient_pipeline;
#[cfg(feature = "wgpu-backend")]
pub mod image_pipeline;
#[cfg(feature = "wgpu-backend")]
pub mod path_pipeline;
#[cfg(feature = "wgpu-backend")]
pub mod registry;
#[cfg(feature = "wgpu-backend")]
pub mod shadow_pipeline;
#[cfg(feature = "wgpu-backend")]
pub mod shape_pipeline;
#[cfg(feature = "wgpu-backend")]
pub mod stencil_pipeline;

#[cfg(feature = "wgpu-backend")]
pub use registry::{PipelineId, PipelineRegistry};
