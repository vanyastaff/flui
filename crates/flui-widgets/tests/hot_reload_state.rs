//! Hot-reload state preservation: `perform_reassemble` re-runs every `build()`
//! while `StatefulView` state stays in the element tree.
//!
//! # Parity oracle
//!
//! Flutter `WidgetsBinding.performReassemble()` → `BuildOwner.reassemble` →
//! every element rebuilt in place. The property that makes hot reload useful —
//! and the one this test pins — is that the *state object* survives: the same
//! `ElementId`, the same render id, no second `create_state`, and the mutated
//! value still readable after the rebuild.
//!
//! This is the FLUI half of the parity design's success criterion #2
//! (`docs/designs/2026-06-28-flutter-parity-hot-reload.md` §9: "`StatefulElement`
//! state ptr stable across `perform_reassemble`"), driving the headless frame
//! driver's `perform_reassemble`/`reassemble_render_tree` pair — the same two
//! calls production `PresentationState::apply_hot_reload` makes.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::common::{lay_out, loose, size};
use flui_widgets::SizedBox;
use flui_widgets::prelude::*;

/// A stateful box whose side length and build count are observable.
///
/// `creates` counts `create_state` calls specifically — not `build` calls — so
/// the test can distinguish "the state object survived and rebuilt" from "a
/// fresh state object was created and happens to render the same".
#[derive(Clone, StatefulView)]
struct Counter {
    side: Arc<AtomicU32>,
    builds: Arc<AtomicU32>,
    creates: Arc<AtomicU32>,
}

struct CounterState {
    side: Arc<AtomicU32>,
    builds: Arc<AtomicU32>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        self.creates.fetch_add(1, Ordering::Relaxed);
        CounterState {
            side: Arc::clone(&self.side),
            builds: Arc::clone(&self.builds),
        }
    }
}

impl ViewState<Counter> for CounterState {
    fn build(&self, _view: &Counter, _ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.fetch_add(1, Ordering::Relaxed);
        let side = self.side.load(Ordering::Relaxed) as f32;
        SizedBox::square(side)
    }
}

#[test]
fn perform_reassemble_rebuilds_in_place_and_preserves_state() {
    let side = Arc::new(AtomicU32::new(40));
    let builds = Arc::new(AtomicU32::new(0));
    let creates = Arc::new(AtomicU32::new(0));

    let mut laid = lay_out(
        Counter {
            side: Arc::clone(&side),
            builds: Arc::clone(&builds),
            creates: Arc::clone(&creates),
        },
        loose(1000.0),
    );

    assert_eq!(laid.size(laid.root()), size(40.0, 40.0));
    let creates_after_mount = creates.load(Ordering::Relaxed);
    let builds_after_mount = builds.load(Ordering::Relaxed);
    let root_before = laid.root();
    let element_before = laid.current_root();

    // Mutate the state's backing value as a tap handler would, without asking
    // the framework to rebuild yet — a reload must pick this up.
    side.store(140, Ordering::Relaxed);

    // The hot-reload entry point: mark every element AND every render object
    // dirty, then pump one frame. This is exactly what
    // `PresentationState::apply_hot_reload(HotReload)` calls.
    laid.perform_reassemble();
    laid.reassemble_render_tree();
    // `tick()` drives a frame WITHOUT self-marking the root dirty (unlike
    // `pump()`), so the rebuild below can only happen because
    // `perform_reassemble` marked the elements dirty — the property under test.
    laid.tick();

    // 1. Every build re-ran — the reload reached the view.
    assert!(
        builds.load(Ordering::Relaxed) > builds_after_mount,
        "perform_reassemble must re-run StatefulView::build",
    );

    // 2. No state object was created: the SAME state survived the reload.
    assert_eq!(
        creates.load(Ordering::Relaxed),
        creates_after_mount,
        "perform_reassemble must preserve State — create_state must not run again",
    );

    // 3. The element and render object kept identity (updated in place, not
    //    remounted) — the property "state ptr stable" reduces to.
    assert_eq!(
        laid.current_root(),
        element_before,
        "the logical render root must keep its id across a reassemble",
    );
    assert_eq!(
        laid.root(),
        root_before,
        "the element id must be stable across a reassemble"
    );
    assert_eq!(
        laid.render_node_count(),
        1,
        "the box must be updated in place, not remounted",
    );

    // 4. The state's mutation is observable through the rebuilt tree.
    assert_eq!(
        laid.size(laid.root()),
        size(140.0, 140.0),
        "the rebuilt view must reflect the state value mutated before the reload",
    );
}

/// The render half alone must also be idempotent: calling
/// `reassemble_render_tree` twice and pumping leaves the tree intact (no
/// duplicate nodes, no lost state), matching the production path's tolerance
/// for a reload that arrives with nothing new to build.
#[test]
fn reassemble_is_idempotent_across_repeated_reloads() {
    let side = Arc::new(AtomicU32::new(30));
    let builds = Arc::new(AtomicU32::new(0));
    let creates = Arc::new(AtomicU32::new(0));

    let mut laid = lay_out(
        Counter {
            side: Arc::clone(&side),
            builds: Arc::clone(&builds),
            creates: Arc::clone(&creates),
        },
        loose(1000.0),
    );
    let creates_after_mount = creates.load(Ordering::Relaxed);

    for _ in 0..3 {
        laid.perform_reassemble();
        laid.reassemble_render_tree();
        laid.tick();
    }

    assert_eq!(
        creates.load(Ordering::Relaxed),
        creates_after_mount,
        "repeated reloads must never re-create state",
    );
    assert_eq!(
        laid.render_node_count(),
        1,
        "repeated reloads must not duplicate render nodes",
    );
    assert_eq!(laid.size(laid.root()), size(30.0, 30.0));
}
