//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use flui_view::{StatelessView, View};

use super::{AppBinding, AppConfig};

/// Run a FLUI application with default configuration.
///
/// This is the internal implementation called by `run_app()`.
pub fn run_app_impl<V>(root: V)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    run_app_with_config_impl(root, AppConfig::default());
}

/// Run a FLUI application with custom configuration.
///
/// This is the internal implementation called by `run_app_with_config()`.
pub fn run_app_with_config_impl<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    // Initialize logging
    init_logging();

    tracing::info!(
        title = %config.title,
        size = ?config.size,
        fps = config.target_fps,
        "Starting FLUI application"
    );

    // Run platform-specific event loop
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    {
        run_desktop(root, config);
    }

    #[cfg(target_os = "android")]
    {
        let _ = (root, config);
        panic!(
            "On Android, use flui_app::run_app_android() from android_main() \
             instead of run_app(). AndroidApp must be provided by the system."
        );
    }

    #[cfg(target_os = "ios")]
    {
        run_ios(config);
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_web(root, config);
    }
}

/// Initialize logging based on environment.
fn init_logging() {
    // Use flui_foundation::log for cross-platform logging (desktop, Android, iOS, WASM).
    // Module was merged from the standalone flui-log crate in D-block PR-C-1 U2.
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        "info,flui_app=debug,flui_view=debug,flui_rendering=debug,wgpu=warn".to_string()
    });

    flui_foundation::log::Logger::new()
        .with_filter(&filter)
        // TRACE ceiling: the per-target filter (RUST_LOG / the default
        // string above) decides what's emitted — a DEBUG ceiling here
        // silently made every trace! unreachable no matter what the
        // user put in RUST_LOG.
        .with_level(flui_foundation::log::Level::TRACE)
        .init();
}

