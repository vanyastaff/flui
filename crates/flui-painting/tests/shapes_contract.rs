//! Rounded polygons, their morph and the Material shapes, read through the public API.
//!
//! The oracle is androidx.graphics.shapes at `androidx/androidx` commit
//! `ba20691fc7e50a58b9514d8d49bff8684cb0fa0e` (`RoundedPolygon.kt`, `Morph.kt`) and Material's
//! shape parameters at commit `080d2b3e5326ba80392d93442c4a51a02dc22650`
//! (`compose/material3/.../MaterialShapes.kt`): the cubic counts, the normalized circle's
//! extent and radius, and the symmetries below are that implementation's values, derived from
//! its algorithm at those revisions.

use std::f64::consts::{FRAC_PI_4, TAU};

use flui_foundation::geometry::{Point, Rect};
use flui_painting::paint::shapes::material_shapes;
use flui_painting::paint::{CornerRounding, Morph, Path, PathCommand, RoundedPolygon, ShapeError};

fn close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: {actual}, expected {expected}"
    );
}

/// The start point and the cubics `(c1, c2, end)` of a closed one-contour path of cubics.
fn cubics(path: &Path) -> (Point<f64>, Vec<[Point<f64>; 3]>) {
    let mut start = None;
    let mut cubics = Vec::new();
    let mut closed = false;
    for command in path.commands() {
        match command {
            PathCommand::MoveTo(p) => {
                assert!(start.is_none(), "one contour");
                start = Some(p);
            }
            PathCommand::CubicTo(c1, c2, p) => cubics.push([c1, c2, p]),
            PathCommand::Close => closed = true,
            other => panic!("unexpected command {other:?}"),
        }
    }
    let start = start.expect("a contour");
    assert!(closed, "the outline is closed");
    assert_eq!(
        cubics.last().expect("a cubic")[2],
        start,
        "the outline ends where it starts"
    );
    (start, cubics)
}

fn anchors(path: &Path) -> Vec<Point<f64>> {
    cubics(path).1.iter().map(|c| c[2]).collect()
}

/// The enclosed area of a closed path of cubics, positive when it turns from +x toward +y.
fn area(path: &Path) -> f64 {
    let (mut p0, cubics) = cubics(path);
    let mut sum = 0.0;
    for [p1, p2, p3] in cubics {
        sum += (p0.x * (6.0 * p1.y + 3.0 * p2.y + p3.y)
            + 3.0 * (p1.x * (-2.0 * p0.y + p2.y + p3.y) - p2.x * (p0.y + p1.y - 2.0 * p3.y))
            - p3.x * (p0.y + 3.0 * p1.y + 6.0 * p2.y))
            / 20.0;
        p0 = p3;
    }
    sum
}

