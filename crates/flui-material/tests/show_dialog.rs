//! `show_dialog` end-to-end coverage — a real [`NavigatorHandle`] mounted
//! under a [`Theme`], pumped through a real [`Vsync`] clock, matching
//! `flui-widgets`' own `tests/routes.rs` harness for `PopupRoute` (the
//! machinery `show_dialog` is built on).

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{lay_out_animated, tight};
use flui_animation::Vsync;
use flui_material::{AlertDialog, Theme, ThemeData, show_dialog};
use flui_types::Color;
use flui_view::prelude::*;
use flui_widgets::{
    ColoredBox, GestureDetector, Navigator, NavigatorHandle, SimpleRoute, Text, VsyncScope,
};

/// `PopupRoute`'s framework-default transition — matching
/// `flui-widgets/tests/routes.rs`'s own constant exactly, since `show_dialog`
/// pushes the same `PopupRoute` machinery those tests already pump.
const TRANSITION: Duration = Duration::from_millis(300);
/// The per-pump virtual-time step.
const FRAME: Duration = Duration::from_millis(50);
/// Enough pumps to carry `TRANSITION` past its end, with margin for this
/// file's tests to settle a *second* transition (a barrier-tap pop's reverse
/// run) within the same per-phase budget: one whole `TRANSITION / FRAME`,
/// plus one frame because the first pump after a controller starts only
/// anchors `t = 0` (Flutter's first ticker tick delivers elapsed 0 — the same
/// `+ 1` `flui-widgets/tests/routes.rs`'s `PUMPS` documents for this
/// identical 300ms/50ms pair), plus one more frame for the reverse
/// transition's post-completion route removal to land. Asserted below rather
/// than merely hoped: changing either duration keeps this budget correct
/// instead of silently under-pumping.
const PUMPS: usize = (TRANSITION.as_millis() / FRAME.as_millis()) as usize + 2;

const _: () = assert!(
    (PUMPS as u128) * FRAME.as_millis() > TRANSITION.as_millis(),
    "PUMPS * FRAME must carry the transition past its end"
);

/// The home page: counts how many times its `State` was created (proving
/// whether a dialog push/pop tore it down and rebuilt it) and how many taps
/// its own content received (proving whether a barrier absorbed them).
#[derive(Clone, StatefulView)]
struct HomePage {
    created: Rc<Cell<u32>>,
    taps: Arc<AtomicUsize>,
}

struct HomePageState;

impl ViewState<HomePage> for HomePageState {
    fn build(&self, view: &HomePage, _ctx: &dyn BuildContext) -> impl IntoView {
        let taps = Arc::clone(&view.taps);
        GestureDetector::new()
            .on_tap(move || {
                taps.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30)))
    }
}

impl StatefulView for HomePage {
    type State = HomePageState;

    /// Flutter's `createState()` — called exactly once when the element is
    /// created, never again across rebuilds. Incrementing `created` here
    /// (rather than relying on a hypothetical rebuild hook) is what makes
    /// `created.get() == 1` after a dialog's push/dismiss round-trip prove
    /// the page's element was never torn down and recreated.
    fn create_state(&self) -> Self::State {
        self.created.set(self.created.get() + 1);
        HomePageState
    }
}

fn app(vsync: &Vsync, navigator: &NavigatorHandle) -> impl View {
    Theme::new(
        ThemeData::light(),
        VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
    )
}

/// A dialog pushed with `show_dialog` covers the page with a dismissible
/// barrier: the page beneath stops receiving taps while it is up, a tap on
/// the barrier (away from the dialog itself) dismisses it, and the page's
/// own `State` survives the whole round-trip untouched — proof `PopupRoute`
/// (`maintain_state: true`, `opaque: false`) never tore the page down.
#[test]
fn dialog_covers_the_page_and_a_barrier_tap_dismisses_it_leaving_page_state_intact() {
    let vsync = Vsync::new();
    let created = Rc::new(Cell::new(0_u32));
    let taps = Arc::new(AtomicUsize::new(0));
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new({
        let created = Rc::clone(&created);
        let taps = Arc::clone(&taps);
        move |_ctx| {
            HomePage {
                created: Rc::clone(&created),
                taps: Arc::clone(&taps),
            }
            .into_view()
            .boxed()
        }
    }));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(800.0, 600.0), vsync);

    // Sanity: the home page is tappable before any dialog is shown.
    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "home page must be tappable before a dialog"
    );
    assert_eq!(
        created.get(),
        1,
        "home page's State must have been created exactly once so far"
    );

    let _result = show_dialog::<(), _, _>(&navigator, |_ctx| {
        AlertDialog::new().title(Text::new("Discard changes?"))
    });
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "the dialog route is now on the stack"
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("the pushed dialog must mount its Material surface over the page");
    assert!(
        laid.size(material).width.get() > 0.0,
        "the dialog surface must have real geometry, not a zero-sized stub"
    );

    // The barrier absorbs the same tap the home page used to receive.
    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "the modal barrier must absorb the pointer — the home page must not see a second tap \
         while the dialog is up"
    );
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "a tap over the dialog's own footprint must not dismiss it"
    );

    // A tap well outside the dialog's centered footprint (its content is
    // narrower than the 800px canvas, inset at least 40px from every edge)
    // hits the barrier itself and dismisses the default-dismissible dialog.
    laid.dispatch_pointer_down(5.0, 5.0);
    laid.dispatch_pointer_up(5.0, 5.0);
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "show_dialog's default barrier_dismissible(true) must let a barrier tap pop the dialog"
    );

    // The page beneath is tappable again, and its `State` was never
    // recreated across the whole push/dismiss round-trip.
    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        2,
        "the home page must be tappable again once the dialog is dismissed"
    );
    assert_eq!(
        created.get(),
        1,
        "the home page's State must survive the dialog's entire push/dismiss lifecycle \
         (PopupRoute.maintainState => true keeps the page mounted throughout)"
    );
}

/// An explicit `navigator.pop()` — not a barrier tap — also removes the
/// dialog `show_dialog` pushed.
#[test]
fn an_explicit_navigator_pop_closes_the_dialog() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<()>::new(|_ctx| {
        ColoredBox::new(Color::rgb(0, 0, 0)).into_view().boxed()
    }));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(800.0, 600.0), vsync);

    let _result = show_dialog::<(), _, _>(&navigator, |_ctx| {
        AlertDialog::new().title(Text::new("Delete this?"))
    });
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "the dialog route is on the stack"
    );

    assert!(
        navigator.pop(),
        "an explicit pop must succeed with a dialog on top"
    );
    for _ in 0..PUMPS {
        laid.pump_for(FRAME);
    }

    assert_eq!(
        navigator.route_ids().len(),
        1,
        "an explicit navigator.pop() must close the dialog show_dialog pushed"
    );
}
