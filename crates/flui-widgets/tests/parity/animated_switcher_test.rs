//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/animated_switcher_test.dart`
//! (tag `3.44.0`), oracle: `packages/flutter/lib/src/widgets/animated_switcher.dart`
//! (tag `3.44.0`).
//!
//! Widget → render-object mapping:
//! - `AnimatedSwitcher` → a `StatefulView` composing `layout_builder`'s output
//!   (default: [`Stack`] → `RenderStack`) over each entry's
//!   `transition_builder` output (default: [`FadeTransition`] →
//!   [`Opacity`](crate) → `RenderOpacity`)
//!   (`crates/flui-widgets/src/animated/animated_switcher.rs`).
//!
//! Key-based substitution: the oracle forces "not `Widget.canUpdate`-compatible"
//! by giving several instances of the SAME concrete type (`Container`) distinct
//! `Key`s. FLUI's `Keyed<V>` wrapper (`flui_foundation::Keyed`) carries no
//! `impl View` (confirmed: no such impl exists anywhere in `flui-view` or
//! `flui-widgets`; keying a widget from the outside requires hand-rolling
//! `impl View { fn key() }`, same as `flui-widgets/src/overlay/mod.rs`'s
//! `OverlayEntryView`) — not a gap `AnimatedSwitcher` itself needs to work
//! around (its own `did_update_view` reuses `View::can_update` exactly as the
//! oracle uses `Widget.canUpdate`, see the widget's module doc), just a
//! missing convenience for a TEST to mint several keyed instances of one
//! type. Cases that only need "force a fresh, non-`can_update`-compatible
//! entry" (1, 2, 10) substitute distinct CONCRETE TYPES instead of distinct
//! keys — an equally valid way to fail `can_update` (which is `view_type_id`
//! equality AND key equality; different types already fail the first half).
//! The one case that specifically exercises a KEY reappearing after its
//! original entry has already gone through the pipeline (11) uses a small
//! test-local `Keyed<W>` — the exact hand-rolled-`impl View` pattern
//! `OverlayEntryView` already uses in production, not new framework surface.
//!
//! Ported: 10 of 12 oracle cases.
//! - `'AnimatedSwitcher fades in a new child.'`
//! - `'AnimatedSwitcher can handle back-to-back changes.'` (adapted: distinct
//!   types substitute for distinct keys, see above — otherwise ported in
//!   full, including the middle child's disappearance; see the harness note
//!   below)
//! - `'AnimatedSwitcher doesn't transition in a new child of the same type.'`
//! - `'AnimatedSwitcher handles null children.'`
//! - `'AnimatedSwitcher doesn't start any animations after dispose.'`
//! - `'AnimatedSwitcher uses custom layout.'`
//! - `'AnimatedSwitcher uses custom transitions.'`
//! - `'AnimatedSwitcher doesn't reset state of the children in transitions.'`
//!   (adapted: three distinct probe types substitute for one `StatefulTest`
//!   type keyed three ways, per the key-substitution note above)
//! - `'AnimatedSwitcher updates widgets without animating if they are
//!   isomorphic.'`
//! - `'AnimatedSwitcher does not crash at zero area'`
//!
//! Adapted-and-narrowed: 1 of 12.
//! - `'AnimatedSwitcher updates previous child transitions if the
//!   transitionBuilder changes.'` — ported the core assertion (every cached
//!   transition, including outgoing ones, is rebuilt under the new
//!   `transition_builder`); the oracle's OWN preceding sub-assertion (each
//!   cached child is `isA<KeyedSubtree>()`) is FLUI's `KeyedEntry` wrapper,
//!   which is a private (non-`pub`) implementation type with nothing for an
//!   integration test outside the module to assert on — the render-type
//!   evidence (transitions actually swap) is asserted instead.
//!
//! Adapted: 1 of 12.
//! - `'AnimatedSwitcher does not duplicate animations if the same child is
//!   entered twice.'` — needs a KEYED widget re-entering after its ORIGINAL
//!   entry already exists (possibly still outgoing); ported as
//!   `reentering_a_still_dismissing_key_starts_a_fresh_entry_not_a_duplicate`
//!   using the test-local `Keyed<W>` wrapper (see above) — functionally
//!   covered, not dropped; see that test.
//!
//! Arithmetic: 10 ported + 1 adapted-and-narrowed + 1 adapted = 12.
//!
//! No divergence: oracle case 2's `container2 findsNothing` after the third
//! back-to-back swap (with no elapsed time between swaps) is not special-cased
//! Flutter behavior — it falls straight out of `AnimationController::reverse()`
//! firing `Dismissed` SYNCHRONOUSLY when called on a controller already
//! sitting at `value == 0.0` (`container2`'s entry, demoted the instant its own
//! forward run started, never having ticked away from 0). `back_to_back_swaps`
//! ports this in full, and `demoting_an_entry_still_at_the_lower_bound_dismisses_it_immediately`
//! pins the mechanism directly at the `AnimationController` level.
//!
//! Harness note (not a divergence): a `pump_widget`/`pump_for` call ticks
//! registered controllers on the virtual clock BEFORE running that frame's
//! build pass (`flui-binding`'s `pump_frame`: `vsync.tick_all` is step 3,
//! `build_scope` is step 4-7) — so a `reverse()`/`forward()` call made
//! DURING a build (as `did_update_view` does) is not observed by
//! `vsync.tick_all` until the FOLLOWING tick, which then spends itself purely
//! anchoring the fresh run's `t = 0` (elapsed `0` for that call) rather than
//! advancing it. Any assertion that reads a just-started run's value after
//! real elapsed time needs one extra `pump_for(Duration::ZERO)` "detection"
//! tick first — the same idiom `animated_container_test.rs`'s
//! `animated_container_retargets_one_property_while_sibling_holds_steady`
//! already uses (`laid.pump_for(Duration::ZERO); // detection frame: anchors
//! the fresh run`). Flutter's own `tester.pump(duration)` has no such
//! artifact (its ticker observes the SAME frame's `forward()`/`reverse()`
//! call), so this is purely how this headless harness sequences one
//! `pump_frame` call, not a product-behavior difference.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use flui_animation::{Animation, AnimationController, Scheduler, Vsync};
use flui_foundation::{ValueKey, ViewKey};
use flui_types::Color;
use flui_view::element::ElementKind;
use flui_view::prelude::{BuildContext, StatefulView};
use flui_view::{BoxedView, IntoView, StatelessView, View, ViewExt, ViewState};
use flui_widgets::{
    AnimatedSwitcher, ColoredBox, Column, FadeTransition, ScaleTransition, SizedBox, Text,
    VsyncScope,
};

