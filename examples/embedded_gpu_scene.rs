//! Same-device GPU content embedded in FLUI's 2D painter.
//!
//! Run `cargo run --example embedded_gpu_scene`. A depth-tested cube is rendered
//! to a sampled GPU texture, then FLUI paints that texture between 2D UI shapes.
//! This is an engine embedder example, not a managed widget or a 3D scene API.

use flui_engine::{WgpuPainter, wgpu};
use flui_foundation::geometry::Rect;
use flui_painting::{
    Paint,
    paint::{FilterQuality, TextureId},
    styling::Color,
};
use std::{sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

const SIDE: u32 = 512;

struct Gpu {
    // Surface retains its window handle owner; no borrowed native handles.
    surface: Option<wgpu::Surface<'static>>,
    window: Option<Arc<Window>>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    config: wgpu::SurfaceConfiguration,
    painter: WgpuPainter,
    cube: wgpu::TextureView,
    depth: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bindings: wgpu::BindGroup,
    started: Instant,
}

impl Gpu {
    async fn new(window: Option<Arc<Window>>) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = window
            .as_ref()
            .map(|window| instance.create_surface(Arc::clone(window)))
            .transpose()?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: surface.as_ref(),
                ..Default::default()
            })
            .await?;
        eprintln!("GPU: {:?}", adapter.get_info());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("embedded GPU scene"),
                ..Default::default()
            })
            .await?;
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let config = if let Some(surface) = &surface {
            let size = window
                .as_ref()
                .expect("BUG: surface has a window")
                .inner_size();
            let mut config = surface
                .get_default_config(&adapter, size.width.max(1), size.height.max(1))
                .ok_or_else(|| anyhow::anyhow!("surface has no configuration"))?;
            config.format = surface
                .get_capabilities(&adapter)
                .formats
                .into_iter()
                .find(|format| !format.is_srgb())
                .ok_or_else(|| anyhow::anyhow!("SDR example requires non-sRGB surface"))?;
            surface.configure(&device, &config);
            config
        } else {
            wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8Unorm,
                color_space: wgpu::SurfaceColorSpace::Srgb,
                width: 800,
                height: 700,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            }
        };
        let mut painter = WgpuPainter::with_shared_device(
            Arc::clone(&device),
            Arc::clone(&queue),
            config.format,
            (config.width, config.height),
        );
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cube color"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let cube = texture.create_view(&wgpu::TextureViewDescriptor::default());
        painter.external_texture_registry_mut().register(
            TextureId::new(1),
            texture,
            SIDE,
            SIDE,
            true,
            true,
        );
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cube depth"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cube angle"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cube angle layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cube angle"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth-tested cube"),
            source: wgpu::ShaderSource::Wgsl(include_str!("embedded_gpu_scene.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cube"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cube"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            surface,
            window,
            device,
            queue,
            config,
            painter,
            cube,
            depth,
            pipeline,
            uniform,
            bindings,
            started: Instant::now(),
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        if width > 0 && height > 0 {
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, &self.config);
            }
            self.painter.resize(width, height);
        }
    }

    fn draw(&mut self) -> anyhow::Result<()> {
        if self.config.width == 0 || self.config.height == 0 {
            return Ok(());
        }
        let output = match self
            .surface
            .as_ref()
            .expect("BUG: interactive draw has surface")
            .get_current_texture()
        {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                if let Some(surface) = &self.surface {
                    surface.configure(&self.device, &self.config);
                }
                return Ok(());
            }
            other @ wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("surface acquisition failed: {other:?}")
            }
        };
        let target = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.encode_frame(&target, self.started.elapsed().as_secs_f32() * 0.65)?;
        if let Some(window) = &self.window {
            window.pre_present_notify();
        }
        self.queue.present(output);
        Ok(())
    }

    fn encode_frame(&mut self, target: &wgpu::TextureView, angle: f32) -> anyhow::Result<()> {
        let mut bytes = [0_u8; 16];
        bytes[..4].copy_from_slice(&angle.to_le_bytes());
        self.queue.write_buffer(&self.uniform, 0, &bytes);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D producer"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.cube,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bindings, &[]);
            pass.draw(0..36, 0..1);
        }
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("surface clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.04,
                            g: 0.055,
                            b: 0.08,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let width = f64::from(self.config.width);
        let height = f64::from(self.config.height);
        let side = (width - 64.0).min(height - 112.0).max(1.0);
        let left = (width - side) / 2.0;
        let top = (height - side) / 2.0;
        self.painter.begin_frame()?;
        self.painter.draw_rect(
            Rect::from_ltrb(left - 8.0, top - 8.0, left + side + 8.0, top + side + 8.0),
            &Paint::fill(Color::rgb(35, 46, 67)),
        );
        self.painter.draw_texture(
            TextureId::new(1),
            Rect::from_ltrb(left, top, left + side, top + side),
            None,
            FilterQuality::Medium,
            1.0,
        );
        // Foreground UI overlaps the GPU viewport: ordering is observable.
        self.painter.draw_rect(
            Rect::from_ltrb(
                left - 16.0,
                top + side * 0.72,
                left + side * 0.35,
                top + side * 0.72 + 24.0,
            ),
            &Paint::fill(Color::rgb(230, 175, 60)),
        );
        let progress = f64::from(angle.sin() * 0.5 + 0.5);
        self.painter.draw_rect(
            Rect::from_ltrb(
                32.0,
                height - 30.0,
                32.0 + (width - 64.0).max(1.0) * progress,
                height - 22.0,
            ),
            &Paint::fill(Color::rgb(85, 195, 175)),
        );
        let rendered = self.painter.render_to_view(target, &mut encoder);
        let rendered = rendered.and_then(|()| self.painter.submit_encoder(encoder).map(|_| ()));
        self.painter.finish_frame();
        rendered?;
        Ok(())
    }
}

