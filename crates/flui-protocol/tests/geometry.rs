//! `LogicalRect::approx_eq`, the tolerance a geometry claim states (ADR-0400 §5): each edge within
//! the tolerance, the boundary included, and a non-finite value close to nothing.

use flui_protocol::geometry::LogicalRect;

#[test]
fn a_rectangle_is_close_when_every_edge_is_within_the_tolerance() {
    let rect = LogicalRect::new(10.0, 20.0, 100.0, 50.0);
    let table: &[(&str, LogicalRect, f64, bool)] = &[
        ("itself, exactly", rect, 0.0, true),
        (
            "off by the tolerance on one edge: the boundary holds",
            LogicalRect::new(10.5, 20.0, 99.5, 50.0),
            0.5,
            true,
        ),
        (
            "past the tolerance on the left edge",
            LogicalRect::new(10.5 + 1e-9, 20.0, 99.5, 50.0),
            0.5,
            false,
        ),
        (
            "the right edge moved by a width change alone",
            LogicalRect::new(10.0, 20.0, 100.75, 50.0),
            0.5,
            false,
        ),
        (
            "the bottom edge moved by the top and the height together",
            LogicalRect::new(10.0, 20.25, 100.0, 50.25),
            0.25,
            false,
        ),
        (
            "every edge within, each by its own amount",
            LogicalRect::new(10.1, 19.9, 100.1, 50.2),
            0.25,
            true,
        ),
        (
            "a not-a-number edge",
            LogicalRect::new(f64::NAN, 20.0, 100.0, 50.0),
            1e9,
            false,
        ),
        (
            "an infinite size",
            LogicalRect::new(10.0, 20.0, f64::INFINITY, 50.0),
            f64::INFINITY,
            false,
        ),
        ("a not-a-number tolerance", rect, f64::NAN, false),
        ("a negative tolerance", rect, -1.0, false),
    ];
    for (case, other, tolerance, close) in table {
        assert_eq!(rect.approx_eq(other, *tolerance), *close, "{case}");
        assert_eq!(
            other.approx_eq(&rect, *tolerance),
            *close,
            "{case}, reversed"
        );
    }
}
