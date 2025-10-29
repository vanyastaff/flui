//! Example demonstrating the unified DevTools overlay
//!
//! Shows all performance metrics in one compact panel, like professional tools.

use flui_engine::*;
#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
use flui_engine::{UnifiedDevToolsOverlay, OverlayCorner};
use flui_types::{Size, Rect};
use std::sync::{Arc, Mutex};

fn main() {
    #[cfg(not(all(feature = "egui", feature = "devtools", feature = "memory-profiler")))]
    {
        eprintln!("This example requires the 'egui', 'devtools', and 'memory-profiler' features");
        eprintln!("Run with: cargo run --example unified_devtools --features egui,devtools,memory-profiler");
        return;
    }

    #[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
    run_unified_devtools();
}

#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
fn run_unified_devtools() {
    use eframe::egui;

    // Create profilers
    let compositor = ProfiledCompositor::new();
    let memory_profiler = Arc::new(Mutex::new(flui_devtools::memory::MemoryProfiler::new()));

    // Create unified overlay
    let unified_overlay = UnifiedDevToolsOverlay::new();

    // Create a scene
    let mut scene = Scene::new(Size::new(800.0, 600.0));

    // Add some visual content
    for i in 0..5 {
        let mut picture = PictureLayer::new();
        picture.draw_rect(
            Rect::from_xywh(100.0 + i as f32 * 140.0, 250.0, 100.0, 150.0),
            Paint {
                color: [0.2 + i as f32 * 0.15, 0.4, 0.7, 0.8],
                ..Default::default()
            },
        );
        scene.add_layer(Box::new(picture));
    }

    eframe::run_native(
        "Unified DevTools Demo",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
            ..Default::default()
        },
        Box::new(move |_cc| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Box::new(UnifiedDevToolsApp {
                compositor,
                memory_profiler,
                unified_overlay,
                scene,
                frame_started: false,
            }))
        }),
    ).unwrap();
}

#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
struct UnifiedDevToolsApp {
    compositor: ProfiledCompositor,
    memory_profiler: Arc<Mutex<flui_devtools::memory::MemoryProfiler>>,
    unified_overlay: UnifiedDevToolsOverlay,
    scene: Scene,
    frame_started: bool,
}

#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
impl eframe::App for UnifiedDevToolsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // End previous frame and start new one
        if self.frame_started {
            self.compositor.end_frame();
        }
        self.compositor.begin_frame();
        self.frame_started = true;

        // Memory profiler updates automatically

        // Request continuous repaint for smooth updates
        ctx.request_repaint();

        egui::CentralPanel::default().show(ctx, |ui| {
            // Get the egui painter from ui
            let egui_painter = ui.painter();
            let mut painter = backends::egui::EguiPainter::new(egui_painter);

            // Composite the scene
            self.compositor.composite(&self.scene, &mut painter);

            // Render unified DevTools overlay
            let viewport_size = Size::new(800.0, 600.0);
            let profiler = self.compositor.profiler();
            let profiler_guard = profiler.lock();
            let memory_guard = self.memory_profiler.lock().unwrap();

            self.unified_overlay.render(
                &profiler_guard,
                Some(&memory_guard),
                &mut painter,
                viewport_size
            );

            // Display UI controls
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.heading("🎯 Unified DevTools Overlay");
                ui.separator();

                // Corner selection
                ui.label("📍 Угол:");
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.unified_overlay.corner == OverlayCorner::TopLeft, "↖ Верх-Лево").clicked() {
                        self.unified_overlay.set_corner(OverlayCorner::TopLeft);
                    }
                    if ui.selectable_label(self.unified_overlay.corner == OverlayCorner::TopRight, "↗ Верх-Право").clicked() {
                        self.unified_overlay.set_corner(OverlayCorner::TopRight);
                    }
                });
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.unified_overlay.corner == OverlayCorner::BottomLeft, "↙ Низ-Лево").clicked() {
                        self.unified_overlay.set_corner(OverlayCorner::BottomLeft);
                    }
                    if ui.selectable_label(self.unified_overlay.corner == OverlayCorner::BottomRight, "↘ Низ-Право").clicked() {
                        self.unified_overlay.set_corner(OverlayCorner::BottomRight);
                    }
                });

                ui.separator();

                // Toggle sections
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.unified_overlay.show_performance, "⚡ Performance");
                    ui.checkbox(&mut self.unified_overlay.show_timeline, "📊 Timeline");
                    ui.checkbox(&mut self.unified_overlay.show_memory, "💾 Memory");
                });

                // Opacity controls
                ui.horizontal(|ui| {
                    ui.label("BG:");
                    ui.add(egui::Slider::new(&mut self.unified_overlay.bg_opacity, 0.0..=1.0).step_by(0.05));
                });

                ui.horizontal(|ui| {
                    ui.label("Text:");
                    ui.add(egui::Slider::new(&mut self.unified_overlay.text_opacity, 0.0..=1.0).step_by(0.05));
                });

                // Width control
                ui.horizontal(|ui| {
                    ui.label("Width:");
                    ui.add(egui::Slider::new(&mut self.unified_overlay.width, 200.0..=500.0).suffix(" px"));
                });

                ui.separator();
                ui.label("Все метрики в одном компактном блоке!");
            });
        });
    }
}
