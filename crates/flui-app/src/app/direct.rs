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
//! `on_ready` (ADR-0039 §1), the same reorder `run_desktop` already
//! received — this function used to open its window *before* starting the
//! event loop, which failed fast on Linux instead of opening anything (a
//! window could not be created outside a running loop). It now runs on every
//! backend.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_app::{AppConfig, run_direct};
//!
//! run_direct(
//!     AppConfig::new().title("Direct Render").size(800, 600),
//!     |builder, w, h| {
//!         use flui_painting::Canvas;
//!         use flui_painting::{styling::Color, paint::Paint};
//!         use flui_foundation::geometry::Rect;
//!
//!         let mut canvas = Canvas::new();
//!         canvas.draw_rect(
//!             Rect::from_ltwh(50.0, 50.0, 200.0, 150.0),
//!             &Paint::fill(Color::BLUE),
//!         );
//!         let picture = canvas.finish();
//!         builder.add_picture(picture);
//!     },
//! ).unwrap();
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_engine::{EngineError, Recoverability, Renderer};
use flui_layer::{Scene, SceneBuilder};
use flui_platform::{DispatchEventResult, FrameOutcome, PlatformInput};

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
///   `SceneBuilder::push_*`, `add_picture`, `add_canvas`, etc. It runs on
///   the thread that called `run_direct` and never leaves it, so it may
///   capture `Rc`, `Cell` and `RefCell` state; it runs with the renderer
///   free, so a resize that arrives meanwhile is applied.
///
/// # Returns
///
/// Returns `Ok(())` when the event loop exits (window closed).
/// Returns `Err` if platform or GPU initialization fails.
///
/// # Platform Support
///
/// Runs on every backend — bootstrap moved inside
/// `on_ready` (ADR-0039 §1), so a backend's requirement of a
/// running event loop before window creation is satisfied on every target.
/// Uses `flui_native::current_platform()` for platform selection.
///
/// **Experimental**: this is a direct-engine escape hatch, not a supported
/// application entry point — see the module docs.
pub fn run_direct(
    config: AppConfig,
    render_fn: impl FnMut(&mut SceneBuilder, f64, f64) + 'static,
) -> anyhow::Result<()> {
    // Managed startup, same contract as `run_app`: install FLUI's default
    // backend only into an empty slot. The historical code called
    // `Logger::init`, which panicked the moment a host — or a second
    // `run_direct` in the same process — already owned the subscriber.
    let _installation = super::logging::init_managed_logging(&config);

    tracing::info!(
        size = ?config.size,
        "Starting FLUI direct render mode"
    );

    let platform = flui_native::current_platform()?;

    /// The actual direct-mode bootstrap: window, GPU renderer, and callback
    /// wiring. Runs once, synchronously, inside `on_ready` (ADR-0039
    /// §1) — a backend may create a window only from inside a
    /// running event loop, so this cannot run before `run()` starts
    /// it (which is what made this function fail fast on Linux
    /// before this migration).
    ///
    /// Returns `Err` on bootstrap failure — `on_ready` itself is fallible
    /// now, so this propagates straight out of `Platform::run` instead of
    /// threading the failure out through a
    /// `Rc<RefCell<Option<anyhow::Error>>>` side channel (the old
    /// `bootstrap_error_slot` pattern, now redundant and removed: this
    /// function's own `Result` return is `run_direct`'s answer).
    fn bootstrap_direct<F>(config: AppConfig, render_fn: F) -> anyhow::Result<()>
    where
        F: FnMut(&mut SceneBuilder, f64, f64) + 'static,
    {
        fn owner_platform_installed<R>(f: impl FnOnce(&flui_native::OwnerPlatform) -> R) -> R {
            crate::app::runner::with_owner_platform(f)
                .expect("BUG: bootstrap_direct runs only after install_owner_platform")
        }

        // 1. Open window. `Ready` is guaranteed inside `on_ready`
        // (ADR-0039 §1).
        let options = owner_platform_installed(|owner| {
            config.window_options(owner.display_snapshot().ok().as_ref())
        });
        let routes = crate::app::runner::window_routes::WindowRoutes::new();
        let window =
            match owner_platform_installed(|owner| owner.open_window(options, routes.clone()))
                .and_then(flui_native::WindowOpen::try_ready)
            {
                Ok(window) => window,
                Err(error) => {
                    tracing::error!(%error, "Window creation failed");
                    return Err(anyhow::Error::from(error).context("Failed to create window"));
                }
            };

        // 2. Create GPU renderer. `Renderer::new` takes ownership of
        // the window's surface target, which it
        // owns rather than borrows, so the renderer stays sound
        // even if this scope's `window` binding is dropped first.
        let phys_size = window
            .state()
            .metrics
            .map_or_else(flui_foundation::geometry::Size::default, |metrics| {
                metrics.physical()
            });
        // Direct mode has no installed host, so the display is sampled here.
        let display = owner_platform_installed(flui_native::OwnerPlatform::display_snapshot)
            .ok()
            .and_then(|snapshot| crate::app::runner::display_color_on(&snapshot, window.as_ref()));
        let renderer = pollster::block_on(Renderer::with_surface_color(
            window.surface_target(),
            config.surface_color,
            display,
        ));
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = ?e, "GPU init failed");
                return Err(anyhow::anyhow!(e).context("GPU initialization failed"));
            }
        };
        renderer.resize(phys_size.width as u32, phys_size.height as u32);
        renderer.set_frame_latency(config.frame_latency);

        tracing::info!(
            gpu = %renderer.capabilities().adapter_name,
            backend = ?renderer.capabilities().backend,
            "GPU renderer initialized"
        );

        // 3. The frame and resize routes share the renderer and the
        // application's closure through one `DirectFrames`, on this thread
        // (ADR-0323 §1). A frame asks for another when its window was asked
        // for: a scene built for a size the surface no longer has.
        let redraw_window = std::rc::Rc::downgrade(&window);
        let frames = Rc::new(DirectFrames::new(
            renderer,
            Box::new(render_fn),
            Box::new(move || {
                if let Some(window) = redraw_window.upgrade() {
                    window.request_redraw();
                }
            }),
        ));

        // 4. Register frame callback
        let frames_frame = Rc::clone(&frames);
        routes.route_frame_requested(move |_| frames_frame.frame());

        // 5. Follow the window's snapshot: a change of its drawable metrics
        // resizes the surface. Weak: the route lives in the window's
        // own handler.
        let observed = std::rc::Rc::downgrade(&window);
        let sized = std::cell::Cell::new(window.state().metrics);
        routes.route_state_changed(move || {
            let Some(window) = observed.upgrade() else {
                return;
            };
            let metrics = window.state().metrics;
            if metrics == sized.get() {
                return;
            }
            sized.set(metrics);
            let Some(metrics) = metrics else {
                return;
            };
            let physical = metrics.physical();
            let (w, h) = (
                physical.width.unsigned_abs(),
                physical.height.unsigned_abs(),
            );
            frames.resize(w, h);
            tracing::debug!(
                width = w,
                height = h,
                scale = metrics.scale().get(),
                "window resized"
            );
        });

        // 6. Register input callback. This is a no-op stub: it neither inspects
        // the input nor requests a redraw (`resolved(false, false)`). Direct mode
        // has no widget tree to dispatch input into; a caller who needs input
        // handling drives it from inside `render_fn` via its own state.
        routes
            .route_input(move |_input: PlatformInput| DispatchEventResult::resolved(false, false));

        // 7. Lifecycle callbacks
        routes.route_closed(|| {
            tracing::info!("Window closed");
        });

        // Deliberately unconditional, unlike `run_desktop`'s and
        // `open_secondary_window`'s windows, which route this through the
        // close-request router. Direct mode installs no
        // `UiRuntime` at all (see step 6 above and `run_direct`'s own doc), so
        // there is no presentation for a veto to be addressed to and no
        // widget tree that could be holding unsaved work. An embedder that
        // wants to refuse a close here owns the loop already and opens
        // its window with a handler of its own — the
        // presentation-addressed API would have nothing to key on.
        let closing = std::rc::Rc::downgrade(&window);
        routes.route_close_requested(
            || {
                tracing::debug!(
                    "Window close requested, allowing (direct mode hosts no ui_runtime)"
                );
                flui_platform::CloseDecision::Close
            },
            move || {
                if let Some(window) = closing.upgrade() {
                    crate::app::runner::close_granted_request(&*window);
                }
            },
        );

        // Direct mode hosts no runtime to keep the loop alive: its last
        // window closing ends it, and the quit is only logged.
        crate::app::runner::route_quit(|| tracing::info!("Platform quit"));
        crate::app::runner::route_exit_policy(|| true);

        // 8. Request initial redraw
        window.request_redraw();

        tracing::info!("Direct render mode initialized, entering event loop");
        Ok(())
    }

    // Owner-host clear guard armed BEFORE `run(...)`, not inside `on_ready`
    // (ADR-0039 §6/§7) — see `run_desktop`'s matching comment.
    let _owner_host_clear_guard = crate::app::runner::OwnerHostClearGuard::arm();
    platform.run(crate::app::runner::app_handler(move |owner| {
        crate::app::runner::install_owner_platform(owner)?;
        bootstrap_direct(config, render_fn)?;
        tracing::info!("FLUI direct render mode ready");
        Ok(())
    }))?;
    Ok(())
}

