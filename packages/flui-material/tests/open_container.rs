//! `OpenContainer`: the container transform, read from the committed scene on
//! virtual time.
//!
//! A home page holds a closed container (200 x 100 at 40, 100) above a marker,
//! inside a scroll view, under a `MaterialApp`. Each row taps the container
//! and reads where the open page's rect lands (the page is a coloured box that
//! the morph scales to the surface width, so its painted left, top and scale are
//! the surface's), what the surface paints, and what is announced or hit.

use crate::common;
use crate::page_transitions::{EPS, PX, close, ms, sample_of};

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::{LaidOut, lay_out_animated, tight};
use flui_material::{
    CloseAction, ContainerTransition, MaterialApp, OpenAction, OpenContainer, Theme, ThemeData,
};
use flui_sdk::animation::{Curve, MotionOverride, Vsync, curves};
use flui_sdk::geometry::{EdgeInsets, Point, Radius};
use flui_sdk::painting::{Alignment, BorderRadius, Color, DrawOp};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    ColoredBox, Column, CrossAxisAlignment, MainAxisSize, NavigatorHandle, Padding, PageRoute,
    Routable, RouteParseError, RoutePath, Router, ScrollController, Semantics,
    SingleChildScrollView, SizedBox, Transform, VsyncScope, WidgetsApp,
};
use flui_testing::{Action, ActionRequest, TreeId};

const WIDTH: f64 = 400.0;
const HEIGHT: f64 = 800.0;
/// Where the closed container sits and how big it is.
const CLOSED_LEFT: f64 = 40.0;
const CLOSED_TOP: f64 = 100.0;
const CLOSED_WIDTH: f64 = 200.0;
const CLOSED_HEIGHT: f64 = 100.0;
/// The closed content, the open page and the marker below the container.
const CLOSED_CONTENT: Color = Color::rgb(10, 100, 200);
const PAGE: Color = Color::rgb(201, 33, 77);
const MARKER: Color = Color::rgb(30, 140, 60);
/// The two surface colours; nothing else paints them.
const CLOSED_SURFACE: Color = Color::rgb(250, 240, 200);
const OPEN_SURFACE: Color = Color::rgb(200, 250, 240);
const CLOSED_ELEVATION: f64 = 8.0;
const CLOSED_RADIUS: f64 = 12.0;
/// What a closed container announces, and what the open page announces.
const CLOSED_LABEL: &str = "Open details";
const PAGE_LABEL: &str = "Details page";

struct Rig {
    navigator: Rc<RefCell<Option<NavigatorHandle>>>,
    /// Space above the app, from [`Setup::inset`].
    inset: f64,
    /// The app's scale, from [`Setup::scale`].
    scale: f64,
    app: LaidOut,
    registry: Vsync,
    scroll: ScrollController,
    results: Rc<RefCell<Vec<Option<i32>>>>,
    close: Rc<RefCell<Option<CloseAction<i32>>>>,
    open: Rc<RefCell<Option<OpenAction>>>,
}

/// How a rig's app is put together.
#[derive(Clone, Copy)]
struct Setup {
    transition: ContainerTransition,
    tappable: bool,
    /// Space above the app, so the navigator does not sit at the window's
    /// origin.
    inset: f64,
    /// Mount the home page under a `Router` (a `WidgetsApp`) instead of a
    /// `MaterialApp` navigator.
    router: bool,
    /// The morph's duration, when not the default.
    duration: Option<Duration>,
    /// A uniform scale on the whole app, about the window's top left.
    scale: f64,
    /// The morph's curve, when not the default.
    curve: Option<fn(f64) -> f64>,
}

impl Setup {
    fn new(transition: ContainerTransition) -> Self {
        Self {
            transition,
            tappable: true,
            inset: 0.0,
            router: false,
            duration: None,
            scale: 1.0,
            curve: None,
        }
    }
}

