//! Pipeline creation for advanced effects (gradients, shadows)
//!
//! This module provides:
//! - [`GradientPipelines`], the per-blend-mode cache behind the linear, radial
//!   and sweep gradients (plus their shared bind-group layout, stops buffer,
//!   and pipeline layout)
//! - the analytical shadow pipeline
//!
//! All of them are specs over [`super::pipelines::create_unit_quad_pipeline`],
//! the shared unit-quad instanced constructor.

use std::collections::HashMap;

use flui_painting::BlendMode;

use super::effects::GradientStop;
use super::pipeline::{CoverageShaderSources, select_coverage_blend};
use super::pipelines::{QuadPipelineSpec, create_unit_quad_pipeline};

/// Create bind group layout for gradient stops (storage buffer)
pub fn create_gradient_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Gradient Stops Bind Group Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// Maximum number of gradient stop slots in the GPU buffer.
///
/// Each gradient consumes up to 8 slots; this cap allows 100 gradients per
/// frame. The three `*_gradient_rect` methods in `painter::gradient` enforce this
/// limit by dropping instances that would overflow it rather than writing
/// past the end of the buffer.
pub const MAX_GRADIENT_STOPS: usize = 8 * 100;

/// Create gradient stops buffer with initial capacity
pub fn create_gradient_stops_buffer(device: &wgpu::Device) -> wgpu::Buffer {
    // Max 8 stops per gradient, support up to 100 gradients per frame
    let capacity = MAX_GRADIENT_STOPS;
    let size = (capacity * std::mem::size_of::<GradientStop>()) as u64;

    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Gradient Stops Buffer"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Create the shared pipeline layout for all gradient pipelines.
///
/// Using the same `PipelineLayout` object ensures bind-group compatibility when
/// switching pipelines within a render pass (a WebGPU requirement), which the
/// per-blend-mode cache switches between constantly — hence
/// [`GradientPipelines`] holding exactly one of these for its whole life.
fn create_gradient_pipeline_layout(
    device: &wgpu::Device,
    viewport_bind_group_layout: &wgpu::BindGroupLayout,
    gradient_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Shared Gradient Pipeline Layout"),
        bind_group_layouts: &[
            Some(viewport_bind_group_layout),
            Some(gradient_bind_group_layout),
        ],
        immediate_size: 0,
    })
}

/// Which of the three instanced gradient shaders a draw uses.
///
/// They are separate modules with separate `VertexOutput` layouts, separate
/// instance buffers, and — since the blend mode became part of the key —
/// separate cache entries per mode. Naming the axis makes a pipeline lookup
/// say which gradient it is asking about instead of which field it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GradientKind {
    /// Colour interpolated along a line.
    Linear,
    /// Colour interpolated with distance from a centre.
    Radial,
    /// Colour interpolated with angle about a centre.
    Sweep,
}

impl GradientKind {
    /// The two assemblies of this kind's shader.
    fn shader_sources(self) -> CoverageShaderSources {
        match self {
            Self::Linear => super::shaders::LINEAR_GRADIENT,
            Self::Radial => super::shaders::RADIAL_GRADIENT,
            Self::Sweep => super::shaders::SWEEP_GRADIENT,
        }
    }

    /// This kind's per-instance vertex layout.
    fn instance_layout(self) -> wgpu::VertexBufferLayout<'static> {
        match self {
            Self::Linear => super::instancing::LinearGradientInstance::desc(),
            Self::Radial => super::instancing::RadialGradientInstance::desc(),
            Self::Sweep => super::instancing::SweepGradientInstance::desc(),
        }
    }

    /// The word this kind goes by in a GPU debug label.
    fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Radial => "Radial",
            Self::Sweep => "Sweep",
        }
    }
}

/// The instanced gradient pipelines, one per (kind, blend mode) actually drawn.
///
/// Gradients are instanced rather than tessellated, so they never pass through
/// `add_tessellated_with_key` and the blend funnel that keys the shape path's
/// pipelines. Before this cache existed there were three gradient pipelines
/// with a hard-coded `ALPHA_BLENDING` blend state, and a gradient paint's blend
/// mode was accepted, carried, and dropped: every mode rendered as `SrcOver`.
///
/// The shape of the cache follows `PipelineCache`, and the coverage decision is
/// literally the same function ([`select_coverage_blend`]) — a gradient that
/// honours `Clear` but folds coverage into the source alpha would feather its
/// clip edge wrong, which is a subtler defect than the one this fixes.
///
/// Lazy rather than eager because the product is 3 kinds × 14 fixed-function
/// modes and a frame draws a handful; the SSAA tile composite cache next door
/// is built the same way for the same reason.
pub(crate) struct GradientPipelines {
    /// Shared by every gradient pipeline, so switching between them inside one
    /// render pass keeps the bind groups valid (a WebGPU requirement — see
    /// [`create_gradient_pipeline_layout`]).
    layout: wgpu::PipelineLayout,

