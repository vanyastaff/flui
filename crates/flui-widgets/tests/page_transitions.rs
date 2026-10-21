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

/// A second pushed page's colour.
const NEXT_PAGE: Color = Color::BLUE;

/// A linear fade over a fixed duration that counts its builds.
struct CountingFade {
    duration: Duration,
    builds: Rc<Cell<usize>>,
}

impl CountingFade {
    fn counting(millis: u64) -> (Rc<dyn PageTransitionBuilder>, Rc<Cell<usize>>) {
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
    alpha_of(app, PAGE)
}

/// The alpha of whatever paints `color`, or `None` when nothing does.
fn alpha_of(app: &LaidOut, color: Color) -> Option<f64> {
    let scene = app.layer_tree().expect("a committed scene");
    for (_, node) in scene.iter() {
        let Layer::Picture(picture) = node.layer() else {
            continue;
        };
        let painted = picture.picture().iter().any(
            |command| matches!(&command.op, DrawOp::Rect { paint, .. } if paint.color == color),
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
    let (fade, builds) = CountingFade::counting(1000);
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
    let (fade, builds) = CountingFade::counting(1000);
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
    let (outer_page, page_builds) = CountingFade::counting(1000);
    let (outer_popup, outer_popup_builds) = CountingFade::counting(400);
    let (inner_popup, popup_builds) = CountingFade::counting(200);
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
/// route is entering leaves its running transition alone, and the next route
/// pushed adopts the replacement.
pub(crate) fn a_replaced_hook_does_not_swap_a_running_transition() {
    let (first, first_builds) = CountingFade::counting(1000);
    let (second, second_builds) = CountingFade::counting(100);
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

    app.pump_for(ms(500));
    assert_alpha(&app, 1.0, "the first route settled");
    let _next = navigator.push(PageRoute::<()>::new(|_ctx, _primary, _secondary| {
        ColoredBox::new(NEXT_PAGE)
            .child(SizedBox::new(80.0, 40.0))
            .into_view()
            .boxed()
    }));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(50));
    let alpha = alpha_of(&app, NEXT_PAGE).expect("the next page is painted");
    assert!(
        (alpha - 0.5).abs() < 1e-9,
        "half way through the replacement's 100 ms: alpha {alpha}"
    );
    assert!(
        second_builds.get() > 0,
        "the next route adopts the replacement"
    );
}

/// Reports whether its navigator can pop from `duration`, the way a builder
/// that sizes its transition by navigation depth would.
struct NavigatorAwareFade {
    navigator: NavigatorHandle,
    queries: Rc<Cell<usize>>,
}

impl PageTransitionBuilder for NavigatorAwareFade {
    fn duration(&self) -> Duration {
        self.queries.set(self.queries.get() + 1);
        if self.navigator.can_pop() {
            ms(200)
        } else {
            ms(400)
        }
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        FadeTransition::new(Rc::clone(primary), child)
            .into_view()
            .boxed()
    }
}

/// An adopted builder's durations are read outside the navigator's locked
/// history flush, so a builder may query the navigator it serves: the push
/// completes instead of deadlocking on the history lock.
///
/// Runs in a child process: the failure mode is a hang.
pub(crate) fn an_adopted_builder_may_query_its_navigator() {
    if crate::common::child_process::selected_case().is_none() {
        crate::common::child_process::run_rows(
            "contracts::page_transition_builders_reach_their_navigator",
            &["can_pop"],
        );
        return;
    }
    let navigator = NavigatorHandle::new();
    let queries = Rc::new(Cell::new(0));
    let fade: Rc<dyn PageTransitionBuilder> = Rc::new(NavigatorAwareFade {
        navigator: navigator.clone(),
        queries: Rc::clone(&queries),
    });
    let registry = Vsync::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let mut app = lay_out_animated(
        VsyncScope::new(
            registry.clone(),
            PageTransitions::new(Navigator::new(navigator.clone())).page(fade),
        ),
        tight(200.0, 100.0),
        registry,
    );
    let _result = navigator.push(PageRoute::<()>::new(page));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(200));
    assert_alpha(&app, 0.5, "half way through the 400 ms read at adoption");
    assert!(queries.get() > 0, "the builder queried its navigator");
    crate::common::child_process::pass();
}

/// Under reduced motion the route animation settles on its first frame, and
/// the adopted builder paints the settled page.
pub(crate) fn an_adopted_transition_settles_on_the_first_reduced_motion_frame() {
    let (fade, builds) = CountingFade::counting(1000);
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

/// A route pushed through a handle before its navigator mounts is installed
/// with no hook resolved yet. It adopts the hook once the navigator mounts:
/// its builder wraps the page and its exit runs the hook's duration.
pub(crate) fn a_route_pushed_before_mount_adopts_the_hook_at_mount() {
    let (fade, builds) = CountingFade::counting(400);
    let registry = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let _result = navigator.push(PageRoute::<()>::new(page));
    let mut app = lay_out_animated(
        VsyncScope::new(
            registry.clone(),
            PageTransitions::new(Navigator::new(navigator.clone())).page(fade),
        ),
        tight(200.0, 100.0),
        registry,
    );
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(500));
    assert!(builds.get() > 0, "the hook's builder wraps the page");
    assert_alpha(&app, 1.0, "settled");

    assert!(navigator.pop());
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(200));
    assert_alpha(&app, 0.5, "the exit runs the hook's 400 ms");
}

/// A builder whose `duration` pushes a page of `color` the first time the
/// navigator reads it, the way a builder reaching a captured handle could.
struct PushingFade {
    navigator: NavigatorHandle,
    color: Color,
    millis: u64,
    pushed: Cell<bool>,
}

impl PageTransitionBuilder for PushingFade {
    fn duration(&self) -> Duration {
        if !self.pushed.replace(true) {
            let color = self.color;
            let _result =
                self.navigator
                    .push(PageRoute::<()>::new(move |_ctx, _primary, _secondary| {
                        ColoredBox::new(color)
                            .child(SizedBox::new(80.0, 40.0))
                            .into_view()
                            .boxed()
                    }));
        }
        ms(self.millis)
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        FadeTransition::new(Rc::clone(primary), child)
            .into_view()
            .boxed()
    }
}

/// A page pushed while the navigator reads a builder's durations adopts the
/// hook being resolved: while mounting, not the empty hooks of a navigator
/// that has none yet, and while replacing, not the outgoing ones. The exit
/// runs the resolved builder's duration.
pub(crate) fn a_route_pushed_while_durations_are_read_adopts_the_new_hooks() {
    let registry = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let pushing = |millis: u64, color: Color| -> Rc<dyn PageTransitionBuilder> {
        Rc::new(PushingFade {
            navigator: navigator.clone(),
            color,
            millis,
            pushed: Cell::new(false),
        })
    };
    let app_with = |hook: Rc<dyn PageTransitionBuilder>| {
        VsyncScope::new(
            registry.clone(),
            PageTransitions::new(Navigator::new(navigator.clone())).page(hook),
        )
    };

    let mut app = lay_out_animated(
        app_with(pushing(1000, PAGE)),
        tight(200.0, 100.0),
        registry.clone(),
    );
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(500));
    assert!(navigator.pop());
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(500));
    assert_alpha(&app, 0.5, "mounting: the exit runs the hook's 1000 ms");
    app.pump_for(ms(600));

    app.pump_widget(app_with(pushing(100, NEXT_PAGE)));
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(500));
    assert!(navigator.pop());
    app.pump_for(Duration::ZERO);
    app.pump_for(ms(50));
    let alpha = alpha_of(&app, NEXT_PAGE).expect("the page pushed during replacement is painted");
    assert!(
        (alpha - 0.5).abs() < 1e-9,
        "replacing: the exit runs the new hook's 100 ms: alpha {alpha}"
    );
}

