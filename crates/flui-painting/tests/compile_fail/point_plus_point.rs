//! Compile-fail test: a position plus a position means nothing, so `Point + Point` does not
//! exist; `Point + Offset` does (ADR-0098 §3).

use flui_foundation::geometry::Point;

fn main() {
    let _sum = Point::new(1.0, 2.0) + Point::new(3.0, 4.0);
}
