//! Compile-fail test: a logical rectangle (`f64`) cannot be intersected with a device-pixel
//! rectangle (`i32`).
//!
//! The scalar alone keeps logical and device-pixel geometry apart (ADR-0098 §5).

use flui_foundation::geometry::{Point, Rect};

fn main() {
    let rect_logical = Rect::from_min_max(Point::new(0.0, 0.0), Point::new(100.0, 100.0));
    let rect_device = Rect::from_min_max(Point::new(0, 0), Point::new(200, 200));

    let _intersection = rect_logical.intersect(&rect_device);
}
