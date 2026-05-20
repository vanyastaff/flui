//! Hot-reloadable scene plugin for FLUI Android demo.
//!
//! This crate compiles to `libflui_scene.so` and is loaded at runtime via `dlopen`.
//! The host app detects file changes and reloads automatically — no app restart needed.
//!
//! Uses the `scene_plugin!` macro from `flui-hot-reload` to generate the FFI wrappers.
//! The user just writes a normal `fn(f32, f32) -> Scene`.

use flui_hot_reload::scene_plugin;
use flui_layer::{CanvasLayer, Layer, LayerTree, Scene};
use flui_types::geometry::{px, Rect, Size};
use flui_types::painting::Paint;
use flui_types::styling::Color;

fn my_scene(width: f32, height: f32) -> Scene {
    let mut tree = LayerTree::new();
    let mut canvas_layer = CanvasLayer::new();
    let canvas = canvas_layer.canvas_mut();

    let scale_x = width / 800.0;
    let scale_y = height / 600.0;

    // Background — deep purple
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
        &Paint::fill(Color::rgb(128, 0, 128)),
    );

    // Large red rectangle (top-left area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(50.0 * scale_x),
            px(50.0 * scale_y),
            px(350.0 * scale_x),
            px(250.0 * scale_y),
        ),
        &Paint::fill(Color::RED),
    );

    // Green rectangle (center area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(200.0 * scale_x),
            px(150.0 * scale_y),
            px(500.0 * scale_x),
            px(350.0 * scale_y),
        ),
        &Paint::fill(Color::GREEN),
    );

    // Blue rectangle (bottom-right area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(400.0 * scale_x),
            px(250.0 * scale_y),
            px(700.0 * scale_x),
            px(450.0 * scale_y),
        ),
        &Paint::fill(Color::BLUE),
    );

    // White rectangle (small, center)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(300.0 * scale_x),
            px(200.0 * scale_y),
            px(450.0 * scale_x),
            px(300.0 * scale_y),
        ),
        &Paint::fill(Color::WHITE),
    );

    // Yellow rectangle (bottom area)
    canvas.draw_rect(
        Rect::from_ltrb(
            px(100.0 * scale_x),
            px(400.0 * scale_y),
            px(600.0 * scale_x),
            px(500.0 * scale_y),
        ),
        &Paint::fill(Color::rgb(255, 200, 0)),
    );

    let root = tree.insert(Layer::Canvas(canvas_layer));
    Scene::new(Size::new(px(width), px(height)), tree, Some(root), 1)
}

// Generate extern "C" wrappers: flui_scene_build, flui_scene_version, flui_scene_drop
scene_plugin!(my_scene);
