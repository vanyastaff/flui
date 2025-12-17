//! Frame Budget Example
//!
//! This example demonstrates frame budget management for maintaining
//! smooth frame rates and detecting janky frames.
//!
//! Run with: `cargo run --example frame_budget -p flui-scheduler`

use flui_scheduler::{
    duration::{FrameDuration, Milliseconds},
    FrameBudget, FrameBudgetBuilder,
};

fn main() {
    println!("=== FLUI Frame Budget Example ===\n");

    // 1. Basic Budget Creation
    println!("--- Basic Budget Creation ---\n");
    demo_basic_budget();

    // 2. Phase Timing
    println!("\n--- Phase Timing ---\n");
    demo_phase_timing();

    // 3. Jank Detection
    println!("\n--- Jank Detection ---\n");
    demo_jank_detection();

    // 4. Budget Builder
    println!("\n--- Budget Builder ---\n");
    demo_budget_builder();

    println!("\n=== Example Complete ===");
}

fn demo_basic_budget() {
    // Create a budget targeting 60 FPS
    let budget = FrameBudget::new(60);

    println!("Frame Budget for 60 FPS:");
    println!("  Target duration: {:.2} ms", budget.target_duration_ms());
    println!("  Target FPS: {}", budget.target_fps());

    // Create budgets for different frame rates
    let budget_30 = FrameBudget::new(30);
    let budget_120 = FrameBudget::new(120);

    println!("\nComparison of frame budgets:");
    println!(
        "  30 FPS: {:.2} ms per frame",
        budget_30.target_duration_ms()
    );
    println!("  60 FPS: {:.2} ms per frame", budget.target_duration_ms());
    println!(
        "  120 FPS: {:.2} ms per frame",
        budget_120.target_duration_ms()
    );
}

fn demo_phase_timing() {
    let mut budget = FrameBudget::new(60);

    println!("Simulating frame phases:\n");

    // Simulate build phase
    budget.record_build_duration(Milliseconds::new(2.5));
    let build_stats = budget.build_stats();
    println!("Build phase:");
    println!("  Duration: {:.2} ms", build_stats.duration.value());
    println!("  Budget used: {:.1}%", build_stats.budget_percent.value());

    // Simulate layout phase
    budget.record_layout_duration(Milliseconds::new(4.0));
    let layout_stats = budget.layout_stats();
    println!("\nLayout phase:");
    println!("  Duration: {:.2} ms", layout_stats.duration.value());
    println!("  Budget used: {:.1}%", layout_stats.budget_percent.value());

    // Simulate paint phase
    budget.record_paint_duration(Milliseconds::new(3.5));
    let paint_stats = budget.paint_stats();
    println!("\nPaint phase:");
    println!("  Duration: {:.2} ms", paint_stats.duration.value());
    println!("  Budget used: {:.1}%", paint_stats.budget_percent.value());

    // Simulate composite phase
    budget.record_composite_duration(Milliseconds::new(1.0));
    let composite_stats = budget.composite_stats();
    println!("\nComposite phase:");
    println!("  Duration: {:.2} ms", composite_stats.duration.value());
    println!(
        "  Budget used: {:.1}%",
        composite_stats.budget_percent.value()
    );

    // Get all phase stats
    let all_stats = budget.all_phase_stats();
    println!("\n--- Total ---");
    println!(
        "  Total duration: {:.2} ms",
        all_stats.total_duration().value()
    );
    println!(
        "  Total budget used: {:.1}%",
        all_stats.total_budget_percent().value()
    );
}

fn demo_jank_detection() {
    let mut budget = FrameBudget::new(60);

    println!("Simulating frames with varying durations:\n");

    // Simulate several frames
    let frame_times = [10.0, 12.0, 8.0, 25.0, 15.0, 35.0, 11.0, 9.0, 20.0, 14.0];

    for (i, &time) in frame_times.iter().enumerate() {
        budget.record_frame_duration(Milliseconds::new(time));

        let is_janky = budget.is_janky();
        let status = if is_janky { "JANKY!" } else { "OK" };

        println!("  Frame {}: {:.1} ms - {}", i + 1, time, status);
    }

    println!("\n--- Statistics ---");
    println!("  Total frames recorded: {}", frame_times.len());
    println!("  Janky frames: {}", budget.jank_count());
    println!(
        "  Jank percentage: {:.1}%",
        budget.jank_percentage().value()
    );
    println!("  Average frame time: {:.2} ms", budget.avg_frame_time_ms());
    println!("  Average FPS: {:.1}", budget.avg_fps());
    println!("  Frame time variance: {:.2}", budget.frame_time_variance());
}

fn demo_budget_builder() {
    println!("Using FrameBudgetBuilder:\n");

    // Build a custom budget
    let budget = FrameBudgetBuilder::new().target_fps(90).build();

    println!("Custom 90 FPS budget:");
    println!("  Target FPS: {}", budget.target_fps());
    println!("  Target duration: {:.2} ms", budget.target_duration_ms());

    // Build with frame duration directly
    let budget_custom = FrameBudgetBuilder::new()
        .frame_duration(FrameDuration::from_fps(144))
        .build();

    println!("\nCustom 144 FPS budget:");
    println!("  Target FPS: {}", budget_custom.target_fps());
    println!(
        "  Target duration: {:.2} ms",
        budget_custom.target_duration_ms()
    );

    // Demonstrate FrameDuration utilities
    println!("\n--- FrameDuration Utilities ---");

    let duration = FrameDuration::from_fps(60);
    let elapsed = Milliseconds::new(10.0);

    println!("\nFor 60 FPS target with 10ms elapsed:");
    println!("  Is over budget: {}", duration.is_over_budget(elapsed));
    println!("  Remaining: {:.2} ms", duration.remaining(elapsed).value());
    println!(
        "  Utilization: {:.1}%",
        duration.utilization(elapsed) * 100.0
    );
    println!(
        "  Is deadline near (>80%): {}",
        duration.is_deadline_near(elapsed)
    );
    println!("  Is janky: {}", duration.is_janky(elapsed));

    // Over budget example
    let elapsed_over = Milliseconds::new(20.0);
    println!("\nFor 60 FPS target with 20ms elapsed:");
    println!(
        "  Is over budget: {}",
        duration.is_over_budget(elapsed_over)
    );
    println!(
        "  Utilization: {:.1}%",
        duration.utilization(elapsed_over) * 100.0
    );
    println!("  Is janky: {}", duration.is_janky(elapsed_over));
}
