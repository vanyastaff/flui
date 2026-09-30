//! Paint and text value contracts: CSS weight mapping, radial-gradient focal
//! interpolation and how an arc joins a path.

use flui_foundation::geometry::{Point, RRect, Rect};
use flui_painting::Alignment;
use flui_painting::paint::{Path, PathCommand};
use flui_painting::styling::{Color, RadialGradient, TileMode};
use flui_painting::typography::FontWeight;

/// A CSS weight snaps to the closest hundred, and an exact half goes the way
/// CSS font matching searches: lighter below 400, heavier from 400 up.
pub(crate) fn font_weight_from_css_breaks_ties_like_css() {
    for (css, expected) in [
        (150, FontWeight::W100),
        (250, FontWeight::W200),
        (350, FontWeight::W300),
        (351, FontWeight::W400),
        (449, FontWeight::W400),
        (450, FontWeight::W500),
        (650, FontWeight::W700),
        (750, FontWeight::W800),
        (-5, FontWeight::W100),
        (1000, FontWeight::W900),
    ] {
        assert_eq!(FontWeight::from_css(css), expected, "css weight {css}");
    }
}

fn radial(center: Alignment, focal: Option<Alignment>) -> RadialGradient {
    RadialGradient::new(
        center,
        0.5,
        vec![Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)],
        None,
        TileMode::Clamp,
        focal,
        None,
    )
}

/// A gradient without a focal point is focused on its center, so a focal
/// point set on one side only moves to or from the other side's center and
/// `t = 1` paints exactly like the other gradient, with no jump at the end.
pub(crate) fn a_one_sided_focal_point_lerps_to_the_other_center() {
    let focused = radial(Alignment::CENTER, Some(Alignment::TOP_LEFT));
    let unfocused = radial(Alignment::CENTER_RIGHT, None);
    let focal = |a: &RadialGradient, b: &RadialGradient, t: f64| {
        RadialGradient::lerp(a, b, t)
            .expect("BUG: gradients with two colours each always lerp")
            .focal
    };

    assert_eq!(
        focal(&focused, &unfocused, 0.5),
        Some(Alignment::new(0.0, -0.5))
    );
    assert_eq!(
        focal(&focused, &unfocused, 1.0),
        Some(Alignment::CENTER_RIGHT)
    );
    assert_eq!(
        focal(&unfocused, &focused, 0.0),
        Some(Alignment::CENTER_RIGHT)
    );
}

fn move_tos(path: &Path) -> usize {
    path.commands()
        .filter(|command| matches!(command, PathCommand::MoveTo(_)))
        .count()
}

/// An arc joins an open contour with a line from the pen to its start, and
/// starts a contour of its own only when none is open; a rounded rectangle is
/// therefore one contour.
pub(crate) fn an_arc_joins_an_open_contour_and_starts_a_closed_one_fresh() {
    let oval = Rect::from_ltrb(10.0, 0.0, 30.0, 20.0);

    let mut open = Path::new();
    open.move_to(Point::new(0.0, 0.0));
    open.line_to(Point::new(10.0, 0.0));
    open.add_arc(oval, 0.0, std::f64::consts::FRAC_PI_2);
    assert_eq!(move_tos(&open), 1);
    assert_eq!(
        open.commands().nth(2),
        Some(PathCommand::LineTo(Point::new(30.0, 10.0)))
    );

    let mut closed = Path::new();
    closed.move_to(Point::new(0.0, 0.0));
    closed.line_to(Point::new(10.0, 0.0));
    closed.close();
    closed.add_arc(oval, 0.0, std::f64::consts::FRAC_PI_2);
    assert_eq!(move_tos(&closed), 2);

    let rrect = Path::from_rrect(RRect::from_rect_xy(
        Rect::from_ltrb(0.0, 0.0, 40.0, 20.0),
        4.0,
        4.0,
    ));
    assert_eq!(move_tos(&rrect), 1);
}
