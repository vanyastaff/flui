//! Compile-fail test: a `DevicePixelRatio` exists only through its checked constructor, so a
//! zero, negative or non-finite ratio cannot be built (ADR-0098 §5).

use flui_foundation::geometry::DevicePixelRatio;

fn main() {
    let _ratio = DevicePixelRatio(0.0);
}
