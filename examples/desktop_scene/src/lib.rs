//! Hot-reloadable scene plugin for desktop.
//!
//! Compiles to `flui_scene.dll` (Windows), `libflui_scene.so` (Linux),
//! or `libflui_scene.dylib` (macOS). Loaded at runtime by a host that
//! installs `flui_hot_reload::ScenePluginHook`.
//!
//! # Usage
//!
//! ```bash
//! # Build the plugin:
//! cargo build -p flui-desktop-scene
//!
//! # Run the host example; it loads target/debug/libflui_scene.so
//! # (.dylib on macOS, flui_scene.dll on Windows) unless
//! # FLUI_SCENE_PLUGIN names another path:
//! cargo run -p flui-hot-reload --features host-hook --example scene_render
//! ```
//!
//! Edit the colors below, rebuild the plugin, and the host will
//! detect the change and reload automatically (on Unix).
//! On Windows, stop the host first due to DLL file locking.

use flui_foundation::geometry::Rect;
use flui_hot_reload::scene_plugin;
use flui_layer::{Scene, SceneBuilder};
use flui_painting::{Canvas, paint::Paint, styling::Color};

fn my_scene(width: f64, height: f64) -> Scene {
    let mut canvas = Canvas::new();

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

    let mut builder = SceneBuilder::new();
    builder.add_picture(canvas.finish());
    builder.build()
}

scene_plugin!(my_scene);
