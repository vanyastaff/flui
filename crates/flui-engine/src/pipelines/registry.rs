//! Pipeline registry for lazy creation and caching of render pipelines.

use std::collections::HashMap;
use std::sync::Arc;

/// Identifies a specific render pipeline in the registry.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub enum PipelineId {
    /// Instanced rounded-rectangle SDF pipeline
    RectInstanced,
    /// Instanced circle/ellipse SDF pipeline
    CircleInstanced,
    /// Instanced arc (partial circle) pipeline
    ArcInstanced,
    /// Tessellated path fill pipeline
    PathFill,
    /// Tessellated path stroke pipeline
    PathStroke,
    /// Textured image quad pipeline
    Image,
    /// Linear gradient fill pipeline
    LinearGradient,
    /// Radial gradient fill pipeline
    RadialGradient,
    /// Sweep (conic/angular) gradient fill pipeline
    SweepGradient,
    /// Box shadow pipeline
    Shadow,
    /// Gaussian blur downsample pass
    BlurDownsample,
    /// Gaussian blur upsample pass
    BlurUpsample,
    /// Final compositing pipeline
    Compositing,
}

impl PipelineId {
    /// Returns a slice of all pipeline IDs.
    #[must_use]
    pub fn all() -> &'static [PipelineId] {
        &[
            Self::RectInstanced,
            Self::CircleInstanced,
            Self::ArcInstanced,
            Self::PathFill,
            Self::PathStroke,
            Self::Image,
            Self::LinearGradient,
            Self::RadialGradient,
            Self::SweepGradient,
            Self::Shadow,
            Self::BlurDownsample,
            Self::BlurUpsample,
            Self::Compositing,
        ]
    }

    /// Returns a human-readable label for this pipeline (used in wgpu debug labels).
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::RectInstanced => "rect_instanced",
            Self::CircleInstanced => "circle_instanced",
            Self::ArcInstanced => "arc_instanced",
            Self::PathFill => "path_fill",
            Self::PathStroke => "path_stroke",
            Self::Image => "image",
            Self::LinearGradient => "linear_gradient",
            Self::RadialGradient => "radial_gradient",
            Self::SweepGradient => "sweep_gradient",
            Self::Shadow => "shadow",
            Self::BlurDownsample => "blur_downsample",
            Self::BlurUpsample => "blur_upsample",
            Self::Compositing => "compositing",
        }
    }
}

/// Holds all compiled render pipelines. Created once at GpuDevice init.
pub struct PipelineRegistry {
    pipelines: HashMap<PipelineId, Arc<wgpu::RenderPipeline>>,
    bind_group_layout: Arc<wgpu::BindGroupLayout>,
    gradient_bind_group_layout: Arc<wgpu::BindGroupLayout>,
}

impl PipelineRegistry {
    /// Creates all render pipelines for the given device and surface format.
    ///
    /// This sets up a shared bind group layout for `FrameUniforms` and
    /// compiles every pipeline variant. Called once during GPU device initialization.
    #[must_use]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame_uniforms_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group_layout = Arc::new(bind_group_layout);

        let gradient_bind_group_layout = Arc::new(
            super::gradient_pipeline::create_gradient_bind_group_layout(device),
        );

        let mut pipelines = HashMap::new();

        // Shape pipelines (instanced SDF)
        pipelines.insert(
            PipelineId::RectInstanced,
            Arc::new(super::shape_pipeline::create_rect_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );
        pipelines.insert(
            PipelineId::CircleInstanced,
            Arc::new(super::shape_pipeline::create_circle_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );
        pipelines.insert(
            PipelineId::ArcInstanced,
            Arc::new(super::shape_pipeline::create_arc_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );

        // Path pipelines (tessellated geometry)
        pipelines.insert(
            PipelineId::PathFill,
            Arc::new(super::path_pipeline::create_path_fill_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );
        pipelines.insert(
            PipelineId::PathStroke,
            Arc::new(super::path_pipeline::create_path_stroke_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );

        // Image pipeline
        pipelines.insert(
            PipelineId::Image,
            Arc::new(super::image_pipeline::create_image_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );

        // Gradient pipelines
        pipelines.insert(
            PipelineId::LinearGradient,
            Arc::new(super::gradient_pipeline::create_linear_gradient_pipeline(
                device,
                format,
                &bind_group_layout,
                &gradient_bind_group_layout,
            )),
        );
        pipelines.insert(
            PipelineId::RadialGradient,
            Arc::new(super::gradient_pipeline::create_radial_gradient_pipeline(
                device,
                format,
                &bind_group_layout,
                &gradient_bind_group_layout,
            )),
        );
        pipelines.insert(
            PipelineId::SweepGradient,
            Arc::new(super::gradient_pipeline::create_sweep_gradient_pipeline(
                device,
                format,
                &bind_group_layout,
                &gradient_bind_group_layout,
            )),
        );

        // Shadow pipeline
        pipelines.insert(
            PipelineId::Shadow,
            Arc::new(super::shadow_pipeline::create_shadow_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );

        // Blur pipelines
        pipelines.insert(
            PipelineId::BlurDownsample,
            Arc::new(super::blur_pipeline::create_blur_downsample_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );
        pipelines.insert(
            PipelineId::BlurUpsample,
            Arc::new(super::blur_pipeline::create_blur_upsample_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );

        // Compositing pipeline
        pipelines.insert(
            PipelineId::Compositing,
            Arc::new(super::blur_pipeline::create_compositing_pipeline(
                device,
                format,
                &bind_group_layout,
            )),
        );

        Self {
            pipelines,
            bind_group_layout,
            gradient_bind_group_layout,
        }
    }

    /// Look up a pipeline by its ID.
    #[must_use]
    pub fn get(&self, id: PipelineId) -> Option<&Arc<wgpu::RenderPipeline>> {
        self.pipelines.get(&id)
    }

    /// Returns the shared bind group layout used by all pipelines for `FrameUniforms`.
    #[must_use]
    pub fn bind_group_layout(&self) -> &Arc<wgpu::BindGroupLayout> {
        &self.bind_group_layout
    }

    /// Returns the bind group layout for gradient-specific data (group 1).
    ///
    /// Contains gradient uniform buffer (binding 0) and stops storage buffer (binding 1).
    #[must_use]
    pub fn gradient_bind_group_layout(&self) -> &Arc<wgpu::BindGroupLayout> {
        &self.gradient_bind_group_layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_id_all_returns_12() {
        assert_eq!(PipelineId::all().len(), 13);
    }

    #[test]
    fn pipeline_id_labels_unique() {
        let labels: Vec<_> = PipelineId::all().iter().map(|p| p.label()).collect();
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn pipeline_id_labels_non_empty() {
        for id in PipelineId::all() {
            assert!(!id.label().is_empty(), "{id:?} has empty label");
        }
    }

    #[test]
    fn pipeline_id_debug_format() {
        let id = PipelineId::RectInstanced;
        let debug = format!("{id:?}");
        assert_eq!(debug, "RectInstanced");
    }

    #[test]
    fn pipeline_id_clone_eq() {
        let a = PipelineId::Shadow;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn pipeline_id_hash_consistent() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        for id in PipelineId::all() {
            assert!(set.insert(*id), "duplicate pipeline id: {id:?}");
        }
        assert_eq!(set.len(), 13);
    }
}
