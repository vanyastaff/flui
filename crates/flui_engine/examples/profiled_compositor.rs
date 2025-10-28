//! Example demonstrating profiled compositor
//!
//! This shows how to use ProfiledCompositor to track rendering performance.

use flui_engine::*;
use flui_types::{Size, Rect, Offset};

fn main() {
    // Create a profiled compositor
    let compositor = ProfiledCompositor::new();

    // Create a simple scene
    let mut scene = Scene::new(Size::new(800.0, 600.0));

    // Add some layers
    let mut picture = PictureLayer::new();
    picture.draw_rect(
        Rect::from_xywh(100.0, 100.0, 200.0, 150.0),
        Paint {
            color: [0.2, 0.6, 1.0, 1.0], // Blue
            ..Default::default()
        },
    );

    let transform = TransformLayer::translate(
        Box::new(picture),
        Offset::new(50.0, 50.0),
    );

    scene.add_layer(Box::new(transform));

    // Create a mock painter (we'll use egui)
    #[cfg(feature = "egui")]
    {
        use eframe::egui;

        eframe::run_native(
            "Profiled Compositor Demo",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
                ..Default::default()
            },
            Box::new(|_cc| {
                Ok(Box::new(ProfiledApp {
                    compositor,
                    scene,
                }))
            }),
        ).unwrap();
    }

    #[cfg(not(feature = "egui"))]
    {
        println!("This example requires the 'egui' feature.");
        println!("Run with: cargo run --example profiled_compositor -p flui_engine --features egui,devtools");
    }
}

#[cfg(feature = "egui")]
struct ProfiledApp {
    compositor: ProfiledCompositor,
    scene: Scene,
}

#[cfg(feature = "egui")]
impl eframe::App for ProfiledApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Get the egui painter from ui
            let egui_painter = ui.painter();
            let mut painter = backends::egui::EguiPainter::new(egui_painter);

            // Composite with profiling
            self.compositor.composite(&self.scene, &mut painter);

            // End frame and get stats
            if let Some(stats) = self.compositor.end_frame() {
                // Display performance info
                ui.label(format!("Frame time: {:.2}ms", stats.total_time_ms()));
                ui.label(format!("FPS: {:.1}", self.compositor.fps()));

                if self.compositor.is_janky() {
                    ui.colored_label(
                        egui::Color32::RED,
                        "⚠ JANK DETECTED",
                    );
                }

                if let Some(paint_phase) = stats.phase(FramePhase::Paint) {
                    ui.label(format!(
                        "Paint time: {:.2}ms",
                        paint_phase.duration_ms()
                    ));
                }
            }

            ui.separator();

            // Compositor stats
            let comp_stats = self.compositor.composition_stats();
            ui.label(format!("Layers painted: {}", comp_stats.layers_painted));
            ui.label(format!("Layers culled: {}", comp_stats.layers_culled));
        });

        // Request repaint for continuous animation
        ctx.request_repaint();
    }
}