use crate::common::{self, lay_out, lay_out_animated, tight};
use crate::harness::screen;

const RUN: Duration = Duration::from_millis(100);

/// The three colors the oracle's `containerOne`/`Two`/`Three` paint (values
/// are cosmetic only — no test reads them back, only opacity/transition
/// identity matters).
fn child_a() -> BoxedView {
    ColoredBox::new(Color::rgba(0, 0, 0, 0)).boxed()
}
fn child_b() -> BoxedView {
    SizedBox::new(10.0, 10.0).boxed()
}
fn child_c() -> BoxedView {
    Text::new("three").boxed()
}

/// `AnimatedSwitcher fades in a new child.`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. Three back-to-back,
/// `can_update`-incompatible children (distinct concrete types substitute for
/// the oracle's distinct `Key`s — see the module doc). Exact fade values at
/// each step follow from `Curves::Linear` (the widget's default in both
/// directions) driving `AnimationController::value` linearly over `RUN`.
#[test]
fn fades_in_a_new_child() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(vsync.clone(), AnimatedSwitcher::new(RUN).child(child_a()));
    let mut laid = lay_out_animated(root, screen(), vsync.clone());

    let opacities = |laid: &common::LaidOut| -> Vec<f32> {
        laid.find_all_by_render_type("RenderOpacity")
            .iter()
            .map(|&id| laid.opacity(id))
            .collect()
    };

    assert_eq!(
        opacities(&laid),
        vec![1.0],
        "the first child mounts fully opaque, no motion"
    );

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(child_b()),
    ));
    laid.pump_for(Duration::ZERO); // detection tick (module doc's harness note): anchors the fresh reverse and forward runs
    laid.pump_for(Duration::from_millis(50));
    let mid = opacities(&laid);
    assert_eq!(
        mid.len(),
        2,
        "outgoing child_a and incoming child_b are both mounted mid-transition"
    );
    assert_eq!(
        mid[0], 0.5,
        "child_a (outgoing) is halfway through its 100ms reverse at 50ms"
    );
    assert_eq!(
        mid[1], 0.5,
        "child_b (incoming) is halfway through its 100ms forward at 50ms"
    );

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(child_c()),
    ));
    laid.pump_for(Duration::ZERO); // detection tick for child_b's fresh reverse and child_c's fresh forward
    laid.pump_for(Duration::from_millis(10));
    let late = opacities(&laid);
    assert_eq!(
        late.len(),
        3,
        "all three entries are still mounted 10ms after the second swap"
    );
    assert!(
        (late[0] - 0.4).abs() < 1e-4,
        "child_a: reversing from 0.5, 10ms further along its own 100ms run -> 0.4, got {}",
        late[0]
    );
    assert!(
        (late[1] - 0.4).abs() < 1e-4,
        "child_b: demoted at 0.5, reversing 10ms into a FRESH 100ms run -> 0.4, got {}",
        late[1]
    );
    assert!(
        (late[2] - 0.1).abs() < 1e-4,
        "child_c: forward 10ms into its 100ms run -> 0.1, got {}",
        late[2]
    );

    laid.pump_for(RUN);
    assert_eq!(
        opacities(&laid),
        vec![1.0],
        "settling the run dismisses every outgoing entry, leaving only child_c at full opacity"
    );
    assert_eq!(
        vsync.len(),
        1,
        "settling must unregister every dismissed entry's controller, leaving only child_c's"
    );
}

