//! One ordered draw isolated into independent paint and geometric coverage.
use crate::{
    device_domain::PreparedCost,
    error::{EngineError, EngineResult},
    render_target::RenderTarget,
    resources::GpuResources,
    texture_pool::PooledTexture,
};
use flui_painting::paint::BlendMode;
use wgpu::util::DeviceExt;

fn limit(resource: &'static str, requested: usize, limit: usize) -> EngineError {
    EngineError::PreparedResourceLimit {
        resource,
        requested,
        limit,
    }
}

pub(crate) struct PortableCoveragePipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
}

pub(crate) struct PreparedCoverageBackdrop {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    origin: (u32, u32),
    extent: (u32, u32),
    target_size: (u32, u32),
    format: wgpu::TextureFormat,
}

impl PreparedCoverageBackdrop {
    pub(crate) fn prepare(
        device: &wgpu::Device,
        resources: &mut GpuResources,
        target: RenderTarget<'_>,
        origin: (u32, u32),
        extent: (u32, u32),
        format: wgpu::TextureFormat,
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<Self> {
        PortableCoveragePipeline::validate_device_limits(device, format)?;
        let destination = target
            .texture
            .ok_or(EngineError::CompositeBackdropUnavailable)?;
        let target_size = (destination.width(), destination.height());
        if destination.format() != format
            || destination.dimension() != wgpu::TextureDimension::D2
            || destination.depth_or_array_layers() != 1
            || destination.sample_count() != 1
            || !destination
                .usage()
                .contains(wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT)
            || extent.0 == 0
            || extent.1 == 0
            || origin
                .0
                .checked_add(extent.0)
                .is_none_or(|end| end > target_size.0)
            || origin
                .1
                .checked_add(extent.1)
                .is_none_or(|end| end > target_size.1)
        {
            return Err(EngineError::InvalidRenderTarget {
                reason: "invalid coverage tile destination or extent",
            });
        }
        let bytes = (extent.0 as usize)
            .checked_mul(extent.1 as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| bytes.checked_add(16))
            .ok_or_else(|| limit("coverage tile bytes", usize::MAX, usize::MAX - 1))?;
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: bytes,
            cpu_bytes: std::mem::size_of::<PreparedCoverage>()
                + 16
                + 4 * std::mem::size_of::<wgpu::BindGroupEntry<'_>>(),
            objects: 4,
        })?;
        let snapshot = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Coverage Tile Destination Snapshot"),
            size: wgpu::Extent3d {
                width: extent.0,
                height: extent.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: destination,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin.0,
                    y: origin.1,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &snapshot,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: extent.0,
                height: extent.1,
                depth_or_array_layers: 1,
            },
        );
        Ok(Self {
            view: snapshot.create_view(&wgpu::TextureViewDescriptor::default()),
            texture: snapshot,
            origin,
            extent,
            target_size,
            format,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Consumes the admitted backdrop with independent resolved tiles and frame-scoped GPU resources"
    )]
    pub(crate) fn composite(
        self,
        pipeline: &PortableCoveragePipeline,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        foreground: &PooledTexture,
        coverage: &PooledTexture,
        mode: BlendMode,
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> EngineResult<()> {
        for tile in [foreground, coverage] {
            let texture = tile.texture();
            if texture.dimension() != wgpu::TextureDimension::D2
                || texture.depth_or_array_layers() != 1
                || texture.sample_count() != 1
                || texture.width() < self.extent.0
                || texture.height() < self.extent.1
                || !texture
                    .usage()
                    .contains(wgpu::TextureUsages::TEXTURE_BINDING)
                || !matches!(
                    texture.format(),
                    wgpu::TextureFormat::Rgba8Unorm
                        | wgpu::TextureFormat::Bgra8Unorm
                        | wgpu::TextureFormat::R8Unorm
                )
            {
                return Err(EngineError::InvalidRenderTarget {
                    reason: "invalid coverage tile source",
                });
            }
        }
        PreparedCoverage {
            _source: None,
            _coverage: None,
            source: foreground.view().clone(),
            coverage: coverage.view().clone(),
            destination: self.view,
            _destination: self.texture,
            size: self.extent,
            origin: self.origin,
            target_size: self.target_size,
            format: self.format,
        }
        .composite(pipeline, device, encoder, target, mode, scissor)
    }
}

