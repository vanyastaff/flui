//! Compile-fail test: Attempting to mix Point<Pixels> with Offset<DevicePixels> should fail
//!
//! This test verifies type-safe Point operations prevent cross-unit mixing.

use flui_types::geometry::{Pixels, DevicePixels, Point, Offset};

fn main() {
    let point = Point::new(Pixels(10.0), Pixels(20.0));
    let offset = Offset::new(DevicePixels(5), DevicePixels(10));

    // This should fail to compile - cannot add Offset<DevicePixels> to Point<Pixels>
    let _result = point + offset;
    //~^ ERROR: the trait bound `Point<Pixels>: Add<Offset<DevicePixels>>` is not satisfied
}