/// `AnimatedSwitcher can handle back-to-back changes.` (adapted: distinct
/// types substitute for distinct keys — see the module doc — otherwise
/// ported in full)
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. Three back-to-back
/// swaps with NO elapsed time between them: `child_a` survives (present
/// throughout, current then outgoing), `child_b` is demoted to outgoing the
/// instant its own forward run started — before any real time gave it a
/// value above `0.0` — so reversing it synchronously dismisses it (see the
/// module doc's harness note); `child_c` is the new current entry.
#[test]
fn back_to_back_swaps() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(vsync.clone(), AnimatedSwitcher::new(RUN).child(child_a()));
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    assert_eq!(
        laid.find_all_by_render_type("RenderDecoratedBox").len(),
        1,
        "child_a present"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "child_b absent"
    );
    assert!(laid.find_text("three").is_none(), "child_c absent");

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(child_b()),
    ));
    assert_eq!(
        laid.find_all_by_render_type("RenderDecoratedBox").len(),
        1,
        "child_a still present (outgoing)"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        1,
        "child_b present (current)"
    );
    assert!(laid.find_text("three").is_none(), "child_c absent");

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(child_c()),
    ));
    assert_eq!(
        laid.find_all_by_render_type("RenderDecoratedBox").len(),
        1,
        "child_a still present (outgoing)"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderConstrainedBox").len(),
        0,
        "child_b dismissed synchronously: it never ticked away from value 0.0 before being reversed"
    );
    assert!(
        laid.find_text("three").is_some(),
        "child_c present (current)"
    );
}

