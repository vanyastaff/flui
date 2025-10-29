//! Memory leak detection stress test
//!
//! This example intentionally leaks memory to demonstrate the leak detection
//! capabilities of the MemoryProfiler. Watch the memory graph grow and see
//! the "⚠ LEAK?" warning appear!

use flui_engine::*;
#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
use flui_engine::DevToolsLayout;
use flui_types::{Size, Rect, Offset};
use std::sync::{Arc, Mutex};

fn main() {
    #[cfg(not(all(feature = "egui", feature = "devtools", feature = "memory-profiler")))]
    {
        println!("This example requires 'egui', 'devtools', and 'memory-profiler' features.");
        println!("Run with: cargo run --example memory_leak_test -p flui_engine --features egui,devtools,memory-profiler");
        return;
    }

    #[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
    run_leak_test();
}

#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
fn run_leak_test() {
    use eframe::egui;

    // Create profilers
    let compositor = ProfiledCompositor::new();
    let memory_profiler = Arc::new(Mutex::new(flui_devtools::memory::MemoryProfiler::new()));

    // Create DevTools layout (compact preset)
    let devtools_layout = DevToolsLayout::compact();

    // Create a scene
    let mut scene = Scene::new(Size::new(800.0, 600.0));

    // Add some visual feedback
    for i in 0..5 {
        let mut picture = PictureLayer::new();
        picture.draw_rect(
            Rect::from_xywh(100.0 + i as f32 * 120.0, 200.0, 100.0, 200.0),
            Paint {
                color: [0.8, 0.3, 0.3, 0.9], // Red - warning color!
                ..Default::default()
            },
        );

        let transform = TransformLayer::translate(
            Box::new(picture),
            Offset::new(i as f32 * 5.0, 0.0),
        );

        scene.add_layer(Box::new(transform));
    }

    eframe::run_native(
        "Memory Leak Detection Test",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
            ..Default::default()
        },
        Box::new(|_cc| {
            Ok(Box::new(LeakTestApp {
                compositor,
                memory_profiler,
                devtools_layout,
                scene,
                frame_started: false,
                frame_count: 0,
                last_memory_snapshot: 0,
                leaked_memory: Vec::new(), // This will grow and never be freed!
                leak_enabled: true,
                leak_rate: 100, // KB per second
            }))
        }),
    ).unwrap();
}

#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
struct LeakTestApp {
    compositor: ProfiledCompositor,
    memory_profiler: Arc<Mutex<flui_devtools::memory::MemoryProfiler>>,
    devtools_layout: DevToolsLayout,
    scene: Scene,
    frame_started: bool,
    frame_count: u64,
    last_memory_snapshot: u64,
    leaked_memory: Vec<Vec<u8>>, // Intentional leak!
    leak_enabled: bool,
    leak_rate: usize, // KB per second
}

