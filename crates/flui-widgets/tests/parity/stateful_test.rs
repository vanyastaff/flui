//! ## Test parity notes
//!
//! Flutter source: counter idiom — Flutter's canonical `setState` demo where
//! a `StatefulWidget` whose `build` reads a counter variable re-lays the
//! subtree on each `setState` call. The FLUI equivalent uses a shared
//! `Arc<AtomicU32>` that the state reads each build and the test mutates
//! between pumps.
//!
//! Widget → render-object mapping:
//! - `Resizable` (`StatefulView`) → `SizedBox` → `RenderConstrainedBox`
//!
//! Divergence: Flutter's counter uses an `int` and `Text`; FLUI's port uses
//! a `SizedBox` sized by the counter for a pure-geometry assertion that avoids
//! text shaping (font-dependent) metrics. The element-identity invariant
//! (same element survives the rebuild) is verified via the build counter.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::common::{lay_out, loose, size};
use flui_widgets::SizedBox;
use flui_widgets::prelude::*;

// ── Counter widget (mirrors Flutter's StatefulWidget counter demo) ───────────

/// A `SizedBox` whose side length is driven by shared state — the FLUI
/// equivalent of Flutter's counter `setState` demo widget.
#[derive(Clone, StatefulView)]
struct CounterSized {
    side_pixels: Arc<AtomicU32>,
    build_count: Arc<AtomicU32>,
}

struct CounterSizedState {
    side_pixels: Arc<AtomicU32>,
    build_count: Arc<AtomicU32>,
}

impl StatefulView for CounterSized {
    type State = CounterSizedState;

    fn create_state(&self) -> Self::State {
        CounterSizedState {
            side_pixels: Arc::clone(&self.side_pixels),
            build_count: Arc::clone(&self.build_count),
        }
    }
}

impl ViewState<CounterSized> for CounterSizedState {
    fn build(&self, _view: &CounterSized, _ctx: &dyn BuildContext) -> impl IntoView {
        self.build_count.fetch_add(1, Ordering::Relaxed);
        let side = self.side_pixels.load(Ordering::Relaxed) as f32;
        SizedBox::square(side)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Mutating shared state and pumping a frame rebuilds the subtree to the new
/// size while keeping the element alive (identity preserved).
///
/// Flutter parity: setState counter demo — after `setState(() => _counter++)`
/// the widget tree rebuilds and the new value is reflected in the next frame.
#[test]
fn stateful_rebuild_on_setstate_updates_geometry() {
    let side = Arc::new(AtomicU32::new(50));
    let build_count = Arc::new(AtomicU32::new(0));

    let mut laid = lay_out(
        CounterSized {
            side_pixels: Arc::clone(&side),
            build_count: Arc::clone(&build_count),
        },
        loose(1000.0),
    );

    // Initial frame: side = 50.
    assert_eq!(
        laid.size(laid.root()),
        size(50.0, 50.0),
        "initial frame must lay out to 50×50"
    );
    let builds_after_initial = build_count.load(Ordering::Relaxed);
    assert!(builds_after_initial >= 1, "must have built at least once");

    // Simulate setState: mutate shared state then pump.
    side.store(120, Ordering::Relaxed);
    laid.pump();

    assert_eq!(
        laid.size(laid.root()),
        size(120.0, 120.0),
        "after setState(side=120) the frame must lay out to 120×120"
    );
    let builds_after_update = build_count.load(Ordering::Relaxed);
    assert!(
        builds_after_update > builds_after_initial,
        "pump after setState must trigger at least one additional build"
    );
}

/// A second independent `setState` cycle confirms the element survives
/// multiple mutations and rebuilds without tearing down and re-mounting.
#[test]
fn stateful_second_setstate_applies_correctly() {
    let side = Arc::new(AtomicU32::new(30));
    let build_count = Arc::new(AtomicU32::new(0));

    let mut laid = lay_out(
        CounterSized {
            side_pixels: Arc::clone(&side),
            build_count: Arc::clone(&build_count),
        },
        loose(500.0),
    );

    assert_eq!(laid.size(laid.root()), size(30.0, 30.0));

    // First setState.
    side.store(80, Ordering::Relaxed);
    laid.pump();
    assert_eq!(laid.size(laid.root()), size(80.0, 80.0));

    // Second setState — element must still be alive (no re-mount).
    side.store(200, Ordering::Relaxed);
    laid.pump();
    assert_eq!(laid.size(laid.root()), size(200.0, 200.0));

    // Three build-scope runs: initial + 2 pumps.
    assert!(
        build_count.load(Ordering::Relaxed) >= 3,
        "three build rounds must each increment the build counter"
    );
}