/// A fade whose last owner's `Drop` panics.
struct PanicOnDrop(Rc<Cell<usize>>);

impl PageTransitionBuilder for PanicOnDrop {
    fn duration(&self) -> Duration {
        ms(100)
    }

    fn build(
        &self,
        _ctx: &dyn BuildContext,
        primary: &RouteAnimation,
        _secondary: &RouteAnimation,
        child: BoxedView,
    ) -> BoxedView {
        FadeTransition::new(Rc::clone(primary), child)
            .into_view()
            .boxed()
    }
}

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
        panic!("a transition builder's Drop panicked");
    }
}

/// When the navigator retires hooks whose page and popup builders both panic
/// in `Drop`, the first panic is contained and the second builder is
/// retained: unwinding out of the first slot must not drop the second into a
/// double panic, which aborts the process.
///
/// Runs in a child process: the failure mode is an abort.
pub(crate) fn retiring_hooks_whose_builders_both_panic_does_not_abort() {
    if crate::common::child_process::selected_case().is_none() {
        crate::common::child_process::run_rows(
            "contracts::page_transition_hooks_retire_panicking_builders",
            &["both_slots"],
        );
        return;
    }
    let drops = Rc::new(Cell::new(0));
    let (navigator, mut app, registry) = mount(|navigator| {
        PageTransitions::new(navigator)
            .page(Rc::new(PanicOnDrop(Rc::clone(&drops))))
            .popup(Rc::new(PanicOnDrop(Rc::clone(&drops))))
            .boxed()
    });
    let _contained = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.pump_widget(VsyncScope::new(
            registry.clone(),
            PageTransitions::new(Navigator::new(navigator.clone())),
        ));
    }));
    assert_eq!(
        drops.get(),
        1,
        "the second builder is retained, not dropped into a second panic"
    );
    crate::common::child_process::pass();
}