// ============================================================================
// Desktop Implementation (Windows, macOS, Linux) via flui-platform
// ============================================================================

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
thread_local! {
    /// Transitional home for the desktop window's [`UiRealm`]
    /// (ADR-0027 migration step 1). `PlatformWindow` callbacks still require
    /// `Send` (the Send costume this ADR retires in steps 2–3), so the
    /// `!Send` runtime cannot move into the frame closure yet; it lives on
    /// the event-loop thread, and the frame closure — invoked only on that
    /// thread — reaches it here. Retires when the platform callback bounds
    /// drop and the runtime becomes the closure's owned state.
    ///
    /// [`UiRealm`]: super::ui_realm::UiRealm
    static DESKTOP_UI_REALM: std::cell::RefCell<Option<super::ui_realm::UiRealm>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
fn run_desktop<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    use std::sync::Arc;

    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_hot_reload::{
        HotReloadTier, WorkerPollOutcome, WorkerReloadDriver, engine::env, set_request_rebuild,
    };
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting desktop platform via flui-platform");

    let worker_driver = config
        .worker_plugin_path
        .clone()
        .or_else(|| std::env::var(env::WORKER_PLUGIN).ok().map(Into::into))
        .map(WorkerReloadDriver::new);
    if worker_driver.is_some() {
        set_request_rebuild(|| {
            AppBinding::instance().perform_hot_reload(HotReloadTier::HotReload);
        });
    }
    let worker_driver = Arc::new(Mutex::new(worker_driver));

    let platform = flui_platform::current_platform().expect("Failed to initialize platform");

    // 1. Open window before run() since run() takes ownership
    let options: WindowOptions = (&config).into();
    let window = platform
        .open_window(options)
        .expect("Failed to create window");

    // 2. Create GPU renderer directly (no DesktopEmbedder)
    let phys_size = window.physical_size();
    let renderer = pollster::block_on(async {
        let handle = PlatformWindowHandle(window.as_ref());
        Renderer::new(&handle).await
    });
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return;
        }
    };
    renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

    // 3. Mount root widget at the LOGICAL size; the framework lays out
    // in logical pixels and the paint root's DPR transform maps to the
    // physical surface. Set the DPR BEFORE attach so the RenderView
    // configuration and the first frame agree on the scale.
    let scale_factor = window.scale_factor() as f32;
    AppBinding::instance()
        .render_pipeline_mut()
        .set_device_pixel_ratio(scale_factor);
    let logical = window.logical_size();
    if let Err(e) = AppBinding::instance().attach_root_widget_with_size(
        &root,
        logical.width.0 as f32,
        logical.height.0 as f32,
    ) {
        tracing::error!("Root widget attach failed: {:?}", e);
        return;
    }
    register_hit_test_render_view();

    // 3b. Wire the wake chain (E0a).
    //
    // `on_need_frame` fires whenever `handle_build_scheduled` determines a new
    // frame is required (e.g. after setState).  The closure calls `wake_frame`
    // which sets `needs_redraw` atomically AND calls `PlatformWindow::
    // request_redraw()` so the winit event loop wakes from idle.
    //
    // Deadlock analysis:
    // * `wake_frame` acquires only `active_window` (leaf Mutex).
    // * The closure is called from `handle_build_scheduled`, which holds no
    //   `inner`/`widgets` lock (see `WidgetsBinding::handle_build_scheduled`
    //   doc).
    // * `on_need_frame` itself is a separate `RwLock` on `WidgetsBinding`,
    //   never held across any `inner` critical section.
    // Therefore: no lock ordering conflict.
    {
        let widgets = AppBinding::instance().widgets();
        widgets.set_on_need_frame(|| {
            AppBinding::instance().wake_frame();
        });
    }

    // Wire `on_build_scheduled` on the BuildOwner so a dirty-element
    // registration (e.g. from setState inside an element build) wakes the
    // platform loop. The callback fires from inside `schedule_build_for`,
    // which runs during a build while the AppBinding `widgets` write lock is
    // held — so it must NOT re-lock `widgets`. It calls `wake_frame`
    // directly (the same effect as the `on_need_frame` callback above),
    // which touches only the `active_window` leaf lock. Routing instead
    // through `WidgetsBinding::instance().handle_build_scheduled()` would be
    // doubly wrong: that global singleton is a different binding from the
    // AppBinding-owned one whose `on_need_frame` was just registered (so the
    // wake silently never fires), and reaching the owned binding via
    // `widgets()` would deadlock on the held write lock.
    {
        let widgets = AppBinding::instance().widgets();
        widgets.with_build_owner_mut(|build_owner| {
            build_owner.set_on_build_scheduled(|| {
                AppBinding::instance().wake_frame();
            });
        });
    }

    // 3c. Construct the per-window owner and its bounded command inbox
    // (ADR-0027 §1/§3). The wake is the existing chain: `wake_frame` sets
    // `needs_redraw` and queues a `RedrawRequested`, so a command sent to an
    // idle loop produces the frame whose drain observes it.
    //
    // Clear a runtime left by a previous `run_desktop` on this thread first
    // (its claim releases on drop) — otherwise a second run in the same
    // process would hit the at-most-one guard and silently never launch.
    DESKTOP_UI_REALM.with(|slot| drop(slot.borrow_mut().take()));
    let ui_realm = match super::ui_realm::UiRealm::new(Arc::new(|| {
        AppBinding::instance().wake_frame();
    })) {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::error!(error = %e, "UiRealm construction failed");
            return;
        }
    };
    tracing::info!(
        realm_id = ?ui_realm.realm_id(),
        inbox_capacity = ui_realm.command_sender().capacity(),
        "UiRealm constructed"
    );
    DESKTOP_UI_REALM.with(|slot| *slot.borrow_mut() = Some(ui_realm));

    // 4. Wrap renderer for callback sharing
    let renderer = Arc::new(Mutex::new(renderer));

    // 5. Register input callback -> AppBinding::handle_input()
    window.on_input(Box::new(move |input: PlatformInput| {
        AppBinding::instance().handle_input(input);
        DispatchEventResult {
            propagate: false,
            default_prevented: true,
        }
    }));

    // 6. Register frame callback -> scheduler + AppBinding::render_frame()
    let renderer_frame = Arc::clone(&renderer);
    let worker_driver_frame = Arc::clone(&worker_driver);
    let frame_budget =
        std::time::Duration::from_secs_f64(1.0 / f64::from(config.target_fps.max(1)));
    let mut last_frame_started: Option<web_time::Instant> = None;
    window.on_request_frame(Box::new(move || {
        if let Some(ref mut driver) = *worker_driver_frame.lock()
            && matches!(driver.poll(), WorkerPollOutcome::Reloaded { .. })
        {
            AppBinding::instance().perform_hot_reload(HotReloadTier::HotReload);
        }

        let binding = AppBinding::instance();
        let scheduler = Scheduler::instance();

        // Owner-inbox drain (ADR-0027 §3): commands and worker results
        // commit HERE, at the frame boundary while the scheduler phase is
        // Idle — never inside the frame transaction below. Runs before the
        // dirty gate so a command-driven redraw request is observed by the
        // very frame its wake produced.
        //
        // The runtime is TAKEN out of the slot for the drain (and restored
        // after) so drained user closures never run under the RefCell
        // borrow: a command that re-enters this frame callback through a
        // nested platform pump then finds an empty slot and skips the
        // drain, instead of panicking the borrow.
        let inbox_redraw = {
            let taken = DESKTOP_UI_REALM.with(|slot| slot.borrow_mut().take());
            match taken {
                Some(mut runtime) => {
                    let report = runtime.drain_commands();
                    if report != super::ui_realm::DrainReport::default() {
                        tracing::trace!(?report, "owner inbox drained");
                    }
                    let redraw = runtime.take_redraw_request();
                    DESKTOP_UI_REALM.with(|slot| *slot.borrow_mut() = Some(runtime));
                    redraw
                }
                None => false,
            }
        };

        // On-demand rendering: skip frame if nothing changed. A frame
        // the SCHEDULER scheduled (a pending animation ticker callback)
        // counts as work: `needs_redraw` is cleared by `mark_rendered`
        // at the end of the previous frame, so without this check the
        // gate starves tickers after one frame — the wake hook gets the
        // event loop here, and this lets the pump actually run.
        let dirty = inbox_redraw || binding.needs_redraw() || binding.has_pending_work();
        if !dirty && !scheduler.is_frame_scheduled() {
            return;
        }

        // Pace pure ticker-driven frames to the configured target FPS.
        // WM_PAINT-style redraw requests carry no vsync: an animation
        // re-requesting a redraw every frame would otherwise spin the
        // render loop as fast as the CPU allows (observed: ~30 000 fps
        // with a Mailbox present mode). Dirty work renders immediately.
        if !dirty && let Some(started) = last_frame_started {
            let elapsed = started.elapsed();
            if elapsed < frame_budget {
                std::thread::sleep(
                    frame_budget
                        .checked_sub(elapsed)
                        .expect("BUG: `elapsed < frame_budget` was checked on the previous line"),
                );
            }
        }

        let now = web_time::Instant::now();
        last_frame_started = Some(now);

        // Scheduler callbacks (animations). NOTE: the global `Scheduler` is driven
        // off this per-frame `Instant::now()`, while the tree-bound `Vsync`
        // (AppBinding::draw_frame) ticks off `AppBinding`'s own `start` origin —
        // two separate clocks ON PURPOSE: the controller sets are disjoint (implicit
        // animations register with `Vsync`; plain controllers carry a private
        // `Scheduler` ticker, never the global one), so the origins never need to
        // agree and no controller is advanced twice.
        // ADR-0021 U1.5: the ONE shared frame ordering — begin (transient +
        // microtasks + the single async-driver poll) -> persistent callbacks ->
        // the pipeline below -> post-frame callbacks -> Idle. `HeadlessBinding`
        // drives the same helper on its binding-local scheduler.
        scheduler.drive_frame(now, || {
            // Render frame via AppBinding
            let mut r = renderer_frame.lock();
            binding.render_frame(&mut *r);

            // GPU device-loss recovery: if the device was lost during this frame
            // (detected by the wgpu callback that fired between render_frame calls),
            // attempt a synchronous rebuild on the runner thread. `pollster` is
            // already a dep and safe to use here — the desktop runner owns this
            // synchronous callback, not an async executor.
            if r.is_device_lost() {
                match pollster::block_on(r.recover()) {
                    Ok(()) => {
                        tracing::warn!("GPU device lost — recovered successfully");
                        // `wake_frame` (not `request_redraw`) so an idle winit loop
                        // actually queues a `RedrawRequested`: device loss is
                        // detected on a quiescent loop, where only flipping the
                        // `needs_redraw` flag would leave the recovered renderer
                        // idle until the next external input/resize.
                        AppBinding::instance().wake_frame();
                    }
                    Err(e) => {
                        // Driver may still be resetting. Log and let the next frame
                        // retry; the device-lost flag remains set so recover() will
                        // be tried again.
                        tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                    }
                }
            }
        });
    }));

    // 7. Register resize callback -> renderer.resize()
    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let w = (size.width.0 * scale_factor) as u32;
        let h = (size.height.0 * scale_factor) as u32;
        renderer_resize.lock().resize(w, h);
        // A monitor change can change the DPR — keep the paint root's
        // scale in sync with the surface.
        AppBinding::instance()
            .render_pipeline_mut()
            .set_device_pixel_ratio(scale_factor);
        AppBinding::instance().request_redraw();
    }));

    // 8. Lifecycle callbacks

    // Platform quit -> transition to Terminating
    platform.on_quit(Box::new(|| {
        tracing::info!("Platform quit");
        AppBinding::instance().transition_lifecycle(LifecycleEvent::Terminating);
    }));

    // Window close -> log and let the platform handle quit
    // (Windows window proc already calls PostQuitMessage on WM_DESTROY)
    window.on_close(Box::new(move || {
        tracing::info!("Window closed");
    }));

    // Window should-close -> allow by default
    window.on_should_close(Box::new(|| {
        tracing::debug!("Window close requested, allowing");
        true
    }));

    // Window active status -> lifecycle Activated/Deactivated
    window.on_active_status_change(Box::new(|active| {
        let event = if active {
            LifecycleEvent::Activated
        } else {
            LifecycleEvent::Deactivated
        };
        AppBinding::instance().transition_lifecycle(event);
    }));

    // Mark lifecycle as started
    AppBinding::instance().transition_lifecycle(LifecycleEvent::Started);

    // 9. Request initial redraw
    window.request_redraw();

    // 10. Store window in AppBinding for runtime access
    AppBinding::instance().set_window(window);

    tracing::info!("Desktop platform initialized with callbacks");

    // Run the event loop (takes ownership of the platform)
    platform.run(Box::new(|| {
        tracing::info!("Platform ready");
    }));

    // Event loop exited: drop the runtime now (releases the at-most-one
    // claim; outstanding senders turn `OwnerGone`) instead of at thread
    // death.
    DESKTOP_UI_REALM.with(|slot| drop(slot.borrow_mut().take()));
}

