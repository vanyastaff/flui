//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use flui_view::{RootRenderView, StatelessView, View};

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
    // Use flui_log for cross-platform logging (desktop, Android, iOS, WASM)
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        "info,flui_app=debug,flui_view=debug,flui_rendering=debug,wgpu=warn".to_string()
    });

    flui_log::Logger::new()
        .with_filter(&filter)
        .with_level(flui_log::Level::DEBUG)
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
    mount_root(&root, phys_size.width.0 as f32, phys_size.height.0 as f32);

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

/// Mount the root widget tree.
///
/// Creates a `RootRenderView` wrapping the user's root widget,
/// creates the root element, and mounts it into AppBinding.
fn mount_root<V>(root: &V, width: f32, height: f32)
where
    V: View + StatelessView + Clone + Send + Sync + 'static,
{
    use flui_view::RootRenderElement;

    let binding = AppBinding::instance();

    // Wrap user widget in RootRenderView
    let root_view = RootRenderView::new(root.clone(), width, height);

    // Create the root element
    let mut root_element = root_view.create_element();

    // Set the PipelineOwner on RootRenderElement before mounting
    if let Some(root_render_element) = root_element
        .as_any_mut()
        .downcast_mut::<RootRenderElement<V>>()
    {
        root_render_element.set_pipeline_owner(binding.render_pipeline_arc());
    }

    // Mount the element (this creates RenderViewObject and inserts into
    // RenderTree). Acquire a transient ElementOwner split-borrow handle
    // from the WidgetsBinding's BuildOwner; the recursive mount path threads
    // the same handle through every descendant lifecycle call (plan §U8).
    {
        let widgets = binding.widgets();
        widgets.with_build_owner_mut(|build_owner| {
            root_element.mount(None, 0, &mut build_owner.element_owner_mut());
        });
    }

    // Verify mounting succeeded
    if let Some(root_render_element) = root_element.as_any().downcast_ref::<RootRenderElement<V>>()
    {
        if let Some(render_id) = root_render_element.render_id() {
            tracing::info!(
                "mount_root: RootRenderElement mounted with render_id={:?}",
                render_id
            );
        } else {
            tracing::error!("mount_root: RootRenderElement has no render_id after mount");
        }
    }

    // Register RenderView with RenderingFlutterBinding for hit testing
    {
        use std::sync::Arc;

        use flui_rendering::{binding::RendererBinding, view::RenderView};

        let renderer = binding.renderer();
        let view = Arc::new(parking_lot::RwLock::new(RenderView::new()));
        renderer.add_render_view(0, view);
        tracing::info!("RenderView registered for hit testing (view_id=0)");
    }

    // Store root element in AppBinding for rebuild support
    binding.set_root_element(root_element);
    tracing::info!("Root element mounted and stored in AppBinding");
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
    mount_root(&root, phys_size.width.0 as f32, phys_size.height.0 as f32);

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
    mount_root(&root, phys_size.width.0 as f32, phys_size.height.0 as f32);

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
    use flui_view::{BuildContext, View};

    use super::*;

    // TODO: Will be used in future integration tests for run_app_impl
    #[allow(dead_code)]
    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(TestView)
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
        assert_eq!(config.size.width, 800.0);
    }
}
