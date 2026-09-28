//! Direct Render — First working FLUI application
//!
//! Demonstrates `run_direct()` which opens a window, initializes the GPU,
//! and runs a render loop with a user-provided scene builder closure.
//! Bypasses the widget tree (flui-view/flui-rendering) for direct engine access.
//!
//! Run with: cargo run --example direct_render

use flui_app::{AppConfig, run_direct};

fn main() -> anyhow::Result<()> {
    run_direct(
        AppConfig::new()
            .with_title("FLUI Direct Render")
            .with_size(800, 600),
        |builder, w, h| {
            use flui_painting::Canvas;
            use flui_types::{Rect, geometry::px, painting::Paint, styling::Color};

            // Record drawing commands into a Picture
            let mut canvas = Canvas::new();

            // Background — dark slate
            canvas.draw_rect(
                Rect::from_ltrb(0.0, 0.0, w, h),
                &Paint::fill(Color::rgb(30, 30, 46)),
            );

            // Red rectangle (top-left)
            canvas.draw_rect(
                Rect::from_ltrb(50.0, 50.0, 300.0, 200.0),
                &Paint::fill(Color::RED),
            );

            // Green rectangle (center, overlapping)
            canvas.draw_rect(
                Rect::from_ltrb(200.0, 150.0, 500.0, 350.0),
                &Paint::fill(Color::GREEN),
            );

            // Blue rectangle (bottom-right)
            canvas.draw_rect(
                Rect::from_ltrb(400.0, 250.0, (w - 50.0), (h - 50.0)),
                &Paint::fill(Color::BLUE),
            );

            // White rectangle (small, center)
            let cx = w / 2.0;
            let cy = h / 2.0;
            canvas.draw_rect(
                Rect::from_ltrb((cx - 60.0), (cy - 40.0), (cx + 60.0), (cy + 40.0)),
                &Paint::fill(Color::WHITE),
            );

            // Yellow bar (bottom)
            canvas.draw_rect(
                Rect::from_ltrb(80.0, (h - 120.0), (w - 80.0), (h - 70.0)),
                &Paint::fill(Color::rgb(255, 200, 0)),
            );

            let picture = canvas.finish();
            builder.add_picture(picture);
        },
    )
}