/// Register the hit-test root `RenderView` with the `RendererBinding`
/// (`view_id = 0`).
///
/// `WidgetsBinding::attach_root_widget` bootstraps the *paint* render tree
/// (`RootRenderElement` → `PipelineOwner`), but hit testing routes through the
/// `RendererBinding`'s own per-view registry. V-7 keeps these two `RenderView`s
/// mapped independently: the paint root lives in the `PipelineOwner`; the
/// hit-test root is registered here by the runner after attach.
fn register_hit_test_render_view() {
    use std::sync::Arc;

    use flui_rendering::{binding::RendererBinding, view::RenderView};

    let renderer = AppBinding::instance().renderer();
    let view = Arc::new(parking_lot::RwLock::new(RenderView::new()));
    renderer.add_render_view_with_config(0, view);
    tracing::info!("RenderView registered for hit testing (view_id=0)");
}

// ============================================================================
// Android Implementation
// ============================================================================

/// Run a FLUI application on Android with default configuration.
///
/// This is the primary entry point for Android apps. Call this from your
/// `android_main()` function:
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     flui_app::run_app_android(app, MyRootView);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android<V>(app: android_activity::AndroidApp, root: V)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    run_app_android_with_config(app, root, AppConfig::default());
}

/// Run a FLUI application on Android with custom configuration.
///
/// Like [`run_app_android`] but allows specifying app configuration.
///
/// ```rust,ignore
/// #[no_mangle]
/// fn android_main(app: AndroidApp) {
///     let config = AppConfig::new()
///         .with_title("My App")
///         .with_size(800, 600);
///     flui_app::run_app_android_with_config(app, MyRootView, config);
/// }
/// ```
#[cfg(target_os = "android")]
pub fn run_app_android_with_config<V>(app: android_activity::AndroidApp, root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    init_logging();

    tracing::info!(
        title = %config.title,
        "Starting FLUI application on Android"
    );

    run_android(root, config, app);
}

