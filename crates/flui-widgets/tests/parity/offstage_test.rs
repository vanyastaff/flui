//! ## Test parity notes
//!
//! ## Oracle-path discrepancy (read this before the denominator below)
//!
//! The assigned oracle path, `packages/flutter/test/widgets/offstage_test.dart`
//! (tag `3.44.0`), does not exist in the Flutter repository at that tag —
//! confirmed with `git ls-tree -r 3.44.0 --name-only | grep -i offstage`,
//! which lists only `packages/flutter/test/rendering/offstage_test.dart`,
//! `packages/flutter/test/widgets/semantics_keep_alive_offstage_test.dart`,
//! and the `examples/api` sample test. `git log --all --follow` on the
//! `rendering/` path traces it back to `packages/unit/test/rendering/
//! offstage_test.dart` (2015) — it has never lived under `test/widgets/`.
//! `grep -cE '^\s*testWidgets\(' packages/flutter/test/rendering/
//! offstage_test.dart` correctly returns `0`: that file has one `test(...)`
//! case (not `testWidgets`), because it drives bare `RenderObject`s through
//! `rendering_tester.dart`'s `layout()` helper rather than a widget tree.
//!
//! The real oracle for `Offstage`/`RenderOffstage` geometry, paint, and
//! hit-test behavior is therefore
//! `packages/flutter/test/rendering/offstage_test.dart`'s single
//! `test('offstage', ...)` case (tag `3.44.0`, 0 `testWidgets`, 1 `test`).
//! Flutter's own widget test suite has no dedicated `Offstage` widget test —
//! `Offstage`'s widget-level hit-test/paint contract is instead exercised
//! indirectly through `visibility_test.dart` (`Visibility(maintainState:
//! true)` wraps a child in `Offstage`), which FLUI already ported in
//! `visibility_test.rs`. This file adds the port that's still missing: the
//! `Offstage` widget driven **directly** (not through a `Visibility`
//! wrapper) — nothing in `crates/flui-widgets/` exercised `Offstage::new()`
//! before this file (confirmed by
//! `grep -rn "Offstage::" crates/flui-widgets/` returning zero direct
//! usages, only `SliverOffstage`/`RenderOffstage`).
//!
//! ## Oracle denominator: 1 `test`, reconciled
//!
//! - `'offstage'` (`rendering/offstage_test.dart`, 3.44.0) — builds
//!   `RenderOffstage(child: RenderCustomPaint(painter: onPaint sets a flag,
//!   child: a fixed-size leaf))` under a chain that hands the leaf real
//!   (non-zero) incoming constraints, and asserts: before layout, the leaf
//!   has no size and the paint flag is unset; after a full layout+paint
//!   pass, the leaf **does** have a size (it was laid out) at its real
//!   computed geometry (not `Size.zero`), and the paint flag is **still**
//!   unset (painting was suppressed). Ported below as
//!   [`offstage_true_child_is_still_laid_out_at_full_size_but_box_shrinks_to_zero`]
//!   (the geometry half) and
//!   [`offstage_true_suppresses_paint_of_the_child_subtree`] (the paint
//!   half).
//!
//! Beyond that single upstream case, this file adds the control/toggle/hit-test
//! coverage the task brief calls for, following the same "beyond-oracle"
//! pattern `opacity_test.rs` uses for `RenderOpacity`'s documented contract
//! (`crates/flui-objects/src/interaction/offstage.rs`, itself citing
//! `proxy_box.dart:3834-3952`):
//! - [`offstage_false_is_a_transparent_paint_and_size_passthrough`] — the
//!   symmetric `offstage=false` control case (box adopts child size, paint
//!   passes through). Not in the Dart oracle (which only constructs the
//!   `offstage=true` default), but required to show the `true` cluster's
//!   suppression is conditional on the flag, not unconditional.
//! - [`offstage_true_suppresses_hit_testing_of_the_child`] /
//!   [`offstage_false_hit_tests_reach_the_child`] — `hitTest => !offstage &&
//!   super.hitTest(…)` (`proxy_box.dart:3927-3930`). The hit-test half of
//!   this contract is already exercised transitively via `Visibility` in
//!   `visibility_test.rs`'s `visibility_false_maintain_state_tap_is_suppressed_by_offstage`
//!   / `visibility_true_maintain_state_tap_reaches_child_through_transparent_offstage`
//!   — these two isolate the same contract on a bare `Offstage` with no
//!   `Visibility` wrapper in between (mirroring how `opacity_test.rs`'s
//!   `opacity_zero_still_lays_out_and_hit_tests_its_child` isolates a
//!   contract `clip_test.rs` already covers through an intermediary).
//! - [`toggling_offstage_without_remounting_flips_hit_testing`] — the
//!   "toggling" cluster the task brief names: drives the flag from `true` to
//!   `false` on the **same** mounted `Offstage` node via `LaidOut::pump_widget`
//!   (a root-swap, not a fresh mount) and confirms both the render-node count
//!   and the render id are unchanged across the toggle (the child's state is
//!   preserved, not discarded and remounted) while hit-testing tracks the
//!   flag. `visibility_test.rs`'s
//!   `visibility_maintain_state_hit_test_tracks_visibility_across_repeated_toggles`
//!   already drives a longer true/false/true/false sequence through
//!   `Visibility`; this test isolates the same toggle directly on `Offstage`.
//!
//! Out of scope, with reason:
//! - **Semantics** (`visitChildrenForSemantics` dropping the offstage
//!   subtree, `proxy_box.dart:3945-3951`) — not headless-checkable; this
//!   harness has no semantics-tree assembly step (`SemanticsTester`
//!   analogue), the same reason `opacity_test.rs` and `visibility_test.rs`
//!   drop their semantics clusters. `RenderOffstage::excludes_semantics_subtree`
//!   has its own unit coverage in
//!   `crates/flui-objects/src/interaction/offstage.rs`'s `#[cfg(test)]`
//!   block, just not exercised end-to-end here.
//! - **Intrinsic dimensions** (`computeMinIntrinsicWidth`/etc returning `0`
//!   when offstage) — the Dart oracle itself does not test intrinsics for
//!   `RenderOffstage`; already covered at the render-object unit level
//!   (`crates/flui-objects/src/interaction/offstage.rs`), not re-derived here
//!   through a widget tree.
//!
//! Widget → render-object mapping:
//! `Offstage` (`crates/flui-widgets/src/interaction/offstage.rs`) →
//! `RenderOffstage` (`crates/flui-objects/src/interaction/offstage.rs`), a
//! `Single`-arity `RenderBox` proxy.
//!
//! ## Constraint choice per cluster
//!
//! The size-shrink test uses `loose(200.0)`: `RenderOffstage`'s own box size
//! is `constraints.smallest()` (`sizedByParent => offstage`,
//! `proxy_box.dart:3896`/`:3905-3910`) — under a *loose* parent that is
//! `(0, 0)`, but under a *tight* parent it is the full tight size (the
//! historical FLUI bug this crate's doc references: returning `Size::ZERO`
//! unconditionally violated a tight parent's constraints). The paint and
//! hit-test tests instead use `screen()` (tight 800×600, matching
//! `visibility_test.rs`'s own hit-test cluster): a hit-test assertion needs
//! the box itself to occupy real, non-zero space so a pointer dispatched
//! inside its bounds proves the flag — not the bounds check — is what
//! suppresses the hit; at `loose(200.0)` the box is zero-sized and any
//! "not hit" result would be equally true whether or not the offstage check
//! existed at all (a vacuous assertion this file's red-check below rules out).

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_rendering::delegates::CustomPainter;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::Canvas;
use flui_types::Size;

