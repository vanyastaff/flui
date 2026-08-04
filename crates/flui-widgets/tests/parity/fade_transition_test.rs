//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/fade_transition_test.dart`
//! (tag `3.44.0`), both cases.
//! - Widget: `packages/flutter/lib/src/widgets/transitions.dart`
//!   `FadeTransition` (line 589) — a `SingleChildRenderObjectWidget`, **not**
//!   an `AnimatedWidget`.
//! - Render object: `packages/flutter/lib/src/rendering/proxy_box.dart`
//!   `RenderAnimatedOpacity` (line 1107, mixing in
//!   `RenderAnimatedOpacityMixin`) — it subscribes to the `Animation<double>`
//!   itself, in its own `attach`, and a tick only calls
//!   `markNeedsCompositingBitsUpdate`/`markNeedsPaint`. The Element tree is
//!   never touched: `FadeTransition` carries no `State` and no per-tick
//!   `build()` to run — `createRenderObject`/`updateRenderObject` are the
//!   only hooks any `RenderObjectWidget` gets, and Flutter calls those on
//!   mount/config-change only, never from an animation tick. An opacity tick
//!   cannot schedule a widget rebuild even in principle.
//!
//! ## FLUI's `FadeTransition` takes the *other* Flutter shape
//!
//! `crates/flui-widgets/src/transitions/fade_transition.rs` ports
//! `FadeTransition` as a `StatefulView` implementing `AnimatedView`
//! (`impl_animated_view!(FadeTransition)`), whose `ViewState::build` re-reads
//! `view.opacity.value()` into a fresh `Opacity::new(..)` on every rebuild.
//! `AnimatedView`'s own doc (`crates/flui-view/src/view/animated.rs:14-17`)
//! names the model it followed: "similar to Flutter's `AnimatedWidget` ...
//! the element is automatically marked dirty and rebuilt" — and that same
//! file's worked doc example (`animated.rs:58-76`) uses `FadeTransition`
//! itself as the illustration, even though the real Flutter `FadeTransition`
//! quoted above never extended `AnimatedWidget`.
//!
//! This is not a hypothesis about FLUI's behavior: this repo's own existing
//! non-parity test `crates/flui-widgets/tests/binding_animation.rs`
//! documents the resulting chain verbatim in its module doc — "the
//! `FadeTransition`'s subscription marks it dirty into the build inbox →
//! `build_scope` drains it and rebuilds" — and
//! `crates/flui-view/src/owner/build_owner.rs`'s own `build_scope` comments
//! name the same mechanism ("`AnimatedView`'s mark-dirty callback already set
//! the flag").
//!
//! FLUI already owns the render object Flutter's actual `FadeTransition`
//! needs: `flui_objects::RenderAnimatedOpacity`
//! (`crates/flui-objects/src/proxy/animated_opacity.rs`) is a
//! behavior-faithful port of `RenderAnimatedOpacityMixin` — its own module
//! doc cites the exact Flutter mixin and mirrors the "listener registered in
//! `attach`" design, updating only via a repaint mark, never a rebuild. But
//! it backs a *different* widget, `AnimatedOpacity`
//! (`crates/flui-widgets/src/animated/animated_opacity.rs`), not
//! `FadeTransition`; `crates/flui-widgets/tests/parity/
//! implicit_animations_test.rs:24-26` documents that mapping explicitly —
//! "`AnimatedOpacity` → `RenderAnimatedOpacity` directly (no per-tick
//! `Opacity` rebuild)". `FadeTransition` never received the same wiring.
//!
//! ## Case ledger
//!
//! 1. `'FadeTransition'` — **divergence pin**, asserting the oracle:
//!    [`ticking_the_opacity_animation_never_rebuilds_the_widget_tree`]. The
//!    oracle drives `controller.forward()` + `pumpAndSettle()` over the
//!    controller's 2-second duration behind a `debugPrintBuildScope` capture
//!    and asserts the build-scope log never grows past its one initial entry
//!    pair — i.e. the running animation causes zero additional widget-tree
//!    rebuild work. FLUI has no `debugPrintBuildScope`/`BuildOwner.buildScope`
//!    logging equivalent, so this port substitutes a build counter on
//!    `FadeTransition`'s own child (the same `Arc<AtomicU32>` idiom
//!    `stateful_test.rs`/`localizations_test.rs` already use) and asserts it
//!    stays flat across the run — the same observable claim ("does ticking
//!    rebuild the widget tree"), made falsifiable without Flutter's specific
//!    logging mechanism. Currently red for the architectural reason above;
//!    see the test's own doc comment for the captured failure.
//! 2. `'No exception when calling markNeedsPaint during opacity changes'`
//!    (regression for flutter/flutter#157312) — **out of scope, missing
//!    capability**. The regression is specific to
//!    `RenderAnimatedOpacityMixin`'s own attach/tick/`markNeedsPaint`
//!    bookkeeping racing an *externally* triggered
//!    `key.currentContext?.findRenderObject()?.markNeedsPaint()` on the
//!    child mid opacity-change — a scenario that only exists because
//!    Flutter's real `FadeTransition` keeps ONE persistent render object
//!    subscribed to the animation for the widget's entire lifetime. FLUI's
//!    `FadeTransition` has no such object: each tick re-derives a value into
//!    a brand-new `Opacity` view (see above), so there is no
//!    render-object-level listener bookkeeping for this regression to
//!    exercise at all — there is nothing this port could perturb that would
//!    be testing the same failure mode. The render object that *does* carry
//!    that bookkeeping, `flui_objects::RenderAnimatedOpacity`, backs
//!    `AnimatedOpacity`, a different widget with its own oracle
//!    (`crates/flui-widgets/tests/parity/implicit_animations_test.rs`); a
//!    substitution here would test `AnimatedOpacity`, not `FadeTransition`,
//!    and could not catch a `FadeTransition`-specific regression, so this
//!    port does not attempt one.
//!
//! ## Production bug filed by this port (case 1)
//!
//! `FadeTransition` (`crates/flui-widgets/src/transitions/fade_transition.rs`)
//! rebuilds its entire subtree on every animation tick instead of updating a
//! persistent render object directly, unlike Flutter's real
//! `FadeTransition`/`RenderAnimatedOpacity` pair. This is not just a
//! spec-fidelity gap: it is the exact rebuild-churn cost
//! `flui_objects::RenderAnimatedOpacity` was built to avoid (see its module
//! doc's "documented divergence" section on why *it* still costs a full
//! repaint rather than a compositor re-blend) — `FadeTransition` pays a full
//! *rebuild* on top of that, every tick, for the whole widget subtree the
//! transition wraps. Pinned above, not fixed here: fixing it means retargeting
//! `FadeTransition` onto `RenderAnimatedOpacity` (the exact move
//! `AnimatedOpacity` already made), a `flui-widgets` design change with its
//! own render-object wiring and `ProxyAnimation` plumbing, outside this test
//! port's scope.

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
/// `3.44.0`). **Divergence pin, asserting the oracle.**
///
/// Ported observable: the oracle's build-scope log stays flat (length 2:
/// one "called"/"finished" pair, from the initial `pumpWidget`) through an
/// idle `tester.pump()` AND through a full `controller.forward()` +
/// `pumpAndSettle()` run. This port's analogue is a build counter on
/// `FadeTransition`'s child (see [`CountingChild`]): it must likewise stay
/// flat across the same three checkpoints. See the module doc's
/// "FLUI's `FadeTransition` takes the *other* Flutter shape" section for why
/// it currently does not.
///
/// Captured failure running this assertion for real (before adding
/// `#[ignore]`, no other change) -- the idle-frame checkpoint and the
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
#[ignore = "divergence pin: FadeTransition rebuilds its subtree every tick \
            instead of driving a render object subscribed to the animation \
            the way Flutter's SingleChildRenderObjectWidget FadeTransition \
            does -- see the module doc's architecture section. NOTE the \
            instrument's blind spot, documented on the test: it counts the \
            CHILD's builds, so child-build memoization would turn this green \
            without the divergence being fixed"]
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
    let subject = laid.find_by_render_type("RenderOpacity");
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