#[cfg(target_os = "android")]
fn run_android<V>(root: V, config: AppConfig, app: android_activity::AndroidApp)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    use std::{path::PathBuf, sync::Arc};

    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_hot_reload::HotReloadDriver;
    use flui_platform::{
        AndroidPlatform, Platform, WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting Android platform via flui-platform");

    // Hot-reload: build plugin path from app's internal data directory
    let plugin_path: PathBuf = app
        .internal_data_path()
        .map(|p| p.join("libflui_scene.so"))
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/libflui_scene.so"));

    let hot_reload = Arc::new(Mutex::new(HotReloadDriver::new(&plugin_path)));

    let platform: Box<dyn Platform> = Box::new(AndroidPlatform::new(app));

    // 1. Open window (wraps the existing ANativeWindow) before run()
    let options: WindowOptions = (&config).into();
    let window = platform
        .open_window(options)
        .expect("Failed to create Android window");

    // 2. Create GPU renderer (Vulkan backend on Android)
    let phys_size = window.physical_size();
    let renderer = pollster::block_on(async {
        let handle = PlatformWindowHandle(window.as_ref());
        Renderer::new(&handle).await
    });
    let mut renderer = match renderer {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GPU init failed: {:?}", e);
            return;
        }
    };
    renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

    // 3. Mount root widget (used when no plugin is active) at the
    // LOGICAL size; the paint root's DPR transform maps to physical.
    let scale_factor = window.scale_factor() as f32;
    AppBinding::instance()
        .render_pipeline_mut()
        .set_device_pixel_ratio(scale_factor);
    let logical = window.logical_size();
    if let Err(e) = AppBinding::instance().attach_root_widget_with_size(
        &root,
        logical.width.0 as f32,
        logical.height.0 as f32,
    ) {
        tracing::error!("Root widget attach failed: {:?}", e);
        return;
    }
    register_hit_test_render_view();

    // 4. Wrap renderer for callback sharing
    let renderer = Arc::new(Mutex::new(renderer));

    // 5. Register input callback -> AppBinding::handle_input()
    window.on_input(Box::new(move |input: PlatformInput| {
        AppBinding::instance().handle_input(input);
        DispatchEventResult {
            propagate: false,
            default_prevented: true,
        }
    }));

    // 6. Register frame callback -- with hot-reload plugin override
    let renderer_frame = Arc::clone(&renderer);
    let hot_reload_frame = Arc::clone(&hot_reload);
    window.on_request_frame(Box::new(move || {
        let mut r = renderer_frame.lock();
        let (w, h) = r.size();
        let mut hr = hot_reload_frame.lock();

        // Poll for plugin updates (mtime check, auto-reload)
        hr.poll(w as f32, h as f32);

        // If plugin is active, use its Scene instead of AppBinding's pipeline
        if let Some(scene) = hr.build_scene(w as f32, h as f32) {
            drop(hr);
            if let Err(e) = r.render_scene(&scene) {
                tracing::error!("Plugin render failed: {:?}", e);
            }
            return;
        }
        drop(hr);
        drop(r);

        // Normal path: use AppBinding's widget pipeline
        let binding = AppBinding::instance();
        // Same gate as the desktop path: a scheduler-scheduled frame
        // (pending animation ticker) is work even when nothing is dirty.
        if !binding.needs_redraw()
            && !binding.has_pending_work()
            && !Scheduler::instance().is_frame_scheduled()
        {
            return;
        }

        let now = web_time::Instant::now();
        let scheduler = Scheduler::instance();
        // The ONE shared frame ordering (ADR-0021 U1.5) — see the desktop path.
        scheduler.drive_frame(now, || {
            let mut r = renderer_frame.lock();
            binding.render_frame(&mut *r);

            // GPU device-loss recovery (same logic as the desktop path).
            if r.is_device_lost() {
                match pollster::block_on(r.recover()) {
                    Ok(()) => {
                        tracing::warn!("GPU device lost — recovered successfully");
                        // `wake_frame` (not `request_redraw`) so an idle winit loop
                        // actually queues a `RedrawRequested`: device loss is
                        // detected on a quiescent loop, where only flipping the
                        // `needs_redraw` flag would leave the recovered renderer
                        // idle until the next external input/resize.
                        AppBinding::instance().wake_frame();
                    }
                    Err(e) => {
                        tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                    }
                }
            }
        });
    }));

    // 7. Register resize callback -> renderer.resize()
    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let w = (size.width.0 * scale_factor) as u32;
        let h = (size.height.0 * scale_factor) as u32;
        renderer_resize.lock().resize(w, h);
        // A monitor change can change the DPR — keep the paint root's
        // scale in sync with the surface.
        AppBinding::instance()
            .render_pipeline_mut()
            .set_device_pixel_ratio(scale_factor as f32);
        AppBinding::instance().request_redraw();
    }));

    // 8. Lifecycle callbacks

    // Platform quit -> transition to Terminating
    platform.on_quit(Box::new(|| {
        tracing::info!("Platform quit");
        AppBinding::instance().transition_lifecycle(LifecycleEvent::Terminating);
    }));

    // Window close (fired by Android Destroy event)
    window.on_close(Box::new(move || {
        tracing::info!("Window closed");
    }));

    // Window active status -> lifecycle Activated/Deactivated
    window.on_active_status_change(Box::new(|active| {
        let event = if active {
            LifecycleEvent::Activated
        } else {
            LifecycleEvent::Deactivated
        };
        AppBinding::instance().transition_lifecycle(event);
    }));

    // Mark lifecycle as started
    AppBinding::instance().transition_lifecycle(LifecycleEvent::Started);

    // 9. Request initial redraw
    window.request_redraw();

    // 10. Store window in AppBinding for runtime access
    AppBinding::instance().set_window(window);

    tracing::info!("Android platform initialized with callbacks (hot-reload enabled)");

    // Run the event loop (takes ownership of the platform)
    platform.run(Box::new(|| {
        tracing::info!("Android platform ready");
    }));
}

