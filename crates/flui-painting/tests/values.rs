//! Paint and text value contracts: CSS weight mapping, radial-gradient focal
//! interpolation and how an arc joins a path.

use flui_foundation::geometry::{Offset, Point, RRect, Rect};
use flui_painting::Alignment;
use flui_painting::paint::{Path, PathCommand};
use flui_painting::styling::{
    Border, BorderRadius, BorderRadiusExt, BorderSide, BorderStyle, BoxDecoration, BoxShadow,
    Color, Gradient, LinearGradient, RadialGradient, SweepGradient, TileMode,
};
use flui_painting::typography::FontWeight;

fn gradient_geometry_extrapolates(through_decoration: bool) {
    let colors = vec![Color::RED, Color::BLUE];
    let pairs = [
        (
            Gradient::Linear(LinearGradient::new(
                Alignment::CENTER,
                Alignment::CENTER_RIGHT,
                colors.clone(),
                None,
                TileMode::Clamp,
            )),
            Gradient::Linear(LinearGradient::new(
                Alignment::new(1.0, 2.0),
                Alignment::new(2.0, 2.0),
                colors.clone(),
                None,
                TileMode::Clamp,
            )),
        ),
        (
            Gradient::Radial(RadialGradient::new(
                Alignment::CENTER,
                0.5,
                colors.clone(),
                None,
                TileMode::Clamp,
                None,
                None,
            )),
            Gradient::Radial(RadialGradient::new(
                Alignment::new(1.0, 2.0),
                1.0,
                colors.clone(),
                None,
                TileMode::Clamp,
                None,
                None,
            )),
        ),
        (
            Gradient::Sweep(SweepGradient::new(
                Alignment::CENTER,
                colors.clone(),
                None,
                TileMode::Clamp,
                0.25,
                1.25,
            )),
            Gradient::Sweep(SweepGradient::new(
                Alignment::new(1.0, 2.0),
                colors,
                None,
                TileMode::Clamp,
                1.25,
                2.25,
            )),
        ),
    ];
    for (a, b) in pairs {
        for (t, expected, expected_radius, expected_angles) in [
            (-0.25, Alignment::new(-0.25, -0.5), 0.375, (0.0, 1.0)),
            (1.25, Alignment::new(1.25, 2.5), 1.125, (1.5, 2.5)),
        ] {
            let mixed = if through_decoration {
                BoxDecoration::<f64>::lerp(
                    &BoxDecoration::with_gradient(a.clone()),
                    &BoxDecoration::with_gradient(b.clone()),
                    t,
                )
                .gradient
                .expect("interpolated decoration gradient")
            } else {
                Gradient::lerp(&a, &b, t).expect("finite gradient geometry")
            };
            match mixed {
                Gradient::Linear(g) => {
                    assert_eq!(g.begin, expected, "linear begin at {t}");
                    assert_eq!(
                        g.end,
                        Alignment::new(expected.x + 1.0, expected.y),
                        "linear end at {t}"
                    );
                }
                Gradient::Radial(g) => {
                    assert_eq!(g.center, expected, "radial center at {t}");
                    assert_eq!(g.radius, expected_radius, "radial radius at {t}");
                }
                Gradient::Sweep(g) => {
                    assert_eq!(g.center, expected, "sweep center at {t}");
                    assert_eq!(
                        (g.start_angle, g.end_angle),
                        expected_angles,
                        "sweep angles at {t}"
                    );
                }
            }
        }
    }
}

pub(crate) fn gradient_geometry_preserves_overshoot() {
    gradient_geometry_extrapolates(false);
}

pub(crate) fn decoration_gradient_geometry_preserves_overshoot() {
    gradient_geometry_extrapolates(true);
    let populated = BoxDecoration::with_color(Color::RED)
        .set_border(Some(Border::all(BorderSide::new(
            Color::BLACK,
            2.0,
            BorderStyle::Solid,
        ))))
        .set_border_radius(Some(BorderRadius::circular(4.0)))
        .set_box_shadow(Some(vec![BoxShadow::new(
            Color::BLACK,
            Offset::new(1.0, 2.0),
            3.0,
            1.0,
        )]));
    let linear = Gradient::Linear(LinearGradient::horizontal(vec![Color::RED, Color::BLUE]));
    let radial = Gradient::Radial(RadialGradient::circular(vec![Color::RED, Color::BLUE]));
    for (a, b) in [
        (
            populated.clone().set_gradient(Some(linear.clone())),
            BoxDecoration::with_gradient(radial),
        ),
        (populated.clone(), BoxDecoration::new()),
        (populated.set_gradient(Some(linear)), BoxDecoration::new()),
    ] {
        for (t, expected) in [(-0.25, &a), (1.25, &b)] {
            assert_eq!(
                BoxDecoration::lerp(&a, &b, t),
                *expected,
                "exact bounded representation at {t}"
            );
        }
    }
}

