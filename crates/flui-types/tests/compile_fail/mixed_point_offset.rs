//! Compile-fail test: a logical point (`f64`) cannot be moved by a device-pixel offset (`i32`).
//!
//! The scalar alone keeps logical and device-pixel geometry apart (ADR-0098 §5).

use flui_types::geometry::{Offset, Point};

fn main() {
    let point = Point::new(10.0, 20.0);
    let offset = Offset::new(5, 10);

    let _result = point + offset;
}
