//! Contract C1 (`setState`) at the widget level: a `StatefulView` whose
//! `ViewState::build` reads a value that changes between frames. After the state
//! changes, pumping a frame must rebuild, reconcile, and re-lay-out the subtree
//! to the new size — proving the retained element keeps identity while its
//! configuration updates.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::common::{lay_out, loose, size};
use flui_widgets::SizedBox;
use flui_widgets::prelude::*;

/// A box whose side length is owned by shared state — the test mutates it
/// between frames to stand in for a `setState` call.
#[derive(Clone, StatefulView)]
struct Resizable {
    side: Arc<AtomicU32>,
    builds: Arc<AtomicU32>,
}

struct ResizableState {
    side: Arc<AtomicU32>,
    builds: Arc<AtomicU32>,
}

impl StatefulView for Resizable {
    type State = ResizableState;

    fn create_state(&self) -> Self::State {
        ResizableState {
            side: Arc::clone(&self.side),
            builds: Arc::clone(&self.builds),
        }
    }
}

impl ViewState<Resizable> for ResizableState {
    fn build(&self, _view: &Resizable, _ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.fetch_add(1, Ordering::Relaxed);
        let side = self.side.load(Ordering::Relaxed) as f32;
        SizedBox::square(side)
    }
}

#[test]
fn set_state_rebuilds_subtree_with_new_state() {
    let side = Arc::new(AtomicU32::new(50));
    let builds = Arc::new(AtomicU32::new(0));
    let mut laid = lay_out(
        Resizable {
            side: Arc::clone(&side),
            builds: Arc::clone(&builds),
        },
        loose(1000.0),
    );
    assert_eq!(laid.size(laid.root()), size(50.0, 50.0));
    let builds_after_mount = builds.load(Ordering::Relaxed);
    assert!(builds_after_mount >= 1, "state should build at least once");

    // setState: the state changes, then the framework pumps a frame.
    side.store(120, Ordering::Relaxed);
    laid.pump();

    assert!(
        builds.load(Ordering::Relaxed) > builds_after_mount,
        "pump should rebuild the StatefulView (state.build re-runs)",
    );
    // The element keeps identity (updated in place, not remounted): one node,
    // same render id, new size.
    assert_eq!(
        laid.render_node_count(),
        1,
        "the box should be updated in place"
    );
    assert_eq!(laid.root(), laid.current_root());
    assert_eq!(
        laid.size(laid.root()),
        size(120.0, 120.0),
        "the rebuilt SizedBox should reflect the new state",
    );
}
