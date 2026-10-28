//! Scene Render - End-to-end GPU compositor proof
//!
//! Demonstrates the full rendering pipeline:
//! Canvas (draw commands) -> DisplayList -> CanvasLayer -> Scene -> Renderer ->
//! GPU -> pixels
//!
//! This proves that flui-engine's `render_scene()` correctly traverses the
//! LayerTree and dispatches DisplayList commands through the GPU backend.
//!
//! # Hot-Reload Support
//!
//! Set the `FLUI_SCENE_PLUGIN` environment variable to point to a scene plugin
//! shared library (`.dll`/`.so`/`.dylib`). The example will load and render
//! the plugin's scene, polling for updates every 500ms.
//!
//! ```bash
//! # Build the desktop scene plugin:
//! cargo build -p flui-desktop-scene
//!
//! # Run with hot-reload (Linux/macOS):
//! FLUI_SCENE_PLUGIN=target/debug/libflui_scene.so cargo run --example scene_render
//!
//! # Run with hot-reload (Windows):
//! set FLUI_SCENE_PLUGIN=target\debug\flui_scene.dll
//! cargo run --example scene_render
//! ```
//!
//! Without the env var, the built-in scene (colored rectangles) is used.
//!
//! Run with: cargo run --example scene_render
//! Native effects gallery: cargo run --example scene_render -- --effects
//! Deterministic PNG: cargo run --example scene_render -- --capture-effects gallery.png

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![expect(clippy::unwrap_used)]
// `Renderer: Send` is re-proved here for the frame callback; see the
// `recursion_limit` rationale at the top of flui-engine's `lib.rs`.
#![recursion_limit = "256"]

#[path = "scene_render/effects.rs"]
mod effects;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use flui_engine::Renderer;
use flui_foundation::geometry::{Rect, Size, WindowMetrics};
use flui_hot_reload::HotReloadDriver;
use flui_layer::{CanvasLayer, Layer, LayerTree, Scene};
use flui_native::current_platform;
use flui_painting::{paint::Paint, styling::Color};
use flui_platform::{
    CloseDecision, DispatchEventResult, FrameOutcome, FrameRequest, PlatformInput, SurfaceChange,
    WindowHandler, WindowOptions,
};

/// Build a scene with colored rectangles (fallback when no plugin is loaded).
/// The window's drawable size, from its snapshot: zero before the host has
/// published drawable metrics.
/// The window's handler: it draws a frame and follows the drawable size once
/// the renderer both need exists, and ignores the rest.
#[derive(Default)]
struct SceneWindow {
    frame: RefCell<Option<Box<dyn FnMut()>>>,
    resized: RefCell<Option<Box<dyn FnMut()>>>,
}

impl WindowHandler for SceneWindow {
    fn input(&self, _input: PlatformInput) -> DispatchEventResult {
        DispatchEventResult::default()
    }

    fn state_changed(&self) {
        if let Some(resized) = self.resized.borrow_mut().as_mut() {
            resized();
        }
    }

    fn frame_requested(&self, _request: FrameRequest) -> FrameOutcome {
        match self.frame.borrow_mut().as_mut() {
            Some(frame) => {
                frame();
                FrameOutcome::Produced
            }
            None => FrameOutcome::Declined,
        }
    }

    fn close_requested(&self) -> CloseDecision {
        CloseDecision::Close
    }

    fn closed(&self) {}

    // A desktop window keeps its surface while it is open.
    fn surface_changed(&self, _change: SurfaceChange) {}
}

fn physical_size(window: &dyn flui_platform::PlatformWindow) -> Size<i32> {
    window
        .state()
        .metrics
        .map_or(Size::new(0, 0), WindowMetrics::physical)
}

