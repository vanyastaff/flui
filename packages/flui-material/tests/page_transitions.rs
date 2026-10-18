//! Material page transitions through the ambient hook (ADR-0221): a
//! [`MaterialApp`] installs the transition its theme names for its platform,
//! and every route pushed with no transitions of its own runs it.
//!
//! Each row reads the pushed page out of the committed scene: where its rect
//! lands and at what alpha, through every compositor ancestor. A row pins the
//! duration (the page is still moving past 300 ms where the transition is
//! longer), the curve (an intermediate value the named curve predicts, which
//! a linear run would miss) and the settled frame.

use crate::common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use common::{LaidOut, lay_out_animated, tight};
use flui_material::{
    MaterialApp, PageTransition, PageTransitionsTheme, SharedAxis, ThemeData, show_dialog,
};
use flui_sdk::animation::{Cubic, Curve, MotionOverride, Vsync, curves};
use flui_sdk::geometry::Point;
use flui_sdk::painting::{Color, DrawOp};
use flui_sdk::platform::TargetPlatform;
use flui_sdk::view::BuildContext;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    ColoredBox, FadeTransition, NavigatorHandle, PageRoute, PageTransitionBuilder, RouteAnimation,
    SizedBox, VsyncScope,
};

/// The pushed page's colour; nothing else in these apps paints it.
const PAGE: Color = Color::rgb(201, 33, 77);
/// The screen.
const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 800.0;
/// Geometry tolerance, in logical pixels.
pub(crate) const PX: f64 = 0.01;
/// Alpha and scale tolerance.
pub(crate) const EPS: f64 = 1e-6;

/// Where the page's rect lands in the committed scene, and at what alpha.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sample {
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) scale: f64,
    pub(crate) alpha: f64,
}

fn sample(app: &LaidOut) -> Option<Sample> {
    sample_of(app, PAGE)
}

/// Where whatever paints `color` lands, and at what alpha.
pub(crate) fn sample_of(app: &LaidOut, color: Color) -> Option<Sample> {
    use flui_layer::Layer;
    let tree = app.layer_tree().expect("a committed scene");
    for (_, node) in tree.iter() {
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
            let corner = |x: f64, y: f64| {
                let (x, y) = command.transform.transform_point(x, y);
                Point::new(x, y)
            };
            let mut start = corner(rect.left(), rect.top());
            let mut end = corner(rect.right(), rect.top());
            let mut alpha = 1.0;
            let mut parent = node.parent();
            while let Some(id) = parent {
                let ancestor = tree.get(id).expect("scene parent exists");
                match ancestor.layer() {
                    Layer::Transform(layer) => {
                        start = layer.transform_point(start);
                        end = layer.transform_point(end);
                    }
                    Layer::Offset(layer) => {
                        start += layer.offset();
                        end += layer.offset();
                    }
                    Layer::Opacity(layer) => {
                        start += layer.offset();
                        end += layer.offset();
                        alpha *= layer.alpha();
                    }
                    _ => {}
                }
                parent = ancestor.parent();
            }
            return Some(Sample {
                left: start.x,
                top: start.y,
                scale: (end.x - start.x) / rect.width(),
                alpha,
            });
        }
    }
    None
}

fn painted(app: &LaidOut) -> Sample {
    sample(app).expect("the pushed page is painted")
}

pub(crate) fn close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{what}: {actual}, expected {expected}"
    );
}

/// The settled page: in place, full size, opaque, and nothing left running.
fn assert_settled(app: &LaidOut, registry: &Vsync) {
    let page = painted(app);
    close(page.left, 0.0, PX, "settled left");
    close(page.top, 0.0, PX, "settled top");
    close(page.scale, 1.0, EPS, "settled scale");
    close(page.alpha, 1.0, EPS, "settled alpha");
    assert!(!registry.has_running(), "the transition has finished");
}

pub(crate) fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn themed(platform: TargetPlatform, transitions: PageTransitionsTheme) -> ThemeData {
    let mut theme = ThemeData::light();
    theme.platform = platform;
    theme.page_transitions_theme = transitions;
    theme
}

