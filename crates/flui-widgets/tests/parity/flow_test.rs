//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/flow_test.dart` (5
//! `testWidgets` cases, tag `3.44.0`), plus `rendering/flow.dart` `RenderFlow`
//! for the `_getSize`/`getConstraintsForChild` sizing formulas.
//!
//! Widget → render-object mapping:
//! - `Flow` → `RenderFlow`
//!
//! Divergences (documented, matching `CustomPaint`'s precedent):
//! - Oracle's `TestFlowDelegate` takes an `Animation<double> startOffset`
//!   (`FlowDelegate`'s `Listenable? repaint` constructor arg) and mutates
//!   `dy` via the animation ticking. No FLUI-side `Listenable`/`Animation`
//!   plumbing exists for render objects yet, so this port uses two static
//!   delegate instances (`start_offset = 0.0` and `start_offset = 50.0`)
//!   instead of one animated delegate whose value changes mid-test — same
//!   observable geometry at each snapshot, no animation-driven repaint.
//! - Oracle taps via `GestureDetector`/`tester.tapAt` (full gesture-arena
//!   resolution). This port uses `Listener::on_pointer_down` + a headless
//!   pointer-down dispatch — the simpler primitive already used by this
//!   crate's `listener.rs` suite — since the assertion under test is
//!   "which child's hit-test wins", not gesture-arena tap semantics.
//!
//! ## Oracle case reconciliation (all 5 `testWidgets`, by name)
//!
//! 1. `'Flow control test'` — split across three cases: the opening
//!    non-overlapping sequential taps are
//!    [`flow_sequential_taps_on_non_overlapping_children_accumulate_the_log`];
//!    the two overlapping `tapAt(20.0, 90.0)` assertions (before/after
//!    `startOffset.value = 50.0`) are
//!    [`flow_reverse_paint_order_hit_test_picks_the_last_painted_overlapping_child`]
//!    and
//!    [`flow_hit_test_inverts_each_child_real_transform_after_offset_changes`].
//! 2. `'paintChild gets called twice'` —
//!    [`flow_delegate_painting_the_same_child_twice_panics`]. Mechanism
//!    divergence, not a behavior one: the oracle catches a recoverable
//!    `FlutterError` via `tester.takeException()`; FLUI's
//!    `FlowPaintingContext::paint_child` enforces the identical
//!    double-paint invariant with a Rust `assert!` (a hard panic).
//! 3. `'Flow opacity layer'` — **not ported, filed to Cross.H**
//!    (`docs/ROADMAP.md`). `FlowPaintingContext::paint_child` has no
//!    `opacity` parameter at all — the trait method's own signature is
//!    missing the surface, so there is no compiling call to write a test
//!    against, `#[ignore]`d or otherwise; adding the parameter is itself
//!    the fix, not a side effect of a test port.
//! 4. `'Flow can set and update clipBehavior'` —
//!    [`update_render_object_applies_every_clip_variant`] in
//!    `flui-widgets/src/layout/flow.rs`'s own test module (co-located with
//!    the narrower clip-behavior tests it extends), looping every `Clip`
//!    variant rather than just `HardEdge`/`None`.
//! 5. `'Flow.unwrapped can set and update clipBehavior'` — **covered by
//!    implication, not separately ported.** Flutter's default `Flow()`
//!    constructor wraps every child in a `RepaintBoundary` via
//!    `RepaintBoundary.wrapAll` (a paint-isolation optimization);
//!    `Flow.unwrapped` skips that wrapping. FLUI's single `Flow::new` never
//!    wraps children in a repaint boundary either — there is no second,
//!    differently-behaving construction path — so case 4's test already
//!    exercises the only code path this crate has, which is architecturally
//!    equivalent to the oracle's `.unwrapped` variant. The `RepaintBoundary`
//!    auto-wrap itself is a paint-isolation perf detail orthogonal to the
//!    `FlowDelegate`/`RenderFlow` behavioral contract this slice scopes to,
//!    so its absence is noted here rather than filed to Cross.H.

use std::sync::Arc;

use flui_rendering::constraints::BoxConstraints;
use flui_types::{Color, Matrix4, Size};
use flui_widgets::row;
use flui_widgets::{ColoredBox, Flow, FlowDelegate, FlowPaintingContext, Listener, SizedBox};
use parking_lot::Mutex;

use crate::harness;

