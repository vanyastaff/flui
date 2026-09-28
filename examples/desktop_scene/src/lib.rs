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
use flui_types::{geometry::Rect, painting::Paint, styling::Color};

fn my_scene(width: f64, height: f64) -> Scene {
    let mut canvas_layer = CanvasLayer::new();
    let canvas = canvas_layer.canvas_mut();

    // Background — deep purple (change this and rebuild to test hot-reload!)
    canvas.draw_rect(
        Rect::from_ltrb(0.0, 0.0, width, height),
        &Paint::fill(Color::rgb(80, 0, 120)),
    );

    // Teal rectangle (top-left)
    canvas.draw_rect(
        Rect::from_ltrb(50.0, 50.0, 350.0, 250.0),
        &Paint::fill(Color::rgb(0, 180, 180)),
    );

    // Coral rectangle (center)
    canvas.draw_rect(
        Rect::from_ltrb(200.0, 150.0, 500.0, 350.0),
        &Paint::fill(Color::rgb(255, 100, 80)),
    );

    // Gold rectangle (bottom-right)
    canvas.draw_rect(
        Rect::from_ltrb(400.0, 250.0, 700.0, 450.0),
        &Paint::fill(Color::rgb(255, 215, 0)),
    );

    // White rectangle (small, center)
    canvas.draw_rect(
        Rect::from_ltrb(300.0, 200.0, 450.0, 300.0),
        &Paint::fill(Color::WHITE),
    );

    Scene::new(LayerTree::new(Layer::from(canvas_layer)))
}

scene_plugin!(my_scene);
