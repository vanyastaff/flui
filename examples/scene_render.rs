//! Scene Render - End-to-end GPU compositor proof
//!
//! Demonstrates the full rendering pipeline:
//! Canvas (draw commands) -> DisplayList -> CanvasLayer -> Scene -> Renderer -> GPU -> pixels
//!
//! This proves that flui-engine's `render_scene()` correctly traverses the
//! LayerTree and dispatches DisplayList commands through the GPU backend.
//!
//! Run with: cargo run --example scene_render

use flui_engine::wgpu::Renderer;
use flui_layer::{CanvasLayer, Layer, LayerTree, Scene};
use flui_platform::traits::PlatformWindow;
use flui_platform::{current_platform, WindowOptions};
use flui_types::geometry::{px, Rect, Size};
use flui_types::painting::Paint;
use flui_types::styling::Color;
use std::sync::{Arc, Mutex};

/// Wrapper for raw-window-handle bridging
struct PlatformWindowHandle {
    window: Arc<dyn PlatformWindow>,
}

impl raw_window_handle::HasWindowHandle for PlatformWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.window.window_handle()
    }
}

impl raw_window_handle::HasDisplayHandle for PlatformWindowHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.window.display_handle()
    }
}

/// Build a scene with colored rectangles to prove the compositor works.
fn build_test_scene(width: f32, height: f32) -> Scene {
    let mut tree = LayerTree::new();

    // Create a CanvasLayer with drawing commands
    let mut canvas_layer = CanvasLayer::new();
    let canvas = canvas_layer.canvas_mut();

    // Background — dark blue
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
        &Paint::fill(Color::rgb(20, 30, 48)),
    );

    // Large red rectangle (top-left)
    canvas.draw_rect(
        Rect::from_ltrb(px(50.0), px(50.0), px(350.0), px(250.0)),
        &Paint::fill(Color::RED),
    );

    // Green rectangle (center)
    canvas.draw_rect(
        Rect::from_ltrb(px(200.0), px(150.0), px(500.0), px(350.0)),
        &Paint::fill(Color::GREEN),
    );

    // Blue rectangle (bottom-right)
    canvas.draw_rect(
        Rect::from_ltrb(px(400.0), px(250.0), px(700.0), px(450.0)),
        &Paint::fill(Color::BLUE),
    );

    // White rectangle (small, center)
    canvas.draw_rect(
        Rect::from_ltrb(px(300.0), px(200.0), px(450.0), px(300.0)),
        &Paint::fill(Color::WHITE),
    );

    // Yellow rectangle (bottom)
    canvas.draw_rect(
        Rect::from_ltrb(px(100.0), px(400.0), px(600.0), px(500.0)),
        &Paint::fill(Color::rgb(255, 200, 0)),
    );

    let root_id = tree.insert(Layer::Canvas(canvas_layer));

    Scene::new(Size::new(px(width), px(height)), tree, Some(root_id), 1)
}

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Scene render example — proving GPU compositor pipeline");

    let platform = current_platform().expect("Failed to initialize platform");
    tracing::info!("Platform: {}", platform.name());

    let platform_for_ready = platform.clone();

    platform.run(Box::new(move || {
        let options = WindowOptions {
            title: "FLUI Scene Render — GPU Compositor Proof".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            resizable: true,
            visible: true,
            decorated: true,
            min_size: None,
            max_size: None,
        };

        let window: Arc<dyn PlatformWindow> = Arc::from(
            platform_for_ready
                .open_window(options)
                .expect("Failed to open window"),
        );

        tracing::info!(
            "Window created: {:?} @ {:.1}x scale",
            window.physical_size(),
            window.scale_factor()
        );

        // Create Renderer from PlatformWindow
        let handle = PlatformWindowHandle {
            window: window.clone(),
        };

        let mut renderer =
            pollster::block_on(Renderer::new(&handle)).expect("Failed to create GPU renderer");

        let phys = window.physical_size();
        renderer.resize(phys.width.0 as u32, phys.height.0 as u32);

        tracing::info!(
            "GPU: {} ({:?})",
            renderer.capabilities().adapter_name,
            renderer.capabilities().backend
        );

        let renderer = Arc::new(Mutex::new(renderer));

        // Register frame callback — build scene and render each frame
        let renderer_frame = Arc::clone(&renderer);
        let window_for_frame = window.clone();
        window.on_request_frame(Box::new(move || {
            let size = window_for_frame.physical_size();
            let w = size.width.0 as f32;
            let h = size.height.0 as f32;

            // Build scene with colored rectangles
            let scene = build_test_scene(w, h);

            // Render scene through the full pipeline
            let mut r = renderer_frame.lock().unwrap();
            if let Err(e) = r.render_scene(&scene) {
                tracing::error!("render_scene failed: {:?}", e);
            }
        }));

        // Register resize callback
        let renderer_resize = Arc::clone(&renderer);
        window.on_resize(Box::new(move |size, scale_factor| {
            let w = (size.width.0 * scale_factor) as u32;
            let h = (size.height.0 * scale_factor) as u32;
            renderer_resize.lock().unwrap().resize(w, h);
        }));

        // Request first frame
        window.request_redraw();

        tracing::info!("Scene render pipeline active — you should see colored rectangles");
    }));

    tracing::info!("Application finished");
}