pub(crate) fn gradient_geometry_rejects_invalid_inputs_before_equal_shortcuts() {
    let colors = vec![Color::RED, Color::BLUE];
    let mut linear = LinearGradient::horizontal(colors.clone());
    linear.begin.x = f64::INFINITY;
    let mut radial = RadialGradient::circular(colors.clone());
    radial.radius = -1.0;
    let mut focal = RadialGradient::circular(colors.clone());
    focal.focal = Some(Alignment::new(f64::INFINITY, 0.0));
    let mut focal_radius = RadialGradient::circular(colors.clone());
    focal_radius.focal_radius = Some(f64::INFINITY);
    let mut sweep = SweepGradient::centered(colors);
    sweep.end_angle = f64::INFINITY;
    for bad in [
        Gradient::Linear(linear),
        Gradient::Radial(radial),
        Gradient::Radial(focal),
        Gradient::Radial(focal_radius),
        Gradient::Sweep(sweep),
    ] {
        rejects_invalid_gradient(&bad);
    }
    for healthy in [
        Gradient::Linear(LinearGradient::horizontal(vec![Color::RED, Color::BLUE])),
        Gradient::Radial(RadialGradient::circular(vec![Color::RED, Color::BLUE])),
        Gradient::Sweep(SweepGradient::centered(vec![Color::RED, Color::BLUE])),
    ] {
        for t in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                Gradient::lerp(&healthy, &healthy, t).is_none(),
                "fraction {t}"
            );
        }
        assert!(Gradient::lerp(&healthy, &healthy, 0.5).is_some());
    }
}