#[cfg(all(feature = "egui", feature = "devtools", feature = "memory-profiler"))]
impl eframe::App for LeakTestApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // End previous frame if it was started
        if self.frame_started {
            let _ = self.compositor.end_frame();
        }

        // Begin new frame
        self.compositor.begin_frame();
        self.frame_started = true;
        self.frame_count += 1;

        // Intentionally leak memory every 60 frames (~1 second at 60fps)
        if self.leak_enabled && self.frame_count % 60 == 0 {
            // Leak 100KB per second
            let chunk_size = self.leak_rate * 1024; // Convert KB to bytes
            let leaked_chunk = vec![0u8; chunk_size];
            self.leaked_memory.push(leaked_chunk);
        }

        // Take memory snapshot every 10 frames
        if self.frame_count - self.last_memory_snapshot >= 10 {
            self.memory_profiler.lock().unwrap().snapshot();
            self.last_memory_snapshot = self.frame_count;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Get the egui painter from ui
            let egui_painter = ui.painter();
            let mut painter = backends::egui::EguiPainter::new(egui_painter);

            // Composite the scene
            self.compositor.composite(&self.scene, &mut painter);

            // Render DevTools overlays
            let viewport_size = Size::new(800.0, 600.0);
            let profiler = self.compositor.profiler();
            let profiler_guard = profiler.lock();
            let memory_guard = self.memory_profiler.lock().unwrap();

            self.devtools_layout.render(
                &profiler_guard,
                Some(&memory_guard),
                &mut painter,
                viewport_size
            );

            // Display memory info and controls
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                let stats = memory_guard.current_stats();
                ui.label(format!("💾 Current Memory: {:.2} MB", stats.total_mb()));

                if let Some(peak) = memory_guard.peak_memory() {
                    ui.label(format!("📈 Peak: {:.2} MB", peak.total_mb()));
                }

                ui.label(format!("📊 Average: {:.2} MB", memory_guard.average_memory_mb()));

                // Calculate leaked amount
                let leaked_mb = (self.leaked_memory.len() * self.leak_rate) as f64 / 1024.0;
                ui.label(format!("💥 Intentionally Leaked: {:.2} MB ({} chunks)",
                    leaked_mb, self.leaked_memory.len()));

                ui.separator();

                // LEAK DETECTION WARNING
                if memory_guard.is_leaking() {
                    ui.colored_label(
                        egui::Color32::RED,
                        "⚠⚠⚠ MEMORY LEAK DETECTED! ⚠⚠⚠"
                    );
                    ui.label("The profiler detected increasing memory usage!");
                } else {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "Waiting for leak detection... (needs ~10 seconds)"
                    );
                }

                ui.separator();

                // DevTools Controls
                ui.heading("🛠 DevTools");

                // Layout preset selector
                ui.horizontal(|ui| {
                    ui.label("Layout:");
                    if ui.button("Compact").clicked() {
                        self.devtools_layout = DevToolsLayout::compact();
                    }
                    if ui.button("Detailed").clicked() {
                        self.devtools_layout = DevToolsLayout::detailed();
                    }
                    if ui.button("Bottom").clicked() {
                        self.devtools_layout = DevToolsLayout::bottom_bar();
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("Right").clicked() {
                        self.devtools_layout = DevToolsLayout::right_side();
                    }
                    if ui.button("Corners").clicked() {
                        self.devtools_layout = DevToolsLayout::corners();
                    }
                    if ui.button("Minimal").clicked() {
                        self.devtools_layout = DevToolsLayout::minimal();
                    }
                });

                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.devtools_layout.show_performance, "📊 FPS");
                    ui.checkbox(&mut self.devtools_layout.show_timeline, "📈 Timeline");
                    ui.checkbox(&mut self.devtools_layout.show_memory, "💾 Memory");
                });

                ui.horizontal(|ui| {
                    ui.label("BG:");
                    let mut opacity = self.devtools_layout.global_opacity;
                    if ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0).step_by(0.05)).changed() {
                        self.devtools_layout.set_opacity(opacity);
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Text:");
                    let mut text_opacity = self.devtools_layout.performance_overlay.text_opacity;
                    if ui.add(egui::Slider::new(&mut text_opacity, 0.0..=1.0).step_by(0.05)).changed() {
                        self.devtools_layout.set_text_opacity(text_opacity);
                    }
                });

                ui.separator();

                // Memory Leak Controls
                ui.heading("🧪 Memory Leak Test");
                ui.horizontal(|ui| {
                    if ui.button(if self.leak_enabled { "⏸ Stop & Free" } else { "▶ Start Leak" }).clicked() {
                        if self.leak_enabled {
                            // Stop leaking AND free memory
                            self.leaked_memory.clear();
                        }
                        self.leak_enabled = !self.leak_enabled;
                    }

                    if ui.button("🗑 Clear All & Reset").clicked() {
                        self.leaked_memory.clear();
                        self.memory_profiler.lock().unwrap().clear_history();
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Leak rate:");
                    ui.add(egui::Slider::new(&mut self.leak_rate, 10..=500).suffix(" KB/s"));
                });

                ui.separator();
                ui.label("💡 Tip: Watch the blue line grow, then press 'Stop & Free' to see it drop!");
            });
        });

        // Request repaint
        ctx.request_repaint();
    }
}