/// `AnimatedSwitcher doesn't transition in a new child of the same type.`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. Two keyless
/// `ColoredBox` values (same concrete type, same `None` key) are
/// `can_update`-compatible — the entry updates in place, no second
/// `FadeTransition`/`RenderOpacity` mounts, and the existing one stays at its
/// already-settled opacity.
#[test]
fn same_type_keyless_child_does_not_transition() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ColoredBox::new(Color::rgba(0, 0, 0, 0))),
    );
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    assert_eq!(laid.find_all_by_render_type("RenderOpacity").len(), 1);
    assert_eq!(laid.opacity(laid.find_by_render_type("RenderOpacity")), 1.0);

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ColoredBox::new(Color::rgba(0, 0, 0, 255))),
    ));
    laid.pump_for(Duration::from_millis(50));

    assert_eq!(
        laid.find_all_by_render_type("RenderOpacity").len(),
        1,
        "a can_update-compatible child update must not start a second transition"
    );
    assert_eq!(
        laid.opacity(laid.find_by_render_type("RenderOpacity")),
        1.0,
        "the untouched entry stays at its already-settled opacity"
    );
}

/// `AnimatedSwitcher handles null children.`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. `None` <-> `Some`
/// transitions animate exactly like any other `can_update`-incompatible swap.
/// Ported in full, including the oracle's tail: a SECOND `pumpWidget` of the
/// SAME child-less switcher while the previous child is still reversing out
/// is a genuine no-op reconfigure (`has_new_child == has_old_child == false`)
/// that must not restart or otherwise disturb the in-flight entry.
#[test]
fn handles_no_child() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(vsync.clone(), AnimatedSwitcher::new(RUN));
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    assert_eq!(laid.find_all_by_render_type("RenderOpacity").len(), 0);

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ColoredBox::new(Color::rgba(0, 0, 0, 255))),
    ));
    laid.pump_for(Duration::ZERO); // detection tick: anchors the fresh forward run
    laid.pump_for(Duration::from_millis(50));
    assert_eq!(laid.opacity(laid.find_by_render_type("RenderOpacity")), 0.5);
    laid.pump_for(RUN);

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ColoredBox::new(Color::rgba(0, 0, 0, 0))),
    ));
    assert_eq!(
        laid.find_all_by_render_type("RenderOpacity").len(),
        1,
        "same-type keyless swap updates in place, does not start a None-shaped transition"
    );
    assert_eq!(
        laid.opacity(laid.find_by_render_type("RenderOpacity")),
        1.0,
        "an in-place update does not restart the transition; opacity stays at its settled value"
    );

    laid.pump_widget(VsyncScope::new(vsync.clone(), AnimatedSwitcher::new(RUN)));
    laid.pump_for(Duration::ZERO); // detection tick: anchors the fresh reverse run
    laid.pump_for(Duration::from_millis(50));
    assert_eq!(
        laid.opacity(laid.find_by_render_type("RenderOpacity")),
        0.5,
        "child -> None transitions out exactly like child -> child"
    );

    // Oracle tail: a SECOND pumpWidget of the same child-less switcher, mid
    // reverse — a genuine no-op reconfigure. `did_update_view` matches
    // `(None, None)` and takes neither branch, so the in-flight outgoing
    // entry must continue completely undisturbed by this reconfigure.
    laid.pump_widget(VsyncScope::new(vsync.clone(), AnimatedSwitcher::new(RUN)));
    // 49ms, not the oracle's 50ms (which would land exactly on the 100ms
    // reverse duration's completion): this harness ticks vsync BEFORE
    // running that frame's build pass (see the module doc's harness note),
    // so a controller dismissing exactly during a `pump_for` call is swept
    // and removed from the tree within that SAME call — there is no
    // observable "still mounted at exactly 0.0" frame the way Flutter's
    // tick-then-rebuild-next-frame ordering provides. 49ms instead proves
    // the same contract just as strongly: if the no-op reconfigure had
    // wrongly restarted the reverse run, this tick would show ~0.5
    // (a fresh run's detection tick) instead of continuing down to ~0.01.
    laid.pump_for(Duration::from_millis(49));
    let opacity = laid.opacity(laid.find_by_render_type("RenderOpacity"));
    assert!(
        (opacity - 0.01).abs() < 1e-3,
        "the no-op reconfigure must not restart the reverse run: total elapsed \
         (50ms + 49ms of the 100ms reverse) should show near-zero opacity, got \
         {opacity} — 0.5 would mean the no-op reconfigure wrongly restarted the run"
    );
}