/// The one location of the `Router` rig.
#[derive(Debug, Clone, PartialEq)]
struct HomeLocation;

impl Routable for HomeLocation {
    fn to_path(&self) -> RoutePath {
        RoutePath::root()
    }

    fn from_path(path: &RoutePath) -> Result<Self, RouteParseError> {
        if path.segments().next().is_none() {
            Ok(Self)
        } else {
            Err(RouteParseError::NoMatch { path: path.clone() })
        }
    }
}

/// A curve given as a plain function.
#[derive(Clone, Copy)]
struct FnCurve(fn(f64) -> f64);

impl Curve for FnCurve {
    fn transform(&self, t: f64) -> f64 {
        (self.0)(t)
    }
}

fn rig(transition: ContainerTransition) -> Rig {
    rig_with(Setup::new(transition))
}

fn rig_with(setup: Setup) -> Rig {
    let Setup {
        transition,
        tappable,
        inset,
        router,
        duration,
        scale,
        curve,
    } = setup;
    let registry = Vsync::new();
    let scroll = ScrollController::new();
    let results = Rc::new(RefCell::new(Vec::new()));
    let close_slot = Rc::new(RefCell::new(None));
    let open_slot = Rc::new(RefCell::new(None));
    let navigator_slot = Rc::new(RefCell::new(None));

    let container = OpenContainer::<i32>::new(
        {
            let open_slot = Rc::clone(&open_slot);
            let navigator_slot = Rc::clone(&navigator_slot);
            move |ctx, open| {
                *open_slot.borrow_mut() = Some(open.clone());
                *navigator_slot.borrow_mut() = NavigatorHandle::maybe_of(ctx);
                ColoredBox::new(CLOSED_CONTENT).child(SizedBox::expand())
            }
        },
        {
            let close_slot = Rc::clone(&close_slot);
            move |_ctx, close| {
                *close_slot.borrow_mut() = Some(close.clone());
                Semantics::new()
                    .container(true)
                    .label(PAGE_LABEL)
                    .child(ColoredBox::new(PAGE).child(SizedBox::expand()))
            }
        },
    )
    .closed_color(CLOSED_SURFACE)
    .open_color(OPEN_SURFACE)
    .closed_elevation(CLOSED_ELEVATION)
    .open_elevation(0.0)
    .closed_radius(BorderRadius::all(Radius::circular(CLOSED_RADIUS)))
    .open_radius(BorderRadius::all(Radius::ZERO))
    .transition(transition)
    .semantic_label(CLOSED_LABEL)
    .tappable(tappable)
    .on_closed({
        let results = Rc::clone(&results);
        move |value| results.borrow_mut().push(value)
    });

    let container = match duration {
        Some(duration) => container.duration(duration),
        None => container,
    };
    let container = match curve {
        Some(curve) => container.curve(FnCurve(curve)),
        None => container,
    };

    let home = SingleChildScrollView::new()
        .position(scroll.position())
        .child(
            Column::new(vec![
                Padding::new(EdgeInsets::new(CLOSED_TOP, 0.0, 0.0, CLOSED_LEFT))
                    .child(SizedBox::new(CLOSED_WIDTH, CLOSED_HEIGHT).child(container))
                    .into_view()
                    .boxed(),
                ColoredBox::new(MARKER)
                    .child(SizedBox::new(WIDTH, 50.0))
                    .into_view()
                    .boxed(),
                SizedBox::new(WIDTH, 2000.0).into_view().boxed(),
            ])
            .main_axis_size(MainAxisSize::Min)
            .cross_axis_alignment(CrossAxisAlignment::Start),
        );

    let home: BoxedView = home.into_view().boxed();
    let app: BoxedView = if router {
        let router = Router::from_location("/", move |_route: &HomeLocation, _ctx| home.clone())
            .expect("the root location parses");
        // A Router app has no `MaterialApp` above it to provide the theme.
        Theme::new(ThemeData::light(), WidgetsApp::router(router))
            .into_view()
            .boxed()
    } else {
        // A page route keeps the home page mounted under the pushed one, as an
        // app that wants its scroll position and its containers to survive does.
        let navigator = NavigatorHandle::new();
        let _home =
            navigator.seed_initial(PageRoute::<()>::new(move |_ctx, _primary, _secondary| {
                home.clone()
            }));
        MaterialApp::new(SizedBox::shrink())
            .navigator(navigator)
            .into_view()
            .boxed()
    };
    let app = lay_out_animated(
        VsyncScope::new(
            registry.clone(),
            Transform::scale(scale, scale)
                .alignment(Alignment::TOP_LEFT)
                .child(Padding::new(EdgeInsets::new(inset, 0.0, 0.0, 0.0)).child(app)),
        ),
        tight(WIDTH, HEIGHT),
        registry.clone(),
    );
    Rig {
        navigator: navigator_slot,
        inset,
        scale,
        app,
        registry,
        scroll,
        results,
        close: close_slot,
        open: open_slot,
    }
}

