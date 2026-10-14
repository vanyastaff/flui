//! [`TabBarView`] widget-level mount/interaction coverage: a page's state
//! survives switching away and back, and the attached pager drives the
//! controller's position — a drag moves it (and the indicator) without
//! committing, a settled release commits.

use crate::common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use common::{LaidOut, lay_out, lay_out_animated, tight};
use flui_material::{TabBar, TabBarView, TabController, Theme, ThemeData};
use flui_sdk::animation::{MotionOverride, Vsync};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{Column, Expanded, MediaQuery, MediaQueryData, PageView, SizedBox};

/// A leaf whose `create_state` is counted — proves whether a tab's content
/// element survived a visibility toggle (`Offstage`) versus being torn down
/// and rebuilt from scratch. Same fixture shape as
/// `flui-cupertino/tests/tab_scaffold.rs`'s own `Probe` (this crate cannot
/// see that one — it's private to that test binary).
#[derive(Clone)]
struct Probe(Rc<Cell<u32>>);

impl View for Probe {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        self.0.set(self.0.get() + 1);
        ProbeState
    }
}

struct ProbeState;

impl ViewState<Probe> for ProbeState {
    fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

/// An inactive tab's own element state survives a switch away and back —
/// `Offstage`, not unmount. Same `_TabSwitchingViewState` contract as above,
/// proven this time via a `create_state` COUNT staying at `1` across a
/// round trip rather than merely becoming nonzero.
///
/// Red-check: key each tab's `Offstage` layer by `(index, current_index)`
/// instead of `index` alone (forcing a fresh element identity on every
/// switch) — `created.get()` would read `2` instead of `1` after the round
/// trip below.
pub fn an_inactive_tabs_state_survives_switching_away_and_back() {
    let created = Rc::new(Cell::new(0_u32));
    let created_for_tab_0 = Rc::clone(&created);

    let controller = TabController::new(2, 0);
    let view = TabBarView::new(vec![
        Probe(created_for_tab_0).into_view().boxed(),
        SizedBox::new(10.0, 10.0).into_view().boxed(),
    ])
    .controller(controller.clone());

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), view),
        tight(400.0, 400.0),
    );
    laid.tick();
    assert_eq!(
        created.get(),
        1,
        "tab 0's Probe must have been created once"
    );

    controller.set_index(1);
    laid.tick();
    controller.set_index(0);
    laid.tick();

    assert_eq!(
        created.get(),
        1,
        "switching away to tab 1 and back to tab 0 must not recreate tab 0's Probe state"
    );
}

const WIDTH: f64 = 400.0;

/// A four-tab bar over a four-page view sharing `controller`, 400 px wide on
/// `vsync`'s clock; the pages span y 48..400.
fn mount_tabs_over_pages(controller: &TabController, vsync: &Vsync) -> LaidOut {
    lay_out_animated(
        tabs_tree(controller, vsync, true),
        tight(WIDTH, WIDTH),
        vsync.clone(),
    )
}

/// The bar, and the view when `with_view`, over `controller`.
fn tabs_tree(controller: &TabController, vsync: &Vsync, with_view: bool) -> VsyncScope {
    let pages = (0..4)
        .map(|_| SizedBox::expand().into_view().boxed())
        .collect();
    let mut column = vec![
        TabBar::secondary(crate::tabs::four_tabs())
            .controller(controller.clone())
            .boxed(),
    ];
    if with_view {
        column.push(Expanded::new(TabBarView::new(pages).controller(controller.clone())).boxed());
    }
    VsyncScope::new(
        vsync.clone(),
        Theme::new(ThemeData::light(), Column::new(column)),
    )
}

/// Drags the pages leftward by `distance` px without releasing.
fn drag_left(laid: &mut LaidOut, distance: f64) {
    let start = 350.0;
    laid.dispatch_pointer_down(start, 200.0);
    // Past the touch slop first, then back onto the exact track, so the
    // pages move by exactly the finger's travel from `start`.
    laid.dispatch_pointer_move(start - 40.0, 200.0);
    laid.dispatch_pointer_move(start - distance, 200.0);
    laid.pump_for(Duration::ZERO);
}

