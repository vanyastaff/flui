//! The ambient page-transition hook (ADR-0221): a route given no transitions
//! adopts the nearest [`PageTransitions`] builder of its kind, with its
//! durations, when its navigator installs it.
//!
//! Every row reads the pushed page out of the committed scene: its painted
//! alpha through the compositor ancestors, so a value pins both the builder
//! that ran and the duration its animation ran over.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{MotionOverride, Vsync};
use flui_painting::{display_list::DrawOp, styling::Color};
use flui_rendering::layer::Layer;
use flui_view::prelude::*;
use flui_view::{BoxedView, BuildContext};
use flui_widgets::navigator::{
    Navigator, NavigatorHandle, PageRoute, PopupRoute, RouteAnimation, SimpleRoute,
};
use flui_widgets::{
    ColoredBox, FadeTransition, PageTransitionBuilder, PageTransitions, SizedBox, VsyncScope,
};

use crate::common::{LaidOut, lay_out_animated, tight};

/// The pushed page's colour; nothing else paints it.
const PAGE: Color = Color::RED;

/// A linear fade over a fixed duration that counts its builds.
struct CountingFade {
    duration: Duration,
    builds: Rc<Cell<usize>>,
}

impl CountingFade {
    fn new(millis: u64) -> (Rc<dyn PageTransitionBuilder>, Rc<Cell<usize>>) {
        let builds = Rc::new(Cell::new(0));
        let builder = Rc::new(Self {
            duration: Duration::from_millis(millis),
            builds: Rc::clone(&builds),
        });
        (builder, builds)
    }
}

impl PageTransitionBuilder for CountingFade {
    fn duration(&self) -> Duration {
        self.duration
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        self.builds.set(self.builds.get() + 1);
        FadeTransition::new(Rc::clone(primary), child)
            .into_view()
            .boxed()
    }
}

fn page(_ctx: &dyn BuildContext, _a: &RouteAnimation, _s: &RouteAnimation) -> BoxedView {
    ColoredBox::new(PAGE)
        .child(SizedBox::new(80.0, 40.0))
        .into_view()
        .boxed()
}

/// A navigator with a plain home route, under `wrap`, on a fresh clock.
fn mount(wrap: impl FnOnce(Navigator) -> BoxedView) -> (NavigatorHandle, LaidOut, Vsync) {
    let registry = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let app = lay_out_animated(
        VsyncScope::new(registry.clone(), wrap(Navigator::new(navigator.clone()))),
        tight(200.0, 100.0),
        registry.clone(),
    );
    (navigator, app, registry)
}

/// The page's alpha in the committed scene, or `None` when it is not painted.
fn page_alpha(app: &LaidOut) -> Option<f64> {
    let scene = app.layer_tree().expect("a committed scene");
    for (_, node) in scene.iter() {
        let Layer::Picture(picture) = node.layer() else {
            continue;
        };
        let painted = picture.picture().iter().any(
            |command| matches!(&command.op, DrawOp::Rect { paint, .. } if paint.color == PAGE),
        );
        if !painted {
            continue;
        }
        let mut alpha = 1.0;
        let mut parent = node.parent();
        while let Some(id) = parent {
            let ancestor = scene.get(id).expect("scene parent exists");
            if let Layer::Opacity(layer) = ancestor.layer() {
                alpha *= layer.alpha();
            }
            parent = ancestor.parent();
        }
        return Some(alpha);
    }
    None
}

fn assert_alpha(app: &LaidOut, expected: f64, what: &str) {
    let alpha = page_alpha(app).expect("the pushed page is painted");
    assert!(
        (alpha - expected).abs() < 1e-9,
        "{what}: alpha {alpha}, expected {expected}"
    );
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// A page route given no transitions adopts the hook's builder and its
/// duration: half way through the hook's 1000 ms it is half faded, which
/// neither the 300 ms default nor a jump cut would paint, and it settles at
/// the hook's end.
pub(crate) fn a_page_route_adopts_the_ambient_builder_and_its_duration() {
    let (fade, builds) = CountingFade::new(1000);
    let (navigator, mut app, registry) =
        mount(|navigator| PageTransitions::new(navigator).page(fade).boxed());
    let _result = navigator.push(PageRoute::<()>::new(page));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(500));
    assert_alpha(&app, 0.5, "half way through the hook's duration");
    assert!(builds.get() > 0, "the hook's builder wraps the page");
    app.pump_for(ms(400));
    assert!(
        registry.has_running(),
        "still running past 300 ms and 900 ms"
    );
    app.pump_for(ms(100));
    assert_alpha(&app, 1.0, "settled at the hook's end");
    assert!(!registry.has_running());
}