/// What direct mode draws into: the window's renderer, or a test's stand-in.
trait DirectSurface {
    /// The surface's size in physical pixels.
    fn size(&self) -> (u32, u32);
    /// Resize the surface to `width` × `height` physical pixels.
    fn resize(&mut self, width: u32, height: u32);
    /// Draw `scene` in full at the surface's size, recovering a lost device
    /// on the way.
    fn draw(&mut self, scene: &Scene);
}

impl DirectSurface for Renderer {
    fn size(&self) -> (u32, u32) {
        Renderer::size(self)
    }

    fn resize(&mut self, width: u32, height: u32) {
        Renderer::resize(self, width, height);
    }

    fn draw(&mut self, scene: &Scene) {
        // Mark full repaint every frame in direct mode (no damage tracking)
        self.mark_full_repaint();

        if let Err(e) = self.render_scene(scene) {
            if e.recoverability() == Recoverability::Recoverable {
                tracing::warn!(error = %e, "recoverable render error, will retry");
            } else {
                // Covers both Fatal (renderer must be recreated) and
                // Unrecoverable (surface misconfig / resource I/O / text error:
                // drop this frame, no blind retry). Neither auto-retries.
                tracing::error!(error = ?e, "Non-recoverable render error");
            }
        }

        // GPU device-loss recovery, on this renderer's own device; a
        // native window's device is rebuilt by the raster executor.
        if self.is_device_lost() {
            match pollster::block_on(self.recover()) {
                Ok(()) => {
                    tracing::warn!("GPU device lost — recovered successfully");
                }
                Err(e @ EngineError::SurfaceTargetUnavailable { .. }) => {
                    // The window owner reports its native handle is gone
                    // or suspended. `HandleError::Unavailable`
                    // is `Recoverability::Recoverable` (worth waiting out);
                    // `HandleError::NotSupported` is `Fatal` (the owner can
                    // never answer this handle kind) — but this frame loop
                    // does not branch on that classification, so every
                    // `Err` here is retried next frame regardless, same as
                    // the fallback arm below.
                    tracing::warn!(
                        error = ?e,
                        "GPU device recovery failed — window target unavailable; retried next frame regardless"
                    );
                }
                Err(e) => {
                    tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                }
            }
        }
    }
}

