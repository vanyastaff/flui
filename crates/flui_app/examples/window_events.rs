//! Example demonstrating window event callbacks
//!
//! This example shows how to handle various system window events:
//! - Focus changes (window gain/lose focus)
//! - Minimization/restoration (window hidden/visible)
//! - DPI/scale changes (moving between monitors)
//! - Theme changes (dark/light mode)
//! - Window movement
//! - Window destruction
//!
//! Run with: cargo run --example window_events
//!
//! Try these interactions:
//! - Click on/off the window to see focus events
//! - Minimize/restore the window
//! - Move window between monitors with different DPI
//! - Change system theme (dark/light mode)
//! - Move the window around
//! - Close the window

use flui_app::*;
use flui_core::render::LeafRender;
use flui_core::view::{AnyView, BuildContext, IntoElement, LeafRenderBuilder};
use flui_types::BoxConstraints;

/// Simple app that demonstrates window event callbacks
#[derive(Debug, Clone)]
struct WindowEventsDemo;

/// Simple render object (just displays a colored rectangle)
#[derive(Debug)]
struct SimpleRender;

impl LeafRender for SimpleRender {
    type Metadata = ();

    fn layout(&mut self, constraints: BoxConstraints) -> flui_types::Size {
        // Fill available space
        constraints.biggest()
    }

    fn paint(&self, _offset: flui_types::Offset) -> flui_core::BoxedLayer {
        // Just return an empty container layer
        // In a real app, you'd render UI here
        Box::new(flui_engine::ContainerLayer::new())
    }
}

impl flui_core::view::View for WindowEventsDemo {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Simple render object wrapped in LeafRenderBuilder
        // The interesting part is the event callbacks, not the UI
        LeafRenderBuilder::new(SimpleRender)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to see event callbacks in action
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Window Events Demo ===");
    println!("Interact with the window to see various events!");
    println!("Check the console for event logs.\n");

    // Create event loop
    let event_loop = winit::event_loop::EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    // Create window
    let window_attributes = winit::window::Window::default_attributes()
        .with_title("Window Events Demo - Try minimize, focus, move, etc.")
        .with_inner_size(winit::dpi::LogicalSize::new(600.0, 400.0));

    let window = std::sync::Arc::new(
        event_loop
            .create_window(window_attributes)
            .expect("Failed to create window"),
    );

    // Create Flui app
    let root_view: Box<dyn AnyView> = Box::new(WindowEventsDemo);
    let mut flui_app = FluiApp::new(root_view, window.clone());

    // === Register event callbacks ===

    // 1. Focus events
    flui_app.event_callbacks_mut().on_focus(|focused| {
        if focused {
            println!("✓ Window GAINED focus - user is interacting");
            // You could:
            // - Resume animations
            // - Increase frame rate
            // - Enable real-time updates
        } else {
            println!("✗ Window LOST focus - user switched away");
            // You could:
            // - Pause animations
            // - Reduce frame rate
            // - Pause background tasks
        }
    });

    // 2. Minimization events
    flui_app.event_callbacks_mut().on_minimized(|minimized| {
        if minimized {
            println!("▼ Window MINIMIZED (occluded) - not visible");
            // You could:
            // - Stop rendering entirely
            // - Pause background work
            // - Reduce CPU/GPU usage to minimum
        } else {
            println!("▲ Window RESTORED (visible) - back on screen");
            // You could:
            // - Resume rendering
            // - Resume background tasks
            // - Request immediate redraw
        }
    });

    // 3. DPI/Scale changes
    flui_app
        .event_callbacks_mut()
        .on_scale_changed(|scale, (_width, _height)| {
            println!("⚡ DPI/Scale changed to {:.2}x", scale);
            // You could:
            // - Reload textures at new scale
            // - Adjust UI scaling
            // - Update font sizes
            // Note: Resize event will provide new dimensions
        });

    // 4. Theme changes
    flui_app.event_callbacks_mut().on_theme_changed(|theme| {
        println!("🎨 System theme changed to: {}", theme);
        // You could:
        // - Update UI colors to match theme
        // - Switch color palette
        // - Reload themed assets
    });

    // 5. Window movement
    flui_app.event_callbacks_mut().on_moved(|x, y| {
        println!("↔ Window moved to position ({}, {})", x, y);
        // You could:
        // - Save window position to settings
        // - Detect which monitor window is on
        // - Adjust behavior based on position
    });

    // 6. Window destruction
    flui_app.event_callbacks_mut().on_destroyed(|| {
        println!("💀 Window DESTROYED - final cleanup");
        // This is called right before window is destroyed
        // Different from app cleanup (which runs on close request)
    });

    // 7. App cleanup (runs on close request, before window destroyed)
    flui_app.set_on_cleanup(|| {
        println!("\n=== App Cleanup ===");
        println!("Application is shutting down gracefully");
        println!("This is where you would:");
        println!("  - Save application state");
        println!("  - Close database connections");
        println!("  - Stop background threads");
        println!("  - Clean up resources");
        println!("===================\n");
    });

    println!("App initialized! Window is ready.\n");

    // Create application state
    let mut app_state = AppState {
        flui_app: Some(flui_app),
        window: Some(window.clone()),
    };

    // Request initial redraw
    window.request_redraw();

    // Run event loop
    event_loop.run_app(&mut app_state)?;

    Ok(())
}

/// Application state for winit event loop
struct AppState {
    flui_app: Option<FluiApp>,
    window: Option<std::sync::Arc<winit::window::Window>>,
}

impl winit::application::ApplicationHandler for AppState {
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Already initialized in main
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        // Dispatch to FluiApp (which will call our callbacks!)
        if let Some(app) = &mut self.flui_app {
            app.handle_window_event(&event);
        }

        // Handle internal events
        match event {
            winit::event::WindowEvent::CloseRequested => {
                println!("\n>>> Close button clicked <<<");
                if let Some(app) = &mut self.flui_app {
                    app.cleanup();
                }
                event_loop.exit();
            }
            winit::event::WindowEvent::Resized(physical_size) => {
                if let Some(app) = &mut self.flui_app {
                    app.resize(physical_size.width, physical_size.height);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            winit::event::WindowEvent::RedrawRequested => {
                if let Some(app) = &mut self.flui_app {
                    let needs_redraw = app.update();
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

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        // Don't request redraw here - only redraw when needed
    }
}