fn build_test_scene(width: f64, height: f64) -> Scene {
    let mut canvas_layer = CanvasLayer::new();
    let canvas = canvas_layer.canvas_mut();

    // Background — dark blue
    canvas.draw_rect(
        Rect::from_ltrb(0.0, 0.0, width, height),
        &Paint::fill(Color::rgb(20, 30, 48)),
    );

    // Large red rectangle (top-left)
    canvas.draw_rect(
        Rect::from_ltrb(50.0, 50.0, 350.0, 250.0),
        &Paint::fill(Color::RED),
    );

    // Green rectangle (center)
    canvas.draw_rect(
        Rect::from_ltrb(200.0, 150.0, 500.0, 350.0),
        &Paint::fill(Color::GREEN),
    );

    // Blue rectangle (bottom-right)
    canvas.draw_rect(
        Rect::from_ltrb(400.0, 250.0, 700.0, 450.0),
        &Paint::fill(Color::BLUE),
    );

    // White rectangle (small, center)
    canvas.draw_rect(
        Rect::from_ltrb(300.0, 200.0, 450.0, 300.0),
        &Paint::fill(Color::WHITE),
    );

    // Yellow rectangle (bottom)
    canvas.draw_rect(
        Rect::from_ltrb(100.0, 400.0, 600.0, 500.0),
        &Paint::fill(Color::rgb(255, 200, 0)),
    );

    Scene::new(LayerTree::new(Layer::from(canvas_layer)))
}

