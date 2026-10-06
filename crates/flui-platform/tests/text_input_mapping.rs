//! The cfg-free rules the Win32 text services are built on, run on every
//! host: a text range's logical rect as the physical screen rect an input
//! method places its windows against.

use flui_foundation::geometry::{Bounds, DevicePixelRatio, DevicePoint, Point, Size};
use flui_platform::shared::text_geometry::{
    ScreenRect, ScreenRectError, range_rect_to_screen, screen_point_to_client,
};

fn ratio(value: f64) -> DevicePixelRatio {
    DevicePixelRatio::new(value).expect("a positive, finite test scale")
}

fn bounds(x: f64, y: f64, width: f64, height: f64) -> Bounds<f64> {
    Bounds::new(Point::new(x, y), Size::new(width, height))
}

fn screen(left: i32, top: i32, right: i32, bottom: i32) -> ScreenRect {
    ScreenRect {
        left,
        top,
        right,
        bottom,
    }
}

const ORIGIN: DevicePoint = DevicePoint::new(100, 200);

fn at_100_percent_the_rect_is_offset_by_the_client_origin() {
    assert_eq!(
        range_rect_to_screen(bounds(30.0, 40.0, 50.0, 20.0), ORIGIN, ratio(1.0)),
        Ok(screen(130, 240, 180, 260))
    );
}

fn at_150_percent_fractional_edges_round_outwards() {
    // 15 × 1.5 = 22.5 → 22; (15 + 11) × 1.5 = 39 exactly; 7 × 1.5 = 10.5 → 10;
    // (7 + 13) × 1.5 = 30 exactly.
    assert_eq!(
        range_rect_to_screen(bounds(15.0, 7.0, 11.0, 13.0), ORIGIN, ratio(1.5)),
        Ok(screen(122, 210, 139, 230))
    );
}

fn at_175_percent_the_scale_comes_from_the_caller() {
    // 10 × 1.75 = 17.5 → 17; 30 × 1.75 = 52.5 → 53; 20 × 1.75 = 35; 40 × 1.75 = 70.
    assert_eq!(
        range_rect_to_screen(bounds(10.0, 20.0, 20.0, 20.0), ORIGIN, ratio(1.75)),
        Ok(screen(117, 235, 153, 270))
    );
}

fn fractional_logical_edges_cover_every_touched_pixel() {
    assert_eq!(
        range_rect_to_screen(
            bounds(0.25, 0.75, 9.5, 0.5),
            DevicePoint::new(0, 0),
            ratio(1.0)
        ),
        Ok(screen(0, 0, 10, 2))
    );
}

fn negative_coordinates_floor_instead_of_truncating() {
    // Truncation would give -2 and -1; the cover is one pixel further out.
    assert_eq!(
        range_rect_to_screen(
            bounds(-2.5, -1.25, 1.0, 1.0),
            DevicePoint::new(0, 0),
            ratio(1.0)
        ),
        Ok(screen(-3, -2, -1, 0))
    );
}

fn float_noise_below_the_tolerance_adds_no_pixel() {
    // 0.1 × 3 is 0.30000000000000004 in f64; the noise must not reach a
    // pixel. A plain floor/ceil would give a top of -1, a right of 14 and a
    // bottom of 21.
    let x = 0.1 * 3.0 * 10.0;
    assert_eq!(
        range_rect_to_screen(
            bounds(x, -1e-9, 10.0 + 1e-9, 20.0 + 2e-9),
            DevicePoint::new(0, 0),
            ratio(1.0)
        ),
        Ok(screen(3, 0, 13, 20))
    );
}

fn an_inverted_rect_is_taken_by_its_edges() {
    assert_eq!(
        range_rect_to_screen(
            bounds(10.0, 10.0, -5.0, -5.0),
            DevicePoint::new(0, 0),
            ratio(2.0)
        ),
        Ok(screen(10, 10, 20, 20))
    );
}

fn a_non_finite_rect_has_no_screen_rect() {
    for rect in [
        bounds(f64::NAN, 0.0, 1.0, 1.0),
        bounds(0.0, 0.0, f64::INFINITY, 1.0),
        bounds(0.0, f64::NEG_INFINITY, 1.0, 1.0),
    ] {
        assert_eq!(
            range_rect_to_screen(rect, ORIGIN, ratio(1.0)),
            Err(ScreenRectError::NotFinite),
            "{rect:?}"
        );
    }
}

