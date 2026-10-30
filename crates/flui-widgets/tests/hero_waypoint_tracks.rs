//! Authored Hero waypoint tracks through the presentation clock and painted scene.
//! An airborne reversal retains the selected track; a fresh pop selects its own.

use std::{cell::Cell, rc::Rc, time::Duration};

use crate::common::{LaidOut, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_foundation::geometry::Rect;
use flui_painting::styling::Color;
use flui_widgets::VsyncScope;
use flui_widgets::prelude::*;

struct SampledHeroTrack {
    track: flui_animation::Keyframes<Rect>,
    samples: Rc<Cell<usize>>,
}

impl flui_animation::Animatable for SampledHeroTrack {
    type Value = Rect;

    fn transform(&self, progress: f64) -> Self::Value {
        self.samples.set(self.samples.get() + 1);
        flui_animation::Animatable::transform(&self.track, progress)
    }
}

fn hero_waypoints(begin: Rect, end: Rect, bend: f64) -> [Rect; 4] {
    [
        begin,
        Rect::from_ltwh(begin.min_x() + bend, begin.min_y() - 60.0, 42.0, 35.0),
        Rect::from_ltwh(end.min_x() - bend * 0.4, end.min_y() + 55.0, 54.0, 25.0),
        end,
    ]
}

fn authored_hero_page(
    width: f64,
    height: f64,
    color: Color,
    bend: f64,
    samples: &Rc<Cell<usize>>,
) -> PageRoute<i32> {
    let samples = Rc::clone(samples);
    PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        let samples = Rc::clone(&samples);
        Center::new()
            .child(
                Hero::new(
                    ValueKey::new("authored"),
                    ColoredBox::new(color).child(SizedBox::new(width, height)),
                )
                .curve(flui_animation::curves::LINEAR)
                .create_rect_tween(move |begin, end| {
                    let points = hero_waypoints(begin, end, bend);
                    SampledHeroTrack {
                        track: flui_animation::Keyframes::builder(
                            points[0],
                            Duration::from_secs(1),
                        )
                        .cubic(points[1], Duration::from_millis(200))
                        .cubic(points[2], Duration::from_millis(450))
                        .cubic(points[3], Duration::from_millis(350))
                        .build()
                        .expect("valid authored rectangle track"),
                        samples: Rc::clone(&samples),
                    }
                })
                .flight_shuttle_builder(|_, _, _, _| ColoredBox::new(Color::BLUE)),
            )
            .into_view()
            .boxed()
    })
    .transitions(|_, _, _, page| page)
    .transition_duration(Duration::from_secs(1))
    .reverse_transition_duration(Duration::from_secs(1))
}

fn waypoint_rect_at(points: [Rect; 4], progress: f64) -> Rect {
    let times = [0.0, 0.2, 0.65, 1.0];
    let positions = points.map(|p| [p.min_x(), p.min_y(), p.width(), p.height()]);
    let segment = if progress < times[1] {
        0
    } else if progress < times[2] {
        1
    } else {
        2
    };
    let duration = times[segment + 1] - times[segment];
    let u = (progress - times[segment]) / duration;
    let h00 = 2.0 * u.powi(3) - 3.0 * u.powi(2) + 1.0;
    let h10 = u.powi(3) - 2.0 * u.powi(2) + u;
    let h01 = -2.0 * u.powi(3) + 3.0 * u.powi(2);
    let h11 = u.powi(3) - u.powi(2);
    let values: [f64; 4] = std::array::from_fn(|coordinate| {
        let derivative = |knot: usize| {
            if knot == 0 || knot == 3 {
                0.0
            } else {
                (positions[knot + 1][coordinate] - positions[knot - 1][coordinate])
                    / (times[knot + 1] - times[knot - 1])
            }
        };
        h00 * positions[segment][coordinate]
            + h10 * duration * derivative(segment)
            + h01 * positions[segment + 1][coordinate]
            + h11 * duration * derivative(segment + 1)
    });
    Rect::from_ltwh(values[0], values[1], values[2], values[3])
}

