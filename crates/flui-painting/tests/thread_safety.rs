//! Thread Safety Tests
//!
//! Tests verifying that Canvas and `DisplayList` can be safely sent across
//! threads for parallel painting and execution.

use std::thread;

use flui_foundation::geometry::Rect;
use flui_painting::styling::Color;
use flui_painting::{Canvas, Paint};

#[test]
fn test_send_to_gpu_thread() {
    // Simulate sending DisplayList to GPU thread for execution
    let mut canvas = Canvas::new();

    // Record many commands
    for i in 0..100 {
        let rect = Rect::from_ltrb(i as f64, 0.0, (i + 1) as f64, 50.0);
        let paint = Paint::fill(Color::RED);
        canvas.draw_rect(rect, &paint);
    }

    let display_list = canvas.finish();

    // Send to "GPU thread"
    let handle = thread::spawn(move || {
        // Simulate GPU execution by iterating commands
        let mut count = 0;
        for _cmd in display_list.commands() {
            count += 1;
        }
        count
    });

    let executed = handle.join().unwrap();
    assert_eq!(executed, 100);
}

#[test]
fn test_parallel_build_then_compose() {
    // Realistic scenario: parallel build of children, then compose on main thread
    let mut handles = vec![];

    // Build children in parallel
    for i in 0..10 {
        let handle = thread::spawn(move || {
            let mut child_canvas = Canvas::new();

            let rect = Rect::from_ltrb((i * 10) as f64, 0.0, (i * 10 + 10) as f64, 50.0);
            let paint = Paint::fill(Color::RED);
            child_canvas.draw_rect(rect, &paint);

            child_canvas
        });

        handles.push(handle);
    }

    // Merge the children's lists on the main thread, the way the paint walk
    // merges adjacent inline runs into one picture.
    let mut merged = flui_painting::DisplayList::new();
    for handle in handles {
        merged.append(handle.join().unwrap().finish());
    }
    assert_eq!(merged.len(), 10);
}
