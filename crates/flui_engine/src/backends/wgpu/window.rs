//! WGPU window integration
//!
//! This module handles the window creation and event loop integration for the wgpu backend.

use crate::app::{AppLogic, WindowConfig};
use crate::backends::wgpu::event_translator;
use crate::backends::wgpu::{WgpuPainter, WgpuRenderer};
use flui_types::Offset;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    keyboard::ModifiersState,
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
        .with_inner_size(winit::dpi::PhysicalSize::new(config.width, config.height))
        .with_resizable(config.resizable);

    if config.maximized {
        window_attributes = window_attributes.with_maximized(true);
    }

    #[allow(deprecated)]
    let window = Arc::new(
        event_loop
            .create_window(window_attributes)
            .map_err(|e| format!("Failed to create window: {}", e))?,
    );

    // Create WGPU renderer
    let renderer = pollster::block_on(async { WgpuRenderer::new(Some(window.clone())).await })
        .map_err(|e| format!("Failed to create WGPU renderer: {}", e))?;

    let renderer = Arc::new(Mutex::new(renderer));
    let mut painter = WgpuPainter::new(renderer.clone());

    // Setup application
    logic.setup();

    // Timing
    let mut last_frame_time = Instant::now();

    // Track modifiers state for event translation
    let mut modifiers = ModifiersState::empty();

    // Track cursor position for events that don't include it
    let mut last_cursor_position = Offset::ZERO;

    // Run event loop
    #[allow(deprecated)]
    event_loop
        .run(move |event, target| {
            match event {
                Event::WindowEvent { event, .. } => {
                    // Translate and dispatch event to application logic
                    if let Some(mut flui_event) =
                        event_translator::translate_window_event(&event, &modifiers)
                    {
                        // Update cursor position for pointer events
                        if let flui_types::Event::Pointer(ref mut pointer_event) = flui_event {
                            match pointer_event {
                                flui_types::PointerEvent::Move(data) => {
                                    last_cursor_position = data.position;
                                }
                                flui_types::PointerEvent::Down(data)
                                | flui_types::PointerEvent::Up(data) => {
                                    // Use last cursor position for click events
                                    data.position = last_cursor_position;
                                    data.local_position = last_cursor_position;
                                }
                                _ => {}
                            }
                        }

                        // Update cursor position for scroll events
                        if let flui_types::Event::Scroll(ref mut scroll_data) = flui_event {
                            scroll_data.position = last_cursor_position;
                        }

                        // Dispatch to application logic
                        logic.on_event(&flui_event);
                    }

                    // Handle window-specific events
                    match event {
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
                        WindowEvent::ModifiersChanged(new_modifiers) => {
                            modifiers = new_modifiers.state();
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        })
        .map_err(|e| format!("Event loop error: {}", e))
}
