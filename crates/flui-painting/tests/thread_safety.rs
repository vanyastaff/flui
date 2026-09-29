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