/// `AnimatedSwitcher doesn't start any animations after dispose.`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0` — asserts
/// `tester.pumpAndSettle() == 1` (exactly one frame was needed to settle after
/// the swap, i.e. nothing kept requesting more). FLUI's harness has no
/// pump-count-until-settled equivalent, so this asserts the mechanism that
/// guarantee rests on directly: unmounting `AnimatedSwitcher` mid-transition
/// must dispose every entry's controller AND unregister it from `vsync` —
/// `vsync.len() == 0` afterward is the evidence that nothing is left for a
/// frame driver to keep ticking, and a further frame must not panic.
///
/// The unmount is a CHILD swap under a stable `VsyncScope` root (not a root
/// swap): `ElementTree::update` on the ROOT position does not itself run
/// `finalize_tree` before this harness's very next `vsync.len()` read is
/// possible to express within one test body (a generic, pre-existing
/// framework/harness property, confirmed independent of `AnimatedSwitcher` by
/// reproducing it with a root-swapped `AnimatedContainer`) — swapping the
/// CHILD under an unchanged root reconciles and finalizes normally within one
/// `pump_widget` call, which is what actually exercises `AnimatedSwitcherState::dispose`.
#[test]
fn no_animation_after_dispose() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ColoredBox::new(Color::rgba(0, 0, 0, 255))),
    );
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    assert_eq!(
        vsync.len(),
        1,
        "the single mounted entry registers its controller"
    );
    laid.pump_for(Duration::from_millis(50));

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        ColoredBox::new(Color::rgba(255, 0, 0, 255)),
    ));
    assert_eq!(
        vsync.len(),
        0,
        "unmounting AnimatedSwitcher mid-transition must dispose and unregister every entry"
    );
    laid.pump_for(RUN); // must not panic: nothing left registered to tick
}

/// `AnimatedSwitcher uses custom layout.`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`.
#[test]
fn uses_custom_layout() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN)
            .layout_builder(|current, previous| {
                let mut children = previous;
                children.extend(current);
                Column::new(children).boxed()
            })
            .child(ColoredBox::new(Color::rgba(0, 0, 0, 0))),
    );
    let laid = lay_out_animated(root, screen(), vsync);

    assert_eq!(
        laid.find_all_by_render_type("RenderFlex").len(),
        1,
        "the custom layout_builder's Column (-> RenderFlex) replaces the default Stack"
    );
}

/// `AnimatedSwitcher uses custom transitions.`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. A custom
/// `transition_builder` (here `ScaleTransition` -> `RenderTransform`) is used
/// instead of the default fade, both for the initial child and for a
/// transitioning-out one.
#[test]
fn uses_custom_transitions() {
    let vsync = Vsync::new();
    let scale_builder = |child: BoxedView, animation: Arc<dyn Animation<f32>>| -> BoxedView {
        ScaleTransition::new(animation, child).boxed()
    };
    let root = VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN)
            .transition_builder(scale_builder)
            .child(ColoredBox::new(Color::rgba(0, 0, 0, 0))),
    );
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    assert_eq!(laid.find_all_by_render_type("RenderTransform").len(), 1);
    assert_eq!(
        laid.find_all_by_render_type("RenderOpacity").len(),
        0,
        "no default fade is built"
    );

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).transition_builder(scale_builder),
    ));
    laid.pump_for(Duration::from_millis(50));
    assert_eq!(
        laid.find_all_by_render_type("RenderTransform").len(),
        1,
        "the outgoing entry (child -> None) is scaled too, via the same custom builder"
    );
}

