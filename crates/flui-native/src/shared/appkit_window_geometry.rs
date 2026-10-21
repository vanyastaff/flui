//! Where an `NSWindow` is, in the terms of the window contract.
//!
//! AppKit measures from the bottom-left corner of the primary screen, in
//! points; the contract measures an outer top-left position in global logical
//! pixels, as every other backend does. A display's usable area is published in
//! device pixels of that display (`origin / scale` is its global logical
//! origin), so a position relative to a display converts through the display's
//! own scale.

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{DisplayInfo, SizeConstraints};

/// Any content limit at or beyond this counts as AppKit's "no limit"
/// (`contentMaxSize` defaults to `CGFLOAT_MAX`, or `FLT_MAX` on older systems).
const UNLIMITED_FROM: f64 = 1.0e30;

/// The outer top-left of a window whose frame AppKit reports as `frame`, in
/// global logical pixels. `primary_height` is the height of the primary screen
/// (`NSScreen.screens[0]`), whose bottom-left corner is AppKit's origin.
///
/// `None` when a number is not finite: a position is never published from a
/// value that could not be applied again.
pub(crate) fn top_left_of_frame(frame: Bounds<f64>, primary_height: f64) -> Option<Point<f64>> {
    let top = primary_height - (frame.origin.y + frame.size.height);
    [frame.origin.x, top, frame.size.width, frame.size.height]
        .iter()
        .all(|number| number.is_finite())
        .then(|| Point::new(frame.origin.x, top))
}

/// The point `setFrameTopLeftPoint:` takes to put a window's outer top-left
/// corner at `top_left`.
pub(crate) fn cocoa_top_left_point(top_left: Point<f64>, primary_height: f64) -> Point<f64> {
    Point::new(top_left.x, primary_height - top_left.y)
}

/// The frame `setFrame:display:` takes to give a window the outer rectangle
/// that starts at `top_left` and measures `size`.
pub(crate) fn cocoa_frame(
    top_left: Point<f64>,
    size: Size<f64>,
    primary_height: f64,
) -> Bounds<f64> {
    Bounds {
        origin: Point::new(top_left.x, primary_height - top_left.y - size.height),
        size,
    }
}

/// The frame that gives a window the outer size `size` while its top edge stays
/// where it is: AppKit keeps a frame's bottom-left corner, so a taller or
/// shorter window has to move to keep its top.
pub(crate) fn cocoa_frame_keeping_top(current: Bounds<f64>, size: Size<f64>) -> Bounds<f64> {
    Bounds {
        origin: Point::new(
            current.origin.x,
            current.origin.y + current.size.height - size.height,
        ),
        size,
    }
}

/// A screen's frame in the display snapshot's terms: device pixels of that
/// screen, measured from the top-left of the primary screen.
pub(crate) fn device_bounds_of_screen(
    frame: Bounds<f64>,
    primary_height: f64,
    scale: f64,
) -> Bounds<i32> {
    let top = primary_height - (frame.origin.y + frame.size.height);
    let device = |points: f64| (points * scale).round() as i32;
    Bounds {
        origin: Point::new(device(frame.origin.x), device(top)),
        size: Size::new(device(frame.size.width), device(frame.size.height)),
    }
}

/// A position or size measured on `display` (from the origin of its usable
/// area, in its logical pixels), as global logical pixels. With no display the
/// point already is global.
pub(crate) fn globalize(at: Point<f64>, display: Option<&DisplayInfo>) -> Point<f64> {
    let Some(display) = display else {
        return at;
    };
    let scale = display.scale_factor;
    if !scale.is_finite() || scale <= 0.0 {
        return at;
    }
    let origin = display.usable_bounds.origin;
    Point::new(
        f64::from(origin.x) / scale + at.x,
        f64::from(origin.y) / scale + at.y,
    )
}

