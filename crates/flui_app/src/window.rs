//! Window management and application entry point
//!
//! This module provides window configuration and the main entry point
//! for running Flui applications using winit + wgpu.

use crate::app::FluiApp;
use flui_core::view::AnyView;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

/// Run a Flui application
///
/// This is the main entry point for running a Flui app. It creates a window
/// using winit and runs the app's event loop with wgpu rendering.
///
/// # Parameters
///
/// - `root_view`: The root view of your application (type-erased via `Box<dyn AnyView>`)
///
/// # Example
///
/// ```rust,ignore
/// use flui_app::*;
/// use flui_core::view::View;
///
/// #[derive(Debug)]
/// struct MyApp;
///
/// impl View for MyApp {
///     fn build(self, ctx: &BuildContext) -> impl IntoElement {
///         // Build your UI here
///         todo!()
///     }
/// }
///
/// fn main() {
///     run_app(Box::new(MyApp)).unwrap();
/// }
/// ```
pub fn run_app(root_view: Box<dyn AnyView>) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize hierarchical logging for development
    flui_core::logging::init_logging(
        flui_core::logging::LogConfig::new(flui_core::logging::LogMode::Development)
    );

    // Create event loop
    let event_loop = EventLoop::new()?;
    // Use Wait mode instead of Poll for better performance and power efficiency
    // Will only wake up when there are events or redraw is requested
    event_loop.set_control_flow(ControlFlow::Wait);

    // Create application state
    let mut app_state = AppState {
        root_view: Some(root_view),
        window: None,
        flui_app: None,
        on_cleanup: None,
    };

    // Run event loop
    event_loop.run_app(&mut app_state)?;

    Ok(())
}

/// Application state for winit event loop
struct AppState {
    root_view: Option<Box<dyn AnyView>>,
    window: Option<Arc<Window>>,
    flui_app: Option<FluiApp>,
    on_cleanup: Option<Box<dyn FnOnce() + Send>>,
}

impl ApplicationHandler for AppState {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            // Create window
            let window_attributes = Window::default_attributes()
                .with_title("Flui App")
                .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));

            let window = Arc::new(
                event_loop
                    .create_window(window_attributes)
                    .expect("Failed to create window"),
            );

            // Initialize Flui app with wgpu
            let root_view = self.root_view.take().expect("Root view already taken");
            let mut flui_app = FluiApp::new(root_view, Arc::clone(&window));

            // Set cleanup callback if provided
            if let Some(cleanup) = self.on_cleanup.take() {
                flui_app.set_on_cleanup(cleanup);
            }

            self.window = Some(Arc::clone(&window));
            self.flui_app = Some(flui_app);

            tracing::info!("Window and Flui app initialized");

            // Request initial redraw to render the first frame
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Dispatch event to user callbacks first
        if let Some(app) = &mut self.flui_app {
            app.handle_window_event(&event);
        }

        // Then handle internal events
        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("Close requested - performing graceful shutdown");

                // Perform cleanup before exiting
                if let Some(app) = &mut self.flui_app {
                    app.cleanup();
                }

                tracing::info!("Cleanup complete, exiting event loop");
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(app) = &mut self.flui_app {
                    app.resize(physical_size.width, physical_size.height);
                    // Request redraw after resize (layout will change)
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(app) = &mut self.flui_app {
                    let needs_redraw = app.update();

                    // Only request another redraw if there's pending work
                    // This prevents infinite redraw loop and saves CPU/GPU
                    if needs_redraw {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Don't request redraw here - only redraw when actually needed
        // This prevents CPU/GPU waste and improves power efficiency
        // Redraw will be triggered by:
        // 1. Window resize
        // 2. User input events (when implemented)
        // 3. Timer/animation events (when implemented)
        // 4. State changes via signals (when they request rebuild)
    }
}
