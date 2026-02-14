//! Hot-reloadable scene plugin for desktop.
//!
//! Compiles to `flui_scene.dll` (Windows), `libflui_scene.so` (Linux),
//! or `libflui_scene.dylib` (macOS). Loaded at runtime by the host
//! via `HotReloadDriver`.
//!
//! # Usage
//!
//! ```bash
//! # Build the plugin:
//! cargo build -p flui-desktop-scene
//!
//! # Run the host example with plugin path:
//! # Linux/macOS:
//! FLUI_SCENE_PLUGIN=target/debug/libflui_scene.so cargo run --example scene_render
//! # Windows:
//! set FLUI_SCENE_PLUGIN=target\debug\flui_scene.dll
//! cargo run --example scene_render
//! ```
//!
//! Edit the colors below, rebuild the plugin, and the host will
//! detect the change and reload automatically (on Unix).
//! On Windows, stop the host first due to DLL file locking.

use flui_hot_reload::scene_plugin;
use flui_layer::{CanvasLayer, Layer, LayerTree, Scene};
use flui_types::geometry::{px, Rect, Size};
use flui_types::painting::Paint;
use flui_types::styling::Color;

fn my_scene(width: f32, height: f32) -> Scene {
    let mut tree = LayerTree::new();
    let mut canvas_layer = CanvasLayer::new();
    let canvas = canvas_layer.canvas_mut();

    // Background — deep purple (change this and rebuild to test hot-reload!)
    canvas.draw_rect(
        Rect::from_ltrb(px(0.0), px(0.0), px(width), px(height)),
        &Paint::fill(Color::rgb(80, 0, 120)),
    );

    // Teal rectangle (top-left)
    canvas.draw_rect(
        Rect::from_ltrb(px(50.0), px(50.0), px(350.0), px(250.0)),
        &Paint::fill(Color::rgb(0, 180, 180)),
    );

    // Coral rectangle (center)
    canvas.draw_rect(
        Rect::from_ltrb(px(200.0), px(150.0), px(500.0), px(350.0)),
        &Paint::fill(Color::rgb(255, 100, 80)),
    );

    // Gold rectangle (bottom-right)
    canvas.draw_rect(
        Rect::from_ltrb(px(400.0), px(250.0), px(700.0), px(450.0)),
        &Paint::fill(Color::rgb(255, 215, 0)),
    );

    // White rectangle (small, center)
    canvas.draw_rect(
        Rect::from_ltrb(px(300.0), px(200.0), px(450.0), px(300.0)),
        &Paint::fill(Color::WHITE),
    );

    let root = tree.insert(Layer::Canvas(canvas_layer));
    Scene::new(Size::new(px(width), px(height)), tree, Some(root), 1)
}

scene_plugin!(my_scene);
