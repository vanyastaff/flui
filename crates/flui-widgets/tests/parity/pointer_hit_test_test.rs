//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/absorb_pointer_test.dart`,
//! `packages/flutter/test/widgets/listener_test.dart`,
//! `packages/flutter/lib/src/widgets/basic.dart`,
//! `packages/flutter/lib/src/rendering/proxy_box.dart`, and
//! `packages/flutter/lib/src/gestures/binding.dart` (tag `3.44.0`).
//!
//! Ported cases:
//! - `'AbsorbPointers do not block siblings'` (`absorb_pointer_test.dart`) —
//!   [`absorb_pointer_does_not_block_a_column_sibling`].
//! - `RenderAbsorbPointer.hitTest` (`proxy_box.dart`: `absorbing ?
//!   size.contains(position) : super.hitTest(...)`) and the `AbsorbPointer`/
//!   `IgnorePointer` doc contracts (`basic.dart`) —
//!   [`absorbing_blocks_a_stack_sibling_below`],
//!   [`absorbing_still_delivers_to_a_listener_above_it`],
//!   [`ignoring_hides_the_subtree_and_itself_from_hit_testing`],
//!   [`ignoring_lets_a_stack_sibling_below_receive_the_pointer`],
//!   [`toggling_ignoring_on_a_mounted_tree_takes_effect_without_a_remount`].
//! - `'Events bubble up the tree'` (`listener_test.dart`) —
//!   [`pointer_events_bubble_from_the_innermost_listener_outward`].
//! - `HitTestBehavior.translucent`'s "receive without blocking" contract
//!   (`gestures/hit_test.dart` doc, exercised end-to-end through the widget →
//!   render-object wiring) —
//!   [`translucent_listener_receives_without_blocking_a_sibling_below`].
//! - `GestureBinding._handlePointerEventImmediately`'s down-cached-route reuse
//!   (`gestures/binding.dart:418-428`: `PointerMoveEvent`/`PointerUpEvent`
//!   look up `_hitTests[event.pointer]` — the path recorded at `PointerDown`
//!   — rather than hit-testing again) —
//!   [`pressed_move_and_up_outside_the_bounds_reach_the_down_route`],
//!   [`pointer_cancel_is_delivered_on_the_route_captured_at_down`],
//!   [`a_rebuilt_listener_callback_receives_the_up_on_the_cached_down_route`],
//!   [`an_unmounted_listener_still_receives_the_up_on_its_active_route`].
//! - A pointer outside every hit-testable bound reaches nothing — the
//!   baseline every case above assumes —
//!   [`a_pointer_outside_every_bound_hits_nothing`].
//!
//! Not ported:
//! - `absorb_pointer_test.dart`'s `group('AbsorbPointer semantics', ...)` (3
//!   cases: `'does not change semantics when not absorbing'`, `'drops
//!   semantics when its ignoreSemantics is true'`, and `'ignores user
//!   interactions'`) — FLUI's semantics tree is Phase 3 per
//!   `crates/flui-widgets/tests/parity/main.rs`'s own phase banner; nothing
//!   to assert against yet. `absorb_pointer_test.dart` has 4 `testWidgets`
//!   total at tag `3.44.0` (verified via `git show 3.44.0:.../absorb_pointer_test.dart`);
//!   this file ports 1 of them (`'AbsorbPointers do not block siblings'`,
//!   above) — 1/4.
//! - `listener_test.dart`'s two `"RenderPointerListener's debugFillProperties
//!   when default/full"` cases — diagnostics-string formatting, not hit-test
//!   behavior; out of scope for a hit-test parity file.
//! - `'Detects hover events from touch devices'` (`listener_test.dart:45`) —
//!   duplicates `crates/flui-widgets/tests/listener.rs:130`
//!   (`listener_routes_buttonless_move_to_hover_callback`) except for the
//!   pointer device kind (mouse there, a touch-originated hover here); FLUI's
//!   hit-test and hover-delivery path does not branch on device kind, so the
//!   existing mouse-driven case already covers the contract this one would
//!   re-assert. `listener_test.dart` has 8 `testWidgets` total at tag
//!   `3.44.0` (verified via `git show 3.44.0:.../listener_test.dart`):
//!   `'Events bubble up the tree'` (ported above), this hover case, the two
//!   `debugFillProperties` cases just above, and the four-case
//!   `'transformed events'` group ported/not-ported in
//!   `pointer_local_position_test.rs` (3 ported, 1 not) — 4/8 ported across
//!   both files, the other 4 explicitly accounted for above and in that
//!   file's own module doc.
//! - `MouseRegion` (Flutter has no single oracle file for it analogous to
//!   `listener_test.dart`; the closest equivalent, `mouse_region_test.dart`,
//!   has 44 cases spanning hover-enter/exit device tracking) needs a
//!   `LaidOut::add_mouse_device` helper that does not exist in this harness —
//!   out of scope for this pass.
//!
//! Divergence: FLUI's childless `GestureDetector` defaults to
//! [`HitTestBehavior::DeferToChild`] where Flutter's childless
//! `GestureDetector` defaults to `translucent`
//! (`crates/flui-widgets/src/interaction/gesture_detector.rs:142` vs
//! `widgets/gesture_detector.dart:1571-1573` at tag `3.44.0`) — already
//! documented in `crates/flui-widgets/tests/parity/gesture_detector_test.rs`'s
//! own module doc, restated here because it's WHY case 1 below probes with a
//! bare [`Listener`] rather than a `GestureDetector` the way Flutter's
//! `'AbsorbPointers do not block siblings'` literally does: a childless
//! `GestureDetector` in FLUI would register `DeferToChild` and never fire,
//! which is not what the upstream test is checking.
//!
//! This file is additive to hit-test coverage that already exists:
//! `crates/flui-widgets/tests/listener.rs` (7 tests: `HitTestBehavior`
//! contract, down/up/hover/move/signal/pan-zoom routing) covers a SINGLE
//! `Listener` in isolation; `crates/flui-widgets/tests/absorb_pointer.rs` (4
//! tests) and `crates/flui-widgets/tests/modifiers.rs:40`
//! (`ignore_pointer_is_a_layout_passthrough`) cover `AbsorbPointer`/
//! `IgnorePointer` as isolated layout pass-throughs and a single
//! absorb-then-`GestureDetector` block; `crates/flui-widgets/tests/mouse_region.rs`
//! (3 tests) covers `MouseRegion` in isolation;
//! `crates/flui-objects/tests/render_object_harness.rs:943` (translucent
//! Listener + Stack sibling), `:1021`/`:1042` (`MouseRegion` sibling/tracker
//! entries), `:5342` (`RenderAbsorbPointer` blocks a child), and `:5361`
//! (`RenderIgnorePointer` lets a sibling through) exercise the render objects
//! directly, below the widget layer; `crates/flui-widgets/tests/parity/gesture_detector_test.rs`
//! covers `HitTestBehavior` on a childless `GestureDetector` stacked over a
//! `Listener`. None of the above cover: siblings across a `Column`/`Expanded`
//! split (case 1), nested absorb/ignore wrapping (cases 3-4), a live
//! `ignoring` toggle on an already-mounted tree (case 6), event bubbling
//! through three nested `Listener`s (case 7), or the down-cached-route reuse
//! for move/up/cancel/rebuild/unmount (cases 9-12) — every case in this file
//! targets one of those gaps.
//!
//! Widget → render-object mapping:
//! - `Listener` → `RenderListener`
//!   (`crates/flui-objects/src/interaction/listener.rs`)
//! - `AbsorbPointer` → `RenderAbsorbPointer`, `IgnorePointer` →
//!   `RenderIgnorePointer` (`crates/flui-objects/src/interaction/`)
//! - `Stack` → `RenderStack` (top-most child tested first, stops at the
//!   first hit that reports `blocks_below`)
//! - `Column`/`Expanded` → `RenderFlex` (`Expanded` forces tight cross-axis
//!   AND main-axis-share constraints on its child, per `FlexFit::Tight`)

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_view::ViewExt;
use flui_widgets::prelude::*;