/// Declares a `StatefulView` marker type (a distinct concrete type per
/// invocation, so each is `can_update`-incompatible with the others) whose
/// state bumps a shared generation counter exactly once, in `init_state`.
/// Substitutes for the oracle's single `StatefulTest` type instantiated three
/// times under three distinct `UniqueKey`s — see the module doc's key
/// substitution note.
macro_rules! generation_probe {
    ($view:ident, $state:ident) => {
        #[derive(Clone, StatefulView)]
        struct $view(Arc<AtomicU32>);

        struct $state(Arc<AtomicU32>);

        impl StatefulView for $view {
            type State = $state;
            fn create_state(&self) -> Self::State {
                $state(Arc::clone(&self.0))
            }
        }

        impl ViewState<$view> for $state {
            fn init_state(&mut self, _ctx: &dyn BuildContext) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            fn build(&self, _view: &$view, _ctx: &dyn BuildContext) -> impl IntoView {
                SizedBox::new(10.0, 10.0)
            }
        }
    };
}

/// `AnimatedSwitcher doesn't reset state of the children in transitions.`
/// (adapted: three distinct probe types substitute for one `StatefulTest`
/// type keyed three ways — see the module doc.)
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. Each of three
/// back-to-back swaps mounts exactly ONE fresh `State` (`init_state` runs
/// once per swap, not once per frame) — an entry animating out must stay
/// MOUNTED, not be torn down and rebuilt.
///
/// Dropped co-assertions (this port keeps ONLY the generation counter, which
/// is this test's own distinguishing point — the others duplicate coverage
/// this file already carries elsewhere on the same shape of children):
/// - The `FadeTransition` COUNT at each step (1, then 2, then 3 — via
///   `find.byType(FadeTransition)` / `.at(0/1/2)`) — the same count
///   trajectory `fades_in_a_new_child` already asserts.
/// - The exact opacity VALUES at each step (`1.0`, then `0.5`, then
///   `0.4`/`0.4`/`0.1`) — the identical trajectory (same durations, same
///   `Curves::Linear`, same three-entries-in-flight shape)
///   `fades_in_a_new_child` already asserts in full, just against
///   `ColoredBox`/`SizedBox`/`Text` instead of `StatefulTest`.
#[test]
fn preserves_child_state_across_transitions() {
    generation_probe!(ProbeA, ProbeAState);
    generation_probe!(ProbeB, ProbeBState);
    generation_probe!(ProbeC, ProbeCState);

    let generation = Arc::new(AtomicU32::new(0));
    let vsync = Vsync::new();
    let root = VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ProbeA(Arc::clone(&generation))),
    );
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    assert_eq!(generation.load(Ordering::SeqCst), 1);

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ProbeB(Arc::clone(&generation))),
    ));
    laid.pump_for(Duration::from_millis(50));
    assert_eq!(
        generation.load(Ordering::SeqCst),
        2,
        "exactly one more State mounted, not re-mounted per frame"
    );

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(ProbeC(Arc::clone(&generation))),
    ));
    laid.pump_for(Duration::from_millis(10));
    assert_eq!(generation.load(Ordering::SeqCst), 3);
    laid.pump_for(RUN);
    assert_eq!(
        generation.load(Ordering::SeqCst),
        3,
        "settling the whole run must not remount anything further"
    );
}

/// `AnimatedSwitcher updates widgets without animating if they are
/// isomorphic.`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. `Text` values with no
/// key are `can_update`-compatible regardless of their `data` — the entry
/// updates in place at full opacity throughout.
#[test]
fn isomorphic_rebuild_does_not_animate() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(Text::new("1")),
    );
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    laid.pump_for(Duration::from_millis(10));
    assert_eq!(laid.opacity(laid.find_by_render_type("RenderOpacity")), 1.0);
    assert!(laid.find_text("1").is_some());
    assert!(laid.find_text("2").is_none());

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(Text::new("2")),
    ));
    laid.pump_for(Duration::from_millis(20));

    assert_eq!(
        laid.find_all_by_render_type("RenderOpacity").len(),
        1,
        "same type, no key: updates the one existing entry, no second transition"
    );
    assert_eq!(laid.opacity(laid.find_by_render_type("RenderOpacity")), 1.0);
    assert!(laid.find_text("1").is_none());
    assert!(laid.find_text("2").is_some());
}

