//! Profiled Rendering Example
//!
//! Demonstrates integration with flui_devtools for performance profiling.
//! Shows FPS, frame time, and jank detection during rendering.

use flui_engine::{ProfiledCompositor, Scene, Paint, PictureLayer, PerformanceOverlay};
use flui_types::{Size, Rect};
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== Profiled Rendering Demo ===\n");
    println!("This example demonstrates devtools integration with frame profiling.\n");

    // Create profiled compositor
    let mut compositor = ProfiledCompositor::new();
    println!("✓ ProfiledCompositor created");

    // Create a mock painter (in a real app, this would be EguiPainter or WgpuPainter)
    struct MockPainter;
    impl flui_engine::Painter for MockPainter {
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _: flui_types::Offset) {}
        fn rotate(&mut self, _: f32) {}
        fn scale(&mut self, _: f32, _: f32) {}
        fn set_opacity(&mut self, _: f32) {}
        fn clip_rect(&mut self, _: Rect) {}
        fn clip_rrect(&mut self, _: flui_engine::RRect) {}
        fn draw_rect(&mut self, _: Rect, _: Paint) {
            // Simulate some rendering work
            thread::sleep(Duration::from_micros(100));
        }
        fn draw_rrect(&mut self, _: flui_engine::RRect, _: Paint) {}
        fn draw_circle(&mut self, _: flui_types::Offset, _: f32, _: Paint) {}
        fn draw_line(&mut self, _: flui_types::Offset, _: flui_types::Offset, _: Paint) {}
    }

    let mut painter = MockPainter;

    // Simulate 60 frames
    println!("\n📊 Rendering 60 frames with profiling...\n");

    for frame_num in 0..60 {
        // Begin frame (would typically be done by the application)
        {
            let mut profiler = compositor.profiler().lock();
            profiler.begin_frame();
        }

        // Create a scene with some layers
        let mut scene = Scene::new(Size::new(800.0, 600.0));

        // Add some content layers
        for i in 0..5 {
            let mut picture = PictureLayer::new();
            let x = (i as f32) * 50.0;
            let rect = Rect::from_xywh(x, 100.0, 40.0, 40.0);
            picture.draw_rect(rect, Paint {
                color: [1.0, 0.0, 0.0, 1.0],
                ..Default::default()
            });
            scene.add_layer(Box::new(picture));
        }

        // Composite with profiling
        compositor.composite(&scene, &mut painter);

        // End frame and get stats
        let stats = compositor.end_frame();

        // Print stats every 10 frames
        if frame_num % 10 == 0 {
            println!("Frame {frame_num}:");
            println!("  Total time: {:.2}ms", stats.total_time_ms());
            println!("  Paint time: {:.2}ms", stats.phase_time_ms(flui_engine::FramePhase::Paint));
            println!("  FPS: {:.1}", compositor.fps());
            println!("  Janky: {}", compositor.is_janky());
            println!("  Layers painted: {}", compositor.composition_stats().layers_painted);
            println!();
        }

        // Simulate some time between frames
        thread::sleep(Duration::from_millis(16)); // ~60fps target
    }

    println!("=== Final Statistics ===");
    let final_stats = compositor.frame_stats();
    println!("Average FPS: {:.1}", compositor.fps());
    println!("Last frame time: {:.2}ms", final_stats.total_time_ms());
    println!("Paint phase: {:.2}ms", final_stats.phase_time_ms(flui_engine::FramePhase::Paint));

    let comp_stats = compositor.composition_stats();
    println!("\nComposition Stats:");
    println!("  Layers painted: {}", comp_stats.layers_painted);
    println!("  Composition time: {:.2}ms", comp_stats.composition_time.as_secs_f64() * 1000.0);
    println!("  Frame number: {}", comp_stats.frame_number);

    println!("\n✓ Profiling demo complete!");
    println!("\nIn a real application, you would:");
    println!("  1. Use ProfiledCompositor instead of regular Compositor");
    println!("  2. Call profiler.begin_frame() at frame start");
    println!("  3. Use profiler.profile_phase() for Build/Layout phases");
    println!("  4. Compositor.composite() automatically profiles Paint phase");
    println!("  5. Call compositor.end_frame() to get frame statistics");
    println!("  6. Optionally render PerformanceOverlay for visual feedback");
}
