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

#[cfg(feature = "serde")]
pub(crate) fn deserialized_paths_keep_geometry_authoritative() {
    let rounded = Path::from_rrect(RRect::from_rect_xy(
        Rect::from_ltrb(0.0, 0.0, 40.0, 20.0),
        4.0,
        4.0,
    ));
    let triangle = Path::polygon(&[
        Point::new(0.0, 0.0),
        Point::new(40.0, 0.0),
        Point::new(0.0, 20.0),
    ]);
    let mut submitted = serde_json::to_value(&triangle).expect("a path serializes");
    let rounded_wire = serde_json::to_value(&rounded).expect("a rounded path serializes");
    submitted["hint"] = rounded_wire["hint"].clone();
    let restored: Path = serde_json::from_value(submitted).expect("path commands deserialize");
    assert!(restored.rrect_hint().is_none());
    assert_eq!(
        restored.commands().collect::<Vec<_>>(),
        triangle.commands().collect::<Vec<_>>()
    );
    assert!(restored.contains(Point::new(5.0, 5.0)));
    assert!(!restored.contains(Point::new(30.0, 15.0)));
}

#[cfg(feature = "serde")]
pub(crate) fn serialized_factory_paths_preserve_their_shapes() {
    let rect = Rect::from_ltrb(2.0, 3.0, 42.0, 23.0);
    for original in [
        Path::rectangle(rect),
        Path::oval(rect),
        Path::from_rrect(RRect::from_rect_xy(rect, 4.0, 4.0)),
    ] {
        let lossless_wire = serde_json::to_value(&original).expect("a factory path serializes");
        let lossless: Path =
            serde_json::from_value(lossless_wire).expect("factory path values deserialize");
        assert_eq!(lossless.rrect_hint(), original.rrect_hint());
        assert_eq!(
            lossless.commands().collect::<Vec<_>>(),
            original.commands().collect::<Vec<_>>()
        );
        let wire = serde_json::to_string(&original).expect("a factory path serializes");
        let restored: Path = serde_json::from_str(&wire).expect("a factory path deserializes");
        assert_eq!(restored.fill_type(), original.fill_type());
        assert_eq!(restored.commands().count(), original.commands().count());
        // JSON float parsing can move the curve coordinates a few ULPs.
        // These fixtures occupy at most 42 logical pixels; 1e-12 remains
        // far below the path constructor's geometric tolerance.
        let point_eq = |actual: Point<f64>, expected: Point<f64>| {
            assert!((actual.x - expected.x).abs() <= 1e-12);
            assert!((actual.y - expected.y).abs() <= 1e-12);
        };
        for (actual, expected) in restored.commands().zip(original.commands()) {
            match (actual, expected) {
                (PathCommand::MoveTo(a), PathCommand::MoveTo(b))
                | (PathCommand::LineTo(a), PathCommand::LineTo(b)) => point_eq(a, b),
                (PathCommand::QuadraticTo(a, b), PathCommand::QuadraticTo(c, d)) => {
                    point_eq(a, c);
                    point_eq(b, d);
                }
                (
                    PathCommand::CubicTo(actual_first, actual_second, actual_end),
                    PathCommand::CubicTo(expected_first, expected_second, expected_end),
                ) => {
                    point_eq(actual_first, expected_first);
                    point_eq(actual_second, expected_second);
                    point_eq(actual_end, expected_end);
                }
                (PathCommand::Close, PathCommand::Close) => {}
                _ => panic!("path round trip changed contour commands: {actual:?}, {expected:?}"),
            }
        }
        assert!(restored.contains(Point::new(22.0, 13.0)));
        assert!(!restored.contains(Point::new(43.0, 13.0)));
    }
}

#[cfg(feature = "serde")]
pub(crate) fn deserialized_images_validate_rgba_dimensions_and_data() {
    use flui_painting::paint::Image;

    for rejected in [
        serde_json::json!({"width": 1, "height": 1, "data": []}),
        serde_json::json!({"width": 1, "height": 1, "data": [255, 0, 0, 255, 0]}),
        serde_json::json!({"width": u32::MAX, "height": u32::MAX, "data": []}),
    ] {
        assert!(serde_json::from_value::<Image>(rejected).is_err());
    }
    let original = Image::from_rgba8(1, 1, vec![255, 0, 0, 255]);
    let wire = serde_json::to_value(&original).expect("an image serializes");
    let restored: Image = serde_json::from_value(wire).expect("a valid image deserializes");
    assert_eq!(restored.size(), original.size());
    assert_eq!(restored.data(), original.data());
    let empty: Image = serde_json::from_value(
        serde_json::to_value(Image::default()).expect("an empty image serializes"),
    )
    .expect("a zero-sized image remains valid");
    assert_eq!(empty.data(), [0_u8; 0]);
}
