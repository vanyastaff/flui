//! Color Filter Demo - T1 ColorFilter live via SceneBuilder
//!
//! Demonstrates all three `ColorFilter` variants from T1 of the
//! `gpu-filters-consumer-chain` spec, each applied via
//! `SceneBuilder::push_color_filter`. The window shows five columns
//! side-by-side, each containing the same coral rectangle:
//!
//! | Column | Filter |
//! |--------|--------|
//! | 1 | Unfiltered (reference) |
//! | 2 | `ColorFilter::Mode { Multiply, cyan }` |
//! | 3 | `ColorFilter::LinearToSrgbGamma` (linear → sRGB) |
//! | 4 | `ColorFilter::SrgbToLinearGamma` (sRGB → linear) |
//! | 5 | `ColorFilter::grayscale()` (Matrix variant) |
//!
//! This is the first live windowed demonstration of `ColorFilter::Mode` and
//! `ColorFilter::LinearToSrgbGamma`/`SrgbToLinearGamma` reachable through
//! the `SceneBuilder` → `Scene` → `render_scene` → GPU path.
//!
//! Run with: cargo run --example color_filter_demo

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
    painting::{BlendMode, ColorFilter, Paint},
};

// ── Color filter definitions ─────────────────────────────────────────────────

/// Describes one column in the demo: a label (for tracing) and a `ColorFilter`
/// to apply, or `None` for the unfiltered reference column.
struct FilterColumn {
    label: &'static str,
    filter: Option<ColorFilter>,
}

fn demo_columns() -> [FilterColumn; 5] {
    [
        FilterColumn {
            label: "unfiltered",
            filter: None,
        },
        FilterColumn {
            label: "Mode/Multiply cyan",
            // Half-opacity cyan multiplied onto the layer content.
            filter: Some(ColorFilter::mode(
                Color::rgba(0, 200, 220, 200),
                BlendMode::Multiply,
            )),
        },
        FilterColumn {
            label: "LinearToSrgbGamma",
            filter: Some(ColorFilter::LinearToSrgbGamma),
        },
        FilterColumn {
            label: "SrgbToLinearGamma",
            filter: Some(ColorFilter::SrgbToLinearGamma),
        },
        FilterColumn {
            label: "grayscale (Matrix)",
            filter: Some(ColorFilter::grayscale()),
        },
    ]
}

// ── Scene construction ────────────────────────────────────────────────────────

/// Builds the color-filter demo scene via `SceneBuilder`.
///
/// Layer tree (constructed via SceneBuilder push/pop):
/// ```text
/// OffsetLayer (zero, root)
///   ├── CanvasLayer  (background)
///   ├── CanvasLayer  (column 0: unfiltered shapes)
///   ├── ColorFilterLayer(Mode/Multiply) — push_color_filter
///   │     └── CanvasLayer (column 1 shapes)
///   ├── ColorFilterLayer(LinearToSrgbGamma) — push_color_filter
///   │     └── CanvasLayer (column 2 shapes)
///   ├── ColorFilterLayer(SrgbToLinearGamma) — push_color_filter
///   │     └── CanvasLayer (column 3 shapes)
///   └── ColorFilterLayer(Matrix/grayscale) — push_color_filter
///         └── CanvasLayer (column 4 shapes)
/// ```
///
/// SceneBuilder API used:
/// - `push_offset(Offset::ZERO)` — root container
/// - `add_canvas(background_canvas)` — full-surface background leaf
/// - For unfiltered column: `add_canvas(shapes_canvas)` leaf directly
/// - For filtered columns: `push_color_filter(filter)` + `add_canvas(shapes_canvas)` + `pop()`
/// - `pop()` — close root offset layer
/// - `build()` — consume builder, returns root `LayerId`
fn build_color_filter_scene(viewport_width: f32, viewport_height: f32) -> Scene {
    let columns = demo_columns();
    let column_count = columns.len() as f32;
    let column_width = viewport_width / column_count;

    let mut tree = LayerTree::new();

    let root_id = {
        let mut builder = SceneBuilder::new(&mut tree);

        // Root offset layer.
        builder.push_offset(Offset::ZERO);

        // Background canvas: dark slate covering the entire viewport.
        let mut background_canvas = CanvasLayer::new();
        background_canvas.canvas_mut().draw_rect(
            Rect::from_ltrb(px(0.0), px(0.0), px(viewport_width), px(viewport_height)),
            &Paint::fill(Color::rgb(22, 28, 40)),
        );
        builder.add_canvas(background_canvas);

        // One column per filter.
        for (col_index, column_spec) in columns.iter().enumerate() {
            let x_offset = col_index as f32 * column_width;

            // Draw the content canvas for this column (shapes + divider).
            let shapes_canvas = build_column_canvas(x_offset, column_width, viewport_height);

            match &column_spec.filter {
                None => {
                    // Unfiltered reference: add directly as a leaf.
                    builder.add_canvas(shapes_canvas);
                }
                Some(filter) => {
                    // Filtered columns: push a ColorFilterLayer, add content, pop.
                    builder.push_color_filter(*filter);
                    builder.add_canvas(shapes_canvas);
                    builder
                        .pop()
                        .expect("SceneBuilder stack must not underflow: color filter was pushed");
                }
            }

            tracing::debug!(col = col_index, filter = column_spec.label, "column added");
        }

        // Close root offset layer.
        builder
            .pop()
            .expect("SceneBuilder stack must not underflow: root offset was pushed");

        builder.build()
    };

    tracing::info!(
        columns = columns.len(),
        "color filter demo scene built via SceneBuilder::push_color_filter"
    );

    Scene::new(
        Size::new(px(viewport_width), px(viewport_height)),
        tree,
        root_id,
        1,
    )
}

