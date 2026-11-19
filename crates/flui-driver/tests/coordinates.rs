//! Surface-logical points to screen device pixels: extreme scales, negative origins,
//! fractional edges, RTL, and the refusals.

use flui_driver::Error;
use flui_driver::coordinates::{DevicePoint, WindowGeometry, to_device};
use flui_driver::protocol::geometry::{LogicalPoint, LogicalSize};

/// What a row expects.
#[derive(Debug)]
enum Expected {
    At(i32, i32),
    Outside,
    Invalid,
}

fn window(x: f64, y: f64, width: f64, height: f64, scale: f64) -> WindowGeometry {
    WindowGeometry::new(
        LogicalPoint::new(x, y),
        LogicalSize::new(width, height),
        scale,
    )
}

fn check(row: &str, mapped: Result<DevicePoint, Error>, expected: &Expected) {
    match (expected, mapped) {
        (Expected::At(x, y), Ok(point)) => {
            assert_eq!(point, DevicePoint::new(*x, *y), "{row}");
        }
        (Expected::Outside, Err(Error::OutsideTarget { .. }))
        | (Expected::Invalid, Err(Error::InvalidArgument { .. })) => {}
        (expected, mapped) => panic!("{row}: expected {expected:?}, mapped {mapped:?}"),
    }
}

#[test]
fn a_point_maps_to_the_device_pixel_it_covers_or_is_refused() {
    let rows: &[(&str, WindowGeometry, LogicalPoint, Expected)] = &[
        (
            "scale 1 at the origin",
            window(0.0, 0.0, 800.0, 600.0, 1.0),
            LogicalPoint::new(10.0, 20.0),
            Expected::At(10, 20),
        ),
        (
            "scale 0.5",
            window(100.0, 50.0, 800.0, 600.0, 0.5),
            LogicalPoint::new(10.0, 20.0),
            Expected::At(55, 35),
        ),
        (
            "scale 8, the last pixel inside",
            window(0.0, 0.0, 800.0, 600.0, 8.0),
            LogicalPoint::new(799.99, 599.99),
            Expected::At(6399, 4799),
        ),
        (
            "a negative origin left of and above the primary monitor",
            window(-1920.0, -100.0, 800.0, 600.0, 1.25),
            LogicalPoint::new(0.0, 0.0),
            Expected::At(-2400, -125),
        ),
        (
            "a negative origin rounds towards negative infinity",
            window(-0.5, -0.5, 800.0, 600.0, 1.0),
            LogicalPoint::new(0.0, 0.0),
            Expected::At(-1, -1),
        ),
        (
            "a fractional origin at scale 1.5",
            window(10.4, 0.0, 800.0, 600.0, 1.5),
            LogicalPoint::new(0.0, 0.0),
            Expected::At(15, 0),
        ),
        (
            "a fractional point just inside the right edge",
            window(0.0, 0.0, 800.0, 600.0, 1.0),
            LogicalPoint::new(799.5, 0.0),
            Expected::At(799, 0),
        ),
        (
            "the right edge itself is past the surface",
            window(0.0, 0.0, 800.0, 600.0, 1.0),
            LogicalPoint::new(800.0, 0.0),
            Expected::Outside,
        ),
        (
            "the bottom edge itself is past the surface",
            window(0.0, 0.0, 800.0, 600.0, 2.0),
            LogicalPoint::new(0.0, 600.0),
            Expected::Outside,
        ),
        (
            "a fraction left of the surface is outside, never clamped in",
            window(0.0, 0.0, 800.0, 600.0, 1.0),
            LogicalPoint::new(-0.25, 10.0),
            Expected::Outside,
        ),
        (
            // An RTL window lays its leading button out on the right and publishes its point
            // measured from the surface's physical left: the mapping takes no direction.
            "an RTL window's leading button, on the right",
            window(100.0, 100.0, 800.0, 600.0, 2.0),
            LogicalPoint::new(740.0, 30.0),
            Expected::At(1680, 260),
        ),
        (
            "an origin past what the screen addresses",
            window(1.0e12, 0.0, 800.0, 600.0, 1.0),
            LogicalPoint::new(0.0, 0.0),
            Expected::Outside,
        ),
        (
            "a scale of zero",
            window(0.0, 0.0, 800.0, 600.0, 0.0),
            LogicalPoint::new(1.0, 1.0),
            Expected::Invalid,
        ),
        (
            "an infinite scale",
            window(0.0, 0.0, 800.0, 600.0, f64::INFINITY),
            LogicalPoint::new(1.0, 1.0),
            Expected::Invalid,
        ),
        (
            "a point that is not a number",
            window(0.0, 0.0, 800.0, 600.0, 1.0),
            LogicalPoint::new(f64::NAN, 1.0),
            Expected::Invalid,
        ),
        (
            "an origin that is not finite",
            window(f64::NEG_INFINITY, 0.0, 800.0, 600.0, 1.0),
            LogicalPoint::new(1.0, 1.0),
            Expected::Invalid,
        ),
    ];
    for (row, geometry, point, expected) in rows {
        check(row, to_device(geometry, *point), expected);
    }
}
