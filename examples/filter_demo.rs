//! Filter Demo - GPU Gaussian blur via SceneBuilder + ImageFilterLayer
//!
//! Demonstrates the full image-filter rendering pipeline via the programmatic
//! SceneBuilder API:
//! SceneBuilder::push_image_filter → add_canvas (content) → pop →
//! Scene → Renderer::render_scene → GPU blur pass → pixels
//!
//! The window shows two copies of the same shapes side-by-side:
//! - LEFT half: sharp / un-blurred (direct CanvasLayer child of root)
//! - RIGHT half: blurred (CanvasLayer child of an ImageFilterLayer with σ=8)
//!
//! Run with: cargo run --example filter_demo

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use flui_engine::wgpu::Renderer;
use flui_layer::{CanvasLayer, LayerTree, Scene, SceneBuilder};
use flui_platform::{WindowOptions, current_platform};
use flui_types::{
    Color, Offset,
    geometry::{Rect, Size, px},
    painting::{ImageFilter, Paint},
};

// ── Scene construction ────────────────────────────────────────────────────────

/// Builds a scene demonstrating `ImageFilterLayer` Gaussian blur side-by-side
/// with an un-blurred copy of the same shapes, using the `SceneBuilder` API.
///
/// Layer tree structure (constructed via SceneBuilder push/pop):
/// ```text
/// OffsetLayer (zero, root — pushed first)
///   ├── CanvasLayer  (background + LEFT un-blurred shapes — add_canvas leaf)
///   └── ImageFilterLayer (Blur σ_x=8, σ_y=8 — push_image_filter)
///         └── CanvasLayer (RIGHT blurred shapes — add_canvas leaf inside filter)
/// ```
///
/// SceneBuilder API used:
/// - `push_offset(Offset::ZERO)` — root container
/// - `add_canvas(sharp_canvas)` — leaf inside the offset layer
/// - `push_image_filter(ImageFilter::blur_xy(8.0, 8.0))` — filter container
/// - `add_canvas(blurred_canvas)` — leaf inside the filter layer
/// - `pop()` — close the filter layer
/// - `pop()` — close the offset layer (root)
/// - `build()` — consume the builder, returns root `LayerId`
fn build_filter_scene(width: f32, height: f32) -> Scene {
    let half_width = width / 2.0;

    let mut tree = LayerTree::new();

    // ── SceneBuilder: construct the layer hierarchy via push/pop ─────────────
    //
    // The builder is scoped so `tree` is available after the borrow ends.
    let root_id = {
        let mut builder = SceneBuilder::new(&mut tree);

        // Root: zero-offset container so both children share one root id.
        builder.push_offset(Offset::ZERO);

        // ── LEFT: background + un-blurred shapes ─────────────────────────────
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
        // Leaf: added as a child of the current parent (the offset layer).
        builder.add_canvas(sharp_canvas);

        // ── RIGHT: blurred shapes wrapped in ImageFilterLayer ─────────────────
        //
        // SceneBuilder::push_image_filter pushes an ImageFilterLayer and makes
        // it the current parent; add_canvas then attaches the content as its child.
        builder.push_image_filter(ImageFilter::blur_directional(8.0, 8.0));
        let mut blurred_canvas = CanvasLayer::new();
        draw_demo_shapes(blurred_canvas.canvas_mut(), half_width, half_width, height);
        builder.add_canvas(blurred_canvas);
        // Close the ImageFilterLayer.
        builder
            .pop()
            .expect("SceneBuilder stack must not underflow: blur filter was pushed");

        // Close the root offset layer.
        builder
            .pop()
            .expect("SceneBuilder stack must not underflow: root offset was pushed");

        builder.build()
    };

    tracing::info!(
        sigma_x = 8.0_f32,
        sigma_y = 8.0_f32,
        "blurred shape via SceneBuilder::push_image_filter(Blur σ=8)"
    );

    Scene::new(Size::new(px(width), px(height)), tree, root_id, 1)
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
        Rect::from_ltrb(px(left), px(height - 55.0), px(right), px(height - 30.0)),
        &Paint::fill(Color::rgb(255, 210, 0)),
    );
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Filter demo — GPU Gaussian blur via SceneBuilder::push_image_filter");

    let platform = current_platform().expect("failed to initialize platform");
    tracing::info!("platform: {}", platform.name());

    let options = WindowOptions {
        title: "FLUI Filter Demo — Gaussian Blur (SceneBuilder API)".to_string(),
        size: Size::new(px(900.0), px(600.0)),
        resizable: true,
        visible: true,
        decorated: true,
        min_size: None,
        max_size: None,
    };

    let window = platform
        .open_window(options)
        .expect("failed to open window");

    tracing::info!(
        physical_size = ?window.physical_size(),
        scale_factor = window.scale_factor(),
        "window created"
    );

    let mut renderer =
        pollster::block_on(Renderer::new(window.as_ref())).expect("failed to create GPU renderer");

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

    platform.run(Box::new(move |_platform| {
        // Keep resources alive through the event loop.
        let _window = &window;
        let _renderer = &renderer;
    }));

    tracing::info!("filter demo finished");
}
