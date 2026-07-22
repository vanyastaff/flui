//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/visibility_test.dart`
//! (tag `3.44.0`, 9 `testWidgets` cases; class `Visibility` implementation is
//! in `indexed_stack.dart`).
//!
//! ## Oracle denominator: 9 `testWidgets`, reconciled
//!
//! - `'Visibility'` — the large sequential test (~22 `pumpWidget` legs).
//!   Split by concern:
//!   - **Layout-size legs** (visible/hidden, default vs. custom replacement,
//!     `maintainState` on/off) — cited, not re-ported: already covered by
//!     [`visibility_true_child_renders_at_natural_size`],
//!     [`visibility_false_shows_replacement_at_zero_size`],
//!     [`visibility_false_maintain_state_wraps_child_in_offstage`],
//!     [`visibility_true_maintain_state_child_paints_normally_via_offstage`]
//!     below, plus `default_visible_shows_the_child_directly_with_no_offstage_wrapper`,
//!     `hidden_without_maintain_state_shows_the_default_replacement`,
//!     `hidden_without_maintain_state_uses_a_custom_replacement`,
//!     `maintain_state_true_and_visible_wraps_the_child_in_a_non_offstage_offstage`,
//!     `maintain_state_true_and_hidden_wraps_the_child_in_an_offstage_offstage`,
//!     `maintain_state_true_and_hidden_lays_the_child_out_at_full_size` in
//!     `crates/flui-widgets/tests/visibility.rs`.
//!   - **Hit-test legs** (`tester.tap(find.byType(Visibility))` /
//!     `tester.tap(…, warnIfMissed: false)`) — ported below:
//!     [`visibility_true_default_tap_reaches_the_child`],
//!     [`visibility_false_default_tap_does_not_reach_the_removed_child`],
//!     [`visibility_false_default_tap_reaches_a_gesture_replacement`],
//!     [`visibility_true_maintain_state_tap_reaches_child_through_transparent_offstage`],
//!     [`visibility_false_maintain_state_tap_is_suppressed_by_offstage`],
//!     [`visibility_maintain_state_hit_test_tracks_visibility_across_repeated_toggles`].
//!   - **`maintainSize`/`maintainSemantics`/`maintainInteractivity`-as-full-feature
//!     legs** (the 5 sub-scenarios that set `maintainSize: true`) — out of
//!     scope: `maintainSize` is a documented, deferred gap (see the divergence
//!     note on `Visibility` in `crates/flui-widgets/src/interaction/visibility.rs`).
//!     [`visibility_maintain_interactivity_true_is_currently_a_no_op_without_maintain_size`]
//!     proves the current no-op with a real (non-vacuous) assertion, and
//!     [`visibility_maintain_interactivity_with_maintain_size_would_let_hidden_taps_through`]
//!     keeps the oracle's real target alive as an `#[ignore]`d red case.
//!   - **`hasSemantics(...)` assertions** — out of scope: FLUI's headless test
//!     harness has no semantics-tree assembly step (no `SemanticsTester`
//!     analogue) to check these against.
//! - `'Visibility with maintain* false excludes focus of child when not
//!   visible'`, `'Visibility with maintain* true does not exclude focus of
//!   child when not visible'`, `'Visibility with maintain* true except
//!   maintainFocusability which is false excludes focus of child when not
//!   visible'` — cited, not re-ported: all three are covered by
//!   `visibility_focus_policy_tracks_hidden_state_without_remounting` in
//!   `crates/flui-widgets/tests/visibility.rs`, which drives the same
//!   `maintain_focusability` on/off matrix against a live `FocusNode`
//!   (FLUI has no `Visibility::maintain()` convenience constructor — the
//!   equivalent explicit builder calls are used instead).
//! - `'Visibility throws assertion error if maintainFocusability is true
//!   without maintainState'` — cited, not re-ported: covered by
//!   `invalid_maintain_focusability_configuration_builds_one_error_child` in
//!   `crates/flui-widgets/tests/visibility.rs`. Divergence: FLUI substitutes
//!   an `ErrorView` child under a `debug_assert!` rather than throwing
//!   `AssertionError` from the constructor — same invariant, FLUI's standard
//!   build-time error-substitution idiom instead of a constructor-time panic.
//! - `'Visibility does not force compositing when visible and maintain*'` —
//!   out of scope: requires `maintainSize` (deferred, see above) plus a
//!   layer-tree/compositing-layer-count inspection capability the headless
//!   test harness does not have (`tester.layers` has no FLUI analogue here).
//! - `'SliverVisibility does not force compositing when visible and
//!   maintain*'` — out of scope: `SliverVisibility` does not exist in FLUI;
//!   no sliver equivalent of `Visibility` has been ported.
//! - `'Visibility.of returns correct value'`, `'Visibility.of works when
//!   multiple Visibility widgets are in hierarchy'` — out of scope, and
//!   already documented as a divergence (not a gap newly found by this
//!   audit): FLUI's `Visibility` omits Flutter's `_VisibilityScope`
//!   `InheritedWidget` and the `Visibility::of` query API entirely — see the
//!   "Divergences from Flutter" note on `Visibility` in
//!   `crates/flui-widgets/src/interaction/visibility.rs`.
//!
//! ## Constraint model
//!
//! All tests use `loose(200.0)` (min=0, max=200×200) instead of tight(800×600)
//! because:
//!
//! 1. **`SizedBox::new(100, 100)` natural size:** under tight(800×600),
//!    `BoxConstraints::enforce({100,100,100,100}, tight)` clamps 100 → 800×600.
//!    Under `loose(200)` it resolves to 100×100.
//!
//! 2. **`RenderOffstage(offstage=true)` constraint contract:** the box takes
//!    `constraints.smallest()` (Flutter's `sizedByParent => offstage`), so under
//!    `loose(200)` it is zero-sized while its child is laid out at full size.
//!    Under tight(800×600) it would legitimately occupy 800×600.
//!
//!    Historical note: this used to return `Size::ZERO` unconditionally, which
//!    violated tight constraints (the old "FLUI-DEV-001"). Fixed by ADR-0020;
//!    `loose(200)` is now a size-legibility choice, not a panic dodge.
//!
//! Widget → render-object mapping:
//! - `Visibility(visible=true)`  → child's render object directly
//! - `Visibility(visible=false)` → `RenderConstrainedBox` (replacement `SizedBox::shrink`)
//! - `Visibility(visible=false, maintain_state=true)`
//!   → `RenderOffstage(offstage=true)` → `RenderSubtreeAnchor` → the child's
//!   `RenderConstrainedBox`
//! - `Visibility(visible=true, maintain_state=true)`
//!   → `RenderOffstage(offstage=false)` → `RenderSubtreeAnchor` → the child's
//!   `RenderConstrainedBox`
//!
//! Divergences:
//! - `maintainAnimation` matches Flutter for descendants registered through an
//!   ambient `VsyncScope`, including production `AppBinding` roots. With no
//!   ambient scope, FLUI's `TickerMode` intentionally passes the child through
//!   to preserve wall-clock fallback behavior.
//! - `maintainSize` is deferred.
//! - `maintain_interactivity` is accepted but is a no-op until `maintainSize`
//!   lands. Tested to confirm it does not panic or break the tree.
//! - Flutter wraps in `_VisibilityScope`; FLUI omits that scope widget.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::common::{loose, size};
use crate::harness::{self, screen};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_widgets::{GestureDetector, SizedBox, Visibility};

