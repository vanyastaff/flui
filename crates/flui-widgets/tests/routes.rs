//! ADR-0020 U5.4: `PageRoute` and `PopupRoute` through the **public** surface.
//!
//! Everything here imports from `flui_widgets::prelude` or the crate root — a
//! missing `pub use` fails to compile rather than fails an assertion. And unlike
//! the in-crate `page_route_tests`, nothing reaches for a `TransitionHandle`: the
//! transitions are driven by pumping a real `Vsync`, which is the only clock an
//! app author has.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/routes_test.dart` — `'route
//! management'`, `'entering/leaving routes'`; `.../navigator_test.dart` —
//! `'Can push, pop, and replace in sequence'`. Expected values are read from
//! `pages.dart` / `routes.dart`, not from running this code.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_animation::Vsync;
use flui_types::Color;
use flui_widgets::prelude::*;
use flui_widgets::{
    ColoredBox, GestureDetector, NavigatorHandle, PageRoute, PopupRoute, RouteAnimation,
    SimpleRoute, VsyncScope,
};
use parking_lot::Mutex;

/// The routes use the framework default, 300 ms.
const TRANSITION: Duration = Duration::from_millis(300);
/// Enough pumps to carry a 300 ms transition past its end. The first pump after a
/// controller starts only anchors `t = 0` (Flutter's first ticker tick delivers
/// elapsed 0), so one extra frame is needed.
const PUMPS: usize = 8;
const FRAME: Duration = Duration::from_millis(50);

/// Records every `(animation, secondary_animation)` pair a page builder sees.
#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(f32, f32)>>>);

impl Seen {
    fn last(&self) -> Option<(f32, f32)> {
        self.0.lock().last().copied()
    }
    fn builds(&self) -> usize {
        self.0.lock().len()
    }
}

/// A page that records both animations on each build.
fn recording_page(
    seen: &Seen,
) -> impl Fn(&dyn BuildContext, &RouteAnimation, &RouteAnimation) -> BoxedView + Send + Sync + 'static
{
    let seen = seen.clone();
    move |_ctx, animation, secondary| {
        seen.0.lock().push((animation.value(), secondary.value()));
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }
}

/// The root: a `VsyncScope` so the navigator's route transitions register with the
/// clock the test pumps. `NavigatorState::init_state` resolves the ambient scope.
fn app(vsync: &Vsync, navigator: &NavigatorHandle) -> impl View {
    VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone()))
}

/// A navigator whose bottom route counts taps, so a barrier that leaks pointers
/// is observable.
fn navigator_with_tappable_home(taps: &Arc<AtomicUsize>) -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    let taps = Arc::clone(taps);
    navigator.seed_initial(SimpleRoute::<i32>::new(move |_ctx| {
        let taps = Arc::clone(&taps);
        GestureDetector::new()
            .on_tap(move || {
                taps.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30)))
            .into_view()
            .boxed()
    }));
    navigator
}

// ============================================================================
// push / pop through a real clock
// ============================================================================

/// A pushed `PageRoute` builds its page immediately at `animation == 0`, then the
/// entrance transition carries it to 1 and the route settles.
#[test]
fn page_route_push_builds_the_page_and_completes_its_entrance() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));

    let seen = Seen::default();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(200.0, 200.0), vsync);

    let _result = navigator.push(PageRoute::<i32>::new(recording_page(&seen)));
    laid.pump_for(FRAME);

    assert_eq!(navigator.route_ids().len(), 2, "the route is on the stack");
    assert_eq!(
        seen.last(),
        Some((0.0, 0.0)),
        "the first build sees a dismissed entrance and no route above"
    );

    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    let (animation, secondary) = seen.last().expect("the page rebuilt as it animated");
    assert!(
        (animation - 1.0).abs() < 1e-4,
        "the entrance transition completed, got {animation}"
    );
    assert!(secondary.abs() < 1e-6, "nothing is above this route");
    assert!(navigator.can_pop(), "two routes, so the top can pop");
}

/// `finishedWhenPopped` (`routes.dart:177`): a popped `PageRoute` stays on the
/// stack while its exit transition runs, and leaves only once it is dismissed.
#[test]
fn page_route_pop_keeps_the_route_until_the_reverse_transition_dismisses() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));

    let seen = Seen::default();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(200.0, 200.0), vsync);

    let _result = navigator
        .push(PageRoute::<i32>::new(recording_page(&seen)).transition_duration(TRANSITION));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(navigator.route_ids().len(), 2);

    assert!(navigator.pop(), "the top route consents to pop");
    laid.pump_for(FRAME);
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "still present: the exit transition is running"
    );

    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "dismissed, so the route finalized and left the stack"
    );
    assert!(!navigator.can_pop());
}

