//! Flutter parity for `fade_transition_test.dart` at tag `3.44.0`, verified
//! against local Flutter checkout `f2d640ef`. `FadeTransition` is backed by
//! a persistent `RenderAnimatedOpacity`: ticks update paint/compositing state
//! without entering the widget tree. The retained-layer limitation remains in
//! `RenderAnimatedOpacity` itself: FLUI repaints rather than blend-updating a
//! retained opacity layer.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_widgets::FadeTransition;
use flui_widgets::prelude::*;

use crate::harness;

// ============================================================================
// Fixtures
// ============================================================================

/// A build-counting leaf, standing in for Flutter's `debugPrintBuildScope`
/// log: every time this `StatefulView`'s `build` runs, `build_count`
/// advances. Placed as `FadeTransition`'s child, it observes whether ticking
/// the transition's opacity animation causes the widget tree beneath it to
/// rebuild — the same `Arc<AtomicU32>` idiom `stateful_test.rs`'s
/// `CounterSized` and `localizations_test.rs`'s probes already use.
#[derive(Clone, StatefulView)]
struct CountingChild {
    build_count: Arc<AtomicU32>,
}

struct CountingChildState {
    build_count: Arc<AtomicU32>,
}

#[derive(Clone, StatelessView)]
struct CountingFadeHost {
    opacity: Arc<dyn Animation<f32>>,
    host_build_count: Arc<AtomicU32>,
    child_build_count: Arc<AtomicU32>,
}

impl StatelessView for CountingFadeHost {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.host_build_count.fetch_add(1, Ordering::Relaxed);
        FadeTransition::new(
            Arc::clone(&self.opacity),
            CountingChild {
                build_count: Arc::clone(&self.child_build_count),
            },
        )
    }
}

impl StatefulView for CountingChild {
    type State = CountingChildState;

    fn create_state(&self) -> Self::State {
        CountingChildState {
            build_count: Arc::clone(&self.build_count),
        }
    }
}

impl ViewState<CountingChild> for CountingChildState {
    fn build(&self, _view: &CountingChild, _ctx: &dyn BuildContext) -> impl IntoView {
        self.build_count.fetch_add(1, Ordering::Relaxed);
        SizedBox::new(10.0, 10.0)
    }
}

// ============================================================================
// Case 1 — 'FadeTransition'
// ============================================================================

/// Flutter parity: `fade_transition_test.dart` `'FadeTransition'` (tag
/// `3.44.0`). Regression pin for the render-driven implementation.
///
/// Ported observable: the oracle's build-scope log stays flat (length 2:
/// one "called"/"finished" pair, from the initial `pumpWidget`) through an
/// idle `tester.pump()` AND through a full `controller.forward()` +
/// `pumpAndSettle()` run. This port's analogue is a build counter on
/// `FadeTransition`'s child (see [`CountingChild`]): it must likewise stay
/// flat across the same three checkpoints.
///
/// Captured RED failure before the render-driven fix -- the idle-frame checkpoint and the
/// forward-run sanity checks (completion, final opacity) all passed first,
/// so the failure below is the interesting one, not an artifact of a
/// mis-driven run:
/// ```text
/// thread '...ticking_the_opacity_animation_never_rebuilds_the_widget_tree' panicked at
/// crates/flui-widgets/tests/parity/fade_transition_test.rs:248:5:
/// assertion `left == right` failed: ticking FadeTransition's opacity
/// animation must never rebuild the widget tree (Flutter's FadeTransition
/// bypasses the Element tree entirely) -- oracle: build count stays flat at
/// the initial-mount count; FLUI actual: it grows every tick
///   left: 12
///  right: 1
/// ```
/// 12 total builds (1 initial mount + 1 detection frame + 11 tick-driven
/// rebuilds, one per `pump_for` call below) against the oracle's flat 1 --
/// every single frame that advances the running controller costs a full
/// rebuild of the child subtree.
#[test]
fn ticking_the_opacity_animation_never_rebuilds_the_widget_tree() {
    let controller = AnimationController::without_ticker(Duration::from_secs(2));
    let opacity: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
    let build_count = Arc::new(AtomicU32::new(0));

    let mut laid = harness::pump_widget(
        FadeTransition::new(
            opacity,
            CountingChild {
                build_count: Arc::clone(&build_count),
            },
        ),
        harness::screen_of(100.0, 50.0),
    );

    let after_initial_mount = build_count.load(Ordering::Relaxed);
    let subject = laid.find_by_render_type("RenderAnimatedOpacity");
    let opacity_at_rest = laid.opacity(subject);
    assert!(
        after_initial_mount >= 1,
        "the child must build at least once during the initial mount"
    );

    // Oracle checkpoint 1: `await tester.pump()` with nothing dirty -- the log
    // stays flat (length 2). This checkpoint alone is not FadeTransition-specific
    // (any idle frame is a no-op), but it establishes the oracle's own baseline
    // before the animation starts. `LaidOut::pump` is NOT the right tool here --
    // its own doc says it is the `setState` equivalent (unconditionally marks the
    // root dirty); `LaidOut::tick` is the bare-frame equivalent Flutter's
    // `tester.pump()` actually is, so that is what this checkpoint drives.
    laid.tick();
    assert_eq!(
        build_count.load(Ordering::Relaxed),
        after_initial_mount,
        "an idle frame with nothing dirty must not rebuild anything"
    );

    // Oracle checkpoint 2 (the interesting one): `controller.forward()` +
    // `pumpAndSettle()` -- driven here as registering the controller with the
    // binding, starting it, then pumping past its full 2-second duration.
    laid.register_controller(controller.clone());
    controller.forward().expect("a fresh controller forwards");
    // Detection frame (matches `binding_animation.rs`/`animated_switcher_test.rs`):
    // the first pump after `forward()` holds the run-start value, movement begins
    // on the next one -- so pump one tick past the 2-second duration to guarantee
    // completion.
    laid.pump_for(Duration::ZERO);
    for _ in 0..11 {
        laid.pump_for(Duration::from_millis(200));
    }
    assert_eq!(
        controller.status(),
        AnimationStatus::Completed,
        "sanity: the forward run must actually complete over the pumps above"
    );
    assert!(
        (laid.opacity(laid.root()) - 1.0).abs() < 1e-4,
        "sanity: the render tree's committed opacity must reach 1.0 -- the tree \
         DID observe the animation's value, just (per the divergence above) via \
         full rebuilds rather than a direct render-object update"
    );

    assert_ne!(
        laid.opacity(subject),
        opacity_at_rest,
        "precondition: the animation must actually have advanced — without \
         this the build-count assertion below could pass simply because \
         nothing ticked",
    );

    assert_eq!(
        build_count.load(Ordering::Relaxed),
        after_initial_mount,
        "ticking FadeTransition's opacity animation must never rebuild the widget \
         tree (Flutter's FadeTransition bypasses the Element tree entirely) -- \
         oracle: build count stays flat at the initial-mount count; FLUI actual: \
         it grows every tick",
    );
}

