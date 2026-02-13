//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations via flui-platform.

use super::{AppBinding, AppConfig};
use flui_view::{RootRenderView, StatelessView, View};

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
        run_android(config);
    }

    #[cfg(target_os = "ios")]
    {
        run_ios(config);
    }

    #[cfg(target_arch = "wasm32")]
    {
        run_web(config);
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
    use crate::embedder::PlatformWindowHandle;
    use flui_engine::wgpu::Renderer;
    use flui_foundation::HasInstance;
    use flui_platform::traits::{DispatchEventResult, LifecycleEvent, PlatformInput};
    use flui_platform::WindowOptions;
    use flui_scheduler::Scheduler;
    use parking_lot::Mutex;
    use std::sync::Arc;

    tracing::info!("Starting desktop platform via flui-platform");

    let platform = flui_platform::current_platform().expect("Failed to initialize platform");
    let platform_inner = Arc::clone(&platform);

    platform.run(Box::new(move || {
        // 1. Open window
        let options: WindowOptions = (&config).into();
        let window = platform_inner
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
                platform_inner.quit();
                return;
            }
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        // 3. Mount root widget
        mount_root(&root, phys_size.width.0 as f32, phys_size.height.0 as f32);

        // 4. Wrap renderer for callback sharing
        let renderer = Arc::new(Mutex::new(renderer));

        // 5. Register input callback → AppBinding::handle_input()
        window.on_input(Box::new(move |input: PlatformInput| {
            AppBinding::instance().handle_input(input);
            DispatchEventResult {
                propagate: false,
                default_prevented: true,
            }
        }));

        // 6. Register frame callback → scheduler + AppBinding::render_frame()
        let renderer_frame = Arc::clone(&renderer);
        window.on_request_frame(Box::new(move || {
            let binding = AppBinding::instance();

            // On-demand rendering: skip frame if nothing changed
            if !binding.needs_redraw() && !binding.has_pending_work() {
                return;
            }

            let now = std::time::Instant::now();

            // Scheduler callbacks (animations)
            let scheduler = Scheduler::instance();
            let _frame_id = scheduler.handle_begin_frame(now);
            scheduler.handle_draw_frame();

            // Render frame via AppBinding
            let mut r = renderer_frame.lock();
            binding.render_frame(&mut r);
        }));

        // 7. Register resize callback → renderer.resize()
        let renderer_resize = Arc::clone(&renderer);
        window.on_resize(Box::new(move |size, scale_factor| {
            let w = (size.width.0 * scale_factor) as u32;
            let h = (size.height.0 * scale_factor) as u32;
            renderer_resize.lock().resize(w, h);
            AppBinding::instance().request_redraw();
        }));

        // 8. Lifecycle callbacks

        // Platform quit → transition to Terminating
        platform_inner.on_quit(Box::new(|| {
            tracing::info!("Platform quit");
            AppBinding::instance().transition_lifecycle(LifecycleEvent::Terminating);
        }));

        // Window close → request platform quit
        let platform_for_close = Arc::clone(&platform_inner);
        window.on_close(Box::new(move || {
            tracing::info!("Window closed");
            platform_for_close.quit();
        }));

        // Window should-close → allow by default
        window.on_should_close(Box::new(|| {
            tracing::debug!("Window close requested, allowing");
            true
        }));

        // Window active status → lifecycle Activated/Deactivated
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

    // Mount the element (this creates RenderViewObject and inserts into RenderTree)
    root_element.mount(None, 0);

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

        use flui_rendering::binding::RendererBinding;
        use flui_rendering::view::RenderView;

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

#[cfg(target_os = "android")]
fn run_android(_config: AppConfig) {
    tracing::info!("Android platform - not yet implemented");
    // TODO: Implement android-activity integration
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
fn run_web(_config: AppConfig) {
    tracing::info!("Web platform - not yet implemented");
    // TODO: Implement wasm-bindgen integration
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_view::{BuildContext, View};

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