/// The application's closure that builds each frame's scene.
type RenderScene = Box<dyn FnMut(&mut SceneBuilder, f64, f64)>;

/// Direct mode's frames: the surface, the application's closure, and the
/// window's request for another frame, shared by the frame and resize routes
/// on the thread that runs the loop.
///
/// The closure builds its scene with the surface free, so a resize that
/// arrives meanwhile (a nested message loop inside the closure) is applied at
/// once. A scene is drawn only at the size it was built for: when the
/// surface changed size during the build, the frame draws nothing, asks the
/// window for another frame and answers [`FrameOutcome::Declined`]. A route
/// entered from inside another answers instead of borrowing twice: a frame
/// inside a frame declines, and a resize while the surface draws is kept for
/// the next frame.
struct DirectFrames<S> {
    surface: RefCell<S>,
    /// A resize that arrived while the surface was drawing.
    pending_size: Cell<Option<(u32, u32)>>,
    render: RefCell<RenderScene>,
    request_frame: Box<dyn Fn()>,
}

impl<S: DirectSurface> DirectFrames<S> {
    fn new(surface: S, render: RenderScene, request_frame: Box<dyn Fn()>) -> Self {
        Self {
            surface: RefCell::new(surface),
            pending_size: Cell::new(None),
            render: RefCell::new(render),
            request_frame,
        }
    }

    /// The surface's size, after a resize kept while it was drawing.
    /// `None` while the surface draws.
    fn current_size(&self) -> Option<(u32, u32)> {
        let mut surface = self.surface.try_borrow_mut().ok()?;
        if let Some((width, height)) = self.pending_size.take() {
            surface.resize(width, height);
        }
        Some(surface.size())
    }