fn material() -> [(&'static str, RoundedPolygon); 8] {
    [
        ("circle", material_shapes::circle()),
        ("oval", material_shapes::oval()),
        ("sunny", material_shapes::sunny()),
        ("cookie_9", material_shapes::cookie_9()),
        ("cookie_4", material_shapes::cookie_4()),
        ("pentagon", material_shapes::pentagon()),
        ("pill", material_shapes::pill()),
        ("soft_burst", material_shapes::soft_burst()),
    ]
}

fn p(x: f64, y: f64) -> Point<f64> {
    Point::new(x, y)
}

fn round(radius: f64) -> CornerRounding {
    CornerRounding::circular(radius).expect("a valid radius")
}

// The oracle's cubic counts: one per arc (three with smoothing), one per edge a corner did
// not consume, plus one when the outline starts in the middle of the first corner's arc.
fn material_cubic_counts() {
    let expected = [
        ("circle", 11),
        ("oval", 9),
        ("sunny", 33),
        ("cookie_9", 19),
        ("cookie_4", 17),
        ("pentagon", 11),
        ("pill", 13),
        ("soft_burst", 41),
    ];
    for ((name, shape), (_, count)) in material().iter().zip(expected) {
        assert_eq!(cubics(&shape.path()).1.len(), count, "{name}");
    }
}

// Normalized: the control-point box fits the unit square with the longer side spanning it;
// the shapes symmetric about both diagonals or four-fold fill it both ways.
fn material_shapes_fill_the_unit_square() {
    for (name, shape) in material() {
        let bounds = shape.bounds();
        let (w, h) = (bounds.width(), bounds.height());
        close(w.max(h), 1.0, 1e-12, name);
        assert!(
            bounds.left() >= -1e-12 && bounds.top() >= -1e-12,
            "{name}: {bounds:?}"
        );
        assert!(
            bounds.right() <= 1.0 + 1e-12 && bounds.bottom() <= 1.0 + 1e-12,
            "{name}"
        );
        if matches!(name, "oval" | "sunny" | "cookie_4" | "pill") {
            close(w.min(h), 1.0, 1e-9, name);
        }
    }
}

// The circle: ten 36° arcs of radius 1 / 2.032630, the box wider along x (a vertex
// direction) than along y (a side's middle), the outline starting on +x.
fn material_circle_matches_the_oracle() {
    let circle = material_shapes::circle();
    let bounds = circle.bounds();
    // Before fitting, the box is 2 tall (arcs meet tangent to y = ±1 at the sides' middles)
    // and 2 · 1.016315 wide (the arc around the vertex on +x has its control points at
    // cos 18° + sin 18° · 4/3 · tan 9°).
    close(bounds.top(), 0.008_026, 1e-6, "top");
    close(bounds.bottom(), 0.991_974, 1e-6, "bottom");
    close(bounds.left(), 0.0, 1e-12, "left");
    close(bounds.right(), 1.0, 1e-12, "right");
    let (start, _) = cubics(&circle.path());
    // The outline starts in the middle of the arc around the vertex on +x.
    close(start.x, 0.5 + 0.491_973, 1e-6, "start x");
    close(start.y, 0.5, 1e-12, "start y");
    for anchor in anchors(&circle.path()) {
        let r = (anchor.x - 0.5).hypot(anchor.y - 0.5);
        close(r, 0.491_973, 1e-6, "anchor radius");
    }
}

// Each star-like shape maps onto itself turned by one repetition about its centre, and the
// mirrored pentagon onto itself reflected about x = 0.5.
fn material_shapes_are_symmetric() {
    // The last anchor is the start, the middle of the first corner's arc, which only that
    // corner has; the rest are where arcs and edges meet, at every corner alike.
    let arc_ends = |points: &[Point<f64>]| points[..points.len() - 1].to_vec();
    let turned = |name: &str, shape: &RoundedPolygon, angle: f64| {
        let points = anchors(&shape.path());
        let (sin, cos) = angle.sin_cos();
        // An odd count of lobes is not centred in its box; the anchors' mean is the centre.
        let ends = arc_ends(&points);
        let n = ends.len() as f64;
        let cx = ends.iter().map(|q| q.x).sum::<f64>() / n;
        let cy = ends.iter().map(|q| q.y).sum::<f64>() / n;
        for q in ends {
            let (x, y) = (q.x - cx, q.y - cy);
            let image = p(cx + x * cos - y * sin, cy + x * sin + y * cos);
            let nearest = points
                .iter()
                .map(|r| (r.x - image.x).hypot(r.y - image.y))
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest < 1e-9,
                "{name}: {q:?} turned has no partner ({nearest})"
            );
        }
    };
    turned("sunny", &material_shapes::sunny(), TAU / 8.0);
    turned("cookie_9", &material_shapes::cookie_9(), TAU / 9.0);
    turned("cookie_4", &material_shapes::cookie_4(), TAU / 4.0);
    turned("soft_burst", &material_shapes::soft_burst(), TAU / 10.0);
    turned("pill", &material_shapes::pill(), TAU / 2.0);

    let points = anchors(&material_shapes::pentagon().path());
    for q in arc_ends(&points) {
        let nearest = points
            .iter()
            .map(|r| (r.x - (1.0 - q.x)).hypot(r.y - q.y))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 1e-9, "pentagon: {q:?} mirrored has no partner");
    }
}