use flui_widgets::{CustomPaint, GestureDetector, Offstage, SizedBox};

use crate::common::{loose, size};
use crate::harness::{pump_widget, screen};

/// A [`CustomPainter`] that flips a shared flag when painted — the FLUI
/// analogue of the Dart oracle's `TestCallbackPainter(onPaint: () { painted
/// = true; })`.
#[derive(Debug)]
struct PaintProbe {
    painted: Arc<AtomicBool>,
}

impl CustomPainter for PaintProbe {
    fn paint(&self, _canvas: &mut Canvas, _size: Size) {
        self.painted.store(true, Ordering::SeqCst);
    }

    fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
        true
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Builds a `CustomPaint` wrapping `leaf` with a fresh [`PaintProbe`],
/// returning the widget and the flag the probe flips when painted.
fn paint_probe(leaf: impl flui_view::IntoView) -> (CustomPaint, Arc<AtomicBool>) {
    let painted = Arc::new(AtomicBool::new(false));
    let probe = CustomPaint::new()
        .painter(Arc::new(PaintProbe {
            painted: Arc::clone(&painted),
        }))
        .child(leaf);
    (probe, painted)
}

/// A childless, opaque `GestureDetector` that flips `flag` to `true` on tap —
/// same idiom `visibility_test.rs`/`opacity_test.rs` use for their hit-test
/// probes.
fn tap_probe(flag: &Arc<AtomicBool>) -> GestureDetector {
    let flag = Arc::clone(flag);
    GestureDetector::new()
        .behavior(HitTestBehavior::Opaque)
        .on_tap(move || flag.store(true, Ordering::SeqCst))
}

// ── Geometry half of the 'offstage' oracle test ─────────────────────────────

/// `Offstage(offstage: true)` under loose constraints: the box itself shrinks
/// to `constraints.smallest()` (zero, under a loose parent), but its child is
/// still laid out at its own real, non-zero preferred size — the child is
/// NOT replaced by a zero-size stand-in, it genuinely ran layout.
///
/// Flutter parity: `rendering/offstage_test.dart` `'offstage'` (3.44.0) — the
/// `expect(child.hasSize, isTrue); expect(child.size, equals(...))` legs
/// after `layout(root, phase: EnginePhase.paint)`.
#[test]
fn offstage_true_child_is_still_laid_out_at_full_size_but_box_shrinks_to_zero() {
    let laid = pump_widget(
        Offstage::new()
            .offstage(true)
            .child(SizedBox::new(100.0, 80.0)),
        loose(200.0),
    );

    let offstage_id = laid.root();
    assert_eq!(
        laid.size(offstage_id),
        Size::ZERO,
        "offstage=true under a loose parent: the box itself must shrink to \
         constraints.smallest() = zero"
    );

    let child_id = laid.only_child(offstage_id);
    assert_eq!(
        laid.size(child_id),
        size(100.0, 80.0),
        "offstage=true: the child must still be laid out at its own real \
         100x80 preferred size (state kept, geometry computed) even though \
         the parent box reports zero"
    );
}

// ── Paint half of the 'offstage' oracle test ────────────────────────────────

/// `Offstage(offstage: true)` under a tight parent: the box itself occupies
/// the full tight size (not zero — see the module doc's constraint-choice
/// note), but painting the subtree is unconditionally suppressed.
///
/// Flutter parity: `rendering/offstage_test.dart` `'offstage'` (3.44.0) — the
/// `expect(painted, isFalse)` legs both before and after layout.
#[test]
fn offstage_true_suppresses_paint_of_the_child_subtree() {
    let (probe, painted) = paint_probe(SizedBox::new(100.0, 80.0));
    let laid = pump_widget(Offstage::new().offstage(true).child(probe), screen());

    let offstage_id = laid.root();
    assert_eq!(
        laid.size(offstage_id),
        size(800.0, 600.0),
        "offstage=true under a TIGHT parent: the box occupies the full tight \
         size, not zero (constraints.smallest() of a tight constraint is the \
         tight size itself)"
    );
    assert!(
        !painted.load(Ordering::SeqCst),
        "offstage=true: the child's CustomPaint painter must never run — \
         RenderOffstage::paint returns before calling paint_child"
    );
}

// ── offstage=false control case (beyond the oracle) ─────────────────────────

/// `Offstage(offstage: false)` is a transparent passthrough: the box adopts
/// the child's size and the child paints normally.
///
/// Not in the Dart oracle (which only constructs `offstage: true`); this is
/// the symmetric control needed to show suppression is conditional on the
/// flag, per `proxy_box.dart`'s `performLayout`/`paint` `if (offstage) {...}
/// else {...}` branches. Uses the same `loose(200.0)` constraints as
/// [`offstage_true_child_is_still_laid_out_at_full_size_but_box_shrinks_to_zero`]
/// (not `screen()`'s tight 800x600) — under a tight parent, EVERY box
/// (offstage or not) is forced to the tight size, so a box-size comparison
/// there would not distinguish this case from the `true` one at all; only
/// under loose constraints does "adopts the child's size" versus "shrinks to
/// `constraints.smallest()`" become a real, distinguishing assertion.
#[test]
fn offstage_false_is_a_transparent_paint_and_size_passthrough() {
    let (probe, painted) = paint_probe(SizedBox::new(100.0, 80.0));
    let laid = pump_widget(Offstage::new().offstage(false).child(probe), loose(200.0));

    let offstage_id = laid.root();
    assert_eq!(
        laid.size(offstage_id),
        size(100.0, 80.0),
        "offstage=false: the box must adopt the child's 100x80 size (not \
         shrink to constraints.smallest() = zero, as the offstage=true case \
         does under the same loose(200) constraints)"
    );
    assert!(
        painted.load(Ordering::SeqCst),
        "offstage=false: the child's CustomPaint painter must run"
    );
}

// ── Hit-test cluster (proxy_box.dart hitTest => !offstage && super.hitTest) ─

/// `Offstage(offstage: true)`: a pointer dispatched inside the box's own
/// (real, non-zero, tight-parent) bounds does not reach the child.
///
/// Flutter parity: `proxy_box.dart:3927-3930` `hitTest => !offstage &&
/// super.hitTest(…)`. Isolates on a bare `Offstage` the same contract
/// `visibility_test.rs`'s `visibility_false_maintain_state_tap_is_suppressed_by_offstage`
/// exercises through a `Visibility` wrapper.
#[test]
fn offstage_true_suppresses_hit_testing_of_the_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let laid = pump_widget(
        Offstage::new().offstage(true).child(tap_probe(&did_tap)),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        !did_tap.load(Ordering::SeqCst),
        "offstage=true: a pointer inside the box's own (real, tight-parent) \
         bounds must not reach the child — the box has non-zero area here, \
         so a miss can only be explained by the offstage flag itself, not by \
         the pointer falling outside the box"
    );
}

