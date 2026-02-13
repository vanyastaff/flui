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
    use crate::embedder::DesktopEmbedder;
    use flui_platform::WindowOptions;
    use parking_lot::Mutex;
    use std::sync::Arc;

    tracing::info!("Starting desktop platform via flui-platform");

    let platform = flui_platform::current_platform().expect("Failed to initialize platform");

    // Clone the Arc for use inside the on_ready closure
    let platform_inner = Arc::clone(&platform);

    platform.run(Box::new(move || {
        // 1. Convert AppConfig to WindowOptions and open window
        let options: WindowOptions = (&config).into();
        let window = platform_inner
            .open_window(options)
            .expect("Failed to create window");

        // 2. Create embedder (GPU renderer + window)
        let embedder = pollster::block_on(async { DesktopEmbedder::new(window).await });
        let embedder = match embedder {
            Ok(emb) => emb,
            Err(e) => {
                tracing::error!("Failed to create embedder: {:?}", e);
                platform_inner.quit();
                return;
            }
        };

        // 3. Mount root widget
        let (width, height) = embedder.size();
        mount_root(&root, width as f32, height as f32);

        // 4. Wrap embedder in Arc<Mutex> for callback sharing
        let embedder = Arc::new(Mutex::new(embedder));

        // 5. Request initial redraw
        {
            let emb = embedder.lock();
            emb.request_redraw();
        }

        tracing::info!("Desktop embedder initialized, callbacks registered by platform");
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
