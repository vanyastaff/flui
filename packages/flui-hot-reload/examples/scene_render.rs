//! A desktop host for a scene plugin.
//!
//! The window is a FLUI application that installs
//! [`flui_hot_reload::ScenePluginHook`] through [`flui::app::AppConfig::dev_reload`]:
//! while the plugin library is loaded, the runner draws the plugin's scene in
//! place of the widget tree, and reloads it when the library is rebuilt. Until
//! then the window shows the root view below, which says what to build.
//!
//! ```bash
//! # Build the desktop scene plugin:
//! cargo build -p flui-desktop-scene
//!
//! # Run the host; it loads target/debug/libflui_scene.so (.dylib on macOS,
//! # flui_scene.dll on Windows) unless FLUI_SCENE_PLUGIN names another path:
//! cargo run -p flui-hot-reload --features host-hook --example scene_render
//! ```
//!
//! On Windows the loaded DLL is locked: stop the host before rebuilding it.
//!
//! The engine's effects gallery (masks, gradients, backdrop, clear, follower,
//! anti-aliasing) is drawn once, headless, into a PNG:
//!
//! ```bash
//! cargo run -p flui-hot-reload --features host-hook --example scene_render -- --capture-effects gallery.png
//! ```

#[path = "scene_render/effects.rs"]
mod effects;

use std::path::{Path, PathBuf};

use flui::Application;
use flui::app::AppConfig;
use flui::prelude::*;
use flui_hot_reload::{ScenePluginHook, env};

/// The gallery's size in physical pixels.
const GALLERY_WIDTH: u32 = 800;
const GALLERY_HEIGHT: u32 = 600;

/// Where `cargo build -p flui-desktop-scene` puts the plugin.
fn default_plugin_path() -> PathBuf {
    let mut path = PathBuf::from("target");
    path.push("debug");
    #[cfg(windows)]
    path.push("flui_scene.dll");
    #[cfg(target_os = "macos")]
    path.push("libflui_scene.dylib");
    #[cfg(all(unix, not(target_os = "macos")))]
    path.push(ScenePluginHook::LIBRARY_NAME);
    path
}

/// What the window shows while no plugin draws.
#[derive(Clone, StatelessView)]
struct WaitingForPlugin {
    path: String,
}

impl StatelessView for WaitingForPlugin {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Container::new()
            .color(Color::rgb(20, 30, 48))
            .child(Center::new().child(Column::new((
                Text::new("No scene plugin is loaded."),
                SizedBox::height(12.0),
                Text::new("Build one with `cargo build -p flui-desktop-scene`;"),
                Text::new(format!("this window draws it once {} exists.", self.path)),
            ))))
    }
}

/// Renders the effects gallery through the headless renderer and writes it to
/// `path` as an RGBA PNG.
fn capture_effects(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let renderer = pollster::block_on(flui_engine::HeadlessRenderer::new())?;
    let scene = effects::build(f64::from(GALLERY_WIDTH), f64::from(GALLERY_HEIGHT));
    let pixels = renderer.render_scene(&scene, (GALLERY_WIDTH, GALLERY_HEIGHT))?;
    image::save_buffer_with_format(
        path,
        &pixels,
        GALLERY_WIDTH,
        GALLERY_HEIGHT,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    tracing::info!(path = %path.display(), "Captured effects gallery ({GALLERY_WIDTH}x{GALLERY_HEIGHT} RGBA)");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--capture-effects" {
            let path = arguments.next().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "--capture-effects requires a PNG path",
                )
            })?;
            return capture_effects(Path::new(&path));
        }
    }

    let plugin_path =
        std::env::var_os(env::SCENE_PLUGIN).map_or_else(default_plugin_path, PathBuf::from);
    tracing::info!(plugin = %plugin_path.display(), "Scene plugin host");

    let root = WaitingForPlugin {
        path: plugin_path.display().to_string(),
    };
    let config = AppConfig::new()
        .title("FLUI Scene Plugin")
        .size(800, 600)
        .dev_reload(ScenePluginHook::new(plugin_path));
    Application::new().config(config).run(root)?;
    Ok(())
}