    /// Format of the render target these pipelines were built for.
    surface_format: wgpu::TextureFormat,

    /// Whether the device can compile the second-source assembly at all.
    ///
    /// `false` withholds the coverage correction for every mode that would
    /// otherwise take it — the documented ADR-0057 fallback, identical to what
    /// `PipelineCache` does on the same device.
    second_source_available: bool,

    /// One pipeline per (kind, mode) encountered, created on first use.
    cache: HashMap<(GradientKind, BlendMode), wgpu::RenderPipeline>,
}

impl GradientPipelines {
    /// Build the shared layout; the pipelines themselves wait for a draw.
    pub(crate) fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        viewport_bind_group_layout: &wgpu::BindGroupLayout,
        gradient_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        Self {
            layout: create_gradient_pipeline_layout(
                device,
                viewport_bind_group_layout,
                gradient_bind_group_layout,
            ),
            surface_format,
            second_source_available: device
                .features()
                .contains(wgpu::Features::DUAL_SOURCE_BLENDING),
            cache: HashMap::new(),
        }
    }

    /// Ensure the pipeline for `(kind, mode)` exists, creating it on first use.
    ///
    /// Split from [`Self::get`] so a caller can take the `&mut self` creation
    /// borrow and the `&self` lookup borrow at different statements — the
    /// render pass that draws the gradients holds `&PipelineSet` for the whole
    /// of its life. Same split, same reason, as `ensure_ssaa_tile_composite`.
    ///
    /// # Panics (debug)
    ///
    /// Debug-panics on an advanced (dst-read) mode. Those cannot be expressed
    /// as a fixed-function blend at all; `dispatch_shader_rect` diverts them
    /// into `DrawItem::AdvancedShape` for `flush_advanced_layer` before a
    /// gradient run is ever recorded, so one arriving here is a routing error.
    pub(crate) fn ensure(&mut self, device: &wgpu::Device, kind: GradientKind, mode: BlendMode) {
        debug_assert!(
            !mode.is_advanced(),
            "advanced blend mode {mode:?} reached GradientPipelines; advanced \
             gradients must divert to DrawItem::AdvancedShape in dispatch_shader_rect",
        );
        if self.cache.contains_key(&(kind, mode)) {
            return;
        }
        let pipeline = self.create(device, kind, mode);
        self.cache.insert((kind, mode), pipeline);
    }

    /// The pipeline for `(kind, mode)`.
    ///
    /// # Panics
    ///
    /// Panics if [`Self::ensure`] was not called for the same pair first.
    pub(crate) fn get(&self, kind: GradientKind, mode: BlendMode) -> &wgpu::RenderPipeline {
        self.cache.get(&(kind, mode)).unwrap_or_else(|| {
            panic!(
                "no cached {kind:?} gradient pipeline for {mode:?}; call \
                 GradientPipelines::ensure before beginning the render pass"
            )
        })
    }

    fn create(
        &self,
        device: &wgpu::Device,
        kind: GradientKind,
        mode: BlendMode,
    ) -> wgpu::RenderPipeline {
        let sources = kind.shader_sources();
        let selection = select_coverage_blend(
            mode,
            sources.folded,
            self.second_source_available
                .then_some(sources.second_source),
        );
        let kind_label = kind.label();
        create_unit_quad_pipeline(
            device,
            self.surface_format,
            &self.layout,
            &QuadPipelineSpec {
                shader_label: &format!("{kind_label} Gradient Shader ({mode:?})"),
                pipeline_label: &format!("{kind_label} Gradient Pipeline ({mode:?})"),
                shader_source: selection.shader,
                instance_layout: kind.instance_layout(),
                blend: selection.blend_state,
                constants: &selection.constants,
            },
        )
    }
}

/// Create shadow rendering pipeline
pub fn create_shadow_pipeline(
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    viewport_bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Shadow Pipeline Layout"),
        bind_group_layouts: &[Some(viewport_bind_group_layout)],
        immediate_size: 0,
    });

    create_unit_quad_pipeline(
        device,
        surface_format,
        &pipeline_layout,
        &QuadPipelineSpec {
            shader_label: "Shadow Shader",
            pipeline_label: "Shadow Pipeline",
            shader_source: include_str!("shaders/effects/shadow.wgsl"),
            instance_layout: super::instancing::ShadowInstance::desc(),
            blend: wgpu::BlendState::ALPHA_BLENDING,
            constants: &[],
        },
    )
}
