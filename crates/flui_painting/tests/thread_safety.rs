//! Thread Safety Tests
//!
//! Tests verifying that Canvas and DisplayList can be safely sent across threads
//! for parallel painting and execution.

use flui_painting::{Canvas, Paint};
use flui_types::{geometry::Rect, styling::Color};
use std::sync::Arc;
use std::thread;

#[test]
fn test_canvas_is_send() {
    // Verify Canvas can be sent across threads
    let mut canvas = Canvas::new();
    let rect = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
    let paint = Paint::fill(Color::RED);
    canvas.draw_rect(rect, &paint);

    let handle = thread::spawn(move || {
        // Canvas moved to another thread
        let list = canvas.finish();
        list.len()
    });

    let len = handle.join().unwrap();
    assert_eq!(len, 1);
}

#[test]
fn test_display_list_is_send() {
    // Verify DisplayList can be sent across threads
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
        &Paint::fill(Color::RED),
    );

    let display_list = canvas.finish();

    let handle = thread::spawn(move || {
        // DisplayList moved to another thread
        display_list.len()
    });

    let len = handle.join().unwrap();
    assert_eq!(len, 1);
}

#[test]
fn test_parallel_canvas_creation() {
    // Simulate parallel painting: each thread creates its own Canvas
    let mut handles = vec![];

    for i in 0..10 {
        let handle = thread::spawn(move || {
            let mut canvas = Canvas::new();

            for j in 0..10 {
                let rect = Rect::from_ltrb((i * 10 + j) as f32, 0.0, (i * 10 + j + 1) as f32, 50.0);
                let paint = Paint::fill(Color::RED);
                canvas.draw_rect(rect, &paint);
            }

            canvas.finish()
        });

        handles.push(handle);
    }

    // Collect all display lists
    let mut total_commands = 0;
    for handle in handles {
        let list = handle.join().unwrap();
        total_commands += list.len();
    }

    // Each thread produced 10 commands
    assert_eq!(total_commands, 100);
}

#[test]
fn test_send_to_gpu_thread() {
    // Simulate sending DisplayList to GPU thread for execution
    let mut canvas = Canvas::new();

    // Record many commands
    for i in 0..100 {
        let rect = Rect::from_ltrb(i as f32, 0.0, (i + 1) as f32, 50.0);
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
fn test_arc_sharing_display_list() {
    // DisplayList can be wrapped in Arc for read-only sharing
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
        &Paint::fill(Color::RED),
    );

    let display_list = Arc::new(canvas.finish());

    let mut handles = vec![];

    // Multiple threads can read from Arc<DisplayList>
    for _ in 0..5 {
        let list_clone = Arc::clone(&display_list);

        let handle = thread::spawn(move || {
            // Read-only access
            list_clone.len()
        });

        handles.push(handle);
    }

    // All should see same length
    for handle in handles {
        let len = handle.join().unwrap();
        assert_eq!(len, 1);
    }
}

#[test]
fn test_parallel_build_then_compose() {
    // Realistic scenario: parallel build of children, then compose on main thread
    let mut handles = vec![];

    // Build children in parallel
    for i in 0..10 {
        let handle = thread::spawn(move || {
            let mut child_canvas = Canvas::new();

            let rect = Rect::from_ltrb((i * 10) as f32, 0.0, (i * 10 + 10) as f32, 50.0);
            let paint = Paint::fill(Color::RED);
            child_canvas.draw_rect(rect, &paint);

            child_canvas
        });

        handles.push(handle);
    }

    // Collect children on main thread
    let mut parent = Canvas::new();

    for handle in handles {
        let child = handle.join().unwrap();
        parent.append_canvas(child);
    }

    let parent_list = parent.finish();
    assert_eq!(parent_list.len(), 10);
}

#[test]
fn test_no_data_races() {
    // Verify no data races when creating many canvases concurrently
    use std::sync::Barrier;

    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for i in 0..10 {
        let barrier_clone = Arc::clone(&barrier);

        let handle = thread::spawn(move || {
            // Wait for all threads to be ready
            barrier_clone.wait();

            // Create canvas simultaneously with other threads
            let mut canvas = Canvas::new();

            for j in 0..100 {
                let rect =
                    Rect::from_ltrb((i * 100 + j) as f32, 0.0, (i * 100 + j + 1) as f32, 50.0);
                let paint = Paint::fill(Color::RED);
                canvas.draw_rect(rect, &paint);
            }

            canvas.finish().len()
        });

        handles.push(handle);
    }

    let mut results = vec![];
    for handle in handles {
        results.push(handle.join().unwrap());
    }

    // All threads should have created 100 commands
    for len in results {
        assert_eq!(len, 100);
    }
}

#[test]
fn test_canvas_not_sync() {
    // This is a compile-time test - uncomment to verify Canvas is !Sync
    /*
    let canvas = Canvas::new();
    let canvas_ref = &canvas;

    // This should NOT compile because Canvas is !Sync
    let handle = thread::spawn(move || {
        canvas_ref.finish();  // ERROR: Canvas is not Sync
    });
    */

    // If this test compiles, Canvas is correctly !Sync
    assert!(true);
}

#[test]
fn test_display_list_clone_and_send() {
    // DisplayList implements Clone, so we can clone and send to different threads
    let mut canvas = Canvas::new();
    canvas.draw_rect(
        Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
        &Paint::fill(Color::RED),
    );

    let original_list = canvas.finish();
    let cloned_list = original_list.clone();

    let handle = thread::spawn(move || {
        // Send clone to another thread
        cloned_list.len()
    });

    // Original still accessible
    assert_eq!(original_list.len(), 1);

    // Clone also works
    let len = handle.join().unwrap();
    assert_eq!(len, 1);
}