use crate::harness;

/// A hit-testable 100×100 leaf — `ColoredBox` wraps a childless `SizedBox` so
/// the box actually hit-tests true (a bare `SizedBox` with no child does
/// not; see `crates/flui-objects/src/layout/constrained_box.rs:130-139`).
/// `ColoredBox` maps to `RenderDecoratedBox` — the same render object
/// Flutter's `DecoratedBox(decoration: BoxDecoration())` produces, which is
/// why case 7 below reuses it in place of Flutter's literal `DecoratedBox`.
fn target() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30)).child(SizedBox::new(100.0, 100.0))
}

fn counter() -> (Rc<Cell<usize>>, impl Fn(&PointerEvent) + Clone) {
    let count = Rc::new(Cell::new(0));
    let probe = Rc::clone(&count);
    (count, move |_event: &PointerEvent| {
        probe.set(probe.get() + 1);
    })
}

/// `AbsorbPointer` (bare, no child) sitting in the LOWER half of a
/// `Column` must not block a `Listener` in the UPPER half.
///
/// Flutter parity: `'AbsorbPointers do not block siblings'`
/// (`absorb_pointer_test.dart`) — `Column(Expanded(GestureDetector),
/// Expanded(AbsorbPointer()))`, taps the `GestureDetector` and expects it to
/// still fire. Ported with a bare `Listener` in place of `GestureDetector`
/// (see the module doc's Divergence note) and split into two dispatches (one
/// per half) so the RED check is explicit on both sides: a wiring bug that
/// made `AbsorbPointer` absorb its OWN half's Listener too, or one that let
/// it swallow the WHOLE column, would each fail a different assertion here.
///
/// `Expanded`'s `FlexFit::Tight` gives the bare, childless `AbsorbPointer` a
/// concrete non-zero size (Flutter's literal shape) — under a merely loose
/// constraint a childless `AbsorbPointer` would size to zero and this test
/// would vacuously pass regardless of the fix.
#[test]
fn absorb_pointer_does_not_block_a_column_sibling() {
    let (hits, on_down) = counter();

    let laid = harness::pump_widget(
        Column::new(vec![
            Expanded::new(Listener::new().on_pointer_down(on_down).child(target())).boxed(),
            Expanded::new(AbsorbPointer::new()).boxed(),
        ]),
        harness::screen_of(100.0, 200.0),
    );

    // Upper half (y in [0, 100)): the Listener's own half.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);
    assert_eq!(
        hits.get(),
        1,
        "the Listener must still fire from within its own half"
    );

    // Lower half (y in [100, 200)): the bare AbsorbPointer's half — must not
    // reach back up to the Listener above it.
    laid.dispatch_pointer_down(50.0, 150.0);
    laid.dispatch_pointer_up(50.0, 150.0);
    assert_eq!(
        hits.get(),
        1,
        "AbsorbPointer's own half must not fire the sibling Listener stacked above it"
    );
}