// ============================================================================
// iOS Implementation
// ============================================================================

#[cfg(target_os = "ios")]
fn run_ios(_config: AppConfig) {
    tracing::info!("iOS platform - not yet implemented");
    // TODO: Implement UIKit integration
}

// ============================================================================
// Web Implementation
// ============================================================================

#[cfg(target_arch = "wasm32")]
fn run_web<V>(root: V, config: AppConfig)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    use std::sync::Arc;

    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting web platform via flui-platform");

    let platform = flui_platform::current_platform().expect("Failed to initialize web platform");

    // 1. Open window (creates canvas) before run() since run() takes ownership
    let options: WindowOptions = (&config).into();
    let window = platform
        .open_window(options)
        .expect("Failed to create canvas window");

    // 2. Shared renderer slot — starts as None, filled async once the WebGPU
    //    adapter is available. `Option` lets the frame callback skip frames that
    //    arrive before the renderer is ready.
    let renderer: Arc<Mutex<Option<Renderer>>> = Arc::new(Mutex::new(None));

    let phys_size = window.physical_size();
    let renderer_init = Arc::clone(&renderer);
    let renderer_window = window.as_ref() as *const dyn flui_platform::PlatformWindow;

    // SAFETY: On wasm32, the runtime is single-threaded and cooperative.
    // The raw pointer to the window is valid for the duration of
    // `spawn_local` because `window` is alive in the enclosing scope, which
    // is not dropped until `platform.run()` ends (after `spawn_local`
    // completes). The pointer is cast back to a shared reference only inside
    // the `async move` block, and no other task can alias or mutate the
    // window concurrently on the single-threaded wasm executor.
    #[allow(unsafe_code)]
    wasm_bindgen_futures::spawn_local(async move {
        // SAFETY: See the block-level SAFETY comment above.
        let window_ref = unsafe { &*renderer_window };
        let handle = PlatformWindowHandle(window_ref);
        let mut r = match Renderer::new(&handle).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                return;
            }
        };
        r.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);
        tracing::info!("WebGPU renderer initialized");
        *renderer_init.lock() = Some(r);
    });

    // 3. Mount root widget at the LOGICAL size; the paint root's DPR
    // transform maps to the physical canvas.
    let scale_factor = window.scale_factor() as f32;
    AppBinding::instance()
        .render_pipeline_mut()
        .set_device_pixel_ratio(scale_factor);
    let logical = window.logical_size();
    if let Err(e) = AppBinding::instance().attach_root_widget_with_size(
        &root,
        logical.width.0 as f32,
        logical.height.0 as f32,
    ) {
        tracing::error!("Root widget attach failed: {:?}", e);
        return;
    }
    register_hit_test_render_view();

    // 4. Register input callback
    window.on_input(Box::new(move |input: PlatformInput| {
        AppBinding::instance().handle_input(input);
        DispatchEventResult {
            propagate: false,
            default_prevented: true,
        }
    }));

    // 5. Register frame callback
    let renderer_frame = Arc::clone(&renderer);
    window.on_request_frame(Box::new(move || {
        let binding = AppBinding::instance();

        // Same gate as the desktop path: a scheduler-scheduled frame
        // (pending animation ticker) is work even when nothing is dirty.
        if !binding.needs_redraw()
            && !binding.has_pending_work()
            && !Scheduler::instance().is_frame_scheduled()
        {
            return;
        }

        let now = web_time::Instant::now();
        let scheduler = Scheduler::instance();
        // The ONE shared frame ordering (ADR-0021 U1.5) — see the desktop path.
        //
        // The early `return` below now leaves the *closure*, not the frame
        // callback, so `end_frame` still runs and the frame cannot be left
        // half-open in the `PersistentCallbacks` phase. Before U1.5 this path
        // returned after `handle_draw_frame`, which happened to be harmless only
        // because that method reset the phase itself.
        scheduler.drive_frame(now, || {
            // Renderer may not be ready yet (async init in flight). Skip this frame;
            // the spawn_local will call request_redraw once the renderer is ready.
            let mut slot = renderer_frame.lock();
            let Some(r) = slot.as_mut() else {
                return;
            };

            binding.render_frame(r);

            // GPU device-loss recovery on wasm. `block_on` is unavailable, but
            // wasm32 is single-threaded and `spawn_local` is the correct async
            // dispatch. We drop the `slot` guard before spawning so the future can
            // re-acquire the lock without deadlocking.
            if r.is_device_lost() {
                drop(slot); // release the lock before spawning the async recovery
                let renderer_recover = Arc::clone(&renderer_frame);
                wasm_bindgen_futures::spawn_local(async move {
                    // Take the renderer OUT of the slot so the lock is NOT held
                    // across `.await`. wasm32 is single-threaded: holding the
                    // `parking_lot::Mutex` guard across `recover().await` (which
                    // suspends in `request_adapter`/`request_device`) would let the
                    // next `on_request_frame` block forever on `lock()` — a hard
                    // hang. While recovery is in flight the slot is `None`, so frame
                    // callbacks skip rendering instead of blocking. A racing second
                    // spawn finds `None` here and returns — no double recovery.
                    let Some(mut renderer) = renderer_recover.lock().take() else {
                        return;
                    };
                    let result = renderer.recover().await;
                    // Restore the renderer regardless of outcome; a failed recover
                    // leaves the device lost and the next frame re-detects + retries.
                    *renderer_recover.lock() = Some(renderer);
                    match result {
                        Ok(()) => {
                            tracing::warn!("GPU device lost — recovered successfully");
                            // `wake_frame` so the idle rAF loop is pumped — see the
                            // native paths; flipping `needs_redraw` alone would leave
                            // the recovered renderer idle until the next input event.
                            AppBinding::instance().wake_frame();
                        }
                        Err(e) => {
                            tracing::error!(error = ?e, "GPU device recovery failed; will retry next frame");
                        }
                    }
            });
        }
        });
    }));

    // 6. Lifecycle callbacks
    platform.on_quit(Box::new(|| {
        tracing::info!("Web platform quit");
        AppBinding::instance().transition_lifecycle(LifecycleEvent::Terminating);
    }));

    window.on_close(Box::new(move || {
        tracing::info!("Canvas window closed");
        // On web, no explicit quit mechanism needed
    }));

    window.on_active_status_change(Box::new(|active| {
        let event = if active {
            LifecycleEvent::Activated
        } else {
            LifecycleEvent::Deactivated
        };
        AppBinding::instance().transition_lifecycle(event);
    }));

    // 7. Store window
    AppBinding::instance().set_window(window);

    tracing::info!("Web platform initialized with callbacks");

    // Run the event loop (takes ownership of the platform)
    platform.run(Box::new(|| {
        tracing::info!("Web platform ready");
    }));
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;
    use flui_view::{BuildContext, IntoView, View, ViewExt};

    use super::*;

    // TODO: Will be used in future integration tests for run_app_impl
    #[allow(dead_code)]
    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            TestView.boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateless(self)
        }
    }

    #[test]
    fn test_config_creation() {
        let config = AppConfig::new().with_title("Test").with_size(800, 600);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size.width, px(800.0));
    }
}
