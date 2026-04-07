//! GPU integration tests for the new engine architecture.
//! Run with: cargo test -p flui-engine --features enable-wgpu-tests

#![cfg(feature = "enable-wgpu-tests")]

use flui_engine::context::gpu_device::GpuDevice;
use flui_engine::debug::DebugEncoder;
use flui_engine::pipelines::registry::PipelineId;
use flui_layer::{CanvasLayer, Layer, Scene};
use flui_types::geometry::units::px;
use flui_types::geometry::Size;

#[test]
fn gpu_device_headless_creates() {
    let gpu = GpuDevice::new_headless().expect("should create headless GPU device");
    assert!(!gpu.capabilities().adapter_name.is_empty());
}

#[test]
fn gpu_device_has_all_pipelines() {
    let gpu = GpuDevice::new_headless().expect("should create headless GPU device");
    for id in PipelineId::all() {
        assert!(
            gpu.pipelines().get(*id).is_some(),
            "pipeline {:?} missing",
            id
        );
    }
}

#[test]
fn debug_encoder_with_empty_scene() {
    let scene = Scene::empty(Size::new(px(800.0), px(600.0)));
    let mut encoder = DebugEncoder::new();
    encoder.process_scene(&scene);
    assert_eq!(encoder.command_count(), 0);
}

#[test]
fn debug_encoder_with_canvas_layer() {
    let canvas = CanvasLayer::new();
    let scene = Scene::from_layer(Size::new(px(800.0), px(600.0)), Layer::Canvas(canvas), 0);
    let mut encoder = DebugEncoder::new();
    encoder.process_scene(&scene);
    // Empty canvas = 0 commands
    assert_eq!(encoder.command_count(), 0);
}

#[test]
fn debug_encoder_reset_clears_state() {
    let scene = Scene::from_layer(
        Size::new(px(800.0), px(600.0)),
        Layer::Canvas(CanvasLayer::new()),
        0,
    );
    let mut encoder = DebugEncoder::new();
    encoder.process_scene(&scene);
    encoder.reset();
    assert_eq!(encoder.command_count(), 0);
    assert_eq!(encoder.rect_count(), 0);
    assert_eq!(encoder.text_run_count(), 0);
}

// ---------------------------------------------------------------------------
// Headless render-to-texture tests
// ---------------------------------------------------------------------------

#[test]
fn headless_clear_to_white() {
    use flui_engine::context::headless_render::read_texture_to_rgba;

    let gpu = GpuDevice::new_headless().expect("GPU init");
    let (texture, view) = gpu.create_render_texture(64, 64);

    let mut enc = gpu.device().create_command_encoder(&Default::default());
    {
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    gpu.queue().submit(std::iter::once(enc.finish()));

    let pixels = read_texture_to_rgba(&gpu, &texture, 64, 64);
    assert_eq!(pixels.len(), 64 * 64 * 4);
    // Bgra8Unorm format: pixel[0]=B, pixel[1]=G, pixel[2]=R, pixel[3]=A
    // White means all channels should be bright
    assert!(
        pixels[0] > 200,
        "B channel should be bright, got {}",
        pixels[0]
    );
    assert!(
        pixels[1] > 200,
        "G channel should be bright, got {}",
        pixels[1]
    );
    assert!(
        pixels[2] > 200,
        "R channel should be bright, got {}",
        pixels[2]
    );
}

#[test]
fn headless_clear_to_red() {
    use flui_engine::context::headless_render::read_texture_to_rgba;

    let gpu = GpuDevice::new_headless().expect("GPU init");
    let (texture, view) = gpu.create_render_texture(32, 32);

    let mut enc = gpu.device().create_command_encoder(&Default::default());
    {
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
    gpu.queue().submit(std::iter::once(enc.finish()));

    let pixels = read_texture_to_rgba(&gpu, &texture, 32, 32);
    assert_eq!(pixels.len(), 32 * 32 * 4);
    // Bgra8Unorm format: pixel[0]=B, pixel[1]=G, pixel[2]=R, pixel[3]=A
    assert!(
        pixels[2] > 200,
        "R channel should be bright, got {}",
        pixels[2]
    );
    assert!(
        pixels[0] < 50,
        "B channel should be dark, got {}",
        pixels[0]
    );
}