// A 2 x 2 square with corners of radius 0.5: arcs centred at (±0.5, ±0.5), the outline
// starting in the middle of the first corner's arc; with full smoothing each corner takes
// the whole half-edge and the edges vanish.
fn rectangle_rounding_by_hand() {
    let square = Rect::from_ltrb(-1.0, -1.0, 1.0, 1.0);
    let plain = RoundedPolygon::rectangle(square, round(0.5)).expect("valid");
    let (start, list) = cubics(&plain.path());
    assert_eq!(list.len(), 9, "four arcs, four edges, the first arc split");
    let mid = 0.5 + 0.5 * FRAC_PI_4.cos();
    close(start.x, mid, 1e-12, "start x");
    close(start.y, mid, 1e-12, "start y");
    close(
        area(&plain.path()),
        4.0 - (4.0 - std::f64::consts::PI) * 0.25,
        1e-3,
        "area",
    );

    let smooth = CornerRounding::try_new(0.5, 1.0).expect("valid");
    let smoothed = RoundedPolygon::rectangle(square, smooth).expect("valid");
    // Full smoothing leaves no arc: two flanking curves per corner, meeting in its middle.
    assert_eq!(
        cubics(&smoothed.path()).1.len(),
        8,
        "two curves per corner, no edges"
    );
    assert!(area(&smoothed.path()) < 4.0);
}

// The outline turns from +x toward +y whatever order the vertices come in.
fn clockwise_input_runs_positive() {
    let ccw = [p(1.0, 0.0), p(0.0, 1.0), p(-1.0, 0.0), p(0.0, -1.0)];
    let cw = [ccw[0], ccw[3], ccw[2], ccw[1]];
    let a = RoundedPolygon::try_new(&ccw, round(0.2)).expect("valid");
    let b = RoundedPolygon::try_new(&cw, round(0.2)).expect("valid");
    assert_eq!(a.path(), b.path());
    assert!(area(&a.path()) > 0.0);
}

// Malformed and overflowing input is refused with the reason.
fn malformed_input_is_refused() {
    let triangle = [p(0.0, 0.0), p(1.0, 0.0), p(0.0, 1.0)];
    let none = CornerRounding::UNROUNDED;
    let cases: [(&str, Result<RoundedPolygon, ShapeError>, ShapeError); 15] = [
        (
            "two vertices",
            RoundedPolygon::try_new(&triangle[..2], none),
            ShapeError::TooFewVertices { count: 2 },
        ),
        (
            "NaN",
            RoundedPolygon::try_new(&[triangle[0], p(f64::NAN, 0.0), triangle[2]], none),
            ShapeError::NonFiniteVertex { index: 1 },
        ),
        (
            "duplicate neighbours",
            RoundedPolygon::try_new(&[triangle[0], triangle[1], triangle[1], triangle[2]], none),
            ShapeError::DuplicateVertex { index: 1 },
        ),
        (
            "collinear",
            RoundedPolygon::try_new(&[p(0.0, 0.0), p(1.0, 1.0), p(3.0, 3.0)], none),
            ShapeError::ZeroArea,
        ),
        (
            "edge length overflows",
            RoundedPolygon::try_new(&[p(f64::MAX, 0.0), p(-f64::MAX, 0.0), p(0.0, 1.0)], none),
            ShapeError::Overflow,
        ),
        (
            "rounding overflows",
            RoundedPolygon::try_new(&triangle, round(f64::MAX)),
            ShapeError::Overflow,
        ),
        (
            "rounding count",
            RoundedPolygon::with_roundings(&triangle, &[none, none]),
            ShapeError::RoundingCountMismatch {
                vertices: 3,
                roundings: 2,
            },
        ),
        (
            "negative radius",
            CornerRounding::circular(-1.0).and_then(|r| RoundedPolygon::try_new(&triangle, r)),
            ShapeError::InvalidRadius,
        ),
        (
            "smoothing above one",
            CornerRounding::try_new(0.1, 1.5).and_then(|r| RoundedPolygon::try_new(&triangle, r)),
            ShapeError::InvalidSmoothing,
        ),
        (
            "star inner radius not inside",
            RoundedPolygon::star(5, 1.0, 1.0, none, none),
            ShapeError::InvalidRadius,
        ),
        (
            "two-sided circle",
            RoundedPolygon::circle(1.0, 2),
            ShapeError::TooFewVertices { count: 2 },
        ),
        (
            "empty rectangle",
            RoundedPolygon::rectangle(Rect::from_ltrb(0.0, 0.0, 0.0, 1.0), none),
            ShapeError::ZeroArea,
        ),
        (
            "inverted rectangle",
            RoundedPolygon::rectangle(Rect::from_ltrb(1.0, 0.0, 0.0, 1.0), none),
            ShapeError::ZeroArea,
        ),
        (
            "circle radius overflows",
            RoundedPolygon::circle(f64::MAX, 3),
            ShapeError::Overflow,
        ),
        (
            "star vertex count overflows",
            RoundedPolygon::star(usize::MAX / 2 + 1, 1.0, 0.5, none, none),
            ShapeError::Overflow,
        ),
    ];
    for (name, result, error) in cases {
        assert_eq!(result.err(), Some(error), "{name}");
    }
}