/// `AbsorbPointer` (`absorbing = true`, the default) stacked ON TOP of a
/// `Listener` must block that sibling from ever receiving the pointer.
///
/// Flutter parity: `RenderAbsorbPointer.hitTest` (`proxy_box.dart`):
/// `absorbing ? size.contains(position) : super.hitTest(...)` — when
/// absorbing, a hit within bounds is unconditional and never descends, so a
/// `Stack` sibling visually behind it (tested next, since `Stack` tests
/// top-most first and stops at the first hit) never gets tested at all. Also
/// pins `basic.dart`'s `AbsorbPointer` doc contract ("this render object
/// absorbs pointers ... regardless of ... layout/painting").
#[test]
fn absorbing_blocks_a_stack_sibling_below() {
    let (below_hits, on_down) = counter();

    let laid = harness::pump_widget(
        Stack::new((
            Listener::new().on_pointer_down(on_down).child(target()),
            AbsorbPointer::new(),
        ))
        .fit(StackFit::Expand),
        harness::screen_of(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);

    assert_eq!(
        below_hits.get(),
        0,
        "an absorbing AbsorbPointer stacked above must block the Listener below it"
    );
}

/// Absorbing still lets a `Listener` ABOVE the `AbsorbPointer` (an
/// ancestor, not a `Stack` sibling) receive the pointer — only the
/// `AbsorbPointer`'s own subtree is swallowed.
///
/// Flutter parity: same `RenderAbsorbPointer.hitTest` contract as case 2,
/// exercised on the opposite side of the absorbing node: `Listener(outer) >
/// AbsorbPointer > Listener(inner)`. `AbsorbPointer` self-registers (its
/// `hitTest` returns `true` within bounds), which is exactly what the outer
/// `Listener`'s `DeferToChild` behavior is watching for — "a descendant was
/// hit" — so the outer still fires even though the inner never gets
/// hit-tested at all.
#[test]
fn absorbing_still_delivers_to_a_listener_above_it() {
    let (outer_hits, on_outer) = counter();
    let (inner_hits, on_inner) = counter();

    let laid = harness::pump_widget(
        Listener::new().on_pointer_down(on_outer).child(
            AbsorbPointer::new().child(Listener::new().on_pointer_down(on_inner).child(target())),
        ),
        harness::screen_of(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);

    assert_eq!(outer_hits.get(), 1, "the outer Listener must still fire");
    assert_eq!(
        inner_hits.get(),
        0,
        "the inner Listener must never be hit-tested — AbsorbPointer swallows its own subtree"
    );
}

/// `IgnorePointer` (`ignoring = true`, the default) hides BOTH itself and
/// its whole subtree from hit-testing — unlike `AbsorbPointer`, it never
/// self-registers. Flipping `ignoring` to `false` restores normal
/// (transparent pass-through) hit-testing for the whole chain.
///
/// Flutter parity: `RenderIgnorePointer.hitTest` (`proxy_box.dart`): `return
/// !ignoring && super.hitTest(...)` — when ignoring, the method returns
/// `false` unconditionally, before even checking its own bounds, so nothing
/// under it (and it itself) ever contributes a hit entry. Also pins
/// `basic.dart`'s `IgnorePointer` doc contract.
#[test]
fn ignoring_hides_the_subtree_and_itself_from_hit_testing() {
    let (outer_hits, on_outer) = counter();
    let (inner_hits, on_inner) = counter();

    let hidden = harness::pump_widget(
        Listener::new().on_pointer_down(on_outer.clone()).child(
            IgnorePointer::new().child(
                Listener::new()
                    .on_pointer_down(on_inner.clone())
                    .child(target()),
            ),
        ),
        harness::screen_of(100.0, 100.0),
    );
    hidden.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(
        outer_hits.get(),
        0,
        "ignoring must hide the subtree from the outer Listener's DeferToChild check too"
    );
    assert_eq!(
        inner_hits.get(),
        0,
        "the inner Listener must never be hit-tested"
    );

    let (outer_hits, on_outer) = counter();
    let (inner_hits, on_inner) = counter();
    let visible = harness::pump_widget(
        Listener::new().on_pointer_down(on_outer).child(
            IgnorePointer::new()
                .ignoring(false)
                .child(Listener::new().on_pointer_down(on_inner).child(target())),
        ),
        harness::screen_of(100.0, 100.0),
    );
    visible.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(
        outer_hits.get(),
        1,
        "ignoring(false) restores the outer Listener"
    );
    assert_eq!(
        inner_hits.get(),
        1,
        "ignoring(false) restores the inner Listener too"
    );
}

/// `IgnorePointer` above a `Stack` sibling lets that sibling receive the
/// pointer while ignoring, and blocks it once `ignoring` is turned off (its
/// own hittable child then wins the `Stack`'s top-most-first search).
///
/// Flutter parity: same `RenderIgnorePointer.hitTest` contract as case 4,
/// read from the `Stack`-sibling angle this time — `!ignoring &&
/// super.hitTest(...)`: while ignoring, the short-circuit means the sibling
/// below is never blocked; once not ignoring, `super.hitTest` (a normal
/// hittable child) returns `true` and — same `Stack` top-most-first-and-stop
/// rule as case 2 — the sibling below never gets tested.
#[test]
fn ignoring_lets_a_stack_sibling_below_receive_the_pointer() {
    let (below_hits, on_down) = counter();
    let ignoring = harness::pump_widget(
        Stack::new((
            Listener::new().on_pointer_down(on_down).child(target()),
            IgnorePointer::new().child(target()),
        ))
        .fit(StackFit::Expand),
        harness::screen_of(100.0, 100.0),
    );
    ignoring.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(
        below_hits.get(),
        1,
        "ignoring must let the sibling below receive the pointer"
    );

    let (below_hits, on_down) = counter();
    let not_ignoring = harness::pump_widget(
        Stack::new((
            Listener::new().on_pointer_down(on_down).child(target()),
            IgnorePointer::new().ignoring(false).child(target()),
        ))
        .fit(StackFit::Expand),
        harness::screen_of(100.0, 100.0),
    );
    not_ignoring.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(
        below_hits.get(),
        0,
        "not ignoring: IgnorePointer's own hittable child wins the Stack's top-most search first"
    );
}

/// Flipping `ignoring` on an ALREADY-MOUNTED tree (`pump_widget`, the
/// harness's view-diffing update path — not a fresh [`harness::pump_widget`]
/// call) takes effect immediately, with no special-case "remount" needed.
///
/// A `RenderId` is a 1-based slab index; a free-then-reinsert during
/// reconciliation can return the SAME id for an unrelated node, so `RenderId`
/// equality across two pumps is not evidence of "the same node kept its
/// identity" (see this file's harness-facts). What actually makes this a
/// remount test (and not a vacuous one) is `LaidOut::pump_widget` itself
/// (`crates/flui-widgets/tests/common/mod.rs:662-667`): it is
/// `swap_root_view` + `pump_frame`, with no second
/// [`harness::pump_widget`] (fresh-mount) call anywhere in this test — the
/// tree is updated IN PLACE. The `on_down` handler installed on the second
/// `pump_widget` call (line below) shares the SAME underlying
/// `Rc<Cell<usize>>` as the handler installed at mount time --
/// [`counter`]'s closure is `Clone`, and every clone closes over that one
/// `Rc`, so the hit count is genuinely cumulative across both states (which
/// is exactly why `hits.get() == 1` after the toggle reads as a delta, not
/// a fresh count). It is the single continuously-live [`LaidOut`] and its
/// in-place update path that prove no remount happened, not the counter
/// identity.
#[test]
fn toggling_ignoring_on_a_mounted_tree_takes_effect_without_a_remount() {
    let (hits, on_down) = counter();

    let mut laid = harness::pump_widget(
        IgnorePointer::new().child(
            Listener::new()
                .on_pointer_down(on_down.clone())
                .child(target()),
        ),
        harness::screen_of(80.0, 80.0),
    );

    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_up(40.0, 40.0);
    assert_eq!(
        hits.get(),
        0,
        "ignoring(true) (the default) must still block at mount time"
    );

    laid.pump_widget(
        IgnorePointer::new()
            .ignoring(false)
            .child(Listener::new().on_pointer_down(on_down).child(target())),
    );

    laid.dispatch_pointer_down(40.0, 40.0);
    assert_eq!(
        hits.get(),
        1,
        "flipping ignoring to false via pump_widget must take effect on the live tree"
    );
}

/// A pointer landing on a target reaches every ancestor `Listener`,
/// innermost first.
///
/// Flutter parity: `'Events bubble up the tree'` (`listener_test.dart`):
/// `Listener(top) > Listener(middle) > DecoratedBox > Listener(bottom) >
/// Text('X')`, `tester.tap(find.text('X'))`, expects `log == ['bottom',
/// 'middle', 'top']`. The hit point is derived from the mounted `Text`
/// node's own geometry (`find_text` + `absolute_offset` + `size / 2`), never
/// hardcoded, so a layout change to any ancestor cannot silently make this
/// pass by accident.
#[test]
fn pointer_events_bubble_from_the_innermost_listener_outward() {
    let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let (top_log, middle_log, bottom_log) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&log));

    let laid = harness::pump_widget(
        Listener::new()
            .on_pointer_down(move |_| top_log.borrow_mut().push("top"))
            .child(
                Listener::new()
                    .on_pointer_down(move |_| middle_log.borrow_mut().push("middle"))
                    .child(
                        ColoredBox::new(Color::rgb(0, 0, 0)).child(
                            Listener::new()
                                .on_pointer_down(move |_| bottom_log.borrow_mut().push("bottom"))
                                .child(Text::new("X")),
                        ),
                    ),
            ),
        harness::screen_of(300.0, 300.0),
    );

    let text_id = laid.find_text("X").expect("Text(\"X\") must be mounted");
    let text_position = laid.absolute_offset(text_id);
    let text_size = laid.size(text_id);
    laid.dispatch_pointer_down(
        text_position.dx.get() + text_size.width.get() / 2.0,
        text_position.dy.get() + text_size.height.get() / 2.0,
    );

    assert_eq!(&*log.borrow(), &["bottom", "middle", "top"]);
}