/// Dragging the view a quarter page moves the position a quarter tab and the
/// linear indicator with it, while the unreleased drag leaves `index` alone.
/// Reduced motion does not hold the finger back: a drag is direct
/// manipulation, not animation.
pub fn a_drag_moves_the_position_and_indicator_but_not_the_index() {
    for preference in [MotionOverride::Full, MotionOverride::Reduce] {
        let vsync = Vsync::new();
        let controller = TabController::new(4, 0);
        let mut laid = mount_tabs_over_pages(&controller, &vsync);
        laid.set_motion_override(preference);
        laid.pump_for(Duration::ZERO);

        drag_left(&mut laid, WIDTH / 4.0);

        let position = controller.position().value();
        assert!(
            (position - 0.25).abs() < 1e-9,
            "a quarter-page drag is a quarter tab under {preference:?}, got {position}"
        );
        let tab = WIDTH / 4.0;
        let (left, right) = crate::tabs::painted_indicator(&laid);
        assert!(
            (left - 0.25 * tab).abs() < 1e-9 && (right - 1.25 * tab).abs() < 1e-9,
            "the linear indicator is a quarter tab along under {preference:?}: {left}..{right}"
        );
        assert_eq!(controller.index(), 0, "an unreleased drag does not commit");
    }
}

/// Releasing a drag past half a page lets the pager settle on the next page,
/// and only then does `index` commit to it.
pub fn a_released_drag_commits_the_page_it_settles_on() {
    let vsync = Vsync::new();
    let controller = TabController::new(4, 0);
    let mut laid = mount_tabs_over_pages(&controller, &vsync);
    laid.pump_for(Duration::ZERO);

    drag_left(&mut laid, 0.75 * WIDTH);
    assert_eq!(controller.index(), 0, "still dragging");
    laid.dispatch_pointer_up(350.0 - 0.75 * WIDTH, 200.0);
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }

    assert_eq!(
        controller.position().value(),
        1.0,
        "the pager rests on page 1"
    );
    assert_eq!(controller.index(), 1, "the settled page commits");
    assert_eq!(controller.previous_index(), 0);
}

/// A jump while the controller's own animation runs the pager stops that run
/// synchronously; the stop must not commit the page the run was interrupted
/// on over the index the jump just committed.
pub fn a_jump_during_a_tab_animation_keeps_the_jumped_index() {
    let vsync = Vsync::new();
    let controller = TabController::new(4, 0);
    let mut laid = mount_tabs_over_pages(&controller, &vsync);
    laid.pump_for(Duration::ZERO);

    controller.animate_to(3);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    // Far enough along that the interrupted page rounds away from the jump's.
    let mid_run = controller.position().value();
    assert!(
        mid_run > 0.75 && mid_run < 3.0,
        "the run is under way: {mid_run}"
    );

    controller.set_index(0);
    assert_eq!(controller.index(), 0, "the jump's index survives the stop");
    for _ in 0..30 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(controller.index(), 0);
    assert_eq!(
        controller.position().value(),
        0.0,
        "the pager rests on the jump"
    );
}

/// A controller drives at most one `TabBarView`: a second view on it is
/// refused (its build fails into an error view) and the first keeps the
/// position.
pub fn a_second_view_on_one_controller_is_refused() {
    let vsync = Vsync::new();
    let controller = TabController::new(2, 0);
    let pages = || {
        (0..2)
            .map(|_| SizedBox::expand().into_view().boxed())
            .collect::<Vec<_>>()
    };
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            MediaQuery::new(
                MediaQueryData::default(),
                Theme::new(
                    ThemeData::light(),
                    Column::new(vec![
                        Expanded::new(TabBarView::new(pages()).controller(controller.clone()))
                            .boxed(),
                        Expanded::new(TabBarView::new(pages()).controller(controller.clone()))
                            .boxed(),
                    ]),
                ),
            ),
        ),
        tight(WIDTH, WIDTH),
        vsync,
    );
    laid.pump_for(Duration::ZERO);
    assert_eq!(
        laid.count_elements_by_view_type::<PageView>(),
        1,
        "only the first view mounts a pager"
    );

    // The first view, at the top, still drives the position.
    laid.dispatch_pointer_down(350.0, 100.0);
    laid.dispatch_pointer_move(310.0, 100.0);
    laid.dispatch_pointer_move(250.0, 100.0);
    laid.pump_for(Duration::ZERO);
    let position = controller.position().value();
    assert!(
        (position - 0.25).abs() < 1e-9,
        "the first view's drag moves the position, got {position}"
    );
}

