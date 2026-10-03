//! First-use clip preparation plus submission completion, including deferred driver work.
//! Set FLUI_BENCH_FALLBACK=1 to require a software adapter; prints the actual adapter.
use criterion::{BatchSize, Criterion};
use flui_engine::{WgpuPainter, wgpu};
use flui_foundation::geometry::{RRect, Rect};
use flui_painting::{
    Paint,
    paint::{Clip, Path},
    styling::Color,
};
use std::{sync::Arc, time::Duration};

pub(super) fn bench(c: &mut Criterion) {
    let fallback = std::env::var_os("FLUI_BENCH_FALLBACK").is_some();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        force_fallback_adapter: fallback,
        apply_limit_buckets: false,
        ..Default::default()
    }))
    .expect("benchmark requires an adapter");
    println!("clip startup adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features() & wgpu::Features::DUAL_SOURCE_BLENDING,
        ..Default::default()
    }))
    .expect("benchmark device");
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("clip startup target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let mut group = c.benchmark_group("clip_first_use_prepare_submit_wait");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));
    for kind in ["rect_hard", "rect_aa", "curve_aa", "path_aa", "mixed_aa"] {
        group.bench_function(kind, |b| {
            b.iter_batched(
                || {
                    let mut painter = WgpuPainter::with_shared_device(
                        Arc::clone(&device),
                        Arc::clone(&queue),
                        format,
                        (64, 64),
                    );
                    painter.begin_frame().expect("begin clip frame");
                    let bounds = Rect::from_xywh(8.25, 8.0, 48.0, 48.0);
                    match kind {
                        "rect_hard" => painter.clip_rect(bounds, Clip::HardEdge),
                        "rect_aa" => painter.clip_rect(bounds, Clip::AntiAlias),
                        "curve_aa" => painter
                            .clip_rrect(RRect::from_rect_circular(bounds, 8.0), Clip::AntiAlias),
                        _ => {
                            let mut path = Path::new();
                            path.add_rect(bounds);
                            painter.clip_path(&path);
                            if kind == "mixed_aa" {
                                painter.clip_rrect(
                                    RRect::from_rect_circular(bounds, 8.0),
                                    Clip::AntiAlias,
                                );
                            }
                        }
                    }
                    painter.draw_rect(
                        Rect::from_xywh(0.0, 0.0, 64.0, 64.0),
                        &Paint::fill(Color::RED),
                    );
                    painter
                },
                |mut painter| {
                    let mut encoder =
                        device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                    painter
                        .render_to_texture(&texture, &mut encoder)
                        .expect("clip frame");
                    let submission = queue.submit([encoder.finish()]);
                    device
                        .poll(wgpu::PollType::Wait {
                            submission_index: Some(submission),
                            timeout: None,
                        })
                        .expect("clip completion");
                    painter.finish_frame();
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}