/// A translucent `Listener` receives the pointer AND does not block a
/// `Stack` sibling visually behind it — the one `HitTestBehavior` variant
/// designed to do both at once.
///
/// Flutter parity: `HitTestBehavior.translucent`'s doc contract
/// (`gestures/hit_test.dart`), the same shape
/// `crates/flui-objects/tests/render_object_harness.rs:943`
/// (`harness_listener_translucent_adds_entry_without_blocking_lower_sibling`)
/// proves at the render-object level — this is that same scenario reached
/// through the widget → render-object wiring. The translucent `Listener` is
/// deliberately CHILDLESS: `RenderListener::hit_test`'s `Translucent` arm
/// only self-registers via `register_self_hit_entry` (without blocking)
/// when its OWN child was NOT hit (`!hit_target`); with a hit child, a
/// translucent `Listener` returns `true` from `hit_test` exactly like
/// `DeferToChild`/`Opaque` do, which DOES block the sibling below through
/// the generic child-hit-stops-the-Stack-search convention (see case 2) —
/// there is nothing translucent-specific about that path.
#[test]
fn translucent_listener_receives_without_blocking_a_sibling_below() {
    let (top_hits, on_top) = counter();
    let (below_hits, on_below) = counter();

    let laid = harness::pump_widget(
        Stack::new((
            Listener::new().on_pointer_down(on_below).child(target()),
            Listener::new()
                .behavior(HitTestBehavior::Translucent)
                .on_pointer_down(on_top),
        ))
        .fit(StackFit::Expand),
        harness::screen_of(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);

    assert_eq!(
        top_hits.get(),
        1,
        "the translucent Listener must still fire"
    );
    assert_eq!(
        below_hits.get(),
        1,
        "a translucent Listener must not block the sibling behind it"
    );
}

/// A pressed move and the matching up, dispatched OUTSIDE the target's
/// bounds, still reach the route captured at `down` — they do not
/// re-hit-test at their current (miss) position. A hover AFTER the contact
/// ends performs a genuinely fresh hit-test and hits nothing out there.
///
/// Flutter parity: `GestureBinding._handlePointerEventImmediately`
/// (`gestures/binding.dart:418-428`): `PointerMoveEvent`/`PointerUpEvent`
/// read `_hitTests[event.pointer]` — the `HitTestResult` recorded at
/// `PointerDownEvent` — rather than hit-testing again; a `PointerHoverEvent`
/// with no down in flight always hit-tests fresh (`event.down` is false, so
/// none of the down/up/move branches apply — it falls through to a bare
/// `dispatchEvent` with no cached result).
#[test]
fn pressed_move_and_up_outside_the_bounds_reach_the_down_route() {
    let (downs, on_down) = counter();
    let (moves, on_move) = counter();
    let (ups, on_up) = counter();
    let (hovers, on_hover) = counter();

    let laid = harness::pump_widget(
        Listener::new()
            .on_pointer_down(on_down)
            .on_pointer_move(on_move)
            .on_pointer_up(on_up)
            .on_pointer_hover(on_hover)
            .child(target()),
        harness::screen_of(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    assert_eq!(downs.get(), 1);

    laid.dispatch_pointer_move(400.0, 400.0);
    assert_eq!(
        moves.get(),
        1,
        "a pressed move must reach the down-cached route even far outside the bounds"
    );

    laid.dispatch_pointer_up(400.0, 400.0);
    assert_eq!(
        ups.get(),
        1,
        "up must reach the down-cached route the same way move did"
    );

    // The contact is over now; a hover performs a fresh hit-test and (400,
    // 400) is genuinely outside the 100x100 target.
    laid.dispatch_pointer_hover(400.0, 400.0);
    assert_eq!(
        hovers.get(),
        0,
        "hover has no cached route to fall back on and must hit-test fresh"
    );
}

/// A cancelled contact delivers `on_pointer_cancel` on the route
/// captured at `down`, and must NOT also deliver `on_pointer_up`.
///
/// `on_pointer_cancel` has zero coverage anywhere else in the existing
/// `flui-widgets` test suite before this case. Flutter parity: the same
/// down-cached-route contract as case 9, for `PointerCancelEvent`
/// (`gestures/binding.dart:418-428`, the `PointerCancelEvent` branch of the
/// same `if`).
#[test]
fn pointer_cancel_is_delivered_on_the_route_captured_at_down() {
    let (cancels, on_cancel) = counter();
    let (ups, on_up) = counter();

    let laid = harness::pump_widget(
        Listener::new()
            .on_pointer_cancel(on_cancel)
            .on_pointer_up(on_up)
            .child(target()),
        harness::screen_of(80.0, 80.0),
    );

    laid.dispatch_pointer_down(40.0, 40.0);
    laid.dispatch_pointer_cancel();

    assert_eq!(
        cancels.get(),
        1,
        "on_pointer_cancel must fire on the down-cached route"
    );
    assert_eq!(
        ups.get(),
        0,
        "a cancelled contact must not also deliver on_pointer_up"
    );
}

/// Rebuilding a `Listener` with a NEW callback (a `setState`-style
/// `pump_widget`, not a fresh mount) between `down` and `up` routes the `up`
/// to the NEW callback, never the old one.
///
/// Flutter parity: same down-cached-route contract as cases 9-10, combined
/// with `Listener::update_render_object`'s documented behavior
/// (`crates/flui-widgets/src/interaction/listener.rs`): "Rebuild replaces
/// the handler INSIDE the existing cell, so an active cached route observes
/// the new configuration."
#[test]
fn a_rebuilt_listener_callback_receives_the_up_on_the_cached_down_route() {
    let (old_ups, on_old_up) = counter();
    let (new_ups, on_new_up) = counter();

    let mut laid = harness::pump_widget(
        Listener::new().on_pointer_up(on_old_up).child(target()),
        harness::screen_of(80.0, 80.0),
    );

    laid.dispatch_pointer_down(40.0, 40.0);

    laid.pump_widget(Listener::new().on_pointer_up(on_new_up).child(target()));

    laid.dispatch_pointer_up(40.0, 40.0);

    assert_eq!(
        old_ups.get(),
        0,
        "the OLD callback must not fire after the rebuild replaced it"
    );
    assert_eq!(
        new_ups.get(),
        1,
        "up must reach the NEW callback on the still-cached down route"
    );
}

/// Unmounting a `Listener` entirely (a `pump_widget` root-swap to a tree
/// that no longer contains it) between `down` and `up` still delivers the
/// `up` to that Listener's callback — the active route's handler cell
/// outlives the render object's own mount lifetime.
///
/// Pins the prose contract at
/// `crates/flui-widgets/src/interaction/listener.rs:265-266`
/// (`did_unmount_render_object`): "Unmount removes the target from NEW route
/// resolution; an active cached route keeps its strong handler cell through
/// Up/Cancel."
#[test]
fn an_unmounted_listener_still_receives_the_up_on_its_active_route() {
    let (ups, on_up) = counter();

    let mut laid = harness::pump_widget(
        Listener::new().on_pointer_up(on_up).child(target()),
        harness::screen_of(80.0, 80.0),
    );

    laid.dispatch_pointer_down(40.0, 40.0);

    // Swap to a tree with no Listener anywhere -- the mounted one is gone.
    laid.pump_widget(target());

    laid.dispatch_pointer_up(40.0, 40.0);

    assert_eq!(
        ups.get(),
        1,
        "an unmounted Listener's active cached route must still deliver its up"
    );
}

/// A pointer outside every hit-testable bound in the tree hits nothing —
/// the baseline every other case in this file assumes when it dispatches
/// inside a target's bounds and expects exactly one delivery.
#[test]
fn a_pointer_outside_every_bound_hits_nothing() {
    let (hits, on_down) = counter();

    let laid = harness::pump_widget(
        Listener::new().on_pointer_down(on_down).child(target()),
        harness::screen_of(50.0, 50.0),
    );

    laid.dispatch_pointer_down(500.0, 500.0);

    assert_eq!(
        hits.get(),
        0,
        "a pointer entirely outside the tree's bounds must hit nothing"
    );
}
