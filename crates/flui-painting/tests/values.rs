//! Paint and text value contracts: CSS weight mapping, radial-gradient focal
//! interpolation and how an arc joins a path.

use flui_foundation::geometry::{Point, RRect, Rect};
use flui_painting::Alignment;
use flui_painting::paint::{Path, PathCommand};
use flui_painting::styling::{
    Color, Gradient, LinearGradient, RadialGradient, SweepGradient, TileMode,
};
use flui_painting::typography::FontWeight;

fn healthy_gradient_like(gradient: &Gradient) -> Gradient {
    let colors = vec![Color::RED, Color::BLUE];
    match gradient {
        Gradient::Linear(_) => Gradient::Linear(LinearGradient::horizontal(colors)),
        Gradient::Radial(_) => Gradient::Radial(RadialGradient::circular(colors)),
        Gradient::Sweep(_) => Gradient::Sweep(SweepGradient::centered(colors)),
    }
}

fn rejects_invalid_gradient(bad: &Gradient) {
    let healthy = healthy_gradient_like(bad);
    assert!(Gradient::lerp(bad, &healthy, 0.5).is_none());
    assert!(Gradient::lerp(&healthy, bad, 0.5).is_none());
    assert!(
        Gradient::lerp(bad, bad, 0.5).is_none(),
        "equal invalid inputs"
    );
    let next =
        Gradient::lerp(&healthy, &healthy, 0.5).expect("valid interpolation after rejection");
    assert_eq!(next.colors(), &[Color::RED, Color::BLUE]);
}

pub(crate) fn linear_nan_stops_are_rejected() {
    let mut gradient = LinearGradient::horizontal(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![0.0, f64::NAN]);
    rejects_invalid_gradient(&Gradient::Linear(gradient));
}

pub(crate) fn radial_infinite_stops_are_rejected() {
    let mut gradient = RadialGradient::circular(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![0.0, f64::INFINITY]);
    rejects_invalid_gradient(&Gradient::Radial(gradient));
}

pub(crate) fn negative_linear_stops_are_rejected() {
    let mut gradient = LinearGradient::horizontal(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![-0.1, 1.0]);
    rejects_invalid_gradient(&Gradient::Linear(gradient));
}

pub(crate) fn radial_stops_above_one_are_rejected() {
    let mut gradient = RadialGradient::circular(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![0.0, 1.1]);
    rejects_invalid_gradient(&Gradient::Radial(gradient));
}

pub(crate) fn extreme_negative_sweep_stops_are_rejected() {
    let mut gradient = SweepGradient::centered(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![-f64::MAX, 1.0]);
    rejects_invalid_gradient(&Gradient::Sweep(gradient));
}

pub(crate) fn extreme_positive_linear_stops_are_rejected() {
    let mut gradient = LinearGradient::horizontal(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![0.0, f64::MAX]);
    rejects_invalid_gradient(&Gradient::Linear(gradient));
}

pub(crate) fn sweep_descending_stops_are_rejected() {
    let mut gradient = SweepGradient::centered(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![1.0, 0.0]);
    rejects_invalid_gradient(&Gradient::Sweep(gradient));
}

pub(crate) fn empty_equal_gradients_are_rejected() {
    rejects_invalid_gradient(&Gradient::Linear(LinearGradient::horizontal(Vec::new())));
}

pub(crate) fn mismatched_equal_gradient_stops_are_rejected() {
    let mut gradient = RadialGradient::circular(vec![Color::GREEN, Color::BLUE]);
    gradient.stops = Some(vec![0.0]);
    rejects_invalid_gradient(&Gradient::Radial(gradient));
}

pub(crate) fn nan_gradient_interpolation_is_rejected() {
    let gradient = SweepGradient::centered(vec![Color::RED, Color::BLUE]);
    assert!(SweepGradient::lerp(&gradient, &gradient, f64::NAN).is_none());
    assert_eq!(
        SweepGradient::lerp(&gradient, &gradient, 0.5)
            .expect("valid interpolation after rejection")
            .colors,
        [Color::RED, Color::BLUE]
    );
}

fn keeps_hard_gradient_transition(gradient: Gradient) {
    let black = vec![Color::BLACK, Color::BLACK];
    let other = match &gradient {
        Gradient::Linear(_) => Gradient::Linear(LinearGradient::horizontal(black)),
        Gradient::Radial(_) => Gradient::Radial(RadialGradient::circular(black)),
        Gradient::Sweep(_) => Gradient::Sweep(SweepGradient::centered(black)),
    };
    let mixed =
        Gradient::lerp(&gradient, &other, 0.5).expect("valid hard-edge gradients interpolate");
    assert_eq!(mixed.stops(), Some([0.0, 0.5, 0.5, 1.0].as_slice()));
    assert_eq!(
        mixed.colors(),
        &[
            Color::rgb(128, 0, 0),
            Color::rgb(128, 0, 0),
            Color::rgb(0, 0, 128),
            Color::rgb(0, 0, 128),
        ],
        "both sides of the red-to-blue discontinuity survive interpolation"
    );
}

pub(crate) fn linear_interpolation_keeps_hard_transitions() {
    let mut gradient =
        LinearGradient::horizontal(vec![Color::RED, Color::RED, Color::BLUE, Color::BLUE]);
    gradient.stops = Some(vec![0.0, 0.5, 0.5, 1.0]);
    keeps_hard_gradient_transition(Gradient::Linear(gradient));
}

pub(crate) fn radial_interpolation_keeps_hard_transitions() {
    let mut gradient =
        RadialGradient::circular(vec![Color::RED, Color::RED, Color::BLUE, Color::BLUE]);
    gradient.stops = Some(vec![0.0, 0.5, 0.5, 1.0]);
    keeps_hard_gradient_transition(Gradient::Radial(gradient));
}

pub(crate) fn sweep_interpolation_keeps_hard_transitions() {
    let mut gradient =
        SweepGradient::centered(vec![Color::RED, Color::RED, Color::BLUE, Color::BLUE]);
    gradient.stops = Some(vec![0.0, 0.5, 0.5, 1.0]);
    keeps_hard_gradient_transition(Gradient::Sweep(gradient));
}

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