fn capture_effects(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let renderer = pollster::block_on(flui_engine::HeadlessRenderer::new())?;
    let scene = effects::build(800.0, 600.0);
    let pixels = renderer.render_layer_tree(scene.tree(), (800, 600))?;
    image::save_buffer_with_format(
        path,
        &pixels,
        800,
        600,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    tracing::info!(path = %path.display(), "Captured effects gallery (800x600 RGBA)");
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
            capture_effects(std::path::Path::new(&path))?;
            return Ok(());
        }
    }

    tracing::info!("Scene render example — proving GPU compositor pipeline");

    let effects_mode = std::env::args().any(|argument| argument == "--effects");
    // Effects mode is deterministic and uses the maintained built-in scene.
    let hot_reload = (!effects_mode)
        .then(|| std::env::var("FLUI_SCENE_PLUGIN").ok())
        .flatten()
        .map(|path| {
            tracing::info!("Hot-reload enabled: {}", path);
            Arc::new(Mutex::new((HotReloadDriver::new(path), true)))
        });

    let platform = current_platform().expect("Failed to initialize platform");
    tracing::info!("Platform: {}", platform.name());

    let title = if effects_mode {
        "FLUI Effects — masks / gradients / backdrop / clear / follower / AA"
    } else if hot_reload.is_some() {
        "FLUI Scene Render — Hot-Reload Active"
    } else {
        "FLUI Scene Render — GPU Compositor Proof"
    };

    let options = WindowOptions::default()
        .with_title(title.to_string())
        .with_size(Size::new(800.0, 600.0))
        .with_resizable(true)
        .with_visible(true)
        .with_decorated(true)
        .with_min_size(None)
        .with_max_size(None);

    // Create window before running the event loop (run() takes ownership)
    let handler = Rc::new(SceneWindow::default());
    let window = platform
        .open_window(options, handler.clone())
        .expect("Failed to open window");

    let opened = window.state().metrics;
    tracing::info!(
        "Window created: {:?} @ {:.1}x scale",
        opened.map(WindowMetrics::physical),
        opened.map_or(1.0, |metrics| metrics.scale().get())
    );

    // `Renderer::new` takes ownership of a `WindowTarget`: the
    // window's surface target, never the window (ADR-0254 §5).
    let mut renderer = pollster::block_on(Renderer::new(window.surface_target()))
        .expect("Failed to create GPU renderer");

    let phys = physical_size(window.as_ref());
    renderer.resize(phys.width as u32, phys.height as u32);

    tracing::info!(
        "GPU: {} ({:?})",
        renderer.capabilities().adapter_name,
        renderer.capabilities().backend
    );

    let renderer = Arc::new(Mutex::new(renderer));

    // Register frame callback — build scene and render each frame
    let renderer_frame = Arc::clone(&renderer);
    let window_for_frame = Rc::downgrade(&window);
    let hot_reload_frame = hot_reload.clone();
    *handler.frame.borrow_mut() = Some(Box::new(move || {
        let Some(window_for_frame) = window_for_frame.upgrade() else {
            return;
        };
        let size = physical_size(window_for_frame.as_ref());
        let w = size.width as f64;
        let h = size.height as f64;

        if !effects_mode && let Some(ref hot_reload) = hot_reload_frame {
            let mut reload = hot_reload.lock().unwrap();
            let (driver, pending_font_reset) = &mut *reload;
            *pending_font_reset |= driver.poll();
            // SAFETY: the scene is rendered and dropped while this driver is
            // locked, before any later reload. The dedicated renderer copies
            // retained font bytes and retains no image-dependent payloads.
            #[expect(unsafe_code)]
            let built = unsafe { driver.build_scene(w, h) };
            if let Some(scene) = built {
                let result = renderer_frame
                    .lock()
                    .unwrap()
                    .render_plugin_scene(&scene, *pending_font_reset);
                match result {
                    Ok(_) => *pending_font_reset = false,
                    Err(error) => tracing::error!(?error, "plugin rendering failed"),
                }
                drop(scene);
                return;
            }
        }
        let scene = if effects_mode {
            effects::build(w, h)
        } else {
            build_test_scene(w, h)
        };

        // Render scene through the full pipeline
        let mut r = renderer_frame.lock().unwrap();
        if let Err(e) = r.render_scene(&scene) {
            tracing::error!("render_scene failed: {:?}", e);
        }
    }));

    // Resize the surface when the snapshot carries a new drawable size.
    let renderer_resize = Arc::clone(&renderer);
    let observed = Rc::downgrade(&window);
    let mut sized = phys;
    *handler.resized.borrow_mut() = Some(Box::new(move || {
        let Some(window) = observed.upgrade() else {
            return;
        };
        let size = physical_size(window.as_ref());
        if size == sized {
            return;
        }
        sized = size;
        renderer_resize
            .lock()
            .unwrap()
            .resize(size.width.unsigned_abs(), size.height.unsigned_abs());
    }));

    // Request first frame
    window.request_redraw();

    if hot_reload.is_some() {
        tracing::info!("Scene render with hot-reload — edit plugin and rebuild to see changes");
    } else {
        tracing::info!("Scene render pipeline active — set FLUI_SCENE_PLUGIN for hot-reload");
    }

    // The handler keeps the window and the renderer alive for the run, and
    // lets the loop end when the window closes.
    platform
        .run(Rc::new(SceneApp {
            _window: window,
            _renderer: renderer,
        }))
        .expect("platform event loop exited with an error");

    tracing::info!("Application finished");
    Ok(())
}

/// The example's application handler: it keeps the window and the renderer
/// for the run and lets the loop end with its last window.
struct SceneApp {
    _window: Rc<dyn flui_native::HostWindow>,
    _renderer: Arc<Mutex<Renderer>>,
}

impl flui_native::AppHandler for SceneApp {
    fn ready(&self, _: flui_native::OwnerPlatform) -> Result<(), flui_native::BootstrapError> {
        tracing::info!("Platform ready");
        Ok(())
    }

    fn quit_requested(&self, _: &flui_native::OwnerPlatform) {}

    fn reopen(&self, _: &flui_native::OwnerPlatform) {}

    fn session_end(
        &self,
        _: &flui_native::OwnerPlatform,
        _: flui_platform::SessionEndPhase,
    ) -> flui_platform::SessionEndAnswer {
        flui_platform::SessionEndAnswer::Proceed
    }

    fn host_notice(&self, _: &flui_native::OwnerPlatform, _: flui_platform::SystemNotice) {}

    fn exit_policy(&self, _: &flui_native::OwnerPlatform) -> flui_native::ExitAnswer {
        flui_native::ExitAnswer::End
    }

    fn wake_deadline(&self, _: &flui_native::OwnerPlatform) -> Option<std::time::Instant> {
        None
    }
}