fn painted_hero_bounds(laid: &LaidOut, color: Color) -> Vec<Rect> {
    use flui_foundation::geometry::Matrix4;
    use flui_painting::display_list::DrawOp;
    use flui_painting::paint::Clip;
    use flui_rendering::layer::Layer;
    let tree = laid.layer_tree().expect("mounted scene");
    let mut painted = Vec::new();
    for (id, node) in tree.iter() {
        let Layer::Picture(picture) = node.layer() else {
            continue;
        };
        for command in picture.picture() {
            let DrawOp::Rect { rect, paint } = &command.op else {
                continue;
            };
            if paint.color != color {
                continue;
            }
            let mut visible = Some(command.transform.transform_rect(rect));
            let mut parent = tree.parent(id);
            while let Some(id) = parent {
                let layer = tree.get_layer(id).expect("live scene ancestor");
                let transform = match layer {
                    Layer::Offset(offset) => Some(Matrix4::translation(
                        offset.offset().dx,
                        offset.offset().dy,
                        0.0,
                    )),
                    Layer::Transform(transform) => Some(*transform.transform()),
                    Layer::Opacity(opacity) => Some(Matrix4::translation(
                        opacity.offset().dx,
                        opacity.offset().dy,
                        0.0,
                    )),
                    _ => None,
                };
                if let Some(transform) = transform {
                    visible = visible.map(|rect| transform.transform_rect(&rect));
                }
                if let Layer::ClipRect(clip) = layer
                    && clip.clip_behavior() != Clip::None
                {
                    visible = visible.and_then(|rect| rect.intersect(&clip.clip_rect()));
                }
                if let Layer::Opacity(opacity) = layer {
                    assert_eq!(opacity.alpha(), 1.0, "the selected Hero is fully visible");
                }
                parent = tree.parent(id);
            }
            if let Some(rect) = visible
                && rect.width() > 0.0
                && rect.height() > 0.0
            {
                painted.push(rect);
            }
        }
    }
    painted
}

fn assert_hero_paint(laid: &LaidOut, color: Color, expected: Rect) {
    let painted = painted_hero_bounds(laid, color);
    assert_eq!(painted.len(), 1, "one visible authored shuttle");
    let actual = painted[0];
    for (got, wanted) in [
        (actual.min_x(), expected.min_x()),
        (actual.min_y(), expected.min_y()),
        (actual.width(), expected.width()),
        (actual.height(), expected.height()),
    ] {
        assert!(
            (got - wanted).abs() < 1e-6,
            "painted rectangle {actual:?}, expected {expected:?}"
        );
    }
}

