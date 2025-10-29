//! Example demonstrating memory profiling with visual graph
//!
//! This shows how to track memory usage over time and detect potential leaks.

use flui_engine::*;
#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
use flui_engine::DevToolsLayout;
use flui_types::{Size, Rect, Offset};
use std::sync::{Arc, Mutex};

fn main() {
    #[cfg(not(all(feature = "egui", feature = "devtools")))]
    {
        println!("This example requires both 'egui' and 'devtools' features.");
        println!("Run with: cargo run --example memory_profiling -p flui_engine --features egui,devtools");
        return;
    }

    #[cfg(all(feature = "egui", feature = "devtools"))]
    run_with_devtools();
}

#[cfg(all(feature = "egui", feature = "devtools"))]
fn run_with_devtools() {
    use eframe::egui;

    // Create profilers
    let compositor = ProfiledCompositor::new();
    let overlay = PerformanceOverlay::new();
    let timeline_graph = FrameTimelineGraph::new();

    // Create memory profiler (requires memory-profiler feature)
    #[cfg(feature = "memory-profiler")]
    let memory_profiler = Arc::new(Mutex::new(flui_devtools::memory::MemoryProfiler::new()));
    #[cfg(feature = "memory-profiler")]
    let memory_graph = MemoryGraph::new();

    // Create a scene with some content
    let mut scene = Scene::new(Size::new(800.0, 600.0));

    // Add some visual elements
    for i in 0..8 {
        let mut picture = PictureLayer::new();
        let hue = i as f32 * 0.125;
        let color = hsv_to_rgb(hue, 0.7, 0.9);

        picture.draw_rect(
            Rect::from_xywh(50.0 + i as f32 * 80.0, 150.0, 70.0, 250.0),
            Paint {
                color,
                ..Default::default()
            },
        );

        let transform = TransformLayer::translate(
            Box::new(picture),
            Offset::new(i as f32 * 5.0, i as f32 * 3.0),
        );

        scene.add_layer(Box::new(transform));
    }

    eframe::run_native(
        "Memory Profiling Demo",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
            ..Default::default()
        },
        Box::new(|_cc| {
            Ok(Box::new(MemoryProfilingApp {
                compositor,
                overlay,
                timeline_graph,
                #[cfg(feature = "memory-profiler")]
                memory_profiler,
                #[cfg(feature = "memory-profiler")]
                memory_graph,
                scene,
                frame_started: false,
                frame_count: 0,
                #[cfg(feature = "memory-profiler")]
                last_memory_snapshot: 0,
            }))
        }),
    ).unwrap();
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

#[cfg(all(feature = "egui", feature = "devtools"))]
struct MemoryProfilingApp {
    compositor: ProfiledCompositor,
    overlay: PerformanceOverlay,
    timeline_graph: FrameTimelineGraph,
    #[cfg(feature = "memory-profiler")]
    memory_profiler: Arc<Mutex<flui_devtools::memory::MemoryProfiler>>,
    #[cfg(feature = "memory-profiler")]
    memory_graph: MemoryGraph,
    scene: Scene,
    frame_started: bool,
    frame_count: u64,
    #[cfg(feature = "memory-profiler")]
    last_memory_snapshot: u64,
}

#[cfg(all(feature = "egui", feature = "devtools"))]
impl eframe::App for MemoryProfilingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // End previous frame if it was started
        if self.frame_started {
            let _ = self.compositor.end_frame();
        }

        // Begin new frame
        self.compositor.begin_frame();
        self.frame_started = true;
        self.frame_count += 1;

        // Take memory snapshot every 10 frames (about 6 times per second at 60fps)
        #[cfg(feature = "memory-profiler")]
        {
            if self.frame_count - self.last_memory_snapshot >= 10 {
                self.memory_profiler.lock().unwrap().snapshot();
                self.last_memory_snapshot = self.frame_count;
            }
        }

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

            // Draw frame timeline graph
            self.timeline_graph.render(&profiler_guard, &mut painter, viewport_size);

            // Draw memory graph (if feature enabled)
            #[cfg(feature = "memory-profiler")]
            {
                let memory_guard = self.memory_profiler.lock().unwrap();
                self.memory_graph.render(&memory_guard, &mut painter, viewport_size);

                // Display memory info in UI as well
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let stats = memory_guard.current_stats();
                    ui.label(format!("Memory Usage: {:.2} MB", stats.total_mb()));

                    if let Some(peak) = memory_guard.peak_memory() {
                        ui.label(format!("Peak: {:.2} MB", peak.total_mb()));
                    }

                    ui.label(format!("Average: {:.2} MB", memory_guard.average_memory_mb()));

                    if memory_guard.is_leaking() {
                        ui.colored_label(
                            egui::Color32::RED,
                            "⚠ WARNING: Potential memory leak detected!"
                        );
                    }
                });
            }

            #[cfg(not(feature = "memory-profiler"))]
            {
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "Memory profiling disabled. Enable with --features memory-profiler"
                    );
                });
            }
        });

        // Request repaint for continuous rendering
        ctx.request_repaint();
    }
}
