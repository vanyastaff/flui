//! Integration tests for [`CupertinoTabScaffold`] — the `_TabSwitchingView`
//! contract: a tab's content is built lazily (only once visited) and its
//! state survives switching away and back (Offstage, not unmount), plus the
//! content-padding contract and the tab bar's own tap wiring.

use crate::common;

use std::cell::Cell;
use std::rc::Rc;

use common::{lay_out, tight};
use flui_cupertino::{
    CupertinoTabBar, CupertinoTabBarItem, CupertinoTabController, CupertinoTabScaffold,
};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{Icon, IconData, MediaQuery, MediaQueryData, SizedBox};

fn two_tab_bar() -> CupertinoTabBar {
    CupertinoTabBar::new(vec![
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
        CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
    ])
}

/// A leaf whose `create_state` is counted — proves whether a tab's content
/// element survived a visibility toggle (Offstage) versus being torn down
/// and rebuilt from scratch.
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
/// `Offstage`, not unmount. The same contract as above, proven this time via
/// a `StatefulView`'s `create_state` count rather than a builder-call count.
///
/// Red-check: key each tab's `Offstage` subtree by `(index, current_index)`
/// instead of `index` alone (forcing a fresh element on every switch) — this
/// test's `created.get() == 1` assertion fails (would read `2`).
pub fn an_inactive_tabs_state_survives_switching_away_and_back() {
    let created = Rc::new(Cell::new(0_u32));
    let created_for_closure = Rc::clone(&created);

    let controller = CupertinoTabController::new(0);
    let scaffold =
        CupertinoTabScaffold::new(two_tab_bar(), controller.clone(), move |_ctx, index| {
            if index == 0 {
                Probe(Rc::clone(&created_for_closure)).into_view().boxed()
            } else {
                SizedBox::new(10.0, 10.0).into_view().boxed()
            }
        });

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
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
        "switching away to tab 1 and back to tab 0 must not recreate tab 0's state"
    );
}

/// Tapping a tab bar item advances the shared controller's index, which
/// rebuilds the scaffold's active tab — an end-to-end proof that
/// `CupertinoTabScaffold` actually wires the bar's `on_tap`, not just that
/// `CupertinoTabController::set_index` compiles.
pub fn tapping_a_tab_item_switches_the_active_tab() {
    let controller = CupertinoTabController::new(0);
    let scaffold = CupertinoTabScaffold::new(two_tab_bar(), controller.clone(), |_ctx, index| {
        if index == 0 {
            SizedBox::new(11.0, 11.0).into_view().boxed()
        } else {
            SizedBox::new(22.0, 22.0).into_view().boxed()
        }
    });

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
    );
    laid.tick();
    assert_eq!(controller.index(), 0);

    // Tab bar sits at the bottom, split into two equal-width items; tap
    // squarely inside the second (Settings) item.
    laid.dispatch_pointer_down(300.0, 790.0);
    laid.dispatch_pointer_up(300.0, 790.0);
    laid.tick();

    assert_eq!(
        controller.index(),
        1,
        "tapping the second tab item must advance the controller to index 1"
    );
}

/// A changed controller is checked against the live bar, and a later valid
/// selection can build content again after the error boundary recovers.
pub fn out_of_range_controller_selection_reports_error_and_recovers() {
    let controller = CupertinoTabController::new(0);
    let scaffold = CupertinoTabScaffold::new(two_tab_bar(), controller.clone(), |_ctx, _index| {
        SizedBox::new(11.0, 11.0).boxed()
    });
    let mut laid = lay_out(
        // Keep the harness render root mounted while the scaffold replaces
        // its own subtree with ErrorView, then builds valid tabs again.
        SizedBox::new(400.0, 800.0).child(MediaQuery::new(MediaQueryData::default(), scaffold)),
        tight(400.0, 800.0),
    );
    assert_eq!(laid.find_all_by_render_type("RenderErrorBox"), []);

    controller.set_index(2);
    laid.tick();
    assert!(
        !laid.find_all_by_render_type("RenderErrorBox").is_empty(),
        "invalid selection must report the build failure, including in release"
    );

    controller.set_index(1);
    laid.tick();
    assert_eq!(laid.find_all_by_render_type("RenderErrorBox"), []);
    laid.dispatch_pointer_down(100.0, 790.0);
    laid.dispatch_pointer_up(100.0, 790.0);
    laid.tick();
    assert_eq!(controller.index(), 0, "recovered content accepts tab taps");
}

pub fn standalone_bar_rejects_an_out_of_range_selection() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        two_tab_bar().current_index(2)
    }));
    assert!(
        result.is_err(),
        "a bar cannot publish a selection outside its items"
    );
}
