//! WGPU window integration
//!
//! This module handles the window creation and event loop integration for the wgpu backend.

use crate::app::{AppLogic, WindowConfig};
use crate::backends::wgpu::{WgpuRenderer, WgpuPainter};
use std::sync::Arc;
use parking_lot::Mutex;
use std::time::Instant;
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
};

/// Run the application with wgpu backend
///
/// This function sets up the wgpu window, initializes the application logic,
/// and runs the main event loop with GPU-accelerated rendering.
///
/// # Arguments
/// * `logic` - The application logic to run
/// * `config` - Window configuration
///
/// # Returns
/// * `Ok(())` on successful shutdown
/// * `Err(String)` if the window fails to initialize or run
pub fn run<L: AppLogic>(mut logic: L, config: WindowConfig) -> Result<(), String> {
    // Create event loop and window
    let event_loop = EventLoop::new().map_err(|e| format!("Failed to create event loop: {}", e))?;

    let mut window_attributes = winit::window::Window::default_attributes()
        .with_title(&config.title)
        .with_inner_size(winit::dpi::PhysicalSize::new(
            config.width,
            config.height,
        ))
        .with_resizable(config.resizable);

    if config.maximized {
        window_attributes = window_attributes.with_maximized(true);
    }

    let window = Arc::new(
        event_loop.create_window(window_attributes)
            .map_err(|e| format!("Failed to create window: {}", e))?
    );

    // Create WGPU renderer
    let renderer = pollster::block_on(async {
        WgpuRenderer::new(Some(window.clone())).await
    }).map_err(|e| format!("Failed to create WGPU renderer: {}", e))?;

    let renderer = Arc::new(Mutex::new(renderer));
    let mut painter = WgpuPainter::new(renderer.clone());

    // Setup application
    logic.setup();

    // Timing
    let mut last_frame_time = Instant::now();

    // Run event loop
    event_loop.run(move |event, target| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    target.exit();
                }
                WindowEvent::Resized(size) => {
                    renderer.lock().resize(size.width, size.height);
                    window.request_redraw();
                }
                WindowEvent::RedrawRequested => {
                    // Calculate delta time
                    let now = Instant::now();
                    let delta_time = now.duration_since(last_frame_time).as_secs_f32();
                    last_frame_time = now;

                    // Update logic
                    logic.update(delta_time);

                    // Render
                    painter.begin_frame();
                    logic.render(&mut painter);

                    if let Err(e) = painter.end_frame() {
                        eprintln!("Render error: {:?}", e);
                    }

                    window.request_redraw();
                }
                WindowEvent::KeyboardInput { event, .. } => {
                    if event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) {
                        target.exit();
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }).map_err(|e| format!("Event loop error: {}", e))
}