impl Rig {
    /// Tap the closed container and anchor the route's clock.
    fn open(&mut self) {
        let (x, y) = (
            self.scale * (CLOSED_LEFT + CLOSED_WIDTH / 2.0),
            self.scale * (self.inset + CLOSED_TOP + CLOSED_HEIGHT / 2.0),
        );
        self.app.dispatch_pointer_down(x, y);
        self.app.dispatch_pointer_up(x, y);
        self.app.pump_for(Duration::ZERO);
    }

    /// Run the entrance to its end.
    fn settle(&mut self) {
        self.app.pump_for(ms(300));
        self.app.tick();
    }

    /// Run the exit to its end, then the frames the route's removal and the
    /// delivery of its result take.
    fn finish_exit(&mut self) {
        self.app.pump_for(ms(300));
        for _ in 0..3 {
            self.app.tick();
        }
    }

    /// Close the page with no value and anchor the exit's clock.
    fn dismiss(&mut self) {
        let close = self.close.borrow().clone().expect("the page was built");
        assert!(close.close(), "the page is the current route");
        self.app.pump_for(Duration::ZERO);
    }

    fn routes(&self) -> usize {
        self.navigator
            .borrow()
            .as_ref()
            .expect("the closed content found its navigator")
            .route_ids()
            .len()
    }
}

fn assert_page_settled(rig: &Rig) {
    let page = sample_of(&rig.app, PAGE).expect("the page is painted");
    close(page.left, 0.0, PX, "settled left");
    close(page.top, 0.0, PX, "settled top");
    close(page.scale, 1.0, EPS, "settled scale");
    close(page.alpha, 1.0, EPS, "settled alpha");
    assert!(!rig.registry.has_running(), "the transition has finished");
}

/// The closed content is back where it was, fully opaque, and the page is gone.
fn assert_closed_again(rig: &Rig) {
    let content = sample_of(&rig.app, CLOSED_CONTENT).expect("the closed content is painted");
    close(content.left, CLOSED_LEFT, PX, "closed left");
    close(content.top, CLOSED_TOP, PX, "closed top");
    close(content.alpha, 1.0, EPS, "closed alpha");
    assert!(sample_of(&rig.app, PAGE).is_none(), "the page is gone");
}

/// The fill path the morph's surface paints in `color`, in its own coordinates.
fn surface_path(app: &LaidOut, color: Color) -> Option<flui_sdk::painting::Path> {
    app.draw_ops()
        .into_iter()
        .find_map(|command| match command.op {
            DrawOp::Path { path, paint } if paint.color == color => Some(path),
            _ => None,
        })
}

/// The elevation of the first shadow in the scene.
fn shadow_elevation(app: &LaidOut) -> Option<f64> {
    app.draw_ops()
        .into_iter()
        .find_map(|command| match command.op {
            DrawOp::Shadow { elevation, .. } => Some(elevation),
            _ => None,
        })
}