/// `AnimatedSwitcher updates previous child transitions if the
/// transitionBuilder changes.` (narrowed — see the module doc for the
/// dropped `isA<KeyedSubtree>()` sub-assertion.)
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. Three back-to-back
/// swaps leave three entries in flight (one current, two outgoing); swapping
/// `transition_builder` alone (same child) must rebuild ALL THREE cached
/// transitions under the new builder, not just future ones.
#[test]
fn transition_builder_change_updates_every_cached_entry() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(vsync.clone(), AnimatedSwitcher::new(RUN).child(child_a()));
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    laid.pump_for(Duration::from_millis(10));

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(child_b()),
    ));
    // Detection tick (see the module doc's harness note): without it, child_a's
    // just-reversed run would be un-anchored, and this call's real 10ms would
    // be spent anchoring rather than advancing.
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(10));

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(child_c()),
    ));
    // Detection tick: child_b was demoted at value 0.1 (not 0.0), so this run
    // needs the same anchor-then-advance sequence to avoid synchronous dismiss.
    // Only 5ms of real time here (not 10ms): child_b's reverse run would
    // complete EXACTLY at 10ms (it was demoted at value 0.1 of a 100ms run),
    // dismissing it before the assertion below can observe all three entries
    // simultaneously.
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(5));

    assert_eq!(
        laid.find_all_by_render_type("RenderOpacity").len(),
        3,
        "three entries (child_a, child_b outgoing; child_c current) are in flight"
    );

    let scale_builder = |child: BoxedView, animation: Arc<dyn Animation<f32>>| -> BoxedView {
        ScaleTransition::new(animation, child).boxed()
    };
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN)
            .transition_builder(scale_builder)
            .child(child_c()),
    ));

    assert_eq!(
        laid.find_all_by_render_type("RenderOpacity").len(),
        0,
        "every cached transition (current AND both outgoing) was rebuilt under the new builder"
    );
    assert_eq!(
        laid.find_all_by_render_type("RenderTransform").len(),
        3,
        "all three entries now render via ScaleTransition"
    );
}

/// A test-local stand-in for Flutter's `KeyedSubtree` / FLUI's private
/// `KeyedEntry` (`animated_switcher.rs`) — hand-rolled `impl View` carrying
/// an explicit key, the same sanctioned pattern
/// `flui-widgets/src/overlay/mod.rs`'s `OverlayEntryView` uses in production.
/// Needed here (and only here) because `flui_foundation::Keyed<V>` has no
/// `impl View` anywhere in the codebase — see the module doc.
#[derive(Clone)]
struct Keyed {
    key: ValueKey<&'static str>,
    child: BoxedView,
}

impl Keyed {
    fn new(key: &'static str, child: impl IntoView) -> Self {
        Self {
            key: ValueKey::new(key),
            child: child.into_view().boxed(),
        }
    }
}

impl View for Keyed {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl StatelessView for Keyed {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}

/// `AnimatedSwitcher does not duplicate animations if the same child is
/// entered twice.` (adapted: uses the test-local `Keyed` wrapper — see the
/// module doc.)
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`. `Text('1', key:
/// '1')` -> `Text('2', key: '2')` -> `Text('1', key: '1')` (same key as the
/// FIRST child, reappearing while that first entry may still be mid-reverse)
/// must not collapse two live entries into one or crash; each swap starts a
/// fresh, independent entry (`AnimatedSwitcher` does no key-based entry
/// lookup — every `can_update`-incompatible swap mints a new `child_number`,
/// exactly like the oracle's `_childNumber += 1`), and the run settles with
/// exactly the reentered child visible.
#[test]
fn reentering_a_still_dismissing_key_starts_a_fresh_entry_not_a_duplicate() {
    let vsync = Vsync::new();
    let root = VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(Keyed::new("1", Text::new("1"))),
    );
    let mut laid = lay_out_animated(root, screen(), vsync.clone());

    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(Keyed::new("2", Text::new("2"))),
    ));
    laid.pump_widget(VsyncScope::new(
        vsync.clone(),
        AnimatedSwitcher::new(RUN).child(Keyed::new("1", Text::new("1"))),
    ));

    laid.pump_for(RUN);
    assert_eq!(
        laid.find_all_by_render_type("RenderOpacity").len(),
        1,
        "the run settles to exactly one entry"
    );
    assert!(
        laid.find_text("1").is_some(),
        "the reentered key-\"1\" child is the one left standing"
    );
    assert!(laid.find_text("2").is_none());
}

