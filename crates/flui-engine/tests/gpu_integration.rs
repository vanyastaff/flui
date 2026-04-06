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
