//! Compile-fail test: a size is never a displacement (ADR-0098 §3).

use flui_foundation::geometry::{Offset, Size};

fn shift(_by: Offset) {}

fn main() {
    shift(Size::new(10.0, 20.0));
}