fn eased(raw: f64) -> f64 {
    curves::FAST_OUT_SLOW_IN.transform(raw)
}

/// Half time: the page's rect is strictly between the closed rect and the
/// navigator's, the surface has morphed its colour, elevation and corners by
/// the same progress, and the closed container still holds its place, so what
/// sits below it does not move.
fn half_time_rect_is_between_and_the_placeholder_keeps_size() {
    let mut rig = rig(ContainerTransition::Fade);
    let marker = sample_of(&rig.app, MARKER).expect("the marker is painted");
    close(marker.top, CLOSED_TOP + CLOSED_HEIGHT, PX, "marker below");

    rig.open();
    rig.app.pump_for(ms(150));
    let t = eased(0.5);
    assert!(0.3 < t && t < 0.9, "the curve is not linear here: {t}");

    let page = sample_of(&rig.app, PAGE).expect("the page is painted mid-flight");
    assert!(
        0.0 < page.left && page.left < CLOSED_LEFT,
        "left is between: {}",
        page.left
    );
    assert!(
        0.0 < page.top && page.top < CLOSED_TOP,
        "top is between: {}",
        page.top
    );
    let closed_scale = CLOSED_WIDTH / WIDTH;
    assert!(
        closed_scale < page.scale && page.scale < 1.0,
        "width is between: {}",
        page.scale
    );
    close(
        page.left,
        CLOSED_LEFT * (1.0 - t),
        PX,
        "left follows the curve",
    );
    close(
        page.top,
        CLOSED_TOP * (1.0 - t),
        PX,
        "top follows the curve",
    );
    close(
        page.scale,
        closed_scale + (1.0 - closed_scale) * t,
        EPS,
        "width follows the curve",
    );

    let marker = sample_of(&rig.app, MARKER).expect("the marker is painted");
    close(
        marker.top,
        CLOSED_TOP + CLOSED_HEIGHT,
        PX,
        "the placeholder keeps the closed size, so the marker stays",
    );
    close(marker.left, 0.0, PX, "the marker's left");

    let color = Color::lerp(CLOSED_SURFACE, OPEN_SURFACE, t);
    let mut path = surface_path(&rig.app, color).expect("the surface paints its morphed colour");
    assert!(
        surface_path(&rig.app, CLOSED_SURFACE).is_none(),
        "the closed surface is not drawn beside the morph"
    );
    let elevation = shadow_elevation(&rig.app).expect("the surface casts a shadow");
    close(
        elevation,
        CLOSED_ELEVATION * (1.0 - t),
        EPS,
        "elevation follows the curve",
    );
    let corner = Point::new(0.3, 0.3);
    assert!(
        !path.contains(corner),
        "the corners are still rounded at half time: radius {}",
        CLOSED_RADIUS * (1.0 - t)
    );
    let bounds = path.bounds();
    assert!(
        bounds.width() > CLOSED_WIDTH && bounds.width() < WIDTH,
        "the surface is between the closed and the open width: {}",
        bounds.width()
    );

    rig.app.pump_for(ms(150));
    rig.app.tick();
    assert_page_settled(&rig);
}

/// The fade schedules: the closed copy and the open page trade places on the
/// eased progress, with the dissolve overlapping and the fade through not.
fn the_transitions_fade_content_on_their_own_schedules() {
    for (transition, closed_end, open_start) in [
        (ContainerTransition::Fade, 0.4, 0.2),
        (ContainerTransition::FadeThrough, 0.3, 0.3),
    ] {
        let mut rig = rig(transition);
        rig.open();
        rig.app.pump_for(ms(30));
        let early = eased(0.1);
        let closed = sample_of(&rig.app, CLOSED_CONTENT).expect("the closed copy is painted");
        close(
            closed.alpha,
            1.0 - early / closed_end,
            EPS,
            &format!("{transition:?}: the closed copy fades out"),
        );
        assert!(
            early < open_start,
            "the open page has not started at 10 %: {early}"
        );

        rig.app.pump_for(ms(120));
        let late = eased(0.5);
        let page = sample_of(&rig.app, PAGE).expect("the page is painted");
        close(
            page.alpha,
            (late - open_start) / (1.0 - open_start),
            EPS,
            &format!("{transition:?}: the open page fades in"),
        );
        assert!(
            sample_of(&rig.app, CLOSED_CONTENT).is_none(),
            "{transition:?}: the closed copy is gone by half time"
        );
    }
}