impl PortableCoveragePipeline {
    pub(crate) fn validate_device_limits(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> EngineResult<()> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Err(EngineError::CompositeBackdropUnavailable);
        }
        let attachment_cost = format
            .target_pixel_byte_cost()
            .and_then(|paint| {
                wgpu::TextureFormat::R8Unorm
                    .target_pixel_byte_cost()
                    .and_then(|coverage| paint.checked_add(coverage))
            })
            .ok_or(EngineError::InvalidRenderTarget {
                reason: "unsupported coverage attachment cost",
            })?;
        let limits = device.limits();
        for (name, requested, cap) in [
            (
                "coverage color attachments",
                2,
                u64::from(limits.max_color_attachments),
            ),
            (
                "coverage attachment bytes per sample",
                u64::from(attachment_cost),
                u64::from(limits.max_color_attachment_bytes_per_sample),
            ),
            (
                "coverage sampled textures",
                3,
                u64::from(limits.max_sampled_textures_per_shader_stage),
            ),
            ("coverage bind groups", 3, u64::from(limits.max_bind_groups)),
            (
                "coverage uniform buffers",
                4,
                u64::from(limits.max_uniform_buffers_per_shader_stage),
            ),
            (
                "coverage uniform binding bytes",
                80,
                limits.max_uniform_buffer_binding_size,
            ),
        ] {
            if cap < requested {
                return Err(limit(name, requested as usize, cap as usize));
            }
        }
        if limits.max_buffer_size < 80 {
            return Err(limit(
                "coverage uniform buffer bytes",
                80,
                limits.max_buffer_size as usize,
            ));
        }
        Ok(())
    }

    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let mut entries = Vec::with_capacity(4);
        for binding in 0..3 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: wgpu::BufferSize::new(16),
            },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Portable Coverage Composite"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Portable Coverage Composite"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Portable Coverage Composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/portable_coverage.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Portable Coverage Composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            format,
        }
    }
}

pub(crate) struct PreparedCoverage {
    _source: Option<wgpu::Texture>,
    _coverage: Option<wgpu::Texture>,
    _destination: wgpu::Texture,
    source: wgpu::TextureView,
    coverage: wgpu::TextureView,
    destination: wgpu::TextureView,
    size: (u32, u32),
    origin: (u32, u32),
    target_size: (u32, u32),
    format: wgpu::TextureFormat,
}