/// `Visibility(visible=true)` passes the child directly to the render tree.
///
/// Flutter parity: `visibility_test.dart` — `Visibility(child: testChild)`:
/// `expect(tester.getSize(find.byType(Visibility)), const Size(800.0, 600.0))`.
/// FLUI equivalent (loose constraints): the child's `RenderConstrainedBox` is
/// present and sized to its natural 100×100.
#[test]
fn visibility_true_child_renders_at_natural_size() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0)).visible(true),
        loose(200.0),
    );

    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(child_id),
        size(100.0, 100.0),
        "visible=true: child SizedBox(100, 100) must resolve to 100×100 under loose(200) \
         — confirms the child render object is present and sized naturally"
    );
}

/// `Visibility(visible=false)` replaces the child with the default
/// `SizedBox::shrink` (0×0) replacement, discarding the child from the tree.
///
/// Flutter parity: `visibility_test.dart` — `Visibility(visible: false, child:…)`:
/// `expect(find.byType(Text, skipOffstage: false), findsNothing)`.
/// FLUI: under `loose(200)` the only `RenderConstrainedBox` in the tree is
/// the `SizedBox::shrink` replacement, which resolves to 0×0 — distinguishing
/// it from the 100×100 original child.
#[test]
fn visibility_false_shows_replacement_at_zero_size() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0)).visible(false),
        loose(200.0),
    );

    // Only one RenderConstrainedBox — the replacement SizedBox::shrink.
    let replacement_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(replacement_id),
        size(0.0, 0.0),
        "visible=false: the replacement SizedBox::shrink must be 0×0 under loose(200); \
         the original 100×100 child must not be in the tree (if it were, this node \
         would be 100×100 not 0×0)"
    );
}

