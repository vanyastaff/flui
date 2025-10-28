//! Example demonstrating PerformanceOverlay with on-screen FPS display
//!
//! This shows how to render a visual performance overlay with FPS,
//! frame time, and jank indicators.

use flui_engine::*;
use flui_types::{Size, Rect, Offset};

fn main() {
    // Create a profiled compositor
    let compositor = ProfiledCompositor::new();

    // Create performance overlay
    let overlay = PerformanceOverlay::new();

    // Create a more complex scene with multiple layers
    let mut scene = Scene::new(Size::new(800.0, 600.0));

    // Add multiple colored rectangles to have something to render
    for i in 0..5 {
        let mut picture = PictureLayer::new();
        let hue = i as f32 * 0.2;
        let color = hsv_to_rgb(hue, 0.7, 0.9);

        picture.draw_rect(
            Rect::from_xywh(50.0 + i as f32 * 60.0, 100.0, 50.0, 200.0),
            Paint {
                color,
                ..Default::default()
            },
        );

        let transform = TransformLayer::translate(
            Box::new(picture),
            Offset::new(i as f32 * 10.0, i as f32 * 5.0),
        );

        scene.add_layer(Box::new(transform));
    }

    // Create a mock painter (we'll use egui)
    #[cfg(feature = "egui")]
    {
        use eframe::egui;

        eframe::run_native(
            "Performance Overlay Demo",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
                ..Default::default()
            },
            Box::new(|_cc| {
                Ok(Box::new(OverlayApp {
                    compositor,
                    overlay,
                    scene,
                    frame_started: false,
                }))
            }),
        ).unwrap();
    }

    #[cfg(not(feature = "egui"))]
    {
        println!("This example requires the 'egui' feature.");
        println!("Run with: cargo run --example performance_overlay -p flui_engine --features egui,devtools");
    }
}

/// Convert HSV to RGB color
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [f32; 4] {
    let c = v * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 0.166 {
        (c, x, 0.0)
    } else if h < 0.333 {
        (x, c, 0.0)
    } else if h < 0.5 {
        (0.0, c, x)
    } else if h < 0.666 {
        (0.0, x, c)
    } else if h < 0.833 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    [r + m, g + m, b + m, 1.0]
}

#[cfg(feature = "egui")]
struct OverlayApp {
    compositor: ProfiledCompositor,
    overlay: PerformanceOverlay,
    scene: Scene,
    frame_started: bool,
}

#[cfg(feature = "egui")]
impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // End previous frame if it was started
        if self.frame_started {
            let _ = self.compositor.end_frame();
        }

        // Begin new frame
        self.compositor.begin_frame();
        self.frame_started = true;

        egui::CentralPanel::default().show(ctx, |ui| {
            // Get the egui painter from ui
            let egui_painter = ui.painter();
            let mut painter = backends::egui::EguiPainter::new(egui_painter);

            // Composite the scene
            self.compositor.composite(&self.scene, &mut painter);

            // Draw performance overlay ON TOP of the scene
            let viewport_size = Size::new(800.0, 600.0);
            let profiler = self.compositor.profiler();
            let profiler_guard = profiler.lock();
            self.overlay.render(&profiler_guard, &mut painter, viewport_size);
        });

        // Request repaint for continuous rendering
        ctx.request_repaint();
    }
}
