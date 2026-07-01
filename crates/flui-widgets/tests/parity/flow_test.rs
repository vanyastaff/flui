//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/flow_test.dart` line 64
//! (`'Flow control test'`), plus `rendering/flow.dart` `RenderFlow` for the
//! `_getSize`/`getConstraintsForChild` sizing formulas.
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
fn flow_hit_test_inverts_each_childs_real_transform_after_offset_changes() {
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
