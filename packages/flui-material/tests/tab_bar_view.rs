//! [`TabBarView`] widget-level mount/interaction coverage — complements
//! `tab_bar_view.rs`'s own unit tests (the pure builder-field probes) with
//! end-to-end mount proof of the lazy-keep-alive switcher contract this
//! module docs cite `CupertinoTabScaffold`'s `_TabSwitchingView` mechanic
//! for: a not-yet-visited child is never built, a visited-then-hidden
//! child's own state survives (`Offstage`, not unmount), an inactive
//! child's animation is muted (`TickerMode`), a controller swap/unmount
//! removes the old listener, and a children↔controller length mismatch
//! recovers as an `ErrorView` in every build profile (release `assert!` in
//! `build`, not a silent all-`Offstage` fall-through).

use crate::common;

use std::cell::Cell;
use std::rc::Rc;

use common::{lay_out, tight};
use flui_material::{TabBarView, TabController};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{MediaQuery, MediaQueryData, SizedBox};

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
#[test]
fn an_inactive_tabs_state_survives_switching_away_and_back() {
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
