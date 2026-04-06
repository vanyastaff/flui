//! GPU pipeline management
//!
//! Creates and caches wgpu render pipelines for each primitive type.
//! Pipelines are lazily initialized and shared across frames.

pub mod registry;
pub mod shape_pipeline;
pub mod path_pipeline;
pub mod image_pipeline;
pub mod gradient_pipeline;
pub mod shadow_pipeline;
pub mod blur_pipeline;