pub(crate) fn gradient_geometry_checks_intermediate_and_output_overflow() {
    let linear_a = LinearGradient::new(
        Alignment::CENTER,
        Alignment::CENTER_RIGHT,
        vec![Color::RED, Color::BLUE],
        None,
        TileMode::Clamp,
    );
    let mut linear_b = linear_a.clone();
    linear_b.begin.x = 1.0;
    linear_b.end.x = 2.0;
    let sweep_a = SweepGradient::new(
        Alignment::CENTER,
        vec![Color::RED, Color::BLUE],
        None,
        TileMode::Clamp,
        0.0,
        1.0,
    );
    let mut sweep_b = sweep_a.clone();
    sweep_b.start_angle = 1.0;
    sweep_b.end_angle = 2.0;
    for (a, b) in [
        (Gradient::Linear(linear_a), Gradient::Linear(linear_b)),
        (Gradient::Sweep(sweep_a), Gradient::Sweep(sweep_b)),
    ] {
        assert!(
            Gradient::lerp(&a, &b, 1e16).is_none(),
            "a unit span lost to translation must be refused: {b:?}"
        );
        let mixed = BoxDecoration::<f64>::lerp(
            &BoxDecoration::with_gradient(a.clone()),
            &BoxDecoration::with_gradient(b.clone()),
            1e16,
        );
        assert_eq!(mixed.gradient, Some(b.clone()));
        let mut reversed = b;
        match &mut reversed {
            Gradient::Linear(g) => g.end.x = 0.0,
            Gradient::Sweep(g) => g.end_angle = 0.0,
            Gradient::Radial(_) => unreachable!(),
        }
        assert!(
            Gradient::lerp(&a, &reversed, 0.5).is_some(),
            "an intended zero span remains valid"
        );
    }
    let mut sweep_from = SweepGradient::centered(vec![Color::RED, Color::BLUE]);
    sweep_from.end_angle = 0.0;
    let mut sweep_to = sweep_from.clone();
    sweep_to.end_angle = 1.0;
    for t in [1e100, -1e100] {
        assert!(SweepGradient::lerp(&sweep_from, &sweep_to, t).is_none());
        let from = Gradient::Sweep(sweep_from.clone());
        let to = Gradient::Sweep(sweep_to.clone());
        assert!(Gradient::lerp(&from, &to, t).is_none());
        let mixed = BoxDecoration::<f64>::lerp(
            &BoxDecoration::with_gradient(from),
            &BoxDecoration::with_gradient(to),
            t,
        );
        let Some(Gradient::Sweep(bounded)) = mixed.gradient else {
            panic!("an unrepresentable sweep must retain its bounded gradient");
        };
        assert_eq!(bounded.start_angle, 0.0);
        assert_eq!(bounded.end_angle, if t > 0.0 { 1.0 } else { 0.0 });
    }
    for span in [f64::from(f32::MAX), -f64::from(f32::MAX)] {
        assert!(SweepGradient::lerp(&sweep_from, &sweep_to, span).is_some());
    }
    let mut large_phase = sweep_from.clone();
    large_phase.start_angle = 1e100;
    large_phase.end_angle = 1e100;
    assert!(SweepGradient::lerp(&large_phase, &large_phase, 0.5).is_some());
    for span in [1e100, -1e100, 1e-100] {
        let mut invalid = sweep_from.clone();
        invalid.end_angle = span;
        assert!(SweepGradient::lerp(&invalid, &invalid, 0.5).is_none());
    }
    let mut linear_a = LinearGradient::horizontal(vec![Color::RED, Color::BLUE]);
    linear_a.begin = Alignment::CENTER;
    linear_a.end = Alignment::CENTER;
    let mut linear_b = linear_a.clone();
    linear_b.begin.x = 1.0;
    linear_b.end.x = -1.0;
    let mut sweep_a = SweepGradient::centered(vec![Color::RED, Color::BLUE]);
    sweep_a.start_angle = 0.0;
    sweep_a.end_angle = 0.0;
    let mut sweep_b = sweep_a.clone();
    sweep_b.start_angle = 1.0;
    sweep_b.end_angle = -1.0;
    for (a, b) in [
        (Gradient::Linear(linear_a), Gradient::Linear(linear_b)),
        (Gradient::Sweep(sweep_a), Gradient::Sweep(sweep_b)),
    ] {
        assert!(
            Gradient::lerp(&a, &b, f64::MAX).is_none(),
            "finite components must not publish an infinite direction or span: {b:?}"
        );
        let mixed = BoxDecoration::<f64>::lerp(
            &BoxDecoration::with_gradient(a.clone()),
            &BoxDecoration::with_gradient(b.clone()),
            f64::MAX,
        );
        assert_eq!(mixed.gradient, Some(b.clone()));
        let mut invalid = b;
        match &mut invalid {
            Gradient::Linear(g) => {
                g.begin.x = f64::MAX;
                g.end.x = -f64::MAX;
            }
            Gradient::Sweep(g) => {
                g.start_angle = f64::MAX;
                g.end_angle = -f64::MAX;
            }
            Gradient::Radial(_) => unreachable!(),
        }
        assert!(Gradient::lerp(&invalid, &invalid, 0.5).is_none());
    }
    let colors = vec![Color::RED, Color::BLUE];
    let mut a = LinearGradient::horizontal(colors);
    let mut b = a.clone();
    a.begin.x = -f64::MAX;
    b.begin.x = f64::MAX;
    let Gradient::Linear(midpoint) = Gradient::lerp(
        &Gradient::Linear(a.clone()),
        &Gradient::Linear(b.clone()),
        0.5,
    )
    .expect("opposite finite extremes have a finite midpoint") else {
        panic!("linear interpolation must remain linear");
    };
    assert_eq!(midpoint.begin.x, 0.0);
    assert!(
        Gradient::lerp(
            &Gradient::Linear(a.clone()),
            &Gradient::Linear(b.clone()),
            2.0
        )
        .is_none()
    );
    let mixed = BoxDecoration::<f64>::lerp(
        &BoxDecoration::with_gradient(Gradient::Linear(a)),
        &BoxDecoration::with_gradient(Gradient::Linear(b.clone())),
        2.0,
    );
    assert_eq!(mixed.gradient, Some(Gradient::Linear(b)));
    let healthy = Gradient::Linear(LinearGradient::horizontal(vec![Color::RED, Color::BLUE]));
    assert!(Gradient::lerp(&healthy, &healthy, 0.5).is_some());
}