/// Closing with a value resolves `on_closed` with it, closing with none
/// resolves `None`, and the source comes back each time.
fn pop_with_a_result_resolves_on_closed() {
    let mut rig = rig(ContainerTransition::Fade);
    rig.open();
    rig.settle();
    assert_page_settled(&rig);

    let close_action = rig.close.borrow().clone().expect("the page was built");
    assert!(close_action.close_with(7));
    rig.app.pump_for(Duration::ZERO);
    // The route has resolved its result, but the exit morph is still on
    // screen: the result waits until the source is back.
    rig.app.pump_for(ms(150));
    assert!(
        sample_of(&rig.app, CLOSED_CONTENT).is_some(),
        "the exit is still running"
    );
    assert!(
        rig.results.borrow().is_empty(),
        "no result under the closing transform"
    );
    rig.app.pump_for(ms(150));
    rig.finish_exit();
    assert_eq!(*rig.results.borrow(), vec![Some(7)], "the result arrives");
    assert_closed_again(&rig);
    assert_eq!(rig.routes(), 1, "the page left the navigator");

    rig.open();
    rig.settle();
    rig.dismiss();
    rig.finish_exit();
    assert_eq!(*rig.results.borrow(), vec![Some(7), None]);
    assert_closed_again(&rig);
}

/// The exit runs the same curve flipped: halfway back the rect is near the
/// closed rect, where the entrance was near the open one.
fn the_exit_runs_the_curve_flipped() {
    let mut rig = rig(ContainerTransition::Fade);
    rig.open();
    rig.settle();
    rig.dismiss();
    rig.app.pump_for(ms(150));
    let t = 1.0 - eased(0.5);
    let page = sample_of(&rig.app, PAGE).expect("the page is painted on the way back");
    close(
        page.left,
        CLOSED_LEFT * (1.0 - t),
        PX,
        "left on the way back",
    );
    close(page.top, CLOSED_TOP * (1.0 - t), PX, "top on the way back");
    assert!(
        page.left > CLOSED_LEFT / 2.0,
        "more than halfway back at half time: {}",
        page.left
    );
}

/// Scrolling the source while the page covers it moves where the exit lands:
/// the reverse run ends at the new rect, not the one measured at the push.
fn scrolling_between_push_and_pop_moves_the_reverse_end() {
    let shift = 30.0;
    let mut rig = rig(ContainerTransition::Fade);
    rig.open();
    rig.settle();
    rig.scroll.jump_to(shift);
    rig.app.pump_for(ms(16));
    rig.dismiss();
    rig.app.pump_for(ms(285));

    // The page has faded out by now; the closed copy the morph draws marks the
    // surface's top.
    let t = 1.0 - eased(0.95);
    let copy =
        sample_of(&rig.app, CLOSED_CONTENT).expect("the closed copy is painted near the end");
    close(
        copy.top,
        (CLOSED_TOP - shift) * (1.0 - t),
        0.5,
        "the exit heads for the scrolled rect",
    );
    assert!(
        copy.top < CLOSED_TOP - shift / 2.0,
        "not the rect measured at the push: {}",
        copy.top
    );

    rig.app.pump_for(ms(15));
    rig.app.tick();
    let content = sample_of(&rig.app, CLOSED_CONTENT).expect("the closed content is back");
    close(
        content.top,
        CLOSED_TOP - shift,
        PX,
        "it lands where it scrolled to",
    );
}

