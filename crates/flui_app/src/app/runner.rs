//! Application runner - entry points for running FLUI apps.
//!
//! This module provides platform-agnostic entry points that delegate
//! to platform-specific implementations.

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
// Desktop Implementation (Windows, macOS, Linux)
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
    use crate::embedder::DesktopEmbedder;
    use winit::{
        application::ApplicationHandler,
        event::WindowEvent,
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        window::WindowId,
    };

    tracing::info!("Starting desktop platform with winit event loop");

    /// Desktop application handler
    struct DesktopApp<V: View + Clone + Send + Sync + 'static> {
        #[allow(dead_code)]
        config: AppConfig,
        /// The user's root widget
        root_widget: V,
        embedder: Option<DesktopEmbedder>,
    }

    impl<V: View + Clone + Send + Sync + 'static> DesktopApp<V> {
        fn new(root_widget: V, config: AppConfig) -> Self {
            Self {
                config,
                root_widget,
                embedder: None,
            }
        }

        /// Mount the root element with the RootRenderView wrapper
        fn mount_root(&mut self, width: f32, height: f32) {
            use flui_view::RootRenderElement;

            // Get AppBinding's PipelineOwner (this is the one used by draw_frame())
            let binding = AppBinding::instance();

            // Wrap user widget in RootRenderView
            let root_view = RootRenderView::new(self.root_widget.clone(), width, height);

            // Create the root element
            let mut root_element = root_view.create_element();

            // Set the PipelineOwner on RootRenderElement before mounting
            // so that mount() can insert RenderView into RenderTree
            if let Some(root_render_element) = root_element
                .as_any_mut()
                .downcast_mut::<RootRenderElement<V>>()
            {
                root_render_element.set_pipeline_owner(binding.render_pipeline_arc());
            }

            // Mount the element (this creates RenderViewObject and inserts into RenderTree)
            root_element.mount(None, 0);

            // Verify mounting succeeded
            if let Some(root_render_element) =
                root_element.as_any().downcast_ref::<RootRenderElement<V>>()
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

            // Store root element in AppBinding for rebuild support
            binding.set_root_element(root_element);
            tracing::info!("Root element mounted and stored in AppBinding");
        }
    }

    impl<V: View + Clone + Send + Sync + 'static> ApplicationHandler for DesktopApp<V> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.embedder.is_some() {
                return;
            }

            tracing::info!("Creating window and GPU renderer");

            // Create embedder with window (no longer needs pipeline_owner - uses AppBinding)
            let embedder = pollster::block_on(async { DesktopEmbedder::new(event_loop).await });

            match embedder {
                Ok(emb) => {
                    // Get window size for RootRenderView
                    let (width, height) = emb.size();
                    let (width, height) = (width as f32, height as f32);

                    // Mount the root element with RootRenderView wrapper
                    self.mount_root(width, height);

                    // Request initial redraw
                    emb.request_redraw();
                    self.embedder = Some(emb);
                    tracing::info!("Desktop embedder initialized successfully");
                }
                Err(e) => {
                    tracing::error!("Failed to create embedder: {:?}", e);
                    event_loop.exit();
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            let Some(embedder) = &mut self.embedder else {
                return;
            };

            // Handle close request
            if matches!(event, WindowEvent::CloseRequested) {
                tracing::info!("Close requested, exiting...");
                event_loop.exit();
                return;
            }

            // Handle redraw
            if matches!(event, WindowEvent::RedrawRequested) {
                // Render frame (AppBinding handles all framework logic)
                embedder.render_frame();

                // On-demand rendering: only request next frame if there's pending work
                // (animations, state changes, etc.)
                if embedder.needs_redraw() {
                    embedder.request_redraw();
                }
                return;
            }

            // Delegate other events to embedder
            embedder.handle_window_event(event, event_loop);
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            if let Some(embedder) = &self.embedder {
                if embedder.needs_redraw() {
                    embedder.request_redraw();
                }
            }
        }
    }

    // Create and run event loop
    // Use Wait for on-demand rendering (Flutter-style) instead of Poll (game-style)
    // Frames are only rendered when:
    // - Widget state changes (mark_needs_build/paint/layout)
    // - Animations are running (scheduler frame callbacks)
    // - Window resize/expose events
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = DesktopApp::new(root, config);
    event_loop.run_app(&mut app).expect("Event loop failed");
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

    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            Box::new(TestView)
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn flui_view::ElementBase> {
            Box::new(flui_view::StatelessElement::new(self))
        }
    }

    #[test]
    fn test_config_creation() {
        let config = AppConfig::new().with_title("Test").with_size(800, 600);

        assert_eq!(config.title, "Test");
        assert_eq!(config.size.width, 800.0);
    }
}
