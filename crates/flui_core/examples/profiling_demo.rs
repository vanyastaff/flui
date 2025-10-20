//! Profiling demonstration
//!
//! This example demonstrates how to use the profiling infrastructure.
//!
//! Run with profiling enabled:
//! ```bash
//! cargo run --example profiling_demo --features profiling
//! ```
//!
//! Then open http://localhost:8585 in puffin_viewer or browser

use flui_core::profiling::{profile_function, profile_scope, profile_expr};
use flui_core::{BoxConstraints, ElementId, Size};
use flui_core::cache::{get_layout_cache, LayoutCacheKey, LayoutResult};

fn simulate_expensive_layout(constraints: BoxConstraints) -> Size {
    profile_function!();

    profile_scope!("compute_intrinsic_width");
    let mut width = constraints.min_width;
    for i in 0..1000 {
        width += (i as f32).sin() * 0.01;
    }

    profile_scope!("compute_intrinsic_height");
    let mut height = constraints.min_height;
    for i in 0..1000 {
        height += (i as f32).cos() * 0.01;
    }

    Size::new(width, height)
}

fn layout_with_cache(element_id: ElementId, constraints: BoxConstraints) -> Size {
    profile_function!();

    let cache = get_layout_cache();
    let key = LayoutCacheKey::new(element_id, constraints);

    let result = profile_expr!("cache_lookup", {
        cache.get_or_compute(key, || {
            profile_scope!("cache_miss_compute");
            LayoutResult::new(simulate_expensive_layout(constraints))
        })
    });

    result.size
}

fn build_widget_tree(depth: usize) {
    profile_function!();

    if depth == 0 {
        return;
    }

    profile_scope!("create_children");
    for i in 0..3 {
        profile_scope!("child_build");
        let element_id = ElementId::new();
        let constraints = BoxConstraints::tight(Size::new(100.0 + i as f32, 100.0));

        // First call - cache miss
        let size1 = layout_with_cache(element_id, constraints);

        // Second call - cache hit (much faster!)
        let size2 = layout_with_cache(element_id, constraints);

        assert_eq!(size1, size2);

        build_widget_tree(depth - 1);
    }
}

fn main() {
    // Initialize profiling
    flui_core::profiling::init();

    println!("Profiling demo started");
    println!("Building widget tree with caching...");

    #[cfg(feature = "profiling")]
    {
        println!("Puffin profiling enabled - starting HTTP server");
        flui_core::profiling::start_server();
        println!("Open http://localhost:8585 in puffin_viewer or browser");
        println!("Or run: puffin_viewer");
    }

    #[cfg(not(feature = "profiling"))]
    {
        println!("Note: Profiling is not enabled. Run with --features profiling");
    }

    // Simulate multiple frames
    for frame in 0..10 {
        profile_scope!("frame");

        println!("Frame {}", frame);

        profile_scope!("build_tree");
        build_widget_tree(3);

        profile_scope!("cleanup");
        // Simulate some cleanup work
        std::thread::sleep(std::time::Duration::from_millis(1));

        flui_core::profiling::finish_frame();
    }

    println!("Demo complete!");
    println!("Cache stats: {:?}", get_layout_cache().stats());

    #[cfg(feature = "profiling")]
    {
        println!("\nKeeping server running for 30 seconds...");
        println!("Press Ctrl+C to exit");
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
}