/// Under reduced motion both directions settle on their first frame.
fn reduced_motion_settles_on_the_first_frame() {
    let mut rig = rig(ContainerTransition::Fade);
    rig.app.set_motion_override(MotionOverride::Reduce);
    rig.open();
    rig.app.tick();
    assert_page_settled(&rig);

    // The exit settles on its first frame too: the route has left by the next
    // one, with no time elapsed. (The tap's 100 ms pressed-state delay, paused
    // while the source was offstage, is a timer, not motion, so it keeps running.)
    rig.dismiss();
    rig.app.tick();
    assert_eq!(rig.routes(), 1, "the exit settled on its first frame");
    assert!(sample_of(&rig.app, PAGE).is_none(), "the page is gone");
    rig.finish_exit();
    assert_closed_again(&rig);
    assert_eq!(*rig.results.borrow(), vec![None]);
}

/// The closed container is one button; while the page flies neither the frozen
/// source nor the morph's closed copy is announced, and assistive technology
/// opens the page with a click.
fn the_closed_container_is_a_button_and_one_surface_is_announced() {
    let mut rig = rig(ContainerTransition::Fade);
    rig.app.enable_semantics();
    rig.app.pump_for(Duration::ZERO);

    let tree = rig.app.a11y_tree().expect("semantics enabled");
    let buttons = tree.find_all_by_label(CLOSED_LABEL);
    assert_eq!(buttons.len(), 1, "{}", tree.describe());
    assert_eq!(buttons[0].role(), flui_testing::a11y::Role::Button);
    assert!(buttons[0].supports_action(Action::Click));
    let id = buttons[0].id();
    rig.app
        .invoke_semantics_action(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: id,
            data: None,
        })
        .expect("the button accepts a click");
    rig.app.pump_for(Duration::ZERO);
    rig.app.pump_for(ms(150));
    assert_eq!(rig.routes(), 2, "the click opened the page");

    let tree = rig.app.a11y_tree().expect("semantics enabled");
    assert!(
        tree.find_all_by_label(CLOSED_LABEL).is_empty(),
        "neither the frozen source nor the closed copy is announced: {}",
        tree.describe()
    );
    assert_eq!(
        tree.find_all_by_label(PAGE_LABEL).len(),
        1,
        "the page is: {}",
        tree.describe()
    );

    rig.app.pump_for(ms(150));
    rig.app.tick();
    rig.dismiss();
    rig.finish_exit();
    let tree = rig.app.a11y_tree().expect("semantics enabled");
    assert_eq!(
        tree.find_all_by_label(CLOSED_LABEL).len(),
        1,
        "the button is back: {}",
        tree.describe()
    );
}

/// With the navigator below the window's origin, the frame the page is pushed
/// shows the closed content exactly where it sat, never shifted by the
/// navigator's offset, and the morph then runs from that rect.
fn the_first_frame_keeps_the_source_in_place_under_an_offset_navigator() {
    let inset = 60.0;
    let mut rig = rig_with(Setup {
        inset,
        ..Setup::new(ContainerTransition::Fade)
    });
    let content = sample_of(&rig.app, CLOSED_CONTENT).expect("the closed content is painted");
    close(
        content.top,
        inset + CLOSED_TOP,
        PX,
        "closed top before the tap",
    );

    rig.open();
    let content = sample_of(&rig.app, CLOSED_CONTENT).expect("the closed content is painted");
    close(
        content.top,
        inset + CLOSED_TOP,
        PX,
        "the push frame shows the closed content in place",
    );
    close(content.left, CLOSED_LEFT, PX, "and at its left");

    rig.app.pump_for(ms(150));
    let t = eased(0.5);
    let page = sample_of(&rig.app, PAGE).expect("the page is painted mid-flight");
    close(
        page.top,
        inset + CLOSED_TOP * (1.0 - t),
        PX,
        "the morph runs from the source's rect inside the navigator",
    );
}