pub(crate) fn gradient_domains_keep_zero_radii_and_signed_angles() {
    let mut a = RadialGradient::circular(vec![Color::RED, Color::BLUE]);
    a.radius = 1.0;
    a.focal_radius = Some(1.0);
    let mut b = a.clone();
    b.radius = 0.0;
    b.focal_radius = Some(0.0);
    let mixed = RadialGradient::lerp(&a, &b, 2.0).expect("finite extrapolated radius");
    assert_eq!(mixed.radius, 0.0);
    assert_eq!(mixed.focal_radius, Some(0.0));
    a.radius = 2.0;
    a.focal_radius = Some(2.0);
    let mixed = RadialGradient::lerp(&a, &b, f64::MAX)
        .expect("negative radius overflow has the finite zero lower bound");
    assert_eq!(mixed.radius, 0.0);
    assert_eq!(mixed.focal_radius, Some(0.0));

    let mut a = SweepGradient::centered(vec![Color::RED, Color::BLUE]);
    a.start_angle = 0.25;
    a.end_angle = 1.25;
    let mut b = a.clone();
    b.start_angle = 1.25;
    b.end_angle = 2.25;
    let mixed = SweepGradient::lerp(&a, &b, -0.5).expect("finite signed angles");
    assert_eq!((mixed.start_angle, mixed.end_angle), (-0.25, 0.75));
}

pub(crate) fn radial_overshoot_refuses_coincident_nonzero_circles() {
    for focal_radius in [false, true] {
        let mut a = RadialGradient::circular(vec![Color::RED, Color::BLUE]);
        let mut b = a.clone();
        if focal_radius {
            a.focal_radius = Some(1.0);
            b.focal_radius = Some(2.0);
        } else {
            a.radius = 1.0;
            b.radius = 2.0;
        }
        for (start, end, t) in [(&a, &b, 1e100), (&b, &a, -1e100)] {
            assert!(RadialGradient::lerp(start, end, t).is_none());
            let start = BoxDecoration::<f64>::with_gradient(Gradient::Radial(start.clone()));
            let end = BoxDecoration::with_gradient(Gradient::Radial(end.clone()));
            let mixed = BoxDecoration::lerp(&start, &end, t);
            assert_eq!(&mixed, if t < 0.0 { &start } else { &end });
            let mut canvas = flui_painting::Canvas::new();
            flui_painting::paint_box_decoration(
                &mut canvas,
                Rect::from_ltrb(0.0, 0.0, 100.0, 100.0),
                &mixed,
                flui_painting::DecorationPaintOptions::default(),
            );
            let list = canvas.finish();
            let flui_painting::DrawOp::Rect { paint, .. } =
                &list.iter().next().expect("painted bounded gradient").op
            else {
                panic!("rectangular decoration must paint a rect");
            };
            let Some(flui_painting::paint::Shader::RadialGradient {
                radius,
                focal_radius: resolved_focal_radius,
                ..
            }) = &paint.shader
            else {
                panic!("radial decoration must record a radial shader");
            };
            assert_eq!(*radius, if focal_radius { 50.0 } else { 200.0 });
            assert_eq!(*resolved_focal_radius, focal_radius.then_some(200.0));
        }
    }
    let mut oversized = RadialGradient::circular(vec![Color::RED, Color::BLUE]);
    oversized.radius = 1e100;
    assert!(RadialGradient::lerp(&oversized, &oversized, 0.5).is_none());
    oversized.radius = f64::from(f32::MAX);
    assert!(RadialGradient::lerp(&oversized, &oversized, 0.5).is_some());

    for focal in [None, Some(Alignment::CENTER)] {
        let mut coincident = RadialGradient::circular(vec![Color::RED, Color::BLUE]);
        coincident.radius = 1.0;
        coincident.focal_radius = Some(1.0);
        coincident.focal = focal;
        assert!(RadialGradient::lerp(&coincident, &coincident, 0.5).is_none());
        let gradient = Gradient::Radial(coincident);
        assert!(Gradient::lerp(&gradient, &gradient, 0.5).is_none());
    }
    let mut a = RadialGradient::circular(vec![Color::RED, Color::BLUE]);
    a.radius = 1.0;
    a.focal_radius = Some(0.0);
    let mut b = a.clone();
    b.radius = 2.0;
    b.focal_radius = Some(1.5);
    assert!(RadialGradient::lerp(&a, &b, 2.0).is_none());
    let mixed = BoxDecoration::<f64>::lerp(
        &BoxDecoration::with_gradient(Gradient::Radial(a)),
        &BoxDecoration::with_gradient(Gradient::Radial(b.clone())),
        2.0,
    );
    assert_eq!(mixed.gradient, Some(Gradient::Radial(b)));
}

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
    // Half way to black in Oklab halves L, a and b, which scales linear light by
    // 1/8: a full channel becomes 0.125 linear, sRGB-encoded as 99.
    assert_eq!(
        mixed.colors(),
        &[
            Color::rgb(99, 0, 0),
            Color::rgb(99, 0, 0),
            Color::rgb(0, 0, 99),
            Color::rgb(0, 0, 99),
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
