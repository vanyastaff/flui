//! Egui window integration
//!
//! This module handles the window creation and event loop integration for the egui backend.

use crate::app::{AppLogic, WindowConfig};
use crate::backends::egui::EguiPainter;
use std::time::Instant;

/// Run the application with egui backend
///
/// This function sets up the egui window, initializes the application logic,
/// and runs the main event loop.
///
/// # Arguments
/// * `logic` - The application logic to run
/// * `config` - Window configuration
///
/// # Returns
/// * `Ok(())` on successful shutdown
/// * `Err(String)` if the window fails to initialize or run
pub fn run<L: AppLogic>(mut logic: L, config: WindowConfig) -> Result<(), String> {
    // Setup application
    logic.setup();

    // Timing
    let mut last_frame_time = Instant::now();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([config.width as f32, config.height as f32])
            .with_title(&config.title)
            .with_resizable(config.resizable)
            .with_maximized(config.maximized),
        vsync: config.vsync,
        ..Default::default()
    };

    eframe::run_simple_native(
        &config.title,
        native_options,
        move |ctx, _frame| {
            // Calculate delta time
            let now = Instant::now();
            let delta_time = now.duration_since(last_frame_time).as_secs_f32();
            last_frame_time = now;

            // Update logic
            logic.update(delta_time);

            // Render
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(25, 25, 25)))
                .show(ctx, |ui| {
                    let painter = ui.painter();
                    let mut flui_painter = EguiPainter::new(painter);
                    logic.render(&mut flui_painter);
                });

            // Request continuous repainting
            ctx.request_repaint();
        },
    )
    .map_err(|e| format!("Egui error: {}", e))
}