/// `Visibility(visible=false, maintain_state=true)` keeps the child alive via
/// `Offstage(offstage=true)`.
///
/// Flutter parity: `visibility_test.dart` — `maintainState=true` branch:
/// `Offstage(offstage: !visible, child: child)`. The child render object is
/// still in the tree (state preserved) but `RenderOffstage` suppresses paint
/// and hit-testing.
///
/// Note: uses `loose(200)` so the `RenderOffstage` box is zero-sized
/// (`constraints.smallest()`), keeping the assertions legible. Its child is
/// nonetheless laid out at full size — see
/// `maintain_state_true_and_hidden_lays_the_child_out_at_full_size`.
#[test]
fn visibility_false_maintain_state_wraps_child_in_offstage() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0))
            .visible(false)
            .maintain_state(true),
        loose(200.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    let anchor_id = laid.find_by_render_type("RenderSubtreeAnchor");
    assert_eq!(
        laid.render_node_count(),
        3,
        "visible=false + maintain_state=true: Offstage, focus anchor, and child"
    );
    assert_eq!(laid.only_child(offstage_id), anchor_id);
    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(laid.only_child(anchor_id), child_id);
    assert_eq!(laid.size(anchor_id), size(100.0, 100.0));
    assert_eq!(laid.size(child_id), size(100.0, 100.0));
}

/// `Visibility(visible=true, maintain_state=true)` wraps the child in
/// `Offstage(offstage=false)`, which is functionally transparent: the child
/// paints and hit-tests normally, and sizes to its natural dimensions.
#[test]
fn visibility_true_maintain_state_child_paints_normally_via_offstage() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0))
            .visible(true)
            .maintain_state(true),
        loose(200.0),
    );

    let offstage_id = laid.find_by_render_type("RenderOffstage");
    let anchor_id = laid.find_by_render_type("RenderSubtreeAnchor");
    assert_eq!(
        laid.render_node_count(),
        3,
        "visible=true + maintain_state=true: Offstage, focus anchor, and child"
    );
    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(laid.only_child(offstage_id), anchor_id);
    assert_eq!(laid.only_child(anchor_id), child_id);
    assert_eq!(laid.size(anchor_id), size(100.0, 100.0));
    assert_eq!(
        laid.size(child_id),
        size(100.0, 100.0),
        "visible=true + maintain_state=true: child SizedBox(100, 100) must resolve \
         to 100×100 under loose(200) — Offstage(offstage=false) is transparent"
    );
}

// ── Hit-test legs of `'Visibility'` ──────────────────────────────────────────
//
// The oracle's giant sequential test also asserts `tester.tap(find.byType(
// Visibility))` (and its `warnIfMissed: false` counterpart) at every leg —
// the layout-size tests above don't exercise the hit-test path at all. These
// tests port that half: whether a pointer event actually reaches the child,
// across the same visible/hidden × maintain_state combinations, using
// `screen()` (Flutter's default 800×600 tight test surface) so the child
// occupies the whole tappable area.

/// A childless, opaque `GestureDetector` that flips `flag` to `true` on tap —
/// the minimal reachable-hit probe shared by the hit-test cases below.
fn tap_probe(flag: &Arc<AtomicBool>) -> GestureDetector {
    let flag = Arc::clone(flag);
    GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .on_tap(move || flag.store(true, Ordering::SeqCst))
}

/// `Visibility(visible=true)` (default): a tap anywhere on the child reaches
/// it — `Visibility` passes the child through untouched.
///
/// Flutter parity: `visibility_test.dart` `'Visibility'` (3.44.0) — the first
/// `pumpWidget(Visibility(child: testChild))` leg:
/// `await tester.tap(find.byType(Visibility)); expect(log, ['tap'])`.
#[test]
fn visibility_true_default_tap_reaches_the_child() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let laid = harness::pump_widget(Visibility::new(tap_probe(&did_tap)).visible(true), screen());

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "visible=true (default): a tap must reach the child"
    );
}

