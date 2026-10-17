//! Path geometry: length, trim, transform and lerp, read through the public `Path` API.

use std::f64::consts::{FRAC_PI_2, PI};

use flui_foundation::geometry::{Matrix4, Offset, Point, Rect};
use flui_painting::paint::{Path, PathCommand, PathFillType};

fn close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: {actual}, expected {expected}"
    );
}

/// A 100 x 100 square traced clockwise from the origin (y-down).
fn square() -> Path {
    Path::rectangle(Rect::from_ltrb(0.0, 0.0, 100.0, 100.0))
}

fn commands(path: &Path) -> Vec<PathCommand> {
    path.commands().collect()
}

fn contours(path: &Path) -> usize {
    path.commands()
        .filter(|command| matches!(command, PathCommand::MoveTo(_)))
        .count()
}

fn endpoints(path: &Path) -> (Point<f64>, Point<f64>) {
    let mut first = None;
    let mut last = None;
    for command in path.commands() {
        let point = match command {
            PathCommand::MoveTo(p)
            | PathCommand::LineTo(p)
            | PathCommand::QuadraticTo(_, p)
            | PathCommand::CubicTo(_, _, p) => p,
            PathCommand::Close => continue,
        };
        first.get_or_insert(point);
        last = Some(point);
    }
    (first.expect("a point"), last.expect("a point"))
}

fn triangle(offset: f64) -> Path {
    Path::polygon(&[
        Point::new(offset, 0.0),
        Point::new(offset + 10.0, 0.0),
        Point::new(offset, 10.0),
    ])
}

/// A closed square counts its closing edge; an open polyline does not; a circle is its
/// circumference.
pub(crate) fn length_traces_what_a_stroke_draws() {
    close(square().length(), 400.0, 1e-9, "square");
    let mut open = Path::new();
    open.move_to(Point::new(0.0, 0.0));
    open.line_to(Point::new(30.0, 0.0));
    open.line_to(Point::new(30.0, 40.0));
    close(open.length(), 70.0, 1e-9, "open polyline");
    let circle = Path::circle(Point::new(0.0, 0.0), 50.0);
    close(circle.length(), 2.0 * PI * 50.0, 1e-2, "circle");
    close(Path::new().length(), 0.0, 0.0, "empty");
}

/// The first half of the square runs along the top and right edges to the far corner.
pub(crate) fn a_half_trim_keeps_the_first_half() {
    let half = square().trim(0.0, 0.5);
    close(half.length(), 200.0, 1e-6, "half length");
    let (start, end) = endpoints(&half);
    assert_eq!(start, Point::new(0.0, 0.0));
    close(end.x, 100.0, 1e-6, "end x");
    close(end.y, 100.0, 1e-6, "end y");

    let curved = Path::circle(Point::new(0.0, 0.0), 50.0).trim(0.25, 0.5);
    close(
        curved.length(),
        PI * 50.0 / 2.0,
        1e-2,
        "a quarter of the circle",
    );
}

/// A window past the end wraps through the start: the left edge, then the top edge.
pub(crate) fn a_trim_with_start_after_end_wraps_through_the_start() {
    let wrapped = square().trim(0.75, 0.25);
    close(wrapped.length(), 200.0, 1e-6, "wrapped length");
    assert_eq!(contours(&wrapped), 2, "{:?}", commands(&wrapped));
    let (start, end) = endpoints(&wrapped);
    assert_eq!(
        start,
        Point::new(0.0, 100.0),
        "starts at the bottom-left corner"
    );
    close(end.x, 100.0, 1e-6, "ends at the top-right corner x");
    close(end.y, 0.0, 1e-6, "ends at the top-right corner y");
}

/// A window over two contours keeps a piece of each, as separate contours.
pub(crate) fn a_trim_across_contours_keeps_each_piece_apart() {
    let mut two = square();
    two.add_rect(Rect::from_ltrb(200.0, 0.0, 300.0, 100.0));
    let middle = two.trim(0.25, 0.75);
    close(middle.length(), 400.0, 1e-6, "half of 800");
    assert_eq!(contours(&middle), 2, "{:?}", commands(&middle));

    // Wrapping measures the concatenated length too: the tail of the second square, then
    // the head of the first.
    let wrapped = two.trim(0.875, 0.125);
    close(wrapped.length(), 200.0, 1e-6, "a quarter of 800");
    assert_eq!(contours(&wrapped), 2, "{:?}", commands(&wrapped));
    let (start, end) = endpoints(&wrapped);
    assert_eq!(
        start,
        Point::new(200.0, 100.0),
        "starts on the second square's left edge"
    );
    close(end.x, 100.0, 1e-6, "ends at the first square's top-right x");
    close(end.y, 0.0, 1e-6, "ends at the first square's top-right y");

    // A zero-length contour between them contributes nothing.
    let mut with_dot = square();
    with_dot.move_to(Point::new(150.0, 50.0));
    with_dot.line_to(Point::new(150.0, 50.0));
    with_dot.add_rect(Rect::from_ltrb(200.0, 0.0, 300.0, 100.0));
    let middle = with_dot.trim(0.25, 0.75);
    close(middle.length(), 400.0, 1e-6, "the dot adds no length");
    assert_eq!(
        contours(&middle),
        2,
        "the dot is not a piece: {:?}",
        commands(&middle)
    );
}