#[derive(Default)]
struct App {
    gpu: Option<Gpu>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let result = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("FLUI engine: GPU cube + 2D UI")
                    .with_inner_size(winit::dpi::LogicalSize::new(800.0, 700.0)),
            )
            .map_err(anyhow::Error::from)
            .and_then(|window| pollster::block_on(Gpu::new(Some(Arc::new(window)))));
        match result {
            Ok(gpu) => {
                gpu.window
                    .as_ref()
                    .expect("BUG: interactive GPU has window")
                    .request_redraw();
                self.gpu = Some(gpu);
            }
            Err(error) => {
                eprintln!("GPU initialization failed: {error:#}");
                event_loop.exit();
            }
        }
    }
    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.gpu = None;
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        if id
            != gpu
                .window
                .as_ref()
                .expect("BUG: interactive GPU has window")
                .id()
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                gpu.resize(size.width, size.height);
                gpu.window
                    .as_ref()
                    .expect("BUG: interactive GPU has window")
                    .request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = gpu.draw() {
                    eprintln!(
                        "GPU frame failed; restart this example to recreate its device: {error:#}"
                    );
                    event_loop.exit();
                } else if gpu.config.width > 0 && gpu.config.height > 0 {
                    gpu.window
                        .as_ref()
                        .expect("BUG: interactive GPU has window")
                        .request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn capture(path: &std::path::Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        path.is_absolute(),
        "--capture requires an absolute PNG path"
    );
    let mut gpu = pollster::block_on(Gpu::new(None))?;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("capture target"),
        size: wgpu::Extent3d {
            width: 800,
            height: 700,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let readback = |gpu: &mut Gpu| -> anyhow::Result<Vec<u8>> {
        let row = 3328_u32;
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture staging"),
            size: u64::from(row) * 700,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(700),
                },
            },
            wgpu::Extent3d {
                width: 800,
                height: 700,
                depth_or_array_layers: 1,
            },
        );
        gpu.painter.submit_encoder(encoder)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        gpu.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })?;
        receiver.recv()??;
        let mapped = staging.slice(..).get_mapped_range()?;
        let mut pixels = Vec::with_capacity(800 * 700 * 4);
        for row_bytes in mapped.chunks_exact(row as usize) {
            pixels.extend_from_slice(&row_bytes[..3200]);
        }
        drop(mapped);
        staging.unmap();
        Ok(pixels)
    };
    let check =
        |pixels: &[u8], x: usize, y: usize, expected: [u8; 4], label: &str| -> anyhow::Result<()> {
            let actual = &pixels[(y * 800 + x) * 4..(y * 800 + x) * 4 + 4];
            anyhow::ensure!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3),
                "{label} at ({x},{y}): {actual:?}, expected {expected:?}"
            );
            Ok(())
        };
    gpu.encode_frame(&view, 0.0)?;
    let initial = readback(&mut gpu)?;
    check(
        &initial,
        400,
        350,
        [217, 64, 51, 255],
        "near red face must occlude later cyan face",
    )?;
    check(
        &initial,
        120,
        490,
        [230, 175, 60, 255],
        "foreground FLUI rectangle",
    )?;
    check(
        &initial,
        350,
        675,
        [85, 195, 175, 255],
        "animated FLUI progress",
    )?;
    check(&initial, 20, 20, [10, 14, 20, 255], "background")?;
    gpu.encode_frame(&view, std::f32::consts::FRAC_PI_2)?;
    let rotated = readback(&mut gpu)?;
    check(
        &rotated,
        400,
        350,
        [230, 166, 51, 255],
        "rotation exposes near yellow face",
    )?;
    image::save_buffer(path, &rotated, 800, 700, image::ColorType::Rgba8)?;
    gpu.painter
        .external_texture_registry_mut()
        .unregister(TextureId::new(1));
    println!(
        "verified depth, rotation, 2D overlay and progress; capture {}",
        path.display()
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 2 && arguments[0] == "--capture" {
        return capture(std::path::Path::new(&arguments[1]));
    }
    anyhow::ensure!(
        arguments.is_empty(),
        "usage: embedded_gpu_scene [--capture <absolute PNG path>]"
    );
    EventLoop::new()?.run_app(&mut App::default())?;
    Ok(())
}
