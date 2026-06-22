//! Filter Demo - GPU Gaussian blur via ImageFilterLayer
//!
//! Demonstrates the full image-filter rendering pipeline:
//! CanvasLayer (content) → ImageFilterLayer → LayerRender::render →
//! Backend::push_image_filter → save_layer_with_image_filter →
//! DrawItem::Filter → GPU blur pass → Renderer::render_scene → pixels
//!
//! The window shows two copies of the same shapes side-by-side:
//! - LEFT half: sharp / un-blurred (direct CanvasLayer child of root)
//! - RIGHT half: blurred (CanvasLayer child of an ImageFilterLayer with σ=8)
//!
//! Run with: cargo run --example filter_demo

use std::sync::{Arc, Mutex};

use flui_engine::wgpu::Renderer;
use flui_layer::{CanvasLayer, ImageFilterLayer, Layer, LayerTree, OffsetLayer, Scene};
use flui_platform::{WindowOptions, current_platform, traits::PlatformWindow};
use flui_types::{
    geometry::{Rect, Size, px},
    painting::Paint,
    styling::Color,
};

// ── Window-handle bridge (mirrors scene_render.rs) ───────────────────────────

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

// ── Scene construction ────────────────────────────────────────────────────────

/// Builds a scene demonstrating `ImageFilterLayer` Gaussian blur side-by-side
/// with an un-blurred copy of the same shapes.
///
/// Layer tree structure:
/// ```text
/// OffsetLayer (zero, root container)
///   ├── CanvasLayer  (background + LEFT un-blurred shapes)
///   └── ImageFilterLayer (Blur σ_x=8, σ_y=8)
///         └── CanvasLayer (RIGHT blurred shapes)
/// ```
fn build_filter_scene(width: f32, height: f32) -> Scene {
    let half_width = width / 2.0;

    let mut tree = LayerTree::new();

    // Root: zero-offset container so both children share one root id.
    let root_id = tree.insert(Layer::from(OffsetLayer::zero()));

    // ── LEFT: background + un-blurred shapes ─────────────────────────────────
    let mut sharp_canvas = CanvasLayer::new();
    {
        let canvas = sharp_canvas.canvas_mut();

        // Full background (dark navy).
        canvas.draw_rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
            &Paint::fill(Color::rgb(18, 26, 42)),
        );

        // Divider between left (sharp) and right (blurred) halves.
        canvas.draw_rect(
            Rect::from_ltrb(
                px(half_width - 1.0),
                px(0.0),
                px(half_width + 1.0),
                px(height),
            ),
            &Paint::fill(Color::rgb(80, 80, 80)),
        );

        // Un-blurred shapes on the left half.
        draw_demo_shapes(canvas, 0.0, half_width, height);
    }
    let sharp_id = tree.insert(Layer::from(sharp_canvas));
    tree.add_child(root_id, sharp_id);

    // ── RIGHT: blurred shapes wrapped in ImageFilterLayer ────────────────────
    let mut blurred_canvas = CanvasLayer::new();
    draw_demo_shapes(blurred_canvas.canvas_mut(), half_width, half_width, height);
    let blurred_content_id = tree.insert(Layer::from(blurred_canvas));

    // ImageFilterLayer::blur_xy(sigma_x, sigma_y) — public convenience ctor.
    let filter_layer = ImageFilterLayer::blur_xy(8.0, 8.0);
    let filter_layer_id = tree.insert(Layer::from(filter_layer));

    // Wire: filter layer is a child of root, blurred canvas is its child.
    tree.add_child(root_id, filter_layer_id);
    tree.add_child(filter_layer_id, blurred_content_id);

    tracing::info!(
        sigma_x = 8.0,
        sigma_y = 8.0,
        "blurred shape via ImageFilterLayer(Blur σ=8)"
    );

    Scene::new(Size::new(px(width), px(height)), tree, Some(root_id), 1)
}