/// `Visibility(visible=false)` (default, no `maintain_state`): the child is
/// fully removed from the tree and replaced by `SizedBox::shrink`, so a tap
/// can never reach it.
///
/// Flutter parity: `visibility_test.dart` `'Visibility'` (3.44.0) — the
/// second `pumpWidget(Visibility(visible: false, child: testChild))` leg:
/// `await tester.tap(find.byType(Visibility), warnIfMissed: false); expect(log, [])`.
#[test]
fn visibility_false_default_tap_does_not_reach_the_removed_child() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let laid = harness::pump_widget(
        Visibility::new(tap_probe(&did_tap)).visible(false),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "visible=false (default, no maintain_state): the child is entirely \
         absent from the tree, so no tap can reach it"
    );
}

/// `Visibility(visible=false, replacement: …)`: the custom replacement is
/// what is actually mounted while hidden, so a tap reaches *it*, never the
/// original (absent) child.
///
/// Flutter parity: `visibility_test.dart` `'Visibility'` (3.44.0) — the
/// `Visibility(replacement: const Placeholder(), visible: false, child:
/// testChild)` leg (`find.byType(Placeholder), findsOneWidget`). FLUI has no
/// `Placeholder` widget, so a tap-probing `GestureDetector` replacement
/// stands in to make "the replacement is live in the tree" hit-test
/// observable rather than just presence-observable.
#[test]
fn visibility_false_default_tap_reaches_a_gesture_replacement() {
    let child_tapped = Arc::new(AtomicBool::new(false));
    let replacement_tapped = Arc::new(AtomicBool::new(false));

    let laid = harness::pump_widget(
        Visibility::new(tap_probe(&child_tapped))
            .visible(false)
            .replacement(tap_probe(&replacement_tapped)),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        replacement_tapped.load(Ordering::SeqCst),
        "visible=false with a custom replacement: the replacement is what is \
         mounted, so a tap must reach it"
    );
    assert!(
        !child_tapped.load(Ordering::SeqCst),
        "the original child is entirely absent while hidden without \
         maintain_state — its tap handler must never fire"
    );
}

/// `Visibility(visible=true, maintain_state=true)`: `Offstage(offstage=false)`
/// is a transparent hit-test proxy, so a tap reaches the child exactly as it
/// would without the wrapper.
///
/// Flutter parity: `visibility_test.dart` `'Visibility'` (3.44.0) — the
/// `Visibility(maintainState: true, child: testChild)` (visible=true) leg:
/// `await tester.tap(find.byType(Visibility), warnIfMissed: false); expect(log, ['tap'])`.
#[test]
fn visibility_true_maintain_state_tap_reaches_child_through_transparent_offstage() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let laid = harness::pump_widget(
        Visibility::new(tap_probe(&did_tap))
            .visible(true)
            .maintain_state(true),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "visible=true + maintain_state=true: Offstage(offstage=false) must \
         not block the tap from reaching the child"
    );
}

/// `Visibility(visible=false, maintain_state=true)`: the child is still fully
/// present and laid out at full size (state preserved), but
/// `Offstage(offstage=true)` suppresses hit-testing — `RenderOffstage::hit_test`
/// returns `false` unconditionally when offstage, matching Flutter's
/// `hitTest => !offstage && super.hitTest(…)` (`proxy_box.dart:3927-3930`).
///
/// Flutter parity: `visibility_test.dart` `'Visibility'` (3.44.0) — the
/// `Visibility(visible: false, maintainState: true, child: testChild)` leg:
/// `await tester.tap(find.byType(Visibility), warnIfMissed: false); expect(log, [])`.
#[test]
fn visibility_false_maintain_state_tap_is_suppressed_by_offstage() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let laid = harness::pump_widget(
        Visibility::new(tap_probe(&did_tap))
            .visible(false)
            .maintain_state(true),
        screen(),
    );

    // The child is still fully present in the tree (state preserved), unlike
    // the maintain_state=false replacement path.
    assert_eq!(
        laid.render_node_count(),
        3,
        "visible=false + maintain_state=true: Offstage, focus anchor, and the \
         still-attached child"
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "visible=false + maintain_state=true: the child is present but \
         Offstage(offstage=true) must suppress hit-testing"
    );
}