fn an_edge_past_the_i32_range_has_no_screen_rect() {
    let max = f64::from(i32::MAX);
    assert_eq!(
        range_rect_to_screen(bounds(max, 0.0, 1.0, 1.0), ORIGIN, ratio(1.0)),
        Err(ScreenRectError::OutOfRange)
    );
    // In range logically, out of range once scaled.
    assert_eq!(
        range_rect_to_screen(bounds(0.0, max / 2.0, 1.0, 1.0), ORIGIN, ratio(3.0)),
        Err(ScreenRectError::OutOfRange)
    );
    // The client origin can push a rect that fits on its own past the range.
    assert_eq!(
        range_rect_to_screen(
            bounds(-10.0, 0.0, 1.0, 1.0),
            DevicePoint::new(i32::MIN, 0),
            ratio(1.0)
        ),
        Err(ScreenRectError::OutOfRange)
    );
}

/// The screen rect for a text range follows the injected scale, covers every
/// partly touched pixel (floor towards negative coordinates, not truncation),
/// ignores float noise, and refuses a rect it cannot express.
#[test]
fn screen_rects_follow_scale_and_round_outwards() {
    let rows: &[(&str, fn())] = &[
        (
            "at_100_percent_the_rect_is_offset_by_the_client_origin",
            at_100_percent_the_rect_is_offset_by_the_client_origin,
        ),
        (
            "at_150_percent_fractional_edges_round_outwards",
            at_150_percent_fractional_edges_round_outwards,
        ),
        (
            "at_175_percent_the_scale_comes_from_the_caller",
            at_175_percent_the_scale_comes_from_the_caller,
        ),
        (
            "fractional_logical_edges_cover_every_touched_pixel",
            fractional_logical_edges_cover_every_touched_pixel,
        ),
        (
            "negative_coordinates_floor_instead_of_truncating",
            negative_coordinates_floor_instead_of_truncating,
        ),
        (
            "float_noise_below_the_tolerance_adds_no_pixel",
            float_noise_below_the_tolerance_adds_no_pixel,
        ),
        (
            "an_inverted_rect_is_taken_by_its_edges",
            an_inverted_rect_is_taken_by_its_edges,
        ),
        (
            "a_non_finite_rect_has_no_screen_rect",
            a_non_finite_rect_has_no_screen_rect,
        ),
        (
            "an_edge_past_the_i32_range_has_no_screen_rect",
            an_edge_past_the_i32_range_has_no_screen_rect,
        ),
    ];
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, row)| std::panic::catch_unwind(row).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "failing rows: {failed:?}");
}

/// A screen point TSF asks about (`GetACPFromPoint`) maps to window-root
/// logical pixels by the injected scale, for every pair of `i32` screen and
/// origin coordinates: the difference of two extremes does not fit an `i32`,
/// but is exact in `f64`.
#[test]
fn screen_points_map_to_logical_client_points() {
    let (min, max) = (i32::MIN, i32::MAX);
    let span = f64::from(max) - f64::from(min);
    let rows: &[(&str, DevicePoint, DevicePoint, f64, Point<f64>)] = &[
        (
            "offset by the client origin",
            DevicePoint::new(130, 240),
            ORIGIN,
            1.0,
            Point::new(30.0, 40.0),
        ),
        (
            "divided by the scale",
            DevicePoint::new(115, 203),
            ORIGIN,
            1.5,
            Point::new(10.0, 2.0),
        ),
        (
            "left of and above the client origin",
            DevicePoint::new(90, 199),
            ORIGIN,
            2.0,
            Point::new(-5.0, -0.5),
        ),
        (
            "the largest screen point past the smallest origin",
            DevicePoint::new(max, max),
            DevicePoint::new(min, min),
            1.0,
            Point::new(span, span),
        ),
        (
            "the smallest screen point before the largest origin",
            DevicePoint::new(min, min + 1),
            DevicePoint::new(max, max),
            2.0,
            Point::new(-span / 2.0, -(span - 1.0) / 2.0),
        ),
        (
            "one past an extreme origin",
            DevicePoint::new(min + 1, max - 1),
            DevicePoint::new(min, max),
            1.0,
            Point::new(1.0, -1.0),
        ),
    ];
    let failed: Vec<&str> = rows
        .iter()
        .filter(|(_, at, origin, scale, expected)| {
            std::panic::catch_unwind(|| screen_point_to_client(*at, *origin, ratio(*scale)))
                .map_or(true, |point| point != *expected)
        })
        .map(|(name, ..)| *name)
        .collect();
    assert!(failed.is_empty(), "failing rows: {failed:?}");
}
