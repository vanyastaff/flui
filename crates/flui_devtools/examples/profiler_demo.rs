//! Performance profiler demo
//!
//! Run with: cargo run --example profiler_demo

use flui_devtools::prelude::*;
use std::thread;
use std::time::Duration;

fn simulate_build_phase() {
    thread::sleep(Duration::from_millis(5));
}

fn simulate_layout_phase() {
    thread::sleep(Duration::from_millis(3));
}

fn simulate_paint_phase() {
    thread::sleep(Duration::from_millis(2));
}

fn main() {
    println!("🎯 FLUI DevTools - Performance Profiler Demo\n");
    
    let profiler = Profiler::new();
    
    // Simulate 10 frames
    for i in 0..10 {
        profiler.begin_frame();
        
        // Profile build phase
        {
            let _guard = profiler.profile_phase(FramePhase::Build);
            simulate_build_phase();
        }
        
        // Profile layout phase
        {
            let _guard = profiler.profile_phase(FramePhase::Layout);
            simulate_layout_phase();
        }
        
        // Profile paint phase
        {
            let _guard = profiler.profile_phase(FramePhase::Paint);
            simulate_paint_phase();
        }
        
        // Simulate jank on frame 5
        if i == 5 {
            thread::sleep(Duration::from_millis(20));
        }
        
        profiler.end_frame();
        
        // Print frame summary
        if let Some(stats) = profiler.frame_stats() {
            print!("Frame #{}: {:.2}ms ", stats.frame_number, stats.total_time_ms());

            if stats.is_jank {
                println!("⚠️  JANK DETECTED!");
            } else {
                println!("✓");
            }

            // Calculate percentages manually
            let total = stats.total_time_ms();
            for phase in &stats.phases {
                let percent = if total > 0.0 { (phase.duration_ms() / total) * 100.0 } else { 0.0 };
                let icon = match phase.phase {
                    FramePhase::Build => "🔨",
                    FramePhase::Layout => "📐",
                    FramePhase::Paint => "🎨",
                    FramePhase::Custom(_) => "⚙️",
                };
                println!("  {} {}: {:.2}ms ({:.1}%)",
                    icon,
                    phase.phase.name(),
                    phase.duration_ms(),
                    percent
                );
            }
            println!();
        }

        thread::sleep(Duration::from_millis(10));
    }

    // Performance summary
    println!("\n📊 Performance Summary:");
    println!("  Average FPS: {:.1}", profiler.average_fps());
    println!("  Jank percentage: {:.1}%", profiler.jank_percentage());

    // Calculate total phase times
    let history = profiler.frame_history();
    let mut total_build = 0.0;
    let mut total_layout = 0.0;
    let mut total_paint = 0.0;

    for stats in &history {
        for phase in &stats.phases {
            match phase.phase {
                FramePhase::Build => total_build += phase.duration_ms(),
                FramePhase::Layout => total_layout += phase.duration_ms(),
                FramePhase::Paint => total_paint += phase.duration_ms(),
                _ => {}
            }
        }
    }

    println!("\n⏱️  Total Phase Times:");
    println!("  Build:  {:.2}ms", total_build);
    println!("  Layout: {:.2}ms", total_layout);
    println!("  Paint:  {:.2}ms", total_paint);

    // Use built-in summary
    println!("\n");
    profiler.print_frame_summary();
}
