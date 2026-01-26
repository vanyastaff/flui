//! Render pipeline management
//!
//! This module manages all render pipelines for different primitive types.
//! Pipelines are created once and cached for reuse.

use std::sync::Arc;
use wgpu::{
    BlendState, ColorTargetState, ColorWrites, Device, FragmentState, FrontFace, MultisampleState,
    PipelineLayoutDescriptor, PolygonMode, PrimitiveState, PrimitiveTopology, RenderPipeline,
    RenderPipelineDescriptor, ShaderModule, TextureFormat, VertexState,
};

use super::vertex::{ImageInstance, PathVertex, RectInstance, RectVertex};

/// Pipeline cache for all render pipelines
///
/// Stores compiled render pipelines for all primitive types.
/// Pipelines are expensive to create, so we create them once and reuse.
pub struct PipelineCache {
    /// Pipeline for rectangle rendering
    rect_pipeline: RenderPipeline,

    /// Pipeline for path rendering
    path_pipeline: RenderPipeline,

    /// Pipeline for image rendering
    image_pipeline: RenderPipeline,
}

impl PipelineCache {
    /// Create a new pipeline cache with all pipelines compiled
    ///
    /// # Arguments
    ///
    /// * `device` - GPU device
    /// * `rect_shader` - Compiled rectangle shader
    /// * `path_shader` - Compiled path shader
    /// * `image_shader` - Compiled image shader
    /// * `surface_format` - Target surface texture format
    pub fn new(
        device: &Device,
        rect_shader: &ShaderModule,
        path_shader: &ShaderModule,
        image_shader: &ShaderModule,
        surface_format: TextureFormat,
    ) -> Self {
        tracing::debug!("Creating render pipelines...");

        let rect_pipeline = Self::create_rect_pipeline(device, rect_shader, surface_format);
        let path_pipeline = Self::create_path_pipeline(device, path_shader, surface_format);
        let image_pipeline = Self::create_image_pipeline(device, image_shader, surface_format);

        tracing::debug!("All render pipelines created");

        Self {
            rect_pipeline,
            path_pipeline,
            image_pipeline,
        }
    }

    /// Get rectangle rendering pipeline
    #[must_use]
    pub fn rect_pipeline(&self) -> &RenderPipeline {
        &self.rect_pipeline
    }

    /// Get path rendering pipeline
    #[must_use]
    pub fn path_pipeline(&self) -> &RenderPipeline {
        &self.path_pipeline
    }

    /// Get image rendering pipeline
    #[must_use]
    pub fn image_pipeline(&self) -> &RenderPipeline {
        &self.image_pipeline
    }

    /// Create rectangle rendering pipeline
    fn create_rect_pipeline(
        device: &Device,
        shader: &ShaderModule,
        surface_format: TextureFormat,
    ) -> RenderPipeline {
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Rect Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Rect Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[RectVertex::desc(), RectInstance::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: surface_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        })
    }

    /// Create path rendering pipeline
    fn create_path_pipeline(
        device: &Device,
        shader: &ShaderModule,
        surface_format: TextureFormat,
    ) -> RenderPipeline {
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Path Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Path Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[PathVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: surface_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        })
    }

    /// Create image rendering pipeline
    fn create_image_pipeline(
        device: &Device,
        shader: &ShaderModule,
        surface_format: TextureFormat,
    ) -> RenderPipeline {
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Image Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Image Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[ImageInstance::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: surface_format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        })
    }
}

/// Builder for creating custom render pipelines
///
/// Provides a fluent API for pipeline creation with sensible defaults.
pub struct PipelineBuilder<'a> {
    device: &'a Device,
    label: Option<&'a str>,
    shader: &'a ShaderModule,
    vertex_entry: &'a str,
    fragment_entry: &'a str,
    surface_format: TextureFormat,
    blend_state: BlendState,
    topology: PrimitiveTopology,
}

impl<'a> PipelineBuilder<'a> {
    /// Create a new pipeline builder
    pub fn new(
        device: &'a Device,
        shader: &'a ShaderModule,
        surface_format: TextureFormat,
    ) -> Self {
        Self {
            device,
            label: None,
            shader,
            vertex_entry: "vs_main",
            fragment_entry: "fs_main",
            surface_format,
            blend_state: BlendState::ALPHA_BLENDING,
            topology: PrimitiveTopology::TriangleList,
        }
    }

    /// Set debug label
    #[must_use]
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Set vertex shader entry point
    #[must_use]
    pub fn vertex_entry(mut self, entry: &'a str) -> Self {
        self.vertex_entry = entry;
        self
    }

    /// Set fragment shader entry point
    #[must_use]
    pub fn fragment_entry(mut self, entry: &'a str) -> Self {
        self.fragment_entry = entry;
        self
    }

    /// Set blend state
    #[must_use]
    pub fn blend_state(mut self, blend: BlendState) -> Self {
        self.blend_state = blend;
        self
    }

    /// Set primitive topology
    #[must_use]
    pub fn topology(mut self, topology: PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }

    /// Build the pipeline
    ///
    /// Note: This is a simplified version without vertex buffers.
    /// Full implementation requires vertex buffer descriptors.
    #[must_use]
    pub fn build(self) -> RenderPipeline {
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: self.label,
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });

        self.device
            .create_render_pipeline(&RenderPipelineDescriptor {
                label: self.label,
                layout: Some(&pipeline_layout),
                vertex: VertexState {
                    module: self.shader,
                    entry_point: Some(self.vertex_entry),
                    buffers: &[], // Caller should set vertex buffers separately
                    compilation_options: Default::default(),
                },
                fragment: Some(FragmentState {
                    module: self.shader,
                    entry_point: Some(self.fragment_entry),
                    targets: &[Some(ColorTargetState {
                        format: self.surface_format,
                        blend: Some(self.blend_state),
                        write_mask: ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: PrimitiveState {
                    topology: self.topology,
                    strip_index_format: None,
                    front_face: FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_cache_exists() {
        // Compile-time check for PipelineCache API
        let _ = std::marker::PhantomData::<PipelineCache>;
    }

    #[test]
    fn test_pipeline_builder_exists() {
        // Compile-time check for PipelineBuilder API
        let _ = std::marker::PhantomData::<PipelineBuilder>;
    }
}
