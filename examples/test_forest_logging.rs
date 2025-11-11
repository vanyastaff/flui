//! Test tracing-forest logging output
//!
//! Run with: cargo run --example test_forest_logging

use std::thread;
use std::time::Duration;

fn main() {
    // Initialize logging with tracing-forest (same as flui_core::logging)
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry, Layer};
    use tracing_forest::ForestLayer;

    let filter = EnvFilter::new("debug");
    let forest_layer = ForestLayer::default().with_filter(filter);
    Registry::default().with(forest_layer).init();

    println!("=== Testing tracing-forest hierarchical logging ===\n");

    // Simulate 3 frames
    for i in 0..3 {
        render_frame(i);
        thread::sleep(Duration::from_millis(10));
    }
}

#[tracing::instrument]
fn render_frame(num: u32) {
    tracing::info!("Starting frame");

    build_phase();
    layout_phase();
    paint_phase();

    tracing::info!("Frame complete");
}

#[tracing::instrument]
fn build_phase() {
    thread::sleep(Duration::from_millis(5));
    tracing::debug!(count = 1, "Build complete");
}

#[tracing::instrument]
fn layout_phase() {
    thread::sleep(Duration::from_millis(3));
    tracing::debug!(count = 1, "Layout complete");
}

#[tracing::instrument]
fn paint_phase() {
    thread::sleep(Duration::from_millis(2));
    tracing::debug!(count = 1, "Paint complete");
}
