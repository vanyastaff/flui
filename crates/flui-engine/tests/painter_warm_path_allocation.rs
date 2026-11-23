//! A warm painter path's recording allocations: cached path draws may grow
//! the recording arenas geometrically, but must not allocate a temporary
//! vertex buffer for every draw.
//!
//! A dedicated integration-test binary (never the crate's `--lib` unit
//! tests) specifically so installing the counting `#[global_allocator]` here
//! cannot affect any other test binary in this workspace — each `tests/*.rs`
//! file compiles to its own process, and `#[global_allocator]` is
//! process-wide.

#![cfg(feature = "testing")]

use flui_foundation::testing::heap::{CountingAllocator, HeapCounts};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// Warm cached paths may grow the recording arenas geometrically, but must
/// not allocate a temporary vertex buffer for every draw. This uses the real
/// public recorder, with encoding checked outside the measured region.
#[test]
fn warm_path_recording_has_no_per_draw_vertex_allocation() {
    use std::sync::Arc;

    use flui_engine::{WgpuPainter, wgpu};
    use flui_foundation::geometry::Point;
    use flui_painting::{paint::Paint, paint::Path, styling::Color};

    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        apply_limit_buckets: false,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(error) => {
            assert!(
                std::env::var_os("FLUI_REQUIRE_GPU").is_none(),
                "FLUI_REQUIRE_GPU is set, but the warm path allocation test has no GPU adapter: {error}"
            );
            eprintln!("warm path allocation test skipped: no GPU adapter ({error})");
            return;
        }
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Warm path allocation test"),
        ..Default::default()
    }))
    .expect("allocation test device");
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Warm path allocation target"),
        size: wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut painter =
        WgpuPainter::with_shared_device(Arc::clone(&device), queue, format, (128, 128));
    let mut path = Path::new();
    for i in 0..128 {
        let angle = f64::from(i) * std::f64::consts::TAU / 128.0;
        let point = Point::new(64.0 + 48.0 * angle.cos(), 64.0 + 48.0 * angle.sin());
        if i == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close();
    let paint = Paint::stroke(Color::RED, 2.0).with_anti_alias(false);
    painter.begin_frame().expect("allocation frame begins");
    const DRAWS: usize = 128;
    for _ in 0..DRAWS {
        painter.draw_path(&path, &paint);
    }
    let before = HeapCounts::now();
    for _ in 0..DRAWS {
        painter.draw_path(&path, &paint);
    }
    let spent = HeapCounts::now().since(before);
    let (calls, bytes) = (spent.allocations, spent.allocated_bytes);
    eprintln!("warm path recording: {DRAWS} cached draws, {calls} allocations, {bytes} bytes");
    // Allow a small number of geometric arena expansions. A per-path
    // temporary Vec alone would require at least DRAWS allocations.
    assert!(
        calls <= 8,
        "warm draws allocated {calls} times for {DRAWS} paths"
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter
        .render_to_view(&view, &mut encoder)
        .expect("all measured path draws must encode, without a latched budget refusal");
    drop(encoder);
    painter.finish_frame();
}