pub(crate) fn authored_cubic_hero_geometry_is_painted_and_retraced() {
    let source = Rect::from_ltwh(385.0, 290.0, 30.0, 20.0);
    let destination = Rect::from_ltwh(370.0, 277.5, 60.0, 45.0);
    let forward = hero_waypoints(source, destination, 80.0);
    for airborne_reverse in [true, false] {
        let source_samples = Rc::new(Cell::new(0));
        let destination_samples = Rc::new(Cell::new(0));
        let vsync = Vsync::new();
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(authored_hero_page(
            30.0,
            20.0,
            Color::RED,
            -120.0,
            &source_samples,
        ));
        let mut laid = lay_out_animated(
            VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
            tight(800.0, 600.0),
            vsync,
        );
        assert_hero_paint(&laid, Color::RED, source);
        let page = authored_hero_page(60.0, 45.0, Color::GREEN, 80.0, &destination_samples);
        let _push = laid.enter_owner_scope(|| navigator.push(page));
        laid.tick();
        for (progress, millis) in [(0.1, 100), (0.2, 100), (0.4, 200), (0.65, 250), (0.8, 150)] {
            laid.pump_for(Duration::from_millis(millis));
            assert_hero_paint(&laid, Color::BLUE, waypoint_rect_at(forward, progress));
        }
        assert!(destination_samples.get() > 0);
        assert_eq!(source_samples.get(), 0, "push chooses destination's path");
        if !airborne_reverse {
            laid.pump_for(Duration::from_millis(200));
            assert_eq!(painted_hero_bounds(&laid, Color::BLUE), []);
            assert_hero_paint(&laid, Color::GREEN, destination);
        }
        assert!(laid.enter_owner_scope(|| navigator.pop()));
        laid.tick();
        for (progress, millis) in [
            (0.65, if airborne_reverse { 150 } else { 350 }),
            (0.4, 250),
            (0.2, 200),
            (0.1, 100),
        ] {
            laid.pump_for(Duration::from_millis(millis));
            let expected = if airborne_reverse {
                waypoint_rect_at(forward, progress)
            } else {
                waypoint_rect_at(hero_waypoints(destination, source, -120.0), 1.0 - progress)
            };
            assert_hero_paint(&laid, Color::BLUE, expected);
        }
        assert_eq!(source_samples.get() > 0, !airborne_reverse);
        laid.pump_for(Duration::from_millis(100));
        assert_eq!(painted_hero_bounds(&laid, Color::BLUE), []);
        assert_hero_paint(&laid, Color::RED, source);
        assert_eq!(
            navigator.route_ids().len(),
            1,
            "only the restored source route remains"
        );
        let samples = (source_samples.get(), destination_samples.get());
        for _ in 0..3 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert_eq!(painted_hero_bounds(&laid, Color::BLUE), []);
        assert_hero_paint(&laid, Color::RED, source);
        assert_eq!(
            (source_samples.get(), destination_samples.get()),
            samples,
            "landed mappings are no longer sampled"
        );
    }
}

fn authored_eased_hero_page(
    width: f64,
    height: f64,
    color: Color,
    duration: Duration,
    curve: flui_animation::SharedCurve,
    samples: &Rc<Cell<usize>>,
) -> PageRoute<i32> {
    let samples = Rc::clone(samples);
    PageRoute::<i32>::new(move |_ctx, _primary, _secondary| {
        let samples = Rc::clone(&samples);
        let curve = curve.clone();
        Center::new()
            .child(
                Hero::new(
                    ValueKey::new("authored"),
                    ColoredBox::new(color).child(SizedBox::new(width, height)),
                )
                .curve(flui_animation::curves::LINEAR)
                .create_rect_tween(move |begin, end| SampledHeroTrack {
                    track: flui_animation::Keyframes::builder(begin, duration)
                        .to(end, duration, curve.clone())
                        .build()
                        .expect("valid authored eased rectangle track"),
                    samples: Rc::clone(&samples),
                })
                .flight_shuttle_builder(|_, _, _, _| ColoredBox::new(Color::BLUE)),
            )
            .into_view()
            .boxed()
    })
    .transitions(|_, _, _, page| page)
    .transition_duration(duration)
    .reverse_transition_duration(duration)
}

fn independent_eased_rect(begin: Rect, end: Rect, progress: f64) -> Rect {
    Rect::from_ltwh(
        begin.min_x() + (end.min_x() - begin.min_x()) * progress,
        begin.min_y() + (end.min_y() - begin.min_y()) * progress,
        begin.width() + (end.width() - begin.width()) * progress,
        begin.height() + (end.height() - begin.height()) * progress,
    )
}

