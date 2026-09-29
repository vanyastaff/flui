//! Compile-fail test: a device-grid point is not a logical point; crossing needs an explicit
//! conversion through a `DevicePixelRatio` (ADR-0098 §5).

use flui_foundation::geometry::{DevicePoint, Point};

fn hit_test(_at: Point) {}

fn main() {
    hit_test(DevicePoint::new(3, 4));
}
