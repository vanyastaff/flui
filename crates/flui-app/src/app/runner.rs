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
        .with_level(flui_foundation::log::Level::DEBUG)
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
fn run_desktop<V>(root: V, config: AppConfig)
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
    use flui_view::WidgetsBinding;
    use parking_lot::Mutex;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting desktop platform via flui-platform");

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

    // 3. Mount root widget
    if let Err(e) = AppBinding::instance().attach_root_widget_with_size(
        &root,
        phys_size.width.0 as f32,
        phys_size.height.0 as f32,
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

    // Wire `on_build_scheduled` on the BuildOwner so that a dirty-element
    // registration (e.g. from setState inside an element build) also triggers
    // `handle_build_scheduled`.  We call it directly — it is already
    // lock-free (reads only the `debug_building_dirty_elements` atomic and
    // then takes `on_need_frame`'s leaf lock).
    {
        let widgets = AppBinding::instance().widgets();
        widgets.with_build_owner_mut(|build_owner| {
            build_owner.set_on_build_scheduled(|| {
                let binding = WidgetsBinding::instance();
                binding.handle_build_scheduled();
            });
        });
    }

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
    window.on_request_frame(Box::new(move || {
        let binding = AppBinding::instance();

        // On-demand rendering: skip frame if nothing changed
        if !binding.needs_redraw() && !binding.has_pending_work() {
            return;
        }

        let now = web_time::Instant::now();

        // Scheduler callbacks (animations)
        let scheduler = Scheduler::instance();
        let _frame_id = scheduler.handle_begin_frame(now);
        scheduler.handle_draw_frame();

        // Render frame via AppBinding
        let mut r = renderer_frame.lock();
        binding.render_frame(&mut r);
    }));

    // 7. Register resize callback -> renderer.resize()
    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let w = (size.width.0 * scale_factor) as u32;
        let h = (size.height.0 * scale_factor) as u32;
        renderer_resize.lock().resize(w, h);
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
}

/// Register the hit-test root [`RenderView`] with the [`RendererBinding`]
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

    // 3. Mount root widget (used when no plugin is active)
    if let Err(e) = AppBinding::instance().attach_root_widget_with_size(
        &root,
        phys_size.width.0 as f32,
        phys_size.height.0 as f32,
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
        if !binding.needs_redraw() && !binding.has_pending_work() {
            return;
        }

        let now = web_time::Instant::now();
        let scheduler = Scheduler::instance();
        let _frame_id = scheduler.handle_begin_frame(now);
        scheduler.handle_draw_frame();

        let mut r = renderer_frame.lock();
        binding.render_frame(&mut r);
    }));

    // 7. Register resize callback -> renderer.resize()
    let renderer_resize = Arc::clone(&renderer);
    window.on_resize(Box::new(move |size, scale_factor| {
        let w = (size.width.0 * scale_factor) as u32;
        let h = (size.height.0 * scale_factor) as u32;
        renderer_resize.lock().resize(w, h);
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
    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_platform::{
        WindowOptions,
        traits::{DispatchEventResult, LifecycleEvent, PlatformInput},
    };
    use flui_scheduler::Scheduler;

    use crate::embedder::PlatformWindowHandle;

    tracing::info!("Starting web platform via flui-platform");

    let platform = flui_platform::current_platform().expect("Failed to initialize web platform");

    // 1. Open window (creates canvas) before run() since run() takes ownership
    let options: WindowOptions = (&config).into();
    let window = platform
        .open_window(options)
        .expect("Failed to create canvas window");

    // 2. Create GPU renderer via wasm-bindgen-futures (async on web)
    let phys_size = window.physical_size();
    let renderer_window = window.as_ref() as *const dyn flui_platform::PlatformWindow;

    // SAFETY: On wasm32, everything is single-threaded, the pointer remains valid
    // within the spawn_local closure. We must use spawn_local because wgpu surface
    // creation is async on web (WebGPU adapter request).
    wasm_bindgen_futures::spawn_local(async move {
        let window_ref = unsafe { &*renderer_window };
        let handle = PlatformWindowHandle(window_ref);
        let renderer = Renderer::new(&handle).await;
        let mut renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("GPU init failed: {:?}", e);
                return;
            }
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);
        tracing::info!("WebGPU renderer initialized");
    });

    // 3. Mount root widget
    let phys_size = window.physical_size();
    if let Err(e) = AppBinding::instance().attach_root_widget_with_size(
        &root,
        phys_size.width.0 as f32,
        phys_size.height.0 as f32,
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
    window.on_request_frame(Box::new(move || {
        let binding = AppBinding::instance();

        if !binding.needs_redraw() && !binding.has_pending_work() {
            return;
        }

        let now = web_time::Instant::now();
        let scheduler = Scheduler::instance();
        let _frame_id = scheduler.handle_begin_frame(now);
        scheduler.handle_draw_frame();

        // Note: renderer is initialized async, frames without a renderer are no-ops
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
        fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
            Box::new(flui_view::StatelessElement::new(
                self,
                flui_view::element::StatelessBehavior,
            ))
        }
    }

    #[test]
    fn test_config_creation() {
        let config = AppConfig::new().with_title("Test").with_size(800, 600);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size.width, px(800.0));
    }
}