/// A theme whose current platform runs `transition`.
fn running(transition: PageTransition) -> ThemeData {
    let platform = TargetPlatform::current();
    themed(
        platform,
        PageTransitionsTheme::default().with(platform, transition),
    )
}

fn mount(theme: ThemeData) -> (NavigatorHandle, LaidOut, Vsync) {
    let registry = Vsync::new();
    let navigator = NavigatorHandle::new();
    let app = lay_out_animated(
        VsyncScope::new(
            registry.clone(),
            MaterialApp::new(SizedBox::shrink())
                .navigator(navigator.clone())
                .theme(theme),
        ),
        tight(WIDTH, HEIGHT),
        registry.clone(),
    );
    (navigator, app, registry)
}

fn page_route() -> PageRoute<()> {
    PageRoute::new(|_ctx, _primary, _secondary| {
        ColoredBox::new(PAGE)
            .child(SizedBox::expand())
            .into_view()
            .boxed()
    })
}

/// Mount under `theme`, push a page route with no transitions of its own and
/// anchor its clock.
fn pushed(theme: ThemeData) -> (NavigatorHandle, LaidOut, Vsync) {
    let (navigator, mut app, registry) = mount(theme);
    let _result = navigator.push(page_route());
    app.pump_for(Duration::ZERO);
    (navigator, app, registry)
}

/// Android: fade-forwards, 450 ms along `EASE_IN_OUT_CUBIC_EMPHASIZED`.
fn android_fades_forwards_over_450_ms() {
    let (_navigator, mut app, registry) = pushed(themed(
        TargetPlatform::Android,
        PageTransitionsTheme::default(),
    ));
    app.pump_for(ms(225));
    let progress = curves::EASE_IN_OUT_CUBIC_EMPHASIZED.transform(0.5);
    let page = painted(&app);
    close(page.alpha, progress, EPS, "half-way alpha");
    close(
        page.left,
        0.25 * WIDTH * (1.0 - progress),
        PX,
        "half-way slide",
    );
    assert!(
        (progress - 0.5).abs() > 0.05,
        "the curve is not linear here"
    );
    app.pump_for(ms(100));
    assert!(painted(&app).alpha < 1.0 - 1e-3, "still entering at 325 ms");
    app.pump_for(ms(125));
    assert_settled(&app, &registry);
}

/// iOS and macOS: the shared parallax slide, 500 ms along
/// `FAST_EASE_IN_TO_SLOW_EASE_OUT`.
fn apple_platforms_slide_over_500_ms() {
    for platform in [TargetPlatform::iOS, TargetPlatform::MacOS] {
        let (_navigator, mut app, registry) =
            pushed(themed(platform, PageTransitionsTheme::default()));
        app.pump_for(ms(250));
        let progress = curves::FAST_EASE_IN_TO_SLOW_EASE_OUT.transform(0.5);
        let page = painted(&app);
        close(page.left, WIDTH * (1.0 - progress), PX, "half-way slide");
        close(page.alpha, 1.0, EPS, "the slide does not fade");
        app.pump_for(ms(100));
        assert!(
            painted(&app).left > 1.0,
            "{platform}: still sliding at 350 ms"
        );
        app.pump_for(ms(150));
        assert_settled(&app, &registry);
    }
}

/// Desktop and web: zoom, 300 ms, scaling along
/// `EASE_IN_OUT_CUBIC_EMPHASIZED` and fading in over 12.5–25 %.
fn desktop_platforms_zoom_over_300_ms() {
    for platform in [
        TargetPlatform::Windows,
        TargetPlatform::Linux,
        TargetPlatform::Unknown,
    ] {
        let (_navigator, mut app, registry) =
            pushed(themed(platform, PageTransitionsTheme::default()));
        app.pump_for(ms(60));
        close(painted(&app).alpha, 0.6, EPS, "alpha at 20 %");
        app.pump_for(ms(90));
        let progress = curves::EASE_IN_OUT_CUBIC_EMPHASIZED.transform(0.5);
        let page = painted(&app);
        close(page.scale, 0.85 + 0.15 * progress, EPS, "half-way scale");
        close(page.alpha, 1.0, EPS, "faded in by 25 %");
        app.pump_for(ms(150));
        assert_settled(&app, &registry);
    }
}

