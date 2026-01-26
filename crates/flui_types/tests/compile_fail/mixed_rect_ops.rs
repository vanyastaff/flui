//! Compile-fail test: Attempting to intersect Rect<Pixels> with Rect<DevicePixels> should fail
//!
//! This test verifies type-safe Rect operations prevent cross-unit mixing.

use flui_types::geometry::{Pixels, DevicePixels, Rect, Point, Size};

fn main() {
    let rect_logical = Rect::from_min_max(
        Point::new(Pixels(0.0), Pixels(0.0)),
        Point::new(Pixels(100.0), Pixels(100.0))
    );

    let rect_device = Rect::from_min_max(
        Point::new(DevicePixels(0), DevicePixels(0)),
        Point::new(DevicePixels(200), DevicePixels(200))
    );

    // This should fail to compile - cannot intersect Rect<Pixels> with Rect<DevicePixels>
    let _intersection = rect_logical.intersect(&rect_device);
    //~^ ERROR: mismatched types
}