    /// Run one frame.
    fn frame(&self) -> FrameOutcome {
        let Some(built_for) = self.current_size() else {
            return FrameOutcome::Declined;
        };
        // A surface with no area has nothing to draw into: no frame ran.
        if built_for.0 == 0 || built_for.1 == 0 {
            return FrameOutcome::Declined;
        }
        let scene = {
            let Ok(mut render) = self.render.try_borrow_mut() else {
                return FrameOutcome::Declined;
            };
            let mut builder = SceneBuilder::new();
            render(&mut builder, f64::from(built_for.0), f64::from(built_for.1));
            Scene::new(builder.build())
        };
        if self.current_size() != Some(built_for) {
            // Built for a size the surface no longer has: not drawn, and the
            // frame is asked again, at the size the surface has now.
            (self.request_frame)();
            return FrameOutcome::Declined;
        }
        self.surface.borrow_mut().draw(&scene);
        FrameOutcome::Produced
    }

    /// Resize the surface, or keep the size for the next frame while the
    /// surface draws.
    fn resize(&self, width: u32, height: u32) {
        match self.surface.try_borrow_mut() {
            Ok(mut surface) => {
                self.pending_size.set(None);
                surface.resize(width, height);
            }
            Err(_) => self.pending_size.set(Some((width, height))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::{Rc, Weak};

    use flui_layer::Scene;
    use flui_platform::FrameOutcome;

    use super::{DirectFrames, DirectSurface};

    /// A surface that keeps its size and the size of each frame it drew.
    struct SurfaceLog {
        size: (u32, u32),
        drawn: Rc<RefCell<Vec<(u32, u32)>>>,
    }

    impl DirectSurface for SurfaceLog {
        fn size(&self) -> (u32, u32) {
            self.size
        }
        fn resize(&mut self, width: u32, height: u32) {
            self.size = (width, height);
        }
        fn draw(&mut self, _scene: &Scene) {
            self.drawn.borrow_mut().push(self.size);
        }
    }

    /// A scene built while the surface was resized (a nested message loop
    /// inside the closure) is not drawn: the frame declines and asks for
    /// another, which draws at the new size.
    ///
    /// Red-check: draw the scene whatever the surface's size — the first
    /// frame draws at 1024 × 768 a scene built for 800 × 600.
    #[test]
    fn a_scene_built_across_a_resize_is_not_drawn_and_the_frame_is_asked_again() {
        let drawn: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
        let asked = Rc::new(Cell::new(0_u32));
        let frames = Rc::new_cyclic(|me: &Weak<DirectFrames<SurfaceLog>>| {
            let me = me.clone();
            let resized = Cell::new(false);
            let counted = Rc::clone(&asked);
            DirectFrames::new(
                SurfaceLog {
                    size: (800, 600),
                    drawn: Rc::clone(&drawn),
                },
                Box::new(move |_, _, _| {
                    if !resized.replace(true) {
                        let frames = me.upgrade().expect("the frames run this closure");
                        frames.resize(1024, 768);
                    }
                }),
                Box::new(move || counted.set(counted.get() + 1)),
            )
        });

        assert_eq!(frames.frame(), FrameOutcome::Declined);
        assert!(drawn.borrow().is_empty(), "the stale scene is not drawn");
        assert_eq!(asked.get(), 1, "the frame is asked again");

        assert_eq!(frames.frame(), FrameOutcome::Produced);
        assert_eq!(*drawn.borrow(), [(1024, 768)]);
    }

    /// A frame asked for from inside the closure's build declines and draws
    /// nothing; the frame around it draws once.
    #[test]
    fn a_frame_asked_for_inside_the_build_declines() {
        let drawn: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
        let nested = Rc::new(Cell::new(None));
        let frames = Rc::new_cyclic(|me: &Weak<DirectFrames<SurfaceLog>>| {
            let me = me.clone();
            let answered = Rc::clone(&nested);
            DirectFrames::new(
                SurfaceLog {
                    size: (800, 600),
                    drawn: Rc::clone(&drawn),
                },
                Box::new(move |_, _, _| {
                    if answered.get().is_none() {
                        let frames = me.upgrade().expect("the frames run this closure");
                        answered.set(Some(frames.frame()));
                    }
                }),
                Box::new(|| {}),
            )
        });

        assert_eq!(frames.frame(), FrameOutcome::Produced);
        assert_eq!(nested.get(), Some(FrameOutcome::Declined));
        assert_eq!(*drawn.borrow(), [(800, 600)]);
    }
}