/// The inner-size limits a window reports through `contentMinSize` and
/// `contentMaxSize`.
///
/// AppKit's "no limit" is a zero minimum and a huge maximum. A limit on one
/// axis only keeps the other axis at `f64::MAX`, which is what writing it back
/// asks AppKit for, so the pair round-trips.
pub(crate) fn constraints_of_content_limits(min: Size<f64>, max: Size<f64>) -> SizeConstraints {
    let floor = |value: f64| {
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    };
    let ceiling = |value: f64| {
        if value.is_finite() && value < UNLIMITED_FROM {
            value
        } else {
            f64::MAX
        }
    };
    let min = Size::new(floor(min.width), floor(min.height));
    let max = Size::new(ceiling(max.width), ceiling(max.height));
    SizeConstraints::new(
        (min.width > 0.0 || min.height > 0.0).then_some(min),
        (max.width < f64::MAX || max.height < f64::MAX).then_some(max),
    )
}

/// The `contentMinSize` and `contentMaxSize` that give a window `constraints`.
pub(crate) fn content_limits_of_constraints(
    constraints: SizeConstraints,
) -> (Size<f64>, Size<f64>) {
    (
        constraints.min.unwrap_or(Size::new(0.0, 0.0)),
        constraints.max.unwrap_or(Size::new(f64::MAX, f64::MAX)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: f64 = 900.0;

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Bounds<f64> {
        Bounds {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    #[test]
    fn a_frame_becomes_an_outer_top_left() {
        // (what the frame is, its cocoa rectangle, the top-left it publishes)
        let rows = [
            (
                "at rest on the primary screen",
                frame(100.0, 300.0, 800.0, 500.0),
                Some((100.0, 100.0)),
            ),
            (
                "flush with the top of the primary screen",
                frame(0.0, 400.0, 800.0, 500.0),
                Some((0.0, 0.0)),
            ),
            (
                "above the primary screen",
                frame(0.0, 1000.0, 800.0, 500.0),
                Some((0.0, -600.0)),
            ),
            (
                "on a screen left of the primary",
                frame(-1920.0, 200.0, 800.0, 500.0),
                Some((-1920.0, 200.0)),
            ),
            (
                "taller than the screen",
                frame(0.0, -100.0, 800.0, 1200.0),
                Some((0.0, -200.0)),
            ),
            (
                "a position that is not a number",
                frame(f64::NAN, 0.0, 800.0, 500.0),
                None,
            ),
            (
                "an infinite extent",
                frame(0.0, 0.0, f64::INFINITY, 500.0),
                None,
            ),
        ];
        for (name, input, expected) in rows {
            let published = top_left_of_frame(input, PRIMARY).map(|at| (at.x, at.y));
            assert_eq!(published, expected, "{name}");
        }
    }

    #[test]
    fn a_top_left_and_a_cocoa_frame_are_each_others_inverse() {
        let size = Size::new(800.0, 500.0);
        for (x, y) in [(0.0, 0.0), (100.0, 100.0), (-1920.0, 200.0), (50.0, -600.0)] {
            let top_left = Point::new(x, y);
            let frame = cocoa_frame(top_left, size, PRIMARY);
            assert_eq!(frame.size, size);
            assert_eq!(
                top_left_of_frame(frame, PRIMARY),
                Some(top_left),
                "({x}, {y})"
            );
            // `setFrameTopLeftPoint:` names the corner of that same frame.
            let point = cocoa_top_left_point(top_left, PRIMARY);
            assert_eq!(point.x, frame.origin.x);
            assert_eq!(point.y, frame.origin.y + frame.size.height);
        }
    }

    #[test]
    fn a_resized_frame_keeps_its_top_edge() {
        let current = frame(100.0, 300.0, 800.0, 500.0);
        // (new height, expected bottom-left y)
        let rows = [(300.0, 500.0), (500.0, 300.0), (700.0, 100.0)];
        for (height, y) in rows {
            let resized = cocoa_frame_keeping_top(current, Size::new(640.0, height));
            assert_eq!(resized.origin, Point::new(100.0, y), "height {height}");
            assert_eq!(
                resized.origin.y + resized.size.height,
                800.0,
                "the top edge"
            );
        }
    }

    #[test]
    fn a_screen_is_published_in_device_pixels_from_the_top_left() {
        // (name, cocoa frame, scale, device bounds as (x, y, width, height))
        let rows = [
            (
                "the primary screen at 2x",
                frame(0.0, 0.0, 1440.0, 900.0),
                2.0,
                (0, 0, 2880, 1800),
            ),
            (
                "a 1x screen above the primary",
                frame(0.0, 900.0, 1920.0, 1080.0),
                1.0,
                (0, -1080, 1920, 1080),
            ),
            (
                "a 2x screen right of the primary",
                frame(1440.0, 0.0, 1280.0, 720.0),
                2.0,
                (2880, 360, 2560, 1440),
            ),
        ];
        for (name, input, scale, (x, y, width, height)) in rows {
            let bounds = device_bounds_of_screen(input, PRIMARY, scale);
            assert_eq!(
                (
                    bounds.origin.x,
                    bounds.origin.y,
                    bounds.size.width,
                    bounds.size.height
                ),
                (x, y, width, height),
                "{name}"
            );
        }
    }

    fn display(scale: f64, usable_origin: (i32, i32)) -> DisplayInfo {
        use flui_platform::{DisplayId, DisplayKey, DisplayRegistry};

        let id: DisplayId = DisplayRegistry::new()
            .mint(&DisplayKey::MacosUuid([1; 16]))
            .expect("a fresh registry has ids");
        let mut info = DisplayInfo::new(
            id,
            "test".to_string(),
            Bounds {
                origin: Point::new(usable_origin.0, usable_origin.1),
                size: Size::new(2880, 1800),
            },
            scale,
        );
        info.usable_bounds = Bounds {
            origin: Point::new(usable_origin.0, usable_origin.1),
            size: Size::new(2880, 1750),
        };
        info
    }

    #[test]
    fn a_position_on_a_display_becomes_a_global_one() {
        let at = Point::new(40.0, 30.0);
        let secondary = display(2.0, (2880, 50));
        let unscaled = display(0.0, (2880, 50));
        // (name, display, expected global position)
        let rows = [
            ("no display: already global", None, (40.0, 30.0)),
            (
                "a 2x display starting at device x 2880",
                Some(&secondary),
                (1480.0, 55.0),
            ),
            (
                "a display whose scale is unusable is not trusted",
                Some(&unscaled),
                (40.0, 30.0),
            ),
        ];
        for (name, display, (x, y)) in rows {
            let global = globalize(at, display);
            assert_eq!((global.x, global.y), (x, y), "{name}");
        }
    }

    #[test]
    fn content_limits_become_constraints_and_back() {
        let none = (Size::new(0.0, 0.0), Size::new(f64::MAX, f64::MAX));
        let default_max = Size::new(f64::from(f32::MAX), f64::from(f32::MAX));
        // (name, appkit min, appkit max, constraints)
        let rows = [
            ("appkit's defaults", none.0, none.1, SizeConstraints::NONE),
            (
                "the older FLT_MAX default",
                none.0,
                default_max,
                SizeConstraints::NONE,
            ),
            (
                "a minimum only",
                Size::new(320.0, 240.0),
                none.1,
                SizeConstraints::new(Some(Size::new(320.0, 240.0)), None),
            ),
            (
                "a maximum only",
                none.0,
                Size::new(1600.0, 1200.0),
                SizeConstraints::new(None, Some(Size::new(1600.0, 1200.0))),
            ),
            (
                "both",
                Size::new(320.0, 240.0),
                Size::new(1600.0, 1200.0),
                SizeConstraints::new(
                    Some(Size::new(320.0, 240.0)),
                    Some(Size::new(1600.0, 1200.0)),
                ),
            ),
            (
                "a limit on the width alone",
                Size::new(0.0, 0.0),
                Size::new(1600.0, f64::MAX),
                SizeConstraints::new(None, Some(Size::new(1600.0, f64::MAX))),
            ),
            (
                "numbers that are not numbers mean no limit",
                Size::new(f64::NAN, -5.0),
                Size::new(f64::NAN, f64::INFINITY),
                SizeConstraints::NONE,
            ),
        ];
        for (name, min, max, expected) in rows {
            let read = constraints_of_content_limits(min, max);
            assert_eq!(read, expected, "{name}");
            let (min, max) = content_limits_of_constraints(read);
            assert_eq!(
                constraints_of_content_limits(min, max),
                read,
                "{name}: round trip"
            );
        }
    }
}