/// The incoming shared-axis and fade-through alpha at half time: the fade
/// starts at 30 %.
const LATE_FADE: f64 = (0.5 - 0.3) / 0.7;

fn shared_axis_moves_30_px_and_fades_in_late() {
    let travel = 30.0 * (1.0 - curves::FAST_OUT_SLOW_IN.transform(0.5));
    for (axis, left, top) in [
        (SharedAxis::Horizontal, travel, 0.0),
        (SharedAxis::Vertical, 0.0, travel),
    ] {
        let (_navigator, mut app, registry) = pushed(running(PageTransition::SharedAxis(axis)));
        app.pump_for(ms(150));
        let page = painted(&app);
        close(page.alpha, LATE_FADE, EPS, "half-way alpha");
        close(page.left, left, PX, "half-way left");
        close(page.top, top, PX, "half-way top");
        app.pump_for(ms(150));
        assert_settled(&app, &registry);
    }
}

fn scaled_shared_axis_grows_from_80_percent() {
    let (_navigator, mut app, registry) =
        pushed(running(PageTransition::SharedAxis(SharedAxis::Scaled)));
    app.pump_for(ms(150));
    let page = painted(&app);
    close(
        page.scale,
        0.8 + 0.2 * curves::FAST_OUT_SLOW_IN.transform(0.5),
        EPS,
        "half-way scale",
    );
    close(page.alpha, LATE_FADE, EPS, "half-way alpha");
    app.pump_for(ms(150));
    assert_settled(&app, &registry);
}

fn fade_through_fades_and_scales_in_over_the_last_14_20() {
    let (_navigator, mut app, registry) = pushed(running(PageTransition::FadeThrough));
    app.pump_for(ms(150));
    let progress = Cubic::new(0.0, 0.0, 0.2, 1.0).transform(LATE_FADE);
    let page = painted(&app);
    close(page.alpha, progress, EPS, "half-way alpha");
    close(page.scale, 0.92 + 0.08 * progress, EPS, "half-way scale");
    app.pump_for(ms(150));
    assert_settled(&app, &registry);
}

fn none_cuts_at_once() {
    let (_navigator, mut app, registry) = pushed(running(PageTransition::None));
    app.tick();
    assert_settled(&app, &registry);
}

/// A linear fade over 1000 ms that counts its builds.
struct CountingFade(Rc<Cell<usize>>);

impl PageTransitionBuilder for CountingFade {
    fn duration(&self) -> Duration {
        ms(1000)
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        self.0.set(self.0.get() + 1);
        FadeTransition::new(Rc::clone(primary), child).boxed()
    }
}

fn a_custom_builder_runs_with_its_own_duration() {
    let builds = Rc::new(Cell::new(0));
    let custom = PageTransition::Custom(Rc::new(CountingFade(Rc::clone(&builds))));
    let (_navigator, mut app, registry) = pushed(running(custom));
    app.pump_for(ms(500));
    assert!(builds.get() > 0, "the custom builder wraps the page");
    close(painted(&app).alpha, 0.5, EPS, "half of its 1000 ms");
    app.pump_for(ms(500));
    assert_settled(&app, &registry);
}

/// A route's own transitions win over the theme's.
fn an_explicit_route_transition_wins_over_the_theme() {
    let (navigator, mut app, _registry) = mount(running(PageTransition::FadeThrough));
    let _result =
        navigator.push(page_route().transitions(|_ctx, _primary, _secondary, child| child));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(100));
    let page = painted(&app);
    close(page.alpha, 1.0, EPS, "the route's own jump cut");
    close(page.scale, 1.0, EPS, "no theme scale");
}

