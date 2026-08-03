//! Direct rendering mode — bypasses the widget tree (flui-view/flui-rendering)
//! and renders through flui-engine directly. The caller provides a closure
//! that builds a `Scene` each frame via `SceneBuilder`.
//!
//! # Status: experimental, direct-engine only
//!
//! This is **not** a supported cross-platform application entry point, and it
//! is not a smaller `run_app`. It is a direct-engine escape hatch for scene
//! and shader work, with two standing limitations:
//!
//! - **There is no input handling.** The input callback is a no-op stub; a
//!   caller that needs input drives it from inside `render_fn` via its own
//!   state.
//! - **There is no damage tracking**, no widget tree, no layout, no hit test,
//!   and no semantics — every frame is a full repaint of a caller-built scene.
//!
//! Window creation, GPU renderer init, and callback wiring all run inside
//! `on_ready` (ADR-0039 slice 2), the same reorder `run_desktop` already
//! received — this function used to open its window *before* starting the
//! event loop, which failed fast on the winit backend (Linux) instead of
//! opening anything (winit cannot create a window outside a running loop).
//! It now runs on every backend, including winit.
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

use flui_engine::{Recoverability, wgpu::Renderer};
use flui_layer::{LayerTree, Scene, SceneBuilder};
use flui_platform::{
    WindowOptions,
    traits::{DispatchEventResult, PlatformInput},
};
use flui_types::{Size, geometry::px};
use parking_lot::Mutex;

use super::AppConfig;

/// Run a FLUI application in direct rendering mode.
///
/// Opens a window, initializes the GPU renderer, and runs a render loop
/// where the user provides a closure that builds a `Scene` each frame
/// via `SceneBuilder`. This bypasses the entire widget/element/render
/// tree machinery (flui-view, flui-rendering) for direct engine access.
///
/// **Experimental / direct-engine only.** This is not a supported
/// cross-platform application entry point — see the module docs for the two
/// standing limitations (no input handling, no damage tracking). Use
/// [`run_app`](crate::run_app) for applications.
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
/// Runs on every backend, including winit (Linux) — bootstrap moved inside
/// `on_ready` (ADR-0039 slice 2), so the winit backend's requirement of a
/// running event loop before window creation is satisfied on every target.
/// Uses `flui_platform::current_platform()` for platform selection.
///
/// **Experimental**: this is a direct-engine escape hatch, not a supported
/// application entry point — see the module docs.
pub fn run_direct(
    config: AppConfig,
    render_fn: impl FnMut(&mut SceneBuilder<'_>, f32, f32) + Send + 'static,
) -> anyhow::Result<()> {
    // Managed startup, same contract as `run_app`: install FLUI's default
    // backend only into an empty slot. The historical code called
    // `Logger::init`, which panicked the moment a host — or a second
    // `run_direct` in the same process — already owned the subscriber.
    let _installation = super::logging::init_managed_logging(&config);

    tracing::info!(
        title = %config.title,
        size = ?config.size,
        "Starting FLUI direct render mode"
    );

    let platform = flui_platform::current_platform()?;

    // `on_ready` has no return path back to this function's caller (it runs
    // synchronously inside `Platform::run`, which returns `()`) — bootstrap
    // failures thread out through this cell instead, same pattern
    // `run_desktop`'s `bootstrap_error_slot` uses.
    let bootstrap_error: std::rc::Rc<std::cell::RefCell<Option<anyhow::Error>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let bootstrap_error_slot = std::rc::Rc::clone(&bootstrap_error);

    /// The actual direct-mode bootstrap: window, GPU renderer, and callback
    /// wiring. Runs once, synchronously, inside `on_ready` (ADR-0039 slice
    /// 2) — the winit backend can only create a window from inside a
    /// running event loop, so this can no longer run before `run()` starts
    /// it (which is what made this function fail fast on that backend
    /// before this migration).
    fn bootstrap_direct<F>(
        config: AppConfig,
        render_fn: F,
        bootstrap_error_slot: std::rc::Rc<std::cell::RefCell<Option<anyhow::Error>>>,
    ) where
        F: FnMut(&mut SceneBuilder<'_>, f32, f32) + Send + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_platform::OwnerPlatform) -> R) -> R {
            crate::app::runner::with_owner_platform(f)
                .expect("BUG: bootstrap_direct runs only after install_owner_platform")
        }

        // 1. Open window. `Ready` is guaranteed inside `on_ready`
        // (ADR-0039 §1).
        let options: WindowOptions = (&config).into();
        let window = match owner_platform_installed(|owner| owner.open_window(options))
            .and_then(flui_platform::WindowOpen::try_ready)
        {
            Ok(window) => window,
            Err(error) => {
                tracing::error!(%error, "Window creation failed");
                *bootstrap_error_slot.borrow_mut() =
                    Some(anyhow::Error::from(error).context("Failed to create window"));
                owner_platform_installed(flui_platform::OwnerPlatform::quit);
                return;
            }
        };

        // 2. Create GPU renderer
        let phys_size = window.physical_size();
        let renderer = pollster::block_on(Renderer::new(window.as_ref()));
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                *bootstrap_error_slot.borrow_mut() =
                    Some(anyhow::anyhow!(e).context("GPU initialization failed"));
                owner_platform_installed(flui_platform::OwnerPlatform::quit);
                return;
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
                if e.recoverability() == Recoverability::Recoverable {
                    tracing::warn!("Recoverable render error (will retry): {}", e);
                } else {
                    // Covers both Fatal (renderer must be recreated) and
                    // Unrecoverable (surface misconfig / resource I/O / text error:
                    // drop this frame, no blind retry). Neither auto-retries.
                    tracing::error!(error = ?e, "Non-recoverable render error");
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

        // 6. Register input callback. This is a no-op stub: it neither inspects
        // the input nor requests a redraw (`resolved(false, false)`). Direct mode
        // has no widget tree to dispatch input into; a caller who needs input
        // handling drives it from inside `render_fn` via its own state.
        window.on_input(Box::new(move |_input: PlatformInput| {
            DispatchEventResult::resolved(false, false)
        }));

        // 7. Lifecycle callbacks
        window.on_close(Box::new(|| {
            tracing::info!("Window closed");
        }));

        window.on_should_close(Box::new(|| {
            tracing::debug!("Window close requested, allowing");
            true
        }));

        owner_platform_installed(|owner| {
            owner.shared().on_quit(Box::new(|| {
                tracing::info!("Platform quit");
            }));
        });

        // 8. Request initial redraw
        window.request_redraw();

        tracing::info!("Direct render mode initialized, entering event loop");
    }

    // Owner-host clear guard armed BEFORE `run(...)`, not inside `on_ready`
    // (ADR-0039 §6/§7, U7) — see `run_desktop`'s matching comment.
    let _owner_host_clear_guard = crate::app::runner::OwnerHostClearGuard::arm();
    platform.run(Box::new(move |owner| {
        crate::app::runner::install_owner_platform(owner);
        bootstrap_direct(config, render_fn, bootstrap_error_slot);
        tracing::info!("FLUI direct render mode ready");
    }));

    if let Some(error) = bootstrap_error.borrow_mut().take() {
        return Err(error);
    }

    Ok(())
}