/// Stacks children top-to-bottom with 25% overlap: child `i` is translated
/// to `dy`, then `dy += 0.75 * child_i.height` for the next child — the
/// oracle's `TestFlowDelegate.paintChildren` formula (`flow_test.dart`
/// L20-25), with `start_offset` standing in for the animated `startOffset`
/// value at a single point in time (see module docs).
#[derive(Debug)]
struct StackedFlowDelegate {
    start_offset: f32,
}

impl FlowDelegate for StackedFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        // Oracle default (`FlowDelegate.getSize`, flow.dart L80): as large
        // as the incoming constraints allow.
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        // `flow_test.dart` L15-17: `constraints.loosen()`.
        constraints.loosen()
    }

    fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
        let mut dy = self.start_offset;
        for i in 0..context.child_count() {
            context.paint_child(i, Matrix4::translation(0.0, dy, 0.0));
            dy += 0.75 * context.child_size(i).height.get();
        }
    }

    fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, old_delegate: &dyn FlowDelegate) -> bool {
        match old_delegate.as_any().downcast_ref::<Self>() {
            Some(old) => (self.start_offset - old.start_offset).abs() > f32::EPSILON,
            None => true,
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Mounts a two-child `Flow(StackedFlowDelegate { start_offset })`, each
/// child a `Listener` over a fixed 100×100 `SizedBox(ColoredBox)`, dispatches
/// a pointer-down at `(x, y)`, and returns which child index(es) fired.
///
/// The `ColoredBox` matters: a bare, childless `SizedBox` maps to a childless
/// `RenderConstrainedBox`, whose `hit_test` returns `false` unconditionally
/// (Flutter parity — `RenderProxyBox` with no child never hit-tests itself).
/// Something must actually occupy the box for a hit to land on it.
fn hit_indices(start_offset: f32, x: f32, y: f32) -> Vec<usize> {
    let hits: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let child = |index: usize| {
        let hits = Arc::clone(&hits);
        Listener::new()
            .on_pointer_down(move |_event| hits.lock().push(index))
            .child(SizedBox::new(100.0, 100.0).child(ColoredBox::new(Color::BLUE)))
    };

    let laid = harness::pump_widget(
        Flow::new(
            Arc::new(StackedFlowDelegate { start_offset }),
            row![child(0), child(1)],
        ),
        harness::screen(),
    );
    laid.dispatch_pointer_down(x, y);

    hits.lock().clone()
}

/// `start_offset = 0.0`: child 0 occupies y ∈ [0, 100), child 1 (dy = 0.75 ×
/// 100 = 75) occupies y ∈ [75, 175) — they overlap on y ∈ [75, 100). A tap
/// at (20, 90) lands inside BOTH children's real (transformed) bounds;
/// `RenderFlow::hit_test` walks paint order in reverse (topmost-painted
/// first), so child 1 (painted last) wins.
///
/// Flutter parity: `flow_test.dart` L98 `await tester.tapAt(const
/// Offset(20.0, 90.0)); expect(log, equals(<int>[1]));`.
#[test]
fn flow_reverse_paint_order_hit_test_picks_the_last_painted_overlapping_child() {
    assert_eq!(
        hit_indices(0.0, 20.0, 90.0),
        vec![1],
        "with start_offset=0.0, (20,90) overlaps both children — the \
         later-painted child (index 1) must win the hit, matching the \
         oracle's log == [1] before startOffset changes",
    );
}

/// `start_offset = 50.0`: child 0 occupies y ∈ [50, 150), child 1 (dy = 50 +
/// 75 = 125) occupies y ∈ [125, 225). A tap at (20, 90) now falls OUTSIDE
/// child 1's real bounds (inverse-transformed local y = 90 − 125 = −35) but
/// inside child 0's (local y = 90 − 50 = 40) — proving the hit-test inverts
/// each child's REAL transform rather than using a stale/shared bounding
/// box.
///
/// Flutter parity: `flow_test.dart` L109-114 `startOffset.value = 50.0; ...
/// await tester.tapAt(const Offset(20.0, 90.0)); expect(log, equals(<int>[0]));`.
#[test]
fn flow_hit_test_inverts_each_child_real_transform_after_offset_changes() {
    assert_eq!(
        hit_indices(50.0, 20.0, 90.0),
        vec![0],
        "with start_offset=50.0, (20,90) now falls outside child 1's real \
         bounds and inside child 0's — matching the oracle's log == [0] \
         after startOffset.value = 50.0",
    );
}

/// `RenderFlow::get_size` = `constraints.constrain(delegate.get_size(...))`
/// (oracle `_getSize`, `flow.dart` L261-264) — an oversized delegate size
/// must be clamped to the incoming bounds, not passed through raw. Mirrors
/// `custom_paint_oversized_preferred_size_is_constrained_to_available_bounds`
/// in `custom_paint_test.rs`.
#[test]
fn flow_get_size_is_constrained_to_the_incoming_bounds() {
    #[derive(Debug)]
    struct OversizedDelegate;
    impl FlowDelegate for OversizedDelegate {
        fn get_size(&self, _constraints: BoxConstraints) -> Size {
            Size::new(
                flui_types::geometry::px(2000.0),
                flui_types::geometry::px(2000.0),
            )
        }
        fn get_constraints_for_child(
            &self,
            _index: usize,
            constraints: BoxConstraints,
        ) -> BoxConstraints {
            constraints
        }
        fn paint_children(&self, _context: &mut FlowPaintingContext<'_, '_>) {}
        fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }
        fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let laid = harness::pump_widget(
        Flow::new(
            Arc::new(OversizedDelegate),
            Vec::<flui_view::BoxedView>::new(),
        ),
        harness::screen(),
    );
    let flow_id = laid.find_by_render_type("RenderFlow");
    assert_eq!(
        laid.size(flow_id),
        flui_types::Size::new(
            flui_types::geometry::px(800.0),
            flui_types::geometry::px(600.0)
        ),
        "delegate.get_size() returning 2000x2000 must be constrained to the \
         800x600 incoming surface, not passed through unclamped",
    );
}

/// `RenderFlow::get_constraints_for_child` is honored verbatim, even when it
/// disagrees with the incoming constraints — the oracle's own
/// `getConstraintsForChild` doc: "children need not respect the given
/// constraints, but they are required to respect the returned constraints."
#[test]
fn flow_children_are_sized_by_the_delegates_constraints_not_the_incoming_ones() {
    #[derive(Debug)]
    struct FixedChildConstraintsDelegate;
    impl FlowDelegate for FixedChildConstraintsDelegate {
        fn get_size(&self, constraints: BoxConstraints) -> Size {
            constraints.biggest()
        }
        fn get_constraints_for_child(
            &self,
            _index: usize,
            _constraints: BoxConstraints,
        ) -> BoxConstraints {
            // Fixed 40x30 regardless of the (much larger) incoming box.
            BoxConstraints::tight(Size::new(
                flui_types::geometry::px(40.0),
                flui_types::geometry::px(30.0),
            ))
        }
        fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
            for i in 0..context.child_count() {
                context.paint_child(i, Matrix4::IDENTITY);
            }
        }
        fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }
        fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
            false
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    // SizedBox(500, 500) requests a much larger size than the delegate's
    // fixed 40x30 child constraints allow — the delegate's constraints win.
    let laid = harness::pump_widget(
        Flow::new(
            Arc::new(FixedChildConstraintsDelegate),
            row![SizedBox::new(500.0, 500.0)],
        ),
        harness::screen(),
    );
    let flow_id = laid.find_by_render_type("RenderFlow");
    let child_id = laid.only_child(flow_id);
    assert_eq!(
        laid.size(child_id),
        flui_types::Size::new(
            flui_types::geometry::px(40.0),
            flui_types::geometry::px(30.0)
        ),
    );
    // Flow never positions children during layout — every child sits at
    // Offset::ZERO regardless of the delegate's paint-time transform
    // (oracle `flow.dart` L327).
    assert_eq!(laid.offset(child_id), flui_types::Offset::ZERO);
}

/// Ports the opening, non-overlapping half of the oracle's `'Flow control
/// test'`: three sequential taps, each at a DIFFERENT child's own center,
/// accumulate into the SAME log rather than each starting fresh — proving
/// ordinary (non-overlapping) hit-testing and log bookkeeping both work
/// correctly alongside the paint-time-transform positioning, before the
/// overlapping `tapAt(20.0, 90.0)` cases (already ported above) exercise the
/// reverse-paint-order tie-break.
///
/// With `start_offset = 0.0`, `StackedFlowDelegate` paints child `i` at
/// `dy = 0.75 * 100 * i` (each child 100×100): child 0 at y ∈ [0, 100),
/// child 1 at y ∈ [75, 175), child 2 at y ∈ [150, 250). Their CENTERS
/// (y = 50, 125, 200) fall outside the 25%-overlap bands, so each tap
/// resolves unambiguously to one child.
///
/// Flutter parity: `flow_test.dart` `'Flow control test'` — `await
/// tester.tap(find.text('0')); expect(log, equals(<int>[0])); await
/// tester.tap(find.text('1')); expect(log, equals(<int>[0, 1])); await
/// tester.tap(find.text('2')); expect(log, equals(<int>[0, 1, 2]));`.
#[test]
fn flow_sequential_taps_on_non_overlapping_children_accumulate_the_log() {
    let hits: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
    let child = |index: usize| {
        let hits = Arc::clone(&hits);
        Listener::new()
            .on_pointer_down(move |_event| hits.lock().push(index))
            .child(SizedBox::new(100.0, 100.0).child(ColoredBox::new(Color::BLUE)))
    };

    let laid = harness::pump_widget(
        Flow::new(
            Arc::new(StackedFlowDelegate { start_offset: 0.0 }),
            row![child(0), child(1), child(2)],
        ),
        harness::screen(),
    );

    laid.dispatch_pointer_down(20.0, 50.0);
    assert_eq!(hits.lock().clone(), vec![0], "tap on child 0's own center");

    laid.dispatch_pointer_down(20.0, 125.0);
    assert_eq!(
        hits.lock().clone(),
        vec![0, 1],
        "tap on child 1's own center must APPEND, not replace, the log"
    );

    laid.dispatch_pointer_down(20.0, 200.0);
    assert_eq!(
        hits.lock().clone(),
        vec![0, 1, 2],
        "tap on child 2's own center must append a third entry"
    );
}

/// A delegate that paints child 0 twice in one `paint_children` pass —
/// mirrors the oracle's `DuplicatePainterOpacityFlowDelegate`, minus the
/// `opacity` argument (`FlowPaintingContext::paint_child` has none; see this
/// file's module docs and the Cross.H filing in `docs/ROADMAP.md` — the
/// double-paint invariant under test here is unrelated to opacity and needs
/// no substitute).
#[derive(Debug)]
struct DuplicatePainterFlowDelegate;

impl FlowDelegate for DuplicatePainterFlowDelegate {
    fn get_size(&self, constraints: BoxConstraints) -> Size {
        constraints.biggest()
    }

    fn get_constraints_for_child(
        &self,
        _index: usize,
        constraints: BoxConstraints,
    ) -> BoxConstraints {
        constraints
    }

    fn paint_children(&self, context: &mut FlowPaintingContext<'_, '_>) {
        for i in 0..context.child_count() {
            context.paint_child(i, Matrix4::IDENTITY);
        }
        if context.child_count() > 0 {
            context.paint_child(0, Matrix4::IDENTITY);
        }
    }

    fn should_relayout(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        false
    }

    fn should_repaint(&self, _old_delegate: &dyn FlowDelegate) -> bool {
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Flutter parity: `flow_test.dart` `'paintChild gets called twice'` — the
/// oracle catches a recoverable `FlutterError` ("Cannot call paintChild
/// twice for the same child.") via `tester.takeException()`. FLUI's
/// `FlowPaintingContext::paint_child` enforces the identical invariant with
/// a Rust `assert!` — a documented MECHANISM divergence (hard panic vs. a
/// catchable error type), not a behavior one: both stacks refuse to let a
/// delegate paint the same child twice, and this test proves FLUI's refusal
/// actually fires through the real mount → layout → paint pipeline
/// (`harness::pump_widget`), not merely at the isolated
/// `FlowPaintingContext` unit-test level already covered by
/// `flow_delegate.rs::paint_child_twice_in_one_pass_panics`.
///
/// The panic that actually propagates out of `pump_widget` is not the
/// assert's own message: the pipeline catches a phase panic and poisons the
/// offending node (`Poisoned { render_object, phase }`) instead of
/// unwinding raw, so `harness::pump_widget`'s `.expect(...)` re-panics with
/// that poisoned-node report. The assert's real message ("paint_child
/// called twice for child 0 in one paint_children pass") still fires first
/// and is visible in the test's captured stderr — this assertion checks the
/// outer report identifies the right node and phase (`RenderFlow`, `paint`),
/// which is the observable proof available at this level.
#[test]
#[should_panic(
    expected = "render_object: \"flui_objects::layout::flow::RenderFlow\", phase: \"paint\""
)]
fn flow_delegate_painting_the_same_child_twice_panics() {
    let _ = harness::pump_widget(
        Flow::new(
            Arc::new(DuplicatePainterFlowDelegate),
            row![SizedBox::new(100.0, 100.0), SizedBox::new(100.0, 100.0)],
        ),
        harness::screen(),
    );
}