// Thin, extreme-aspect and subnormal-scale polygons are polygons: they build, and a subnormal one still
// normalizes to the unit square with every coordinate finite.
fn extreme_but_valid_input_builds() {
    let thin = RoundedPolygon::try_new(
        &[p(0.0, 0.0), p(1.0, 0.0), p(1.0, 1e-13)],
        CornerRounding::UNROUNDED,
    );
    assert!(
        thin.is_ok_and(|shape| area(&shape.path()) > 0.0),
        "a thin triangle"
    );
    // Scaled to unit size its height would underflow to zero; measured as given it does not.
    let sliver = RoundedPolygon::try_new(
        &[
            p(0.0, 0.0),
            p(f64::MAX, 0.0),
            p(f64::MAX, f64::MIN_POSITIVE),
        ],
        CornerRounding::UNROUNDED,
    );
    assert!(sliver.is_ok(), "an extreme-aspect triangle: {sliver:?}");

    let tiny = RoundedPolygon::try_new(
        &[p(0.0, 0.0), p(1e-310, 0.0), p(0.0, 1e-310)],
        CornerRounding::UNROUNDED,
    )
    .expect("a subnormal triangle");
    let unit = tiny.normalized();
    let bounds = unit.bounds();
    close(bounds.left(), 0.0, 1e-12, "left");
    close(bounds.top(), 0.0, 1e-12, "top");
    close(bounds.right(), 1.0, 1e-12, "right");
    close(bounds.bottom(), 1.0, 1e-12, "bottom");
    assert!(
        cubics(&unit.path())
            .1
            .iter()
            .flatten()
            .all(Point::is_finite)
    );
    let morph = Morph::new(&unit, &material_shapes::circle());
    assert!(
        morph.path(0.5).is_some(),
        "a subnormal shape morphs once normalized"
    );
    let raw = Morph::new(
        &tiny,
        &RoundedPolygon::circle(1e-310, 8).expect("a subnormal circle"),
    );
    assert!(
        raw.path(0.5).is_some(),
        "a subnormal shape morphs as it stands"
    );
}