fn authored_easing_flight(spring: bool) {
    use flui_animation::{
        LinearStop, PiecewiseLinear, SharedCurve, SpringCurve, SpringDescription, Tolerance,
    };
    let source = Rect::from_ltwh(385.0, 290.0, 30.0, 20.0);
    let destination = Rect::from_ltwh(370.0, 277.5, 60.0, 45.0);
    let (curve, duration, checkpoints) = if spring {
        let curve = SpringCurve::try_new(
            SpringDescription::try_new(1.0, 100.0, 6.0).unwrap(),
            Tolerance::DEFAULT,
        )
        .unwrap();
        let duration = curve.duration();
        (SharedCurve::new(curve), duration, [50, 100, 330])
    } else {
        let curve = PiecewiseLinear::try_new(&[
            LinearStop::new(0.0),
            LinearStop::new(0.5).at(0.25),
            LinearStop::new(0.5).at(0.75),
            LinearStop::new(1.0),
        ])
        .unwrap();
        (
            SharedCurve::new(curve),
            Duration::from_secs(1),
            [125, 375, 875],
        )
    };
    let expected_progress = |elapsed: Duration| {
        let time = elapsed.as_secs_f64();
        if spring {
            // Independent zero-velocity unit spring, omega=10, zeta=0.3.
            let damped = 10.0 * 0.91_f64.sqrt();
            1.0 - (-3.0 * time).exp()
                * ((damped * time).cos() + 3.0 / damped * (damped * time).sin())
        } else if time < 0.25 {
            2.0 * time
        } else if time <= 0.75 {
            0.5
        } else {
            0.5 + 2.0 * (time - 0.75)
        }
    };
    let source_samples = Rc::new(Cell::new(0));
    let destination_samples = Rc::new(Cell::new(0));
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(authored_eased_hero_page(
        30.0,
        20.0,
        Color::RED,
        duration,
        SharedCurve::new(flui_animation::curves::LINEAR),
        &source_samples,
    ));
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
        tight(800.0, 600.0),
        vsync.clone(),
    );
    assert_hero_paint(&laid, Color::RED, source);
    let _push = laid.enter_owner_scope(|| {
        navigator.push(authored_eased_hero_page(
            60.0,
            45.0,
            Color::GREEN,
            duration,
            curve,
            &destination_samples,
        ))
    });
    laid.tick();
    let mut elapsed = Duration::ZERO;
    for millis in checkpoints {
        let at = Duration::from_millis(millis);
        laid.pump_for(at.checked_sub(elapsed).expect("increasing checkpoints"));
        elapsed = at;
        assert_hero_paint(
            &laid,
            Color::BLUE,
            independent_eased_rect(source, destination, expected_progress(elapsed)),
        );
    }
    assert!(destination_samples.get() > 0);
    assert_eq!(source_samples.get(), 0);
    if spring {
        assert!(
            expected_progress(elapsed) > 1.0,
            "the painted spring actually overshot"
        );
    }
    assert!(laid.enter_owner_scope(|| navigator.pop()));
    laid.tick();
    for millis in checkpoints[..2].iter().rev() {
        let at = Duration::from_millis(*millis);
        laid.pump_for(elapsed.checked_sub(at).expect("decreasing checkpoints"));
        elapsed = at;
        assert_hero_paint(
            &laid,
            Color::BLUE,
            independent_eased_rect(source, destination, expected_progress(elapsed)),
        );
    }
    assert_eq!(
        source_samples.get(),
        0,
        "airborne reversal keeps the selected easing"
    );
    laid.pump_for(elapsed);
    assert_eq!(painted_hero_bounds(&laid, Color::BLUE), []);
    assert_hero_paint(&laid, Color::RED, source);
    assert_eq!(navigator.route_ids().len(), 1);
    laid.pump_widget(SizedBox::shrink());
    assert!(vsync.is_empty(), "unmount withdraws the route's owners");
    let samples = (source_samples.get(), destination_samples.get());
    laid.pump_for(Duration::from_millis(100));
    assert_eq!((source_samples.get(), destination_samples.get()), samples);
    assert_eq!(painted_hero_bounds(&laid, Color::BLUE), []);
}

pub(crate) fn authored_piecewise_hero_geometry_is_painted_and_retraced() {
    authored_easing_flight(false);
}

pub(crate) fn authored_spring_hero_geometry_is_painted_and_retraced() {
    authored_easing_flight(true);
}