/// Under reduced motion the theme's transition settles on the first frame,
/// and it is the theme's builder that paints that frame.
fn reduced_motion_settles_on_the_first_frame() {
    for platform in [
        TargetPlatform::Android,
        TargetPlatform::iOS,
        TargetPlatform::Windows,
    ] {
        let (navigator, mut app, registry) =
            mount(themed(platform, PageTransitionsTheme::default()));
        app.set_motion_override(MotionOverride::Reduce);
        let _result = navigator.push(page_route());
        app.pump_for(Duration::ZERO);
        app.tick();
        assert_settled(&app, &registry);
    }

    let builds = Rc::new(Cell::new(0));
    let custom = PageTransition::Custom(Rc::new(CountingFade(Rc::clone(&builds))));
    let (navigator, mut app, registry) = mount(running(custom));
    app.set_motion_override(MotionOverride::Reduce);
    let _result = navigator.push(page_route());
    app.pump_for(Duration::ZERO);
    app.tick();
    assert!(
        builds.get() > 0,
        "the theme's builder wraps the reduced-motion page"
    );
    assert_settled(&app, &registry);
}

/// The page a push covers; nothing else in these apps paints it.
const COVERED: Color = Color::rgb(30, 140, 60);

/// Push a page over a settled `COVERED` page under `transition`, sample the
/// covered page `at` into the push, then pop and sample it again at the same
/// progress on the way back: the covered page runs its secondary animation
/// forward, then in reverse.
fn covered_page(
    transition: PageTransition,
    duration: Duration,
    at: Duration,
    expect: impl Fn(Sample, &str),
) {
    let rest = duration.saturating_sub(at);
    let (navigator, mut app, registry) = mount(running(transition));
    let _covered = navigator.push(PageRoute::<()>::new(|_ctx, _primary, _secondary| {
        ColoredBox::new(COVERED)
            .child(SizedBox::expand())
            .into_view()
            .boxed()
    }));
    app.pump_for(Duration::ZERO);
    app.pump_for(duration);
    app.tick();
    assert!(!registry.has_running(), "the covered page settled");

    let _result = navigator.push(page_route());
    app.pump_for(Duration::ZERO);
    app.pump_for(at);
    expect(
        sample_of(&app, COVERED).expect("the covered page is painted during the push"),
        "push",
    );
    app.pump_for(rest);
    app.tick();

    assert!(navigator.pop());
    app.pump_for(Duration::ZERO);
    app.pump_for(rest);
    expect(
        sample_of(&app, COVERED).expect("the covered page is painted during the pop"),
        "pop",
    );
    app.pump_for(at);
    app.tick();
    let settled = sample_of(&app, COVERED).expect("the uncovered page is painted");
    close(settled.left, 0.0, PX, "uncovered left");
    close(settled.scale, 1.0, EPS, "uncovered scale");
    close(settled.alpha, 1.0, EPS, "uncovered alpha");
}

/// Fade-forwards slides the covered page a quarter width back and fades it
/// out along the same curve as the entering page.
fn fade_forwards_moves_and_fades_the_covered_page() {
    let progress = curves::EASE_IN_OUT_CUBIC_EMPHASIZED.transform(0.5);
    covered_page(
        PageTransition::FadeForwards,
        ms(450),
        ms(225),
        |page, when| {
            close(page.left, -0.25 * WIDTH * progress, PX, when);
            close(page.alpha, 1.0 - progress, EPS, when);
        },
    );
}

/// Zoom grows the covered page toward 105 %.
fn zoom_grows_the_covered_page() {
    let progress = curves::EASE_IN_OUT_CUBIC_EMPHASIZED.transform(0.5);
    covered_page(PageTransition::Zoom, ms(300), ms(150), |page, when| {
        close(page.scale, 1.0 + 0.05 * progress, EPS, when);
        close(page.alpha, 1.0, EPS, when);
    });
}

/// Shared axis moves the covered page 30 px back (or grows it toward 110 %)
/// and fades it out over the first 30 %: at 15 % it is half faded.
fn shared_axis_moves_and_fades_the_covered_page() {
    let leaving = curves::FAST_OUT_SLOW_IN.transform(0.15);
    covered_page(
        PageTransition::SharedAxis(SharedAxis::Horizontal),
        ms(300),
        ms(45),
        |page, when| {
            close(page.left, -30.0 * leaving, PX, when);
            close(page.alpha, 0.5, EPS, when);
        },
    );
    covered_page(
        PageTransition::SharedAxis(SharedAxis::Scaled),
        ms(300),
        ms(45),
        |page, when| {
            close(page.scale, 1.0 + 0.1 * leaving, EPS, when);
            close(page.alpha, 0.5, EPS, when);
        },
    );
}

