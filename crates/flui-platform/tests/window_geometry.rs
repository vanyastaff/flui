//! The geometry a snapshot holds is finite by its types: a report that is not
//! a window's geometry builds no value, so it cannot be published.

use flui_foundation::geometry::{Point, Size};
use flui_platform::{
    DisplayKey, DisplayRegistry, InvalidGeometry, SizeConstraints, WindowExtent, WindowGeometry,
    WindowPosition,
};

/// Sides no window has: what a size divided by a zero scale, a compositor's
/// NaN or an inverted rectangle reads as.
const BAD_SIDES: [f64; 4] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0];

fn an_extent_is_finite_and_not_negative() {
    for (width, height) in [(0.0, 0.0), (1.0, 0.0), (800.0, 600.0), (f64::MAX, 1.0)] {
        let extent = WindowExtent::try_new(Size::new(width, height)).expect("an extent");
        assert_eq!((extent.width(), extent.height()), (width, height));
        assert_eq!(extent.get(), Size::new(width, height));
    }
    for bad in BAD_SIDES {
        for size in [Size::new(bad, 600.0), Size::new(800.0, bad)] {
            assert!(
                matches!(
                    WindowExtent::try_new(size),
                    Err(InvalidGeometry::Extent { .. })
                ),
                "{size:?}"
            );
        }
    }
    assert_eq!(WindowExtent::default(), WindowExtent::ZERO);
}

fn a_rectangle_is_a_finite_position_and_an_extent() {
    let display = DisplayRegistry::new()
        .mint(&DisplayKey::Injected(1))
        .expect("an id");
    let at = |x, y| WindowPosition::try_new(display, Point::new(x, y)).expect("a finite point");
    let size = Size::new(800.0, 600.0);
    for position in [None, Some(at(-1920.0, -40.0)), Some(at(0.0, 0.0))] {
        let geometry = WindowGeometry::try_new(position, size).expect("a rectangle");
        assert_eq!(geometry.position(), position);
        assert_eq!(geometry.size(), size);
        assert_eq!(geometry.extent(), size);
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for point in [Point::new(bad, 0.0), Point::new(0.0, bad)] {
            assert!(
                matches!(
                    WindowPosition::try_new(display, point),
                    Err(InvalidGeometry::Position { .. })
                ),
                "{point:?}"
            );
        }
    }
    for bad in BAD_SIDES {
        assert!(
            matches!(
                WindowGeometry::try_new(None, Size::new(bad, 600.0)),
                Err(InvalidGeometry::Extent { .. })
            ),
            "{bad}"
        );
    }
    let moved = WindowGeometry::unplaced(WindowExtent::try_new(size).expect("an extent"))
        .at(Some(at(4.0, 2.0)));
    assert_eq!(
        moved,
        WindowGeometry::try_new(Some(at(4.0, 2.0)), size).expect("a rectangle")
    );
    assert_eq!(moved.position().map(WindowPosition::display), Some(display));
}

fn limits_are_extents() {
    assert_eq!(
        SizeConstraints::try_new(None, None),
        Ok(SizeConstraints::NONE)
    );
    let limits = SizeConstraints::try_new(Some(Size::new(200.0, 100.0)), Some(Size::new(0.0, 0.0)))
        .expect("finite limits");
    assert_eq!(
        limits.min.map(WindowExtent::get),
        Some(Size::new(200.0, 100.0))
    );
    assert_eq!(limits.max, Some(WindowExtent::ZERO));
    for bad in BAD_SIDES {
        assert!(SizeConstraints::try_new(Some(Size::new(bad, 1.0)), None).is_err());
        assert!(SizeConstraints::try_new(None, Some(Size::new(1.0, bad))).is_err());
    }
}

/// The refusal of a report equals itself, a NaN in it included, so a host,
/// a test or a diagnostic that compares two refusals of the same report sees
/// one; refusals of different numbers differ.
fn a_refusal_equals_itself() {
    let refusals = [
        WindowExtent::try_new(Size::new(f64::NAN, 600.0)).expect_err("a NaN side"),
        WindowPosition::try_new(
            DisplayRegistry::new()
                .mint(&DisplayKey::Injected(1))
                .expect("an id"),
            Point::new(0.0, f64::NAN),
        )
        .expect_err("a NaN coordinate"),
        WindowExtent::try_new(Size::new(-1.0, 600.0)).expect_err("a negative side"),
    ];
    for (index, refusal) in refusals.iter().enumerate() {
        for (other_index, other) in refusals.iter().enumerate() {
            assert_eq!(
                refusal == other,
                index == other_index,
                "{refusal} / {other}"
            );
        }
    }
}

#[test]
fn window_geometry() {
    crate::run_table(
        "window_geometry",
        &[
            (
                "an_extent_is_finite_and_not_negative",
                an_extent_is_finite_and_not_negative,
            ),
            (
                "a_rectangle_is_a_finite_position_and_an_extent",
                a_rectangle_is_a_finite_position_and_an_extent,
            ),
            ("limits_are_extents", limits_are_extents),
            ("a_refusal_equals_itself", a_refusal_equals_itself),
        ],
    );
}
