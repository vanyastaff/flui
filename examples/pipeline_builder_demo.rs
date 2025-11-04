//! PipelineBuilder Demo
//!
//! This example demonstrates the fluent builder API for configuring PipelineOwner
//! with different presets and custom options.
//!
//! Run with: cargo run --example pipeline_builder_demo

use flui_core::pipeline::{PipelineBuilder, RecoveryPolicy};
use std::time::Duration;

fn main() {
    println!("=== PipelineBuilder Demo ===\n");

    // Example 1: Minimal configuration (lowest overhead)
    println!("1. Minimal Configuration:");
    let minimal = PipelineBuilder::minimal().build();
    println!("   - Metrics: {:?}", minimal.metrics().is_some());
    println!("   - Error Recovery: {:?}", minimal.error_recovery().is_some());
    println!("   - Cancellation: {:?}", minimal.cancellation_token().is_some());
    println!("   - Batching: {:?}", minimal.is_batching_enabled());
    println!("   -> Use for: Maximum performance, no debugging\n");

    // Example 2: Development configuration
    println!("2. Development Configuration:");
    let dev = PipelineBuilder::development().build();
    println!("   - Metrics: {:?}", dev.metrics().is_some());
    println!("   - Error Recovery: {:?}", dev.error_recovery().is_some());
    println!("   - Cancellation: {:?}", dev.cancellation_token().is_some());
    println!("   - Batching: {:?}", dev.is_batching_enabled());
    println!("   -> Use for: Development, shows error widgets\n");

    // Example 3: Testing configuration
    println!("3. Testing Configuration:");
    let test = PipelineBuilder::testing().build();
    println!("   - Metrics: {:?}", test.metrics().is_some());
    println!("   - Error Recovery: {:?}", test.error_recovery().is_some());
    println!("   - Cancellation: {:?}", test.cancellation_token().is_some());
    println!("   - Batching: {:?}", test.is_batching_enabled());
    println!("   -> Use for: Unit tests, fail-fast behavior\n");

    // Example 4: Production configuration
    println!("4. Production Configuration:");
    let prod = PipelineBuilder::production().build();
    println!("   - Metrics: {:?}", prod.metrics().is_some());
    println!("   - Error Recovery: {:?}", prod.error_recovery().is_some());
    println!("   - Cancellation: {:?}", prod.cancellation_token().is_some());
    println!("   - Batching: {:?}", prod.is_batching_enabled());
    println!("   -> Use for: Production, graceful degradation\n");

    // Example 5: Custom configuration
    println!("5. Custom Configuration:");
    let custom = PipelineBuilder::new()
        .with_metrics()
        .with_batching(Duration::from_millis(8)) // 120fps target
        .with_error_recovery(RecoveryPolicy::SkipFrame)
        .with_build_callback(|| {
            println!("   [Callback] Build scheduled!");
        })
        .build();

    println!("   - Metrics: {:?}", custom.metrics().is_some());
    println!("   - Error Recovery: {:?}", custom.error_recovery().is_some());
    println!("   - Cancellation: {:?}", custom.cancellation_token().is_some());
    println!("   - Batching: {:?}", custom.is_batching_enabled());
    println!("   -> Use for: Custom requirements\n");

    // Example 6: Chaining methods
    println!("6. Method Chaining:");
    let chained = PipelineBuilder::production()
        .with_build_callback(|| {
            println!("   [Callback] Frame requested!");
        })
        .build();

    println!("   - Started from production preset");
    println!("   - Added custom callback");
    println!("   -> Use for: Extending presets with custom logic\n");

    // Example 7: Trigger callback
    println!("7. Testing Callback:");
    let mut with_callback = PipelineBuilder::new()
        .with_build_callback(|| {
            println!("   ✓ Callback triggered!");
        })
        .build();

    println!("   Scheduling build...");
    with_callback.schedule_build_for(flui_core::ElementId::new(1), 0);
    println!("   -> Callback executed on schedule_build_for()\n");

    // Example 8: Batching statistics
    println!("8. Build Batching:");
    let mut batched = PipelineBuilder::new()
        .with_batching(Duration::from_millis(16))
        .build();

    println!("   Scheduling 3 builds for same element...");
    batched.schedule_build_for(flui_core::ElementId::new(42), 0);
    batched.schedule_build_for(flui_core::ElementId::new(42), 0);
    batched.schedule_build_for(flui_core::ElementId::new(42), 0);

    println!("   Flushing batch...");
    batched.flush_batch();

    let (batches, saved) = batched.batching_stats();
    println!("   - Batches flushed: {}", batches);
    println!("   - Builds saved: {} (deduplication!)", saved);
    println!("   - Dirty count: {}", batched.dirty_count());
    println!("   -> Use for: Optimizing rapid setState() calls\n");

    println!("=== Demo Complete ===");
    println!("\nKey Takeaways:");
    println!("- Use presets for common scenarios (production/development/testing)");
    println!("- Chain methods for custom configurations");
    println!("- Batching reduces redundant rebuilds");
    println!("- Callbacks enable custom rendering logic");
    println!("- Zero runtime overhead (builder runs at startup)");
}
