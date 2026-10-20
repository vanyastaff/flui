//! ADR-0078: the presentation capabilities are acquired only in `init_state` and
//! `did_change_dependencies`. `ViewState::apply_view` receives a `BuildContext`,
//! which has none.

use flui_view::BoxedView;
use flui_view::prelude::*;

#[derive(Clone)]
struct Probe {
    child: BoxedView,
}

struct ProbeState;

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        ProbeState
    }
}

impl ViewState<Probe> for ProbeState {
    fn apply_view(&mut self, _view: &Probe, ctx: &dyn BuildContext) {
        let _ = ctx.rebuild_handle();
    }

    fn build(&self, view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        view.child.clone()
    }
}

fn main() {}
