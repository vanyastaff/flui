//! Integration tests for [`CupertinoTabScaffold`] — the `_TabSwitchingView`
//! contract: a tab's content is built lazily (only once visited) and its
//! state survives switching away and back (Offstage, not unmount), plus the
//! content-padding contract and the tab bar's own tap wiring.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{lay_out, tight};
use flui_cupertino::{
    CupertinoTabBar, CupertinoTabBarItem, CupertinoTabController, CupertinoTabScaffold,
};
use flui_types::geometry::px;
use flui_view::prelude::*;
use flui_widgets::prelude::EdgeInsets;
use flui_widgets::{Icon, IconData, MediaQuery, MediaQueryData, SizedBox};

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
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
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

/// A tab's content is only ever built the first time it becomes active —
/// `_TabSwitchingViewState.shouldBuildTab` (`tab_scaffold.dart`, oracle tag
/// `3.44.0`). Tab 1 must never be built while tab 0 stays active.
///
/// Red-check: drop the `should_build[index]` gate in `tab_scaffold.rs`'s
/// `build` (call `tab_builder` unconditionally for every index) — this
/// test's "tab 1 never called before it's visited" assertion fails on the
/// very first mount.
///
/// Note: once a tab *has* been visited, the oracle calls `tabBuilder` again
/// on every subsequent rebuild for it too (`shouldBuildTab[index] ?
/// widget.tabBuilder(...) : ...` re-evaluates every build) — Element
/// diffing, not a build-count cache, is what keeps its *state* stable (see
/// `an_inactive_tabs_state_survives_switching_away_and_back` below). This
/// test only proves the "not yet visited" half of the contract.
#[test]
fn a_tab_is_not_built_until_it_becomes_active() {
    let called_indices: Rc<Cell<Vec<usize>>> = Rc::new(Cell::new(Vec::new()));
    let called_for_closure = Rc::clone(&called_indices);

    let controller = CupertinoTabController::new(0);
    let scaffold =
        CupertinoTabScaffold::new(two_tab_bar(), controller.clone(), move |_ctx, index| {
            let mut called = called_for_closure.take();
            called.push(index);
            called_for_closure.set(called);
            SizedBox::new(10.0, 10.0).into_view().boxed()
        });

    let mut laid = lay_out(
        MediaQuery::new(MediaQueryData::default(), scaffold),
        tight(400.0, 800.0),
    );
    laid.tick();

    assert!(
        called_indices.take().iter().all(|&i| i == 0),
        "tab 1 must never be called while tab 0 stays active"
    );

    controller.set_index(1);
    laid.tick();

    assert!(
        called_indices.take().contains(&1),
        "tab 1 must be called once it becomes active"
    );
}

/// An inactive tab's own element state survives a switch away and back —
/// `Offstage`, not unmount. Flutter parity: the same
/// `_TabSwitchingViewState` contract above, proven this time via a
/// `StatefulView`'s `create_state` count rather than a builder-call count.
///
/// Red-check: key each tab's `Offstage` subtree by `(index, current_index)`
/// instead of `index` alone (forcing a fresh element on every switch) — this
/// test's `created.get() == 1` assertion fails (would read `2`).
#[test]
fn an_inactive_tabs_state_survives_switching_away_and_back() {
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

/// Content is pushed up by exactly the tab bar's `preferred_size().height`
/// plus `MediaQuery.padding.bottom` — `tab_scaffold.dart`'s `bottomPadding`
/// (oracle tag `3.44.0`).
#[test]
fn content_is_padded_above_the_tab_bar_plus_the_bottom_inset() {
    let media = MediaQueryData {
        padding: EdgeInsets::new(px(0.0), px(0.0), px(20.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let controller = CupertinoTabController::new(0);
    let scaffold = CupertinoTabScaffold::new(two_tab_bar(), controller, |_ctx, _index| {
        SizedBox::new(60.0, 30.0).into_view().boxed()
    });

    let laid = lay_out(MediaQuery::new(media, scaffold), tight(400.0, 800.0));

    // The tab bar's own build wraps its row (and each item) in further
    // `Padding`s — the content's own is the only one carrying this exact
    // bottom value.
    let matching_padding = laid
        .find_all_by_render_type("RenderPadding")
        .into_iter()
        .filter_map(|id| laid.render_property(id, "padding"))
        .find(|padding| padding.contains("bottom: 70px"));
    assert!(
        matching_padding.is_some(),
        "some Padding must carry the content's 50.0 tab bar height + 20.0 bottom inset = 70.0"
    );
}

/// Tapping a tab bar item advances the shared controller's index, which
/// rebuilds the scaffold's active tab — an end-to-end proof that
/// `CupertinoTabScaffold` actually wires the bar's `on_tap`, not just that
/// `CupertinoTabController::set_index` compiles.
#[test]
fn tapping_a_tab_item_switches_the_active_tab() {
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
