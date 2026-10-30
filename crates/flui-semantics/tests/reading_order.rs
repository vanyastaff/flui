//! `reading_order`: lines from the top, each read in the document's direction.

use flui_foundation::geometry::Rect;
use flui_semantics::{TextDirection, reading_order};

fn ltwh(left: f64, top: f64, width: f64, height: f64) -> Rect {
    Rect::from_ltwh(left, top, width, height)
}

/// Each case is the boxes as given, the direction and the order read.
///
/// Red-check: group lines by top edge alone (the mixed-size rows split).
#[test]
fn reading_order_reads_lines_from_the_top_in_the_direction() {
    let cases: [(&str, Vec<Rect>, TextDirection, Vec<usize>); 9] = [
        (
            "a line, then the line below",
            vec![
                ltwh(0.0, 30.0, 50.0, 20.0),
                ltwh(60.0, 0.0, 50.0, 20.0),
                ltwh(0.0, 0.0, 50.0, 20.0),
            ],
            TextDirection::Ltr,
            vec![2, 1, 0],
        ),
        (
            "right to left along each line",
            vec![
                ltwh(0.0, 30.0, 50.0, 20.0),
                ltwh(60.0, 0.0, 50.0, 20.0),
                ltwh(0.0, 0.0, 50.0, 20.0),
            ],
            TextDirection::Rtl,
            vec![1, 2, 0],
        ),
        (
            "a small text aligned to a tall one's bottom shares its line",
            vec![ltwh(60.0, 23.0, 40.0, 17.0), ltwh(0.0, 0.0, 50.0, 40.0)],
            TextDirection::Ltr,
            vec![1, 0],
        ),
        (
            "a small text aligned to a tall one's top shares its line",
            vec![ltwh(60.0, 0.0, 40.0, 17.0), ltwh(0.0, 0.0, 50.0, 40.0)],
            TextDirection::Ltr,
            vec![1, 0],
        ),
        (
            "a centre just inside the band joins the line",
            vec![ltwh(100.0, 0.0, 50.0, 40.0), ltwh(0.0, 18.0, 50.0, 24.0)],
            TextDirection::Ltr,
            vec![1, 0],
        ),
        (
            "a centre just past the band starts a line",
            vec![ltwh(100.0, 0.0, 50.0, 40.0), ltwh(0.0, 38.0, 50.0, 22.0)],
            TextDirection::Ltr,
            vec![0, 1],
        ),
        (
            "overlapping steps stay separate lines",
            vec![
                ltwh(0.0, 30.0, 50.0, 20.0),
                ltwh(0.0, 15.0, 50.0, 20.0),
                ltwh(0.0, 0.0, 50.0, 20.0),
            ],
            TextDirection::Ltr,
            vec![2, 1, 0],
        ),
        (
            "stacked texts read in the order given",
            vec![ltwh(0.0, 0.0, 50.0, 20.0), ltwh(0.0, 0.0, 50.0, 20.0)],
            TextDirection::Rtl,
            vec![0, 1],
        ),
        (
            "a box with no place reads last",
            vec![ltwh(f64::NAN, 0.0, 50.0, 20.0), ltwh(0.0, 40.0, 50.0, 20.0)],
            TextDirection::Ltr,
            vec![1, 0],
        ),
    ];
    for (name, boxes, direction, expected) in cases {
        assert_eq!(reading_order(&boxes, direction), expected, "{name}");
    }
}