/// `Offstage(offstage: false)`: the same pointer reaches the child.
///
/// Flutter parity: `proxy_box.dart:3927-3930` — the `!offstage` branch of
/// `hitTest`. Isolates the contract
/// `visibility_test.rs`'s `visibility_true_maintain_state_tap_reaches_child_through_transparent_offstage`
/// exercises through a `Visibility` wrapper.
#[test]
fn offstage_false_hit_tests_reach_the_child() {
    let did_tap = Arc::new(AtomicBool::new(false));
    let laid = pump_widget(
        Offstage::new().offstage(false).child(tap_probe(&did_tap)),
        screen(),
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);

    assert!(
        did_tap.load(Ordering::SeqCst),
        "offstage=false: the pointer must reach the child"
    );
}

// ── Toggling cluster ─────────────────────────────────────────────────────────

/// Toggling `offstage` on the same mounted node (a root-swap via
/// `LaidOut::pump_widget`, not a fresh mount) flips hit-testing without
/// discarding the child's render state — the render-node count and the
/// render-tree root id are unchanged across the toggle.
///
/// Flutter parity: the same `hitTest => !offstage && super.hitTest(…)`
/// contract, driven across a live toggle rather than two separate mounts —
/// what `Offstage`'s own doc calls out as the point of the widget ("keeps its
/// child in the tree ... cheaper than rebuilding"). `visibility_test.rs`'s
/// `visibility_maintain_state_hit_test_tracks_visibility_across_repeated_toggles`
/// already drives a longer sequence through `Visibility`; this isolates one
/// toggle directly on `Offstage`.
#[test]
fn toggling_offstage_without_remounting_flips_hit_testing() {
    let did_tap = Arc::new(AtomicBool::new(false));

    let mut laid = pump_widget(
        Offstage::new().offstage(true).child(tap_probe(&did_tap)),
        screen(),
    );
    let render_id_before = laid.root();
    let render_node_count_before = laid.render_node_count();

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);
    assert!(
        !did_tap.load(Ordering::SeqCst),
        "leg 1 (offstage=true): must not reach the child"
    );

    laid.pump_widget(Offstage::new().offstage(false).child(tap_probe(&did_tap)));

    assert_eq!(
        laid.current_root(),
        render_id_before,
        "toggling offstage must not remount the node — the render id must \
         be unchanged"
    );
    assert_eq!(
        laid.render_node_count(),
        render_node_count_before,
        "toggling offstage must not change the render-node count — the \
         child's state is preserved, not discarded and rebuilt"
    );

    laid.dispatch_pointer_down(400.0, 300.0);
    laid.dispatch_pointer_up(400.0, 300.0);
    assert!(
        did_tap.load(Ordering::SeqCst),
        "leg 2 (offstage=false, same node): the toggle must let the tap \
         reach the child"
    );
}