#[test]
fn mounted_fade_source_swap_preserves_render_and_child_identity() {
    let controller_a = AnimationController::without_ticker(Duration::from_secs(1));
    let controller_b = AnimationController::without_ticker(Duration::from_secs(1));
    controller_a.set_value(0.2);
    controller_b.set_value(0.6);
    let animation_a: Arc<dyn Animation<f32>> = Arc::new(controller_a.clone());
    let animation_b: Arc<dyn Animation<f32>> = Arc::new(controller_b.clone());
    let host_build_count = Arc::new(AtomicU32::new(0));
    let child_build_count = Arc::new(AtomicU32::new(0));

    let mut laid = harness::pump_widget(
        CountingFadeHost {
            opacity: animation_a,
            host_build_count: Arc::clone(&host_build_count),
            child_build_count: Arc::clone(&child_build_count),
        },
        harness::screen_of(100.0, 50.0),
    );
    let render_id = laid.find_by_render_type("RenderAnimatedOpacity");
    let child_render_id = laid.find_by_render_type("RenderConstrainedBox");
    let child_builds_after_mount = child_build_count.load(Ordering::Relaxed);

    laid.pump_widget(CountingFadeHost {
        opacity: animation_b,
        host_build_count: Arc::clone(&host_build_count),
        child_build_count: Arc::clone(&child_build_count),
    });

    assert_eq!(
        laid.find_by_render_type("RenderAnimatedOpacity"),
        render_id,
        "source replacement updates the mounted render object in place"
    );
    assert_eq!(
        laid.find_by_render_type("RenderConstrainedBox"),
        child_render_id,
        "the reconciled child keeps its mounted render identity"
    );
    assert_eq!(laid.opacity(render_id), 0.6);
    assert_eq!(
        child_build_count.load(Ordering::Relaxed),
        child_builds_after_mount + 1,
        "the deliberately non-memoized child rebuilds exactly once during the \
         mounted source-swap reconciliation"
    );
    assert_eq!(host_build_count.load(Ordering::Relaxed), 2);

    controller_a.set_value(0.9);
    assert_eq!(laid.opacity(render_id), 0.6, "the old source is detached");
    laid.tick();
    assert_eq!(
        child_build_count.load(Ordering::Relaxed),
        child_builds_after_mount + 1,
        "a frame after the old source ticks must not reconcile the child"
    );
    controller_b.set_value(0.7);
    assert_eq!(
        laid.opacity(render_id),
        0.7,
        "the new source drives opacity"
    );
    laid.tick();
    assert_eq!(
        child_build_count.load(Ordering::Relaxed),
        child_builds_after_mount + 1,
        "neither old nor new source ticks trigger another child reconciliation"
    );
}