/// The full window is the path itself; degenerate windows and paths give an empty path;
/// fractions outside the unit range are clamped; the fill type survives.
pub(crate) fn degenerate_trims_are_empty_and_finite() {
    let path = square();
    assert!(path.trim(0.0, 1.0).shares_commands_with(&path));
    assert!(path.trim(0.4, 0.4).is_empty(), "equal fractions");
    assert!(path.trim(f64::NAN, 0.5).is_empty(), "NaN start");
    assert!(path.trim(0.0, f64::INFINITY).is_empty(), "infinite end");
    assert!(Path::new().trim(0.2, 0.8).is_empty(), "empty path");
    let mut point = Path::new();
    point.move_to(Point::new(5.0, 5.0));
    point.line_to(Point::new(5.0, 5.0));
    assert!(point.trim(0.2, 0.8).is_empty(), "a zero-length path");
    close(path.trim(-1.0, 0.5).length(), 200.0, 1e-6, "clamped start");

    let mut even_odd = square();
    even_odd.set_fill_type(PathFillType::EvenOdd);
    assert_eq!(even_odd.trim(0.1, 0.6).fill_type(), PathFillType::EvenOdd);
}

/// Scaling scales the length, rotating keeps it, translating matches `translate`;
/// perspective and non-finite matrices are refused.
pub(crate) fn transform_maps_the_curves_and_refuses_perspective() {
    let circle = Path::circle(Point::new(0.0, 0.0), 10.0);
    let doubled = circle
        .transform(&Matrix4::scaling(2.0, 2.0, 1.0))
        .expect("an affine scale");
    close(
        doubled.length(),
        2.0 * circle.length(),
        1e-6,
        "scaled length",
    );

    let rotated = square()
        .transform(&Matrix4::rotation_z(FRAC_PI_2))
        .expect("an affine rotation");
    close(rotated.length(), 400.0, 1e-6, "rotated length");

    let moved = square()
        .transform(&Matrix4::translation(7.0, -3.0, 0.0))
        .expect("an affine translation");
    assert_eq!(
        commands(&moved),
        commands(&square().translate(Offset::new(7.0, -3.0)))
    );

    let mut perspective = Matrix4::IDENTITY;
    perspective.m[3] = 0.01;
    assert!(square().transform(&perspective).is_none(), "perspective");
    let mut broken = Matrix4::IDENTITY;
    broken.m[0] = f64::NAN;
    assert!(square().transform(&broken).is_none(), "non-finite");
    let mut pole = Matrix4::IDENTITY;
    pole.m[15] = 0.0;
    assert!(
        square().transform(&pole).is_none(),
        "w = 0 has no finite image"
    );
}

/// Compatible paths interpolate point by point with exact ends; incompatible ones, a
/// non-finite `t` and an overflowing result give `None`.
pub(crate) fn lerp_morphs_compatible_paths_only() {
    let (a, b) = (triangle(0.0), triangle(20.0));
    let at = |t: f64| Path::lerp(&a, &b, t).expect("compatible");
    assert_eq!(commands(&at(0.0)), commands(&a), "exact at 0");
    assert_eq!(commands(&at(1.0)), commands(&b), "exact at 1");
    assert_eq!(
        commands(&at(0.5)),
        commands(&triangle(10.0)),
        "between at 0.5"
    );
    assert_eq!(
        commands(&at(1.5)),
        commands(&triangle(30.0)),
        "overshoot extrapolates"
    );

    let mut curved = Path::new();
    curved.move_to(Point::new(0.0, 0.0));
    curved.cubic_to(
        Point::new(1.0, 1.0),
        Point::new(2.0, 1.0),
        Point::new(10.0, 0.0),
    );
    curved.line_to(Point::new(0.0, 10.0));
    curved.close();
    assert!(Path::lerp(&a, &curved, 0.5).is_none(), "different kinds");
    assert!(
        Path::lerp(&a, &square(), 0.5).is_none(),
        "different lengths"
    );
    let mut even_odd = triangle(20.0);
    even_odd.set_fill_type(PathFillType::EvenOdd);
    assert!(Path::lerp(&a, &even_odd, 0.5).is_none(), "different fill");
    assert!(Path::lerp(&a, &b, f64::NAN).is_none(), "NaN t");
    let far = Path::polygon(&[
        Point::new(1e300, 0.0),
        Point::new(1e300, 1.0),
        Point::new(0.0, 1.0),
    ]);
    assert!(
        Path::lerp(&a, &far, 1e10).is_none(),
        "an overflowing result is refused"
    );
}