// The morph's ends trace the two shapes, its middle lies between them, and its domain
// follows `Path::lerp`: overshoot allowed, a non-finite `t` or result refused.
fn morph_ends_middle_and_domain() {
    let square =
        RoundedPolygon::rectangle(Rect::from_ltrb(0.0, 0.0, 1.0, 1.0), round(0.05)).expect("valid");
    let circle = material_shapes::circle();
    let morph = Morph::new(&square, &circle);
    let (a, b) = (area(&square.path()), area(&circle.path()));

    let start = morph.path(0.0).expect("start");
    let end = morph.path(1.0).expect("end");
    close(area(&start), a, 1e-9, "start area");
    close(area(&end), b, 1e-9, "end area");
    let same_box = |a: Rect<f64>, b: Rect<f64>, what: &str| {
        let pairs = [
            (a.left(), b.left()),
            (a.top(), b.top()),
            (a.right(), b.right()),
            (a.bottom(), b.bottom()),
        ];
        for (x, y) in pairs {
            close(x, y, 1e-12 * y.abs().max(1.0), what);
        }
    };
    same_box(
        start.compute_bounds(),
        square.path().compute_bounds(),
        "start bounds",
    );
    same_box(
        end.compute_bounds(),
        circle.path().compute_bounds(),
        "end bounds",
    );

    let middle = area(&morph.path(0.5).expect("middle"));
    assert!(b < middle && middle < a, "{b} < {middle} < {a}");

    assert!(
        morph.path(1.5).is_some_and(|path| area(&path) < b),
        "overshoot keeps shrinking"
    );
    assert_eq!(morph.path(f64::NAN), None);
    assert_eq!(morph.path(f64::INFINITY), None);
    let big = Morph::new(
        &RoundedPolygon::circle(10.0, 8).expect("valid"),
        &RoundedPolygon::rectangle(Rect::from_ltrb(-10.0, -10.0, 10.0, 10.0), round(1.0))
            .expect("valid"),
    );
    assert!(big.path(2.0).is_some());
    assert_eq!(
        big.path(f64::MAX),
        None,
        "an overshoot past the finite range"
    );

    // Outlines whose perimeter overflows although every coordinate and the extent are finite
    // still morph: the measure is scale-free.
    let huge_square =
        RoundedPolygon::rectangle(Rect::from_ltrb(-8e307, -8e307, 8e307, 8e307), round(1e307))
            .expect("finite extent");
    let huge_circle = RoundedPolygon::circle(7e307, 8).expect("finite extent");
    let huge = Morph::new(&huge_square, &huge_circle);
    let middle = huge.path(0.5).expect("a finite middle");
    assert!(
        middle.compute_bounds().width() > 1.3e308,
        "{:?}",
        middle.compute_bounds()
    );
    same_box(
        huge.path(1.0).expect("end").compute_bounds(),
        huge_circle.path().compute_bounds(),
        "huge end",
    );

    let same = Morph::new(&circle, &circle);
    close(
        area(&same.path(0.5).expect("identity")),
        b,
        1e-9,
        "identity morph",
    );
}

// Matching pairs corners of the same convexity: a star morphing into its own copy turned
// by one point keeps its points pointed all the way through.
fn morph_matches_corners() {
    let star = |angle: f64| {
        let shape = RoundedPolygon::star(5, 1.0, 0.4, round(0.05), round(0.05)).expect("valid");
        let (sin, cos) = angle.sin_cos();
        let vertices: Vec<_> = (0..10)
            .map(|i| {
                let a = TAU * f64::from(i) / 10.0;
                let r = if i % 2 == 0 { 1.0 } else { 0.4 };
                let (x, y) = (r * a.cos(), r * a.sin());
                p(x * cos - y * sin, x * sin + y * cos)
            })
            .collect();
        let roundings = vec![round(0.05); 10];
        let turned = RoundedPolygon::with_roundings(&vertices, &roundings).expect("valid");
        if angle == 0.0 { shape } else { turned }
    };
    let morph = Morph::new(&star(0.0), &star(TAU / 5.0));
    let full = area(&star(0.0).path());
    for t in [0.25, 0.5, 0.75] {
        close(
            area(&morph.path(t).expect("frame")),
            full,
            1e-3 * full,
            "a matched star keeps its area",
        );
    }
}

#[test]
fn shapes_contract() {
    crate::cases::run_cases(
        "shapes_contract",
        &[
            ("material_cubic_counts", material_cubic_counts),
            (
                "material_shapes_fill_the_unit_square",
                material_shapes_fill_the_unit_square,
            ),
            (
                "material_circle_matches_the_oracle",
                material_circle_matches_the_oracle,
            ),
            (
                "material_shapes_are_symmetric",
                material_shapes_are_symmetric,
            ),
            ("rectangle_rounding_by_hand", rectangle_rounding_by_hand),
            (
                "clockwise_input_runs_positive",
                clockwise_input_runs_positive,
            ),
            ("malformed_input_is_refused", malformed_input_is_refused),
            (
                "extreme_but_valid_input_builds",
                extreme_but_valid_input_builds,
            ),
            ("morph_ends_middle_and_domain", morph_ends_middle_and_domain),
            ("morph_matches_corners", morph_matches_corners),
        ],
    );
}