/// The `secondaryAnimation` of the lower `PageRoute` is driven by the upper one's
/// entrance (`routes.dart:429-443`) — observed from inside the lower page's own
/// builder, which is the only place an app author can see it.
#[test]
fn pushing_a_page_route_drives_the_previous_page_routes_secondary_animation() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));

    let lower = Seen::default();
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(200.0, 200.0), vsync);

    // `maintain_state(true)` is the default, but say it: the lower page must stay
    // built for its secondary animation to be observable at all.
    let _lower = navigator.push(PageRoute::<i32>::new(recording_page(&lower)).maintain_state(true));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(lower.last().map(|(a, s)| (a > 0.99, s)), Some((true, 0.0)));

    let builds_before = lower.builds();
    let _upper = navigator.push(PageRoute::<i32>::new(|_ctx, _a, _s| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    assert!(
        lower.builds() > builds_before,
        "the lower page rebuilds as its secondary animation runs"
    );
    let (animation, secondary) = lower.last().expect("a build");
    assert!(
        (animation - 1.0).abs() < 1e-4,
        "the lower page's own animation stays completed, got {animation}"
    );
    assert!(
        (secondary - 1.0).abs() < 1e-4,
        "the upper page's entrance drove it to 1, got {secondary}"
    );
}

// ============================================================================
// the modal barrier
// ============================================================================

/// `ModalBarrier`'s `onDismiss ?? () => Navigator.maybePop(context)`: a tap on a
/// dismissible barrier pops the popup.
#[test]
fn a_tap_on_a_dismissible_popup_barrier_pops_the_route() {
    let vsync = Vsync::new();
    let taps = Arc::new(AtomicUsize::new(0));
    let navigator = navigator_with_tappable_home(&taps);
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(200.0, 200.0), vsync);

    let _result = navigator.push(
        PopupRoute::<i32>::new(|_ctx, _a, _s| SizedBox::new(10.0, 10.0).into_view().boxed())
            .barrier_dismissible(true)
            .barrier_color(Color::rgb(0, 0, 0)),
    );
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(navigator.route_ids().len(), 2);

    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_up(20.0, 20.0);
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    assert_eq!(
        navigator.route_ids().len(),
        1,
        "the barrier tap popped the popup"
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "and the page underneath never saw the pointer"
    );
}

/// A non-dismissible barrier absorbs the pointer without popping: the tap reaches
/// neither the popup's page nor the route below.
#[test]
fn a_tap_on_a_non_dismissible_popup_barrier_absorbs_but_does_not_pop() {
    let vsync = Vsync::new();
    let taps = Arc::new(AtomicUsize::new(0));
    let navigator = navigator_with_tappable_home(&taps);
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(200.0, 200.0), vsync);

    let _result = navigator.push(PopupRoute::<i32>::new(|_ctx, _a, _s| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_up(20.0, 20.0);
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    assert_eq!(
        navigator.route_ids().len(),
        2,
        "a non-dismissible barrier ignores the tap"
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "but it still absorbs the pointer, so the page below is untouched"
    );
}

/// A `PopupRoute` does not occlude, so the page below stays built — the whole
/// point of `PopupRoute.opaque => false` (`routes.dart:2391`).
#[test]
fn a_popup_route_leaves_the_page_below_built() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    let below = Seen::default();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(200.0, 200.0), vsync);

    let _page = navigator.push(PageRoute::<i32>::new(recording_page(&below)));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    let builds_before = below.builds();

    let _popup = navigator.push(PopupRoute::<i32>::new(|_ctx, _a, _s| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    assert_eq!(navigator.route_ids().len(), 3);
    let (animation, secondary) = below.last().expect("a build");
    assert!((animation - 1.0).abs() < 1e-4);
    assert!(
        secondary.abs() < 1e-6,
        "a popup is not a PageRoute, so it drives no secondary animation \
         (pages.dart:58): got {secondary}"
    );
    let _ = builds_before;
}

// ============================================================================
// the public surface itself
// ============================================================================

/// The names ADR-0020 §7e signed off are reachable from the prelude and the crate
/// root, with the shapes the docs promise. A missing export breaks the build.
#[test]
fn the_signed_off_route_surface_is_usable_from_the_prelude() {
    let page: PageRoute<u32> =
        PageRoute::new(|_ctx, _a, _s| SizedBox::shrink().into_view().boxed())
            .named("/home")
            .transition_duration(TRANSITION)
            .reverse_transition_duration(TRANSITION)
            .maintain_state(true)
            .barrier_dismissible(false)
            .with_current_result(7)
            .transitions(|_ctx, _a, _s, child| child);

    let popup: PopupRoute<u32> =
        PopupRoute::new(|_ctx, _a, _s| SizedBox::shrink().into_view().boxed())
            .named("/dialog")
            .barrier_dismissible(true)
            .barrier_color(Color::rgb(0, 0, 0))
            .maintain_state(true)
            .with_current_result(9)
            .transitions(|_ctx, _a, _s, child| child);

    // `Debug` is part of the surface: a route in a panic message must not be opaque.
    assert!(format!("{page:?}").contains("PageRoute"));
    assert!(format!("{popup:?}").contains("PopupRoute"));
}

/// The private layers stay private. `Overlay`, `OverlayEntry`, `TransitionRoute`
/// and `ModalRoute` have no sign-off, so an app author cannot name them — this
/// test documents that by *not* being able to, and by pinning what replaced them.
///
/// `RouteBindingSlot` **is** public: it is the opaque cell `NavigatorRoute`
/// exposes so an animated route can receive a capability it cannot read.
#[test]
fn the_binding_slot_is_opaque_and_the_private_layers_stay_private() {
    let slot = flui_widgets::RouteBindingSlot::new();
    assert!(!slot.is_bound(), "an unpushed route's slot is empty");

    // There is deliberately no accessor: `slot.get()` is `pub(crate)`.
    // `flui_widgets::Overlay`, `::TransitionRoute` and `::ModalRoute` do not exist.
    assert_eq!(format!("{slot:?}"), "RouteBindingSlot { bound: false }");
}