/// Fade-through fades the covered page out over the first 30 % along
/// `Cubic(0.4, 0, 1, 1)`, in place.
fn fade_through_fades_the_covered_page() {
    let faded = Cubic::new(0.4, 0.0, 1.0, 1.0).transform(0.5);
    covered_page(
        PageTransition::FadeThrough,
        ms(300),
        ms(45),
        |page, when| {
            close(page.alpha, 1.0 - faded, EPS, when);
            close(page.left, 0.0, PX, when);
            close(page.scale, 1.0, EPS, when);
        },
    );
}

/// Each platform default, one row per transition.
pub fn page_transitions_by_platform_and_variant() {
    common::run_cases(&[
        ("android fade forwards", android_fades_forwards_over_450_ms),
        ("apple parallax slide", apple_platforms_slide_over_500_ms),
        ("desktop zoom", desktop_platforms_zoom_over_300_ms),
        ("shared axis", shared_axis_moves_30_px_and_fades_in_late),
        (
            "scaled shared axis",
            scaled_shared_axis_grows_from_80_percent,
        ),
        (
            "fade through",
            fade_through_fades_and_scales_in_over_the_last_14_20,
        ),
        ("none", none_cuts_at_once),
        ("custom", a_custom_builder_runs_with_its_own_duration),
        (
            "explicit transition",
            an_explicit_route_transition_wins_over_the_theme,
        ),
        ("reduced motion", reduced_motion_settles_on_the_first_frame),
        (
            "fade forwards covered page",
            fade_forwards_moves_and_fades_the_covered_page,
        ),
        ("zoom covered page", zoom_grows_the_covered_page),
        (
            "shared axis covered page",
            shared_axis_moves_and_fades_the_covered_page,
        ),
        (
            "fade through covered page",
            fade_through_fades_the_covered_page,
        ),
    ]);
}

/// `show_dialog` under a `MaterialApp` fades in over 150 ms along
/// `EASE_OUT`: part-way at 75 ms, opaque at 150 ms.
pub fn a_material_dialog_fades_in_over_150_ms() {
    let (navigator, mut app, registry) = mount(ThemeData::light());
    let _result = show_dialog::<(), _, _>(&navigator, |_ctx| {
        ColoredBox::new(PAGE).child(SizedBox::new(100.0, 100.0))
    });
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(75));
    close(
        painted(&app).alpha,
        curves::EASE_OUT.transform(0.5),
        EPS,
        "half-time alpha",
    );
    app.pump_for(ms(75));
    close(painted(&app).alpha, 1.0, EPS, "faded in");
    assert!(!registry.has_running());
}

/// The defaults per platform, and identity equality for custom builders.
pub fn page_transitions_theme_defaults_and_equality() {
    let theme = PageTransitionsTheme::default();
    for (platform, expected) in [
        (TargetPlatform::Android, PageTransition::FadeForwards),
        (TargetPlatform::iOS, PageTransition::Cupertino),
        (TargetPlatform::MacOS, PageTransition::Cupertino),
        (TargetPlatform::Windows, PageTransition::Zoom),
        (TargetPlatform::Linux, PageTransition::Zoom),
        (TargetPlatform::Fuchsia, PageTransition::Zoom),
        (TargetPlatform::Unknown, PageTransition::Zoom),
    ] {
        assert_eq!(theme.transition_for(platform), expected, "{platform}");
    }
    let builder: Rc<dyn PageTransitionBuilder> = Rc::new(CountingFade(Rc::default()));
    let other: Rc<dyn PageTransitionBuilder> = Rc::new(CountingFade(Rc::default()));
    assert_eq!(
        PageTransition::Custom(Rc::clone(&builder)),
        PageTransition::Custom(Rc::clone(&builder))
    );
    assert_ne!(
        PageTransition::Custom(builder),
        PageTransition::Custom(other)
    );
    assert_ne!(
        PageTransition::SharedAxis(SharedAxis::Horizontal),
        PageTransition::SharedAxis(SharedAxis::Vertical)
    );
}