impl PreparedCoverage {
    pub(crate) fn prepare(
        device: &wgpu::Device,
        resources: &mut GpuResources,
        target: RenderTarget<'_>,
        region: (u32, u32, u32, u32),
        encoder: &mut wgpu::CommandEncoder,
    ) -> EngineResult<Self> {
        let texture = target
            .texture
            .ok_or(EngineError::CompositeBackdropUnavailable)?;
        let format = texture.format();
        PortableCoveragePipeline::validate_device_limits(device, format)?;
        if texture.dimension() != wgpu::TextureDimension::D2
            || texture.depth_or_array_layers() != 1
            || texture.sample_count() != 1
            || !texture
                .usage()
                .contains(wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::RENDER_ATTACHMENT)
        {
            return Err(EngineError::InvalidRenderTarget {
                reason: "coverage requires a single-layer, single-sample D2 COPY_SRC render attachment",
            });
        }
        let size = (region.2, region.3);
        let origin = (region.0, region.1);
        let target_size = (texture.width(), texture.height());
        if origin
            .0
            .checked_add(size.0)
            .is_none_or(|end| end > target_size.0)
            || origin
                .1
                .checked_add(size.1)
                .is_none_or(|end| end > target_size.1)
        {
            return Err(EngineError::InvalidRenderTarget {
                reason: "coverage crop exceeds target extent",
            });
        }
        if size.0 == 0
            || size.1 == 0
            || size.0.max(size.1) > device.limits().max_texture_dimension_2d
        {
            return Err(EngineError::InvalidTargetSize {
                width: size.0,
                height: size.1,
            });
        }
        let bytes = (size.0 as usize)
            .checked_mul(size.1 as usize)
            .and_then(|pixels| pixels.checked_mul(9))
            .and_then(|bytes| bytes.checked_add(48))
            .ok_or_else(|| limit("coverage prepared bytes", usize::MAX, usize::MAX - 1))?;
        resources.reserve_prepared(PreparedCost {
            gpu_bytes: bytes,
            cpu_bytes: std::mem::size_of::<Self>()
                + 48
                + 5 * std::mem::size_of::<wgpu::BindGroupEntry<'_>>(),
            objects: 10,
        })?;
        let create = |label, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let isolation_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let source = create("Isolated Paint", format, isolation_usage);
        let coverage = create(
            "Independent Geometry Coverage",
            wgpu::TextureFormat::R8Unorm,
            isolation_usage,
        );
        let destination = create(
            "Coverage Destination Snapshot",
            format,
            wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin.0,
                    y: origin.1,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &destination,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        Ok(Self {
            source: source.create_view(&wgpu::TextureViewDescriptor::default()),
            coverage: coverage.create_view(&wgpu::TextureViewDescriptor::default()),
            destination: destination.create_view(&wgpu::TextureViewDescriptor::default()),
            _source: Some(source),
            _coverage: Some(coverage),
            _destination: destination,
            size,
            origin,
            target_size,
            format,
        })
    }

    pub(crate) fn source_view(&self) -> &wgpu::TextureView {
        &self.source
    }
    pub(crate) fn coverage_view(&self) -> &wgpu::TextureView {
        &self.coverage
    }

    pub(crate) fn composite(
        self,
        pipeline: &PortableCoveragePipeline,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        mode: BlendMode,
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> EngineResult<()> {
        let mode = mode_index(mode)?;
        if pipeline.format != self.format {
            return Err(EngineError::CompositeBackdropUnavailable);
        }
        let scissor = scissor.unwrap_or((0, 0, self.target_size.0, self.target_size.1));
        if scissor.2 == 0 || scissor.3 == 0 {
            return Ok(());
        }
        if scissor
            .0
            .checked_add(scissor.2)
            .is_none_or(|right| right > self.target_size.0)
            || scissor
                .1
                .checked_add(scissor.3)
                .is_none_or(|bottom| bottom > self.target_size.1)
        {
            return Err(EngineError::InvalidTargetSize {
                width: scissor.2,
                height: scissor.3,
            });
        }
        let left = scissor.0.max(self.origin.0);
        let top = scissor.1.max(self.origin.1);
        let right = (scissor.0 + scissor.2).min(self.origin.0 + self.size.0);
        let bottom = (scissor.1 + scissor.3).min(self.origin.1 + self.size.1);
        if right <= left || bottom <= top {
            return Ok(());
        }
        let scissor = (left, top, right - left, bottom - top);
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Portable Coverage Mode"),
            contents: bytemuck::cast_slice(&[mode, self.origin.0, self.origin.1, 0]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Portable Coverage Composite"),
            layout: &pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.coverage),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.destination),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Portable Coverage Composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &binding, &[]);
        pass.set_scissor_rect(scissor.0, scissor.1, scissor.2, scissor.3);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

fn mode_index(mode: BlendMode) -> EngineResult<u32> {
    Ok(match mode {
        BlendMode::Clear => 0,
        BlendMode::Src => 1,
        BlendMode::Dst => 2,
        BlendMode::SrcOver => 3,
        BlendMode::DstOver => 4,
        BlendMode::SrcIn => 5,
        BlendMode::DstIn => 6,
        BlendMode::SrcOut => 7,
        BlendMode::DstOut => 8,
        BlendMode::SrcATop => 9,
        BlendMode::DstATop => 10,
        BlendMode::Xor => 11,
        BlendMode::Plus => 12,
        BlendMode::Modulate => 13,
        _ => return Err(EngineError::UnsupportedCoverageBlend { mode }),
    })
}