/// Draws the demo shapes into a `CanvasLayer` for one column of the display.
///
/// Each column contains a coral rectangle, a teal rectangle, a white accent
/// square, and a yellow strip — the same geometry as `filter_demo.rs` so the
/// color-filter effect is visually comparable.
fn build_column_canvas(x_offset: f32, column_width: f32, viewport_height: f32) -> CanvasLayer {
    let mut canvas_layer = CanvasLayer::new();
    let canvas = canvas_layer.canvas_mut();

    let margin = 20.0;
    let left = x_offset + margin;
    let right = x_offset + column_width - margin;
    let center_x = x_offset + column_width / 2.0;

    // Thin column divider on the right edge.
    canvas.draw_rect(
        Rect::from_ltrb(
            px(x_offset + column_width - 1.0),
            px(0.0),
            px(x_offset + column_width),
            px(viewport_height),
        ),
        &Paint::fill(Color::rgb(60, 60, 60)),
    );

    // Large coral rectangle (primary subject).
    canvas.draw_rect(
        Rect::from_ltrb(
            px(left),
            px(50.0),
            px(right),
            px(viewport_height / 2.0 - 20.0),
        ),
        &Paint::fill(Color::rgb(220, 80, 60)),
    );

    // Overlapping teal rectangle.
    canvas.draw_rect(
        Rect::from_ltrb(
            px(center_x - 50.0),
            px(viewport_height / 2.0 - 50.0),
            px(center_x + 50.0),
            px(viewport_height - 60.0),
        ),
        &Paint::fill(Color::rgb(30, 180, 160)),
    );

    // Small white accent square.
    canvas.draw_rect(
        Rect::from_ltrb(
            px(center_x - 20.0),
            px(viewport_height / 2.0 - 20.0),
            px(center_x + 20.0),
            px(viewport_height / 2.0 + 20.0),
        ),
        &Paint::fill(Color::WHITE),
    );

    // Yellow strip at the bottom.
    canvas.draw_rect(
        Rect::from_ltrb(
            px(left),
            px(viewport_height - 50.0),
            px(right),
            px(viewport_height - 30.0),
        ),
        &Paint::fill(Color::rgb(255, 210, 0)),
    );

    canvas_layer
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!(
        "Color filter demo — T1 ColorFilter (Mode/Gamma/Matrix) via SceneBuilder::push_color_filter"
    );

    let platform = current_platform().expect("failed to initialize platform");
    tracing::info!("platform: {}", platform.name());

    let options = WindowOptions {
        title: "FLUI Color Filter Demo — Mode / Gamma / Matrix (SceneBuilder API)".to_string(),
        size: Size::new(px(1100.0), px(600.0)),
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
        let viewport_width = size.width.0 as f32;
        let viewport_height = size.height.0 as f32;

        let scene = build_color_filter_scene(viewport_width, viewport_height);

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

    tracing::info!(
        "color filter demo ready — 5 columns: unfiltered | Mode/Multiply | LinearToSrgb | SrgbToLinear | Grayscale"
    );

    platform.run(Box::new(move |_platform| {
        // Keep resources alive through the event loop.
        let _window = &window;
        let _renderer = &renderer;
    }));

    tracing::info!("color filter demo finished");
}