/// Pins the mechanism the module doc's Divergence note names: reversing a
/// controller that is ALREADY at its lower bound (value `0.0`) transitions it
/// straight to `Dismissed` — synchronously, with no elapsed time. This is
/// what makes the oracle's `container2` (demoted the instant its own forward
/// run started, at value `0.0`) disappear immediately rather than animate out
/// over the reverse duration.
#[test]
fn demoting_an_entry_still_at_the_lower_bound_dismisses_it_immediately() {
    let controller = AnimationController::new(RUN, Arc::new(Scheduler::new()));
    assert_eq!(controller.value(), 0.0);
    let _ = controller.reverse();
    assert_eq!(
        controller.status(),
        flui_animation::AnimationStatus::Dismissed,
        "reverse() called while already at the lower bound must dismiss synchronously"
    );
}

/// `AnimatedSwitcher does not crash at zero area`
///
/// Oracle: `animated_switcher_test.dart`, tag `3.44.0`.
#[test]
fn does_not_crash_at_zero_area() {
    let short = Duration::from_micros(500);
    let mut laid = lay_out(
        SizedBox::shrink().child(AnimatedSwitcher::new(short)),
        tight(0.0, 0.0),
    );
    assert_eq!(laid.size(laid.current_root()), common::size(0.0, 0.0));

    laid.pump_widget(SizedBox::shrink().child(AnimatedSwitcher::new(short).child(Text::new("x"))));
    assert_eq!(laid.size(laid.current_root()), common::size(0.0, 0.0));

    laid.pump_widget(SizedBox::shrink().child(AnimatedSwitcher::new(short).child(Text::new("y"))));
    assert_eq!(laid.size(laid.current_root()), common::size(0.0, 0.0));
}

/// Not an oracle case — pins the harness-tick-ordering mechanism the module
/// doc's harness note describes and every `Duration::ZERO` detection tick in
/// this file relies on: a controller's very first tick after a fresh
/// `reverse()`/`forward()` call only anchors the run (elapsed `0`); a SECOND
/// tick is required to observe real progress. Isolated to a bare
/// `AnimationController` + `FadeTransition`, decoupled from
/// `AnimatedSwitcher`, so a future change to `pump_frame`'s phase ordering
/// fails this test directly instead of only failing as an unexplained
/// timing flake in the oracle ports above.
#[test]
fn reverse_needs_a_detection_tick_before_progress_is_observable() {
    let vsync = Vsync::new();
    let controller = AnimationController::new(RUN, Arc::new(Scheduler::new()));
    controller.set_value(1.0);
    let root = VsyncScope::new(
        vsync.clone(),
        FadeTransition::new(Arc::new(controller.clone()), SizedBox::new(1.0, 1.0)),
    );
    let mut laid = lay_out_animated(root, screen(), vsync.clone());
    laid.register_controller(controller.clone());
    assert_eq!(laid.opacity(laid.find_by_render_type("RenderOpacity")), 1.0);

    let _ = controller.reverse();
    laid.pump_for(Duration::from_millis(50));
    assert_eq!(
        laid.opacity(laid.find_by_render_type("RenderOpacity")),
        1.0,
        "the FIRST tick after reverse() only anchors the run; it must not advance the value"
    );

    laid.pump_for(Duration::from_millis(50));
    assert_eq!(
        laid.opacity(laid.find_by_render_type("RenderOpacity")),
        0.5,
        "the SECOND tick observes real elapsed time against the anchor set by the first"
    );
}