/// The oracle's "toggle the visibility off and on a few times" block: with
/// `maintain_state=true`, hit-testing must track `visible` on every
/// `pumpWidget` root-swap, not just on first mount.
///
/// Flutter parity: `visibility_test.dart` `'Visibility'` (3.44.0) — the
/// repeated `Visibility(maintainState: true, child: testChild)` /
/// `Visibility(visible: false, maintainState: true, child: testChild)`
/// alternation (lines documented as "Now we toggle the visibility off and on
/// a few times to make sure that works"), asserting `log` gains/misses
/// `'tap'` at each leg.
#[test]
fn visibility_maintain_state_hit_test_tracks_visibility_across_repeated_toggles() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let mut laid = harness::pump_widget(
        Visibility::new(tap_probe(&did_tap)).maintain_state(true),
        screen(),
    );

    // Leg 1: visible (default) — tap reaches.
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    assert!(
        did_tap.load(Ordering::SeqCst),
        "leg 1 (visible): must reach"
    );
    did_tap.store(false, Ordering::SeqCst);

    // Leg 2: hidden — tap suppressed.
    laid.pump_widget(
        Visibility::new(tap_probe(&did_tap))
            .maintain_state(true)
            .visible(false),
    );
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    assert!(
        !did_tap.load(Ordering::SeqCst),
        "leg 2 (hidden): must not reach"
    );

    // Leg 3: visible again — the state-preserved child resumes receiving hits.
    laid.pump_widget(Visibility::new(tap_probe(&did_tap)).maintain_state(true));
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    assert!(
        did_tap.load(Ordering::SeqCst),
        "leg 3 (visible again): must reach"
    );
    did_tap.store(false, Ordering::SeqCst);

    // Leg 4: hidden again.
    laid.pump_widget(
        Visibility::new(tap_probe(&did_tap))
            .maintain_state(true)
            .visible(false),
    );
    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);
    assert!(
        !did_tap.load(Ordering::SeqCst),
        "leg 4 (hidden again): must not reach"
    );
}

/// Documents a real, currently-accepted divergence: `maintain_interactivity`
/// is a no-op until `maintainSize` is implemented (see the "Divergences from
/// Flutter" note on `Visibility` in
/// `crates/flui-widgets/src/interaction/visibility.rs`). Flutter itself only
/// allows `maintainInteractivity` when `maintainSize` is also set, so there
/// is no Flutter-legal configuration this could instead assert the true
/// target against — this proves the accepted-but-inert flag really has no
/// effect today, rather than silently omitting the case.
///
/// Red-check performed (not committed): temporarily changed
/// `Visibility::build` to gate `Offstage::offstage` on
/// `!self.maintain_interactivity` as well as `!self.visible` (i.e.
/// accidentally wiring the flag through without `maintainSize`) — this test
/// failed under that mutation, confirming the assertion is sensitive to the
/// real seam and not vacuously true. Reverted after confirming.
#[test]
fn visibility_maintain_interactivity_true_is_currently_a_no_op_without_maintain_size() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let laid = harness::pump_widget(
        Visibility::new(tap_probe(&did_tap))
            .visible(false)
            .maintain_state(true)
            .maintain_interactivity(true),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "maintain_interactivity=true currently has no effect beyond \
         maintain_state — Offstage still suppresses hit-testing while \
         hidden (documented divergence: full semantics deferred with \
         maintainSize)"
    );
}

/// The real Flutter target: `maintainInteractivity` (with `maintainSize`)
/// lets a hidden-but-space-occupying child keep receiving pointer events.
/// FLUI cannot pass this until `maintainSize` lands — kept as a real,
/// `#[ignore]`d assertion of the oracle's actual expectation rather than
/// narrowed away, per the project's no-narrowing rule.
///
/// Un-ignore when `maintainSize` closes the gap documented on `Visibility`
/// in `crates/flui-widgets/src/interaction/visibility.rs` (Cross.H).
///
/// Flutter parity: `visibility_test.dart` `'Visibility'` (3.44.0) — the
/// `maintainInteractivity: true` (with `maintainSize: true`) leg: `await
/// tester.tap(find.byType(Visibility))` reaches the child even while hidden.
#[test]
#[ignore = "un-ignore when maintainSize lands (Cross.H) — see the Visibility divergence note"]
fn visibility_maintain_interactivity_with_maintain_size_would_let_hidden_taps_through() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let laid = harness::pump_widget(
        Visibility::new(tap_probe(&did_tap))
            .visible(false)
            .maintain_state(true)
            .maintain_interactivity(true),
        screen(),
    );

    laid.dispatch_pointer_down(10.0, 10.0);
    laid.dispatch_pointer_up(10.0, 10.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "with maintainSize (not yet implemented), maintain_interactivity=true \
         should let a tap reach the hidden-but-space-occupying child"
    );
}
