//! Compile-fail test: Attempting to mix Pixels and DevicePixels should fail
//!
//! This test verifies that the type system prevents mixing incompatible unit types
//! at compile time, fulfilling User Story 2 requirements.

use flui_types::geometry::{Pixels, DevicePixels};

fn main() {
    let logical = Pixels(100.0);
    let device = DevicePixels(200);

    // This should fail to compile - cannot add Pixels and DevicePixels
    let _mixed = logical + device;
    //~^ ERROR: cannot add `DevicePixels` to `Pixels`
}