/// With no hook in scope a route keeps the jump cut: fully painted on its
/// first frame.
pub(crate) fn a_route_with_no_hook_in_scope_keeps_the_jump_cut() {
    let (navigator, mut app, _registry) = mount(|navigator| navigator.into_view().boxed());
    let _result = navigator.push(PageRoute::<()>::new(page));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(100));
    assert_alpha(&app, 1.0, "a jump cut");
}

/// An explicit `.transitions(…)` wins: the hook's builder never runs. An
/// explicit duration alone keeps the hook's builder but runs it over the
/// route's own duration.
pub(crate) fn an_explicit_transition_or_duration_wins_over_the_hook() {
    let (fade, builds) = CountingFade::new(1000);
    let (navigator, mut app, _registry) =
        mount(|navigator| PageTransitions::new(navigator).page(fade).boxed());
    let _cut = navigator
        .push(PageRoute::<()>::new(page).transitions(|_ctx, _primary, _secondary, child| child));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(500));
    assert_eq!(builds.get(), 0, "the explicit transition replaced the hook");
    assert_alpha(&app, 1.0, "the explicit jump cut");
    app.pump_for(ms(1000));
    assert!(navigator.pop());
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(1000));
    app.tick();

    let _timed = navigator.push(PageRoute::<()>::new(page).transition_duration(ms(2000)));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(1000));
    assert_alpha(&app, 0.5, "the hook's fade over the route's own 2000 ms");
    assert!(builds.get() > 0);
}

/// A popup adopts the popup slot and a page the page slot. A nested scope
/// that fills only the popup slot keeps the enclosing page builder.
pub(crate) fn each_route_kind_adopts_the_nearest_builder_of_its_kind() {
    let (outer_page, page_builds) = CountingFade::new(1000);
    let (outer_popup, outer_popup_builds) = CountingFade::new(400);
    let (inner_popup, popup_builds) = CountingFade::new(200);
    let (navigator, mut app, _registry) = mount(|navigator| {
        PageTransitions::new(PageTransitions::new(navigator).popup(inner_popup))
            .page(outer_page)
            .popup(outer_popup)
            .boxed()
    });
    let _popup = navigator.push(PopupRoute::<()>::new(page));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(100));
    assert_alpha(&app, 0.5, "the inner popup slot's 200 ms");
    assert!(popup_builds.get() > 0);
    assert_eq!(page_builds.get(), 0, "a popup never takes the page slot");
    assert_eq!(outer_popup_builds.get(), 0, "the nearest popup slot wins");
    app.pump_for(ms(100));
    assert!(navigator.pop());
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(200));
    app.tick();

    let _page = navigator.push(PageRoute::<()>::new(page));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(250));
    assert_alpha(&app, 0.25, "the outer page slot's 1000 ms");
}

/// A route adopts its builder once, at install: replacing the hook while the
/// route is entering leaves its running transition alone.
pub(crate) fn a_replaced_hook_does_not_swap_a_running_transition() {
    let (first, first_builds) = CountingFade::new(1000);
    let (second, second_builds) = CountingFade::new(100);
    let registry = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let app_with = |hook: Rc<dyn PageTransitionBuilder>| {
        VsyncScope::new(
            registry.clone(),
            PageTransitions::new(Navigator::new(navigator.clone())).page(hook),
        )
    };
    let mut app = lay_out_animated(app_with(first), tight(200.0, 100.0), registry.clone());
    let _result = navigator.push(PageRoute::<()>::new(page));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(250));
    app.pump_widget(app_with(second));
    app.pump_for(ms(250));
    assert_alpha(&app, 0.5, "the adopted 1000 ms fade keeps running");
    assert!(first_builds.get() > 0);
    assert_eq!(
        second_builds.get(),
        0,
        "the installed route never swaps builders"
    );
}

/// Under reduced motion the route animation settles on its first frame, and
/// the adopted builder paints the settled page.
pub(crate) fn an_adopted_transition_settles_on_the_first_reduced_motion_frame() {
    let (fade, builds) = CountingFade::new(1000);
    let (navigator, mut app, registry) =
        mount(|navigator| PageTransitions::new(navigator).page(fade).boxed());
    app.set_motion_override(MotionOverride::Reduce);
    let _result = navigator.push(PageRoute::<()>::new(page));
    app.pump_for(Duration::ZERO);
    app.tick();
    assert!(builds.get() > 0, "the adopted builder still wraps the page");
    assert_alpha(&app, 1.0, "settled on the first frame");
    assert!(!registry.has_running());
}