/// Under a scale above the navigator, the morph runs from where the source
/// appears: the scale is applied once, not to the source's on-screen rect a
/// second time.
fn a_scaled_navigator_morphs_from_the_source_as_it_appears() {
    let scale = 0.5;
    let mut rig = rig_with(Setup {
        scale,
        ..Setup::new(ContainerTransition::Fade)
    });
    let content = sample_of(&rig.app, CLOSED_CONTENT).expect("the closed content is painted");
    close(
        content.left,
        scale * CLOSED_LEFT,
        PX,
        "closed left on screen",
    );
    close(content.top, scale * CLOSED_TOP, PX, "closed top on screen");

    rig.open();
    rig.app.pump_for(ms(150));
    let t = eased(0.5);
    let page = sample_of(&rig.app, PAGE).expect("the page is painted mid-flight");
    close(
        page.left,
        scale * CLOSED_LEFT * (1.0 - t),
        PX,
        "left from the source as it appears",
    );
    close(
        page.top,
        scale * CLOSED_TOP * (1.0 - t),
        PX,
        "top from the source as it appears",
    );
    let closed_scale = CLOSED_WIDTH / WIDTH;
    close(
        page.scale,
        scale * (closed_scale + (1.0 - closed_scale) * t),
        EPS,
        "width from the source as it appears",
    );
}

/// A custom curve may return any finite value. One that spikes to the edge of
/// `f64` mid-run must not carry the surface to infinity: it is drawn at the
/// bounded overshoot, with finite geometry.
fn a_spiking_curve_keeps_the_geometry_finite() {
    fn spike(t: f64) -> f64 {
        if t > 0.0 && t < 1.0 { 1e308 } else { t }
    }
    let mut rig = rig_with(Setup {
        curve: Some(spike),
        ..Setup::new(ContainerTransition::Fade)
    });
    rig.open();
    rig.app.pump_for(ms(150));
    let page = sample_of(&rig.app, PAGE).expect("the page is painted at the bounded overshoot");
    for (value, what) in [
        (page.left, "left"),
        (page.top, "top"),
        (page.scale, "scale"),
    ] {
        assert!(value.is_finite(), "{what} is finite: {value}");
    }
    assert!(page.scale > 1.0, "drawn past the open rect: {}", page.scale);

    rig.app.pump_for(ms(150));
    rig.app.tick();
    assert_page_settled(&rig);
}

/// A zero duration settles the route before the morph mounts, so no animation
/// tick ever measures its frame: the page must still appear, and be the thing a
/// tap reaches, instead of staying offstage over a frozen source.
fn a_zero_duration_opens_at_once() {
    let mut rig = rig_with(Setup {
        duration: Some(Duration::ZERO),
        ..Setup::new(ContainerTransition::Fade)
    });
    rig.open();
    rig.app.tick();
    rig.app.tick();
    assert_page_settled(&rig);
    assert!(
        sample_of(&rig.app, CLOSED_CONTENT).is_none(),
        "the source is frozen under the page"
    );

    rig.dismiss();
    rig.finish_exit();
    assert_closed_again(&rig);
    assert_eq!(*rig.results.borrow(), vec![None]);
}

/// Under a `Router`, whose navigator admits only popups, the container still
/// opens its page, morphs it, and delivers the result.
fn it_opens_under_a_router() {
    let mut rig = rig_with(Setup {
        router: true,
        ..Setup::new(ContainerTransition::Fade)
    });
    // The Router mounts its first page over a few frames.
    for _ in 0..10 {
        rig.app.pump_for(ms(50));
    }
    rig.open();
    assert_eq!(rig.routes(), 2, "the page was pushed");
    rig.app.pump_for(ms(150));
    let page = sample_of(&rig.app, PAGE).expect("the page is painted mid-flight");
    assert!(
        0.0 < page.top && page.top < CLOSED_TOP,
        "the page morphs: {}",
        page.top
    );

    rig.settle();
    let close_action = rig.close.borrow().clone().expect("the page was built");
    assert!(close_action.close_with(3));
    rig.app.pump_for(Duration::ZERO);
    rig.finish_exit();
    assert_eq!(*rig.results.borrow(), vec![Some(3)]);
    assert_closed_again(&rig);
}