/// Draws the demo shape set into `canvas`, offset by `x_offset` px within a
/// column of `column_width` × `height`.
fn draw_demo_shapes(
    canvas: &mut flui_painting::Canvas,
    x_offset: f32,
    column_width: f32,
    height: f32,
) {
    let margin = 40.0;
    let left = x_offset + margin;
    let right = x_offset + column_width - margin;
    let center_x = x_offset + column_width / 2.0;

    // Large coral rectangle.
    canvas.draw_rect(
        Rect::from_ltrb(px(left), px(60.0), px(right), px(height / 2.0 - 20.0)),
        &Paint::fill(Color::rgb(220, 80, 60)),
    );

    // Overlapping teal rectangle.
    canvas.draw_rect(
        Rect::from_ltrb(
            px(center_x - 80.0),
            px(height / 2.0 - 60.0),
            px(center_x + 80.0),
            px(height - 60.0),
        ),
        &Paint::fill(Color::rgb(30, 180, 160)),
    );

    // Small white accent square.
    canvas.draw_rect(
        Rect::from_ltrb(
            px(center_x - 30.0),
            px(height / 2.0 - 30.0),
            px(center_x + 30.0),
            px(height / 2.0 + 30.0),
        ),
        &Paint::fill(Color::WHITE),
    );

    // Yellow strip at the bottom.
    canvas.draw_rect(
        Rect::from_ltrb(
            px(left),
            px(height - 55.0),
            px(right),
            px(height - 30.0),
        ),
        &Paint::fill(Color::rgb(255, 210, 0)),
    );
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Filter demo — GPU Gaussian blur via ImageFilterLayer");

    let platform = current_platform().expect("failed to initialize platform");
    tracing::info!("platform: {}", platform.name());

    let options = WindowOptions {
        title: "FLUI Filter Demo — Gaussian Blur (ImageFilterLayer)".to_string(),
        size: Size::new(px(900.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: None,
        max_size: None,
    };

    let window: Arc<dyn PlatformWindow> =
        Arc::from(platform.open_window(options).expect("failed to open window"));

    tracing::info!(
        physical_size = ?window.physical_size(),
        scale_factor = window.scale_factor(),
        "window created"
    );

    let handle = PlatformWindowHandle {
        window: window.clone(),
    };

    let mut renderer =
        pollster::block_on(Renderer::new(&handle)).expect("failed to create GPU renderer");

    let physical = window.physical_size();
    renderer.resize(physical.width.0 as u32, physical.height.0 as u32);

    tracing::info!(
        adapter = renderer.capabilities().adapter_name,
        backend = ?renderer.capabilities().backend,
        "GPU adapter selected"
    );

    let renderer = Arc::new(Mutex::new(renderer));

    // Frame callback: rebuild and render the scene each frame.
    let renderer_for_frame = Arc::clone(&renderer);
    let window_for_frame = window.clone();
    window.on_request_frame(Box::new(move || {
        let size = window_for_frame.physical_size();
        let scene_width = size.width.0 as f32;
        let scene_height = size.height.0 as f32;

        let scene = build_filter_scene(scene_width, scene_height);

        let mut locked_renderer = renderer_for_frame.lock().unwrap();
        if let Err(render_error) = locked_renderer.render_scene(&scene) {
            tracing::error!(?render_error, "render_scene failed");
        }
    }));

    // Resize callback: update the renderer's surface dimensions.
    let renderer_for_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |new_size, scale_factor| {
        let surface_width = (new_size.width.0 * scale_factor) as u32;
        let surface_height = (new_size.height.0 * scale_factor) as u32;
        renderer_for_resize
            .lock()
            .unwrap()
            .resize(surface_width, surface_height);
    }));

    window.request_redraw();

    tracing::info!("filter demo ready — left=sharp, right=blurred (σ=8)");

    platform.run(Box::new(move || {
        // Keep resources alive through the event loop.
        let _window = &window;
        let _renderer = &renderer;
    }));

    tracing::info!("filter demo finished");
}
