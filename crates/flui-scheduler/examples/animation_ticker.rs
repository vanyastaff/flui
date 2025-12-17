//! Animation Ticker Example
//!
//! This example demonstrates how to use Tickers for smooth animations,
//! including manual and scheduled tickers, muting, and elapsed time tracking.
//!
//! Run with: `cargo run --example animation_ticker -p flui-scheduler`

use flui_scheduler::{
    scheduler::Scheduler,
    ticker::{ScheduledTicker, Ticker, TickerFuture},
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    println!("=== FLUI Animation Ticker Example ===\n");

    // 1. Manual Ticker Demo
    println!("--- Manual Ticker ---\n");
    demo_manual_ticker();

    // 2. Scheduled Ticker Demo
    println!("\n--- Scheduled Ticker ---\n");
    demo_scheduled_ticker();

    // 3. Ticker Future Demo
    println!("\n--- Ticker Future ---\n");
    demo_ticker_future();

    println!("\n=== Example Complete ===");
}

fn demo_manual_ticker() {
    let scheduler = Scheduler::new();
    let mut ticker = Ticker::new();

    let frame_count = Arc::new(AtomicU32::new(0));

    // Start the ticker with a callback
    let fc = Arc::clone(&frame_count);
    ticker.start(move |elapsed| {
        fc.fetch_add(1, Ordering::SeqCst);
        println!("  Tick! Elapsed: {:.3}s", elapsed);
    });

    println!("Ticker state: {:?}", ticker.state());
    println!("Is active: {}", ticker.is_active());

    // Manually tick a few times
    println!("\nManual ticking (3 times):");
    for _ in 0..3 {
        ticker.tick(&scheduler);
        std::thread::sleep(Duration::from_millis(16)); // ~60 FPS
    }

    // Mute the ticker
    println!("\nMuting ticker...");
    ticker.mute();
    println!("Ticker state: {:?}", ticker.state());
    println!("Is muted: {}", ticker.is_muted());

    // Tick while muted (should not fire callback)
    ticker.tick(&scheduler);
    println!("Ticked while muted - callback NOT fired");

    // Unmute and tick again
    println!("\nUnmuting ticker...");
    ticker.unmute();
    ticker.tick(&scheduler);

    // Stop the ticker
    ticker.stop();
    println!("\nTicker stopped. State: {:?}", ticker.state());
    println!("Total ticks: {}", frame_count.load(Ordering::SeqCst));
}

fn demo_scheduled_ticker() {
    let scheduler = Arc::new(Scheduler::new());
    let mut ticker = ScheduledTicker::new(scheduler.clone());

    println!("Created ScheduledTicker with ID: {:?}", ticker.id());

    let animation_progress = Arc::new(parking_lot::Mutex::new(0.0f64));

    // Start an animation that progresses over time
    let ap = Arc::clone(&animation_progress);
    ticker.start(move |elapsed| {
        // Simulate animation progress (0.0 to 1.0 over 1 second)
        let progress = (elapsed % 1.0).min(1.0);
        *ap.lock() = progress;
    });

    println!("Animation started!");
    println!("Ticker state: {:?}", ticker.state());

    // Execute a few frames
    println!("\nExecuting 5 frames:");
    for i in 1..=5 {
        scheduler.execute_frame();
        let progress = *animation_progress.lock();
        println!("  Frame {}: Progress = {:.1}%", i, progress * 100.0);
        std::thread::sleep(Duration::from_millis(100));
    }

    // Get elapsed time
    let elapsed = ticker.elapsed();
    println!("\nTotal elapsed time: {:.3}s", elapsed.value());

    // Stop the ticker
    ticker.stop();
    println!("Ticker stopped");
}

fn demo_ticker_future() {
    println!("Creating TickerFuture...");

    // Create a pending future
    let future = TickerFuture::new();
    println!("Is pending: {}", future.is_pending());
    println!("Is complete: {}", future.is_complete());
    println!("Is canceled: {}", future.is_canceled());

    // Create a pre-completed future
    let complete_future = TickerFuture::complete();
    println!("\nPre-completed future:");
    println!("Is pending: {}", complete_future.is_pending());
    println!("Is complete: {}", complete_future.is_complete());

    // Clone and share futures
    let _future_clone = future.clone();
    println!("\nFuture cloned successfully");

    // Get the or_cancel derivative
    let _or_cancel = future.or_cancel();
    println!("Created or_cancel derivative future");

    // Demonstrate when_complete_or_cancel
    let callback_called = Arc::new(AtomicU32::new(0));
    let cc = Arc::clone(&callback_called);

    complete_future.when_complete_or_cancel(move || {
        cc.fetch_add(1, Ordering::SeqCst);
    });

    // Give time for callback to execute
    std::thread::sleep(Duration::from_millis(10));

    println!(
        "Completion callback called: {} time(s)",
        callback_called.load(Ordering::SeqCst)
    );
}
