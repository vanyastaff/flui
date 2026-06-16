//! Direct rendering mode — bypasses the widget tree (flui-view/flui-rendering)
//! and renders through flui-engine directly.
//!
//! This is the simplest way to get pixels on screen with FLUI. The user
//! provides a closure that builds a `Scene` each frame via `SceneBuilder`.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::{AppConfig, run_direct};
//!
//! run_direct(
//!     AppConfig::new().with_title("Direct Render").with_size(800, 600),
//!     |builder, w, h| {
//!         use flui_painting::Canvas;
//!         use flui_types::{geometry::px, painting::Paint, Color, Rect};
//!
//!         let mut canvas = Canvas::new();
//!         canvas.draw_rect(
//!             Rect::from_ltwh(px(50.0), px(50.0), px(200.0), px(150.0)),
//!             &Paint::fill(Color::BLUE),
//!         );
//!         let picture = canvas.finish();
//!         builder.add_picture(picture);
//!     },
//! ).unwrap();
//! ```

use std::sync::Arc;

use flui_engine::wgpu::Renderer;
use flui_layer::{LayerTree, Scene, SceneBuilder};
use flui_platform::{
    WindowOptions,
    traits::{DispatchEventResult, PlatformInput},
};
use flui_types::{Size, geometry::px};
use parking_lot::Mutex;

use super::AppConfig;
use crate::embedder::PlatformWindowHandle;

/// Run a FLUI application in direct rendering mode.
///
/// Opens a window, initializes the GPU renderer, and runs a render loop
/// where the user provides a closure that builds a `Scene` each frame
/// via `SceneBuilder`. This bypasses the entire widget/element/render
/// tree machinery (flui-view, flui-rendering) for direct engine access.
///
/// # Arguments
///
/// * `config` - Application configuration (title, size, etc.)
/// * `render_fn` - Called each frame with a `SceneBuilder` and the current
///   viewport size (width, height) in pixels. Build your scene using
///   `SceneBuilder::push_*`, `add_picture`, `add_canvas`, etc.
///
/// # Returns
///
/// Returns `Ok(())` when the event loop exits (window closed).
/// Returns `Err` if platform or GPU initialization fails.
///
/// # Platform Support
///
/// Currently supports desktop platforms (Windows, macOS, Linux).
/// Uses `flui_platform::current_platform()` for platform selection.
pub fn run_direct(
    config: AppConfig,
    render_fn: impl FnMut(&mut SceneBuilder<'_>, f32, f32) + Send + 'static,
) -> anyhow::Result<()> {
    // Initialize logging
    let filter = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "info,flui_app=debug,flui_engine=debug,wgpu=warn".to_string());

    flui_foundation::log::Logger::new()
        .with_filter(&filter)
        .with_level(flui_foundation::log::Level::DEBUG)
        .init();

    tracing::info!(
        title = %config.title,
        size = ?config.size,
        "Starting FLUI direct render mode"
    );

    let platform = flui_platform::current_platform()?;

    // 1. Open window
    let options: WindowOptions = (&config).into();
    let window = platform
        .open_window(options)
        .map_err(|e| anyhow::anyhow!("Failed to create window: {}", e))?;

    // 2. Create GPU renderer
    let phys_size = window.physical_size();
    let renderer = pollster::block_on(async {
        let handle = PlatformWindowHandle(window.as_ref());
        Renderer::new(&handle).await
    });
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return Err(anyhow::anyhow!("GPU initialization failed: {}", e));
        }
    };
    renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

    tracing::info!(
        gpu = %renderer.capabilities().adapter_name,
        backend = ?renderer.capabilities().backend,
        "GPU renderer initialized"
    );

    // 3. Wrap renderer and render_fn for callback sharing
    let renderer = Arc::new(Mutex::new(renderer));
    let render_fn = Arc::new(Mutex::new(render_fn));
    let frame_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // 4. Register frame callback
    let renderer_frame = Arc::clone(&renderer);
    let render_fn_frame = Arc::clone(&render_fn);
    let frame_counter_frame = Arc::clone(&frame_counter);
    window.on_request_frame(Box::new(move || {
        let mut r = renderer_frame.lock();
        let (w, h) = r.size();

        if w == 0 || h == 0 {
            return;
        }

        // Mark full repaint every frame in direct mode (no damage tracking)
        r.mark_full_repaint();

        // Build scene via user closure
        let mut tree = LayerTree::new();
        {
            let mut builder = SceneBuilder::new(&mut tree);
            let mut rfn = render_fn_frame.lock();
            rfn(&mut builder, w as f32, h as f32);
            // builder dropped here, releasing borrow on tree
        }

        let root = tree.root();
        let frame = frame_counter_frame.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let scene = Scene::new(Size::new(px(w as f32), px(h as f32)), tree, root, frame);

        if let Err(e) = r.render_scene(&scene) {
            if e.is_recoverable() {
                tracing::warn!("Recoverable render error (will retry): {}", e);
            } else {
                tracing::error!(error = ?e, "Fatal render error");
            }
        }

        // GPU device-loss recovery: same logic as runner.rs frame callbacks.
        if r.is_device_lost() {
            match pollster::block_on(r.recover()) {
                Ok(()) => {
                    tracing::warn!("GPU device lost — recovered successfully");
                }
                Err(e) => {
                    tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                }
            }
        }
    }));

    // 5. Register resize callback
    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let w = (size.width.0 * scale_factor) as u32;
        let h = (size.height.0 * scale_factor) as u32;
        if w > 0 && h > 0 {
            renderer_resize.lock().resize(w, h);
            tracing::debug!("Window resized to {}x{} (scale: {})", w, h, scale_factor);
        }
    }));

    // 6. Register input callback (triggers redraw on any input)
    window.on_input(Box::new(move |_input: PlatformInput| DispatchEventResult {
        propagate: false,
        default_prevented: false,
    }));

    // 7. Lifecycle callbacks
    window.on_close(Box::new(|| {
        tracing::info!("Window closed");
    }));

    window.on_should_close(Box::new(|| {
        tracing::debug!("Window close requested, allowing");
        true
    }));

    platform.on_quit(Box::new(|| {
        tracing::info!("Platform quit");
    }));

    // 8. Request initial redraw
    window.request_redraw();

    tracing::info!("Direct render mode initialized, entering event loop");

    // 9. Run event loop (takes ownership of platform)
    platform.run(Box::new(|| {
        tracing::info!("FLUI direct render mode ready");
    }));

    Ok(())
}