/// Removing the view while the controller animates a tab change keeps the
/// committed target: tearing the pager down stops its run, and that stop is
/// not the user settling a page.
pub fn unmounting_the_view_mid_animation_keeps_the_committed_index() {
    let vsync = Vsync::new();
    let controller = TabController::new(4, 0);
    let mut laid = mount_tabs_over_pages(&controller, &vsync);
    laid.pump_for(Duration::ZERO);

    controller.animate_to(3);
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(150));
    let mid_run = controller.position().value();
    assert!(mid_run > 0.5 && mid_run < 2.5, "mid-run: {mid_run}");

    laid.pump_widget(tabs_tree(&controller, &vsync, false));
    laid.pump_for(Duration::ZERO);

    assert_eq!(controller.index(), 3, "the target survives the teardown");
    assert_eq!(controller.position().value(), 3.0);
}

/// A pager whose viewport collapsed to nothing moves its page without
/// publishing pixels; position observers still hear the change.
pub fn a_page_change_in_a_collapsed_viewport_reaches_position_observers() {
    use flui_sdk::foundation::Listenable as _;
    use flui_sdk::painting::Alignment;
    use flui_sdk::widgets::Align;

    let vsync = Vsync::new();
    let controller = TabController::new(4, 0);
    let tree = |width: f64| {
        let pages = (0..4)
            .map(|_| SizedBox::expand().into_view().boxed())
            .collect();
        VsyncScope::new(
            vsync.clone(),
            Align::new(Alignment::TOP_LEFT).child(
                SizedBox::new(width, 100.0)
                    .child(TabBarView::new(pages).controller(controller.clone())),
            ),
        )
    };
    let mut laid = lay_out_animated(tree(300.0), tight(WIDTH, WIDTH), vsync.clone());
    laid.pump_for(Duration::ZERO);
    laid.pump_widget(tree(0.0));
    laid.pump_for(Duration::ZERO);

    let heard = Rc::new(Cell::new(0));
    let counter = Rc::clone(&heard);
    let _observer = controller
        .position()
        .add_listener(Rc::new(move || counter.set(counter.get() + 1)));
    controller.set_index(2);

    assert_eq!(controller.position().value(), 2.0);
    assert!(heard.get() >= 1, "the position change was published");
}

/// A tab tap on an attached view settles on the next frame under reduced
/// motion, as the owned position does; under full motion it is still on its
/// way.
pub fn reduced_motion_settles_an_attached_view_on_the_first_frame() {
    for (preference, settled) in [
        (MotionOverride::Full, false),
        (MotionOverride::Reduce, true),
    ] {
        let vsync = Vsync::new();
        let controller = TabController::new(4, 0);
        let mut laid = mount_tabs_over_pages(&controller, &vsync);
        laid.set_motion_override(preference);
        laid.pump_for(Duration::ZERO);

        controller.animate_to(2);
        laid.pump_for(Duration::ZERO);
        laid.pump_for(Duration::from_millis(16));

        assert_eq!(
            controller.position().value() == 2.0,
            settled,
            "position {} under {preference:?}",
            controller.position().value()
        );
    }
}

/// A view whose children do not match the controller fails its build without
/// claiming the controller, so a correct view on it still mounts.
pub fn a_failed_view_does_not_claim_the_controller() {
    let vsync = Vsync::new();
    let controller = TabController::new(2, 0);
    let pages = |count: usize| {
        (0..count)
            .map(|_| SizedBox::expand().into_view().boxed())
            .collect::<Vec<_>>()
    };
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            Column::new(vec![
                Expanded::new(TabBarView::new(pages(3)).controller(controller.clone())).boxed(),
                Expanded::new(TabBarView::new(pages(2)).controller(controller)).boxed(),
            ]),
        ),
        tight(WIDTH, WIDTH),
        vsync,
    );
    laid.pump_for(Duration::ZERO);

    assert_eq!(
        laid.count_elements_by_view_type::<PageView>(),
        1,
        "the matching view mounts its pager"
    );
}