/// A container that is not tappable is announced by its label but is no
/// button: it offers no click, and a tap does not open the page. Its closed
/// content still opens it through the `OpenAction` it was handed.
fn an_untappable_container_is_no_button() {
    let mut rig = rig_with(Setup {
        tappable: false,
        ..Setup::new(ContainerTransition::Fade)
    });
    rig.app.enable_semantics();
    rig.app.pump_for(Duration::ZERO);

    let tree = rig.app.a11y_tree().expect("semantics enabled");
    let nodes = tree.find_all_by_label(CLOSED_LABEL);
    assert_eq!(nodes.len(), 1, "{}", tree.describe());
    assert_ne!(nodes[0].role(), flui_testing::a11y::Role::Button);
    assert!(!nodes[0].supports_action(Action::Click));

    rig.open();
    rig.app.pump_for(ms(16));
    assert_eq!(rig.routes(), 1, "a tap does not open it");

    let action = rig.open.borrow().clone().expect("the closed content built");
    action.open();
    rig.app.pump_for(Duration::ZERO);
    assert_eq!(rig.routes(), 2, "its OpenAction does");
}

/// While the page is open the source cannot open a second one, whether the
/// request comes from a tap on the flying surface or from a call that still
/// reaches the hidden source's action.
fn the_page_opens_once() {
    let mut rig = rig(ContainerTransition::Fade);
    let action = rig.open.borrow().clone().expect("the closed content built");
    rig.open();
    rig.app.pump_for(ms(150));
    assert_eq!(rig.routes(), 2);

    action.open();
    action.open();
    rig.app.dispatch_pointer_down(150.0, 160.0);
    rig.app.dispatch_pointer_up(150.0, 160.0);
    rig.app.pump_for(ms(16));
    assert_eq!(rig.routes(), 2, "no second page opened");

    rig.app.pump_for(ms(300));
    rig.app.tick();
    rig.dismiss();
    rig.finish_exit();
    action.open();
    rig.app.pump_for(Duration::ZERO);
    assert_eq!(rig.routes(), 2, "the page opens again once it has closed");
}

/// Every `OpenContainer` row.
pub fn open_container_morph_by_scenario() {
    common::run_cases(&[
        (
            "half time",
            half_time_rect_is_between_and_the_placeholder_keeps_size,
        ),
        (
            "fade schedules",
            the_transitions_fade_content_on_their_own_schedules,
        ),
        ("pop result", pop_with_a_result_resolves_on_closed),
        ("flipped exit", the_exit_runs_the_curve_flipped),
        (
            "reverse re-measure",
            scrolling_between_push_and_pop_moves_the_reverse_end,
        ),
        ("reduced motion", reduced_motion_settles_on_the_first_frame),
        (
            "semantics",
            the_closed_container_is_a_button_and_one_surface_is_announced,
        ),
        ("opens once", the_page_opens_once),
        ("untappable", an_untappable_container_is_no_button),
        (
            "offset navigator",
            the_first_frame_keeps_the_source_in_place_under_an_offset_navigator,
        ),
        ("router", it_opens_under_a_router),
        ("zero duration", a_zero_duration_opens_at_once),
        (
            "scaled navigator",
            a_scaled_navigator_morphs_from_the_source_as_it_appears,
        ),
        ("spiking curve", a_spiking_curve_keeps_the_geometry_finite),
    ]);
}
