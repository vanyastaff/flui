//! End-to-end gesture recognition: a `GestureDetector`'s `on_tap` fires when the
//! user taps (pointer down then up) its child. Drives the real recognizer +
//! presentation-owned arena through the hit-test + binding dispatch path. The
//! binding alone closes and sweeps the shared arena.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{lay_out, tight};
use flui_types::Color;
use flui_widgets::{ColoredBox, GestureDetector, SizedBox};

#[test]
fn gesture_detector_fires_on_tap_for_a_down_up_on_the_child() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            // A hit-testable child so the DeferToChild Listener registers.
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    assert_eq!(taps.load(Ordering::SeqCst), 0, "no tap before any pointer");

    // A tap = down then up at the same place.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on the child fires on_tap exactly once",
    );
}

#[test]
fn gesture_detector_does_not_fire_without_a_hittable_target() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    // DeferToChild over a childless SizedBox (hit-tests false) → nothing is hit,
    // so no pointer reaches the recognizer.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "no tap when nothing under the detector is hit",
    );
}

#[test]
fn gesture_detector_does_not_fire_when_the_pointer_moves_past_slop() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // Down, then a move well past the (mouse) touch slop, then up: the tap is
    // cancelled by the drag, so on_tap must NOT fire.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_move(50.0, 90.0);
    laid.dispatch_pointer_up(50.0, 90.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a pointer that drags past slop does not tap",
    );
}

#[test]
fn gesture_detector_cancel_aborts_the_tap_without_wedging_the_detector() {
    let taps = Arc::new(AtomicUsize::new(0));
    let in_cb = Arc::clone(&taps);

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                in_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // A cancelled contact must NOT tap...
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_cancel();
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a cancelled contact does not tap"
    );

    // ...and must not leave the recognizer wedged: a fresh tap still works.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);
    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a tap after a cancel still fires (the cancel swept the arena entry)",
    );
}

/// Flutter parity (tag `3.44.0`): `packages/flutter/lib/src/gestures/arena.dart`
/// `GestureArenaManager` — "The first member to accept or the last member to
/// not reject wins" (line 110). A drag past the slop makes the tap recognizer
/// reject itself, leaving the pan recognizer as the last remaining (and thus
/// winning) member. The upstream Flutter case asserts exactly this
/// arena-elimination behavior, and this test already covers it end to end, so
/// the citation lives here instead of duplicating the case in the parity
/// corpus.
#[test]
fn gesture_detector_recognizes_a_pan_and_suppresses_the_tap() {
    let taps = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let updates = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(AtomicUsize::new(0));
    let (tap_cb, start_cb, update_cb, end_cb) = (
        Arc::clone(&taps),
        Arc::clone(&starts),
        Arc::clone(&updates),
        Arc::clone(&ends),
    );

    // The detector wants BOTH a tap and a pan; the arena must hand a real drag
    // to the pan recognizer and cancel the tap.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_start(move |_cx, _details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_update(move |_cx, _details| {
                update_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_end(move |_cx, _details| {
                end_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // Down, then a move well past the pan slop (60px > 18px) that crosses into a
    // drag, a second move, then up. Every position stays inside the 100×100
    // child so the headless harness (which re-hit-tests each event — no pointer
    // capture) keeps routing to the detector.
    laid.dispatch_pointer_down(50.0, 20.0);
    laid.dispatch_pointer_move(50.0, 80.0);
    laid.dispatch_pointer_move(50.0, 90.0);
    laid.dispatch_pointer_up(50.0, 90.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "the drag started exactly once"
    );
    assert!(
        updates.load(Ordering::SeqCst) >= 1,
        "the drag reported at least one update as the pointer moved",
    );
    assert_eq!(
        ends.load(Ordering::SeqCst),
        1,
        "the drag ended exactly once on up"
    );
    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a drag past the slop cancels the competing tap — they are mutually exclusive",
    );
}

/// Flutter parity (tag `3.44.0`): `packages/flutter/lib/src/gestures/arena.dart`
/// `GestureArenaManager` (line 110, see the citation on
/// `gesture_detector_recognizes_a_pan_and_suppresses_the_tap` above) — with
/// no movement, the tap recognizer is the arena's front (first-added, and
/// here only remaining) member on sweep, so it wins without waiting for the
/// pan recognizer to reject itself.
#[test]
fn gesture_detector_quick_tap_beats_the_pan_recognizer() {
    let taps = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (tap_cb, start_cb) = (Arc::clone(&taps), Arc::clone(&starts));

    // Same dual-gesture detector, but a quick down→up with no movement: the tap
    // is the arena's front member and wins; the pan never starts.
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                tap_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_pan_start(move |_cx, _details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a quick down+up fires the tap"
    );
    assert_eq!(
        starts.load(Ordering::SeqCst),
        0,
        "no movement means the pan never starts",
    );
}

#[test]
fn secondary_tap_fires_on_secondary_down_up() {
    let primary_taps = Arc::new(AtomicUsize::new(0));
    let secondary_taps = Arc::new(AtomicUsize::new(0));
    let (primary_cb, secondary_cb) = (Arc::clone(&primary_taps), Arc::clone(&secondary_taps));

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                primary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_secondary_tap(move |_cx| {
                secondary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    assert_eq!(
        secondary_taps.load(Ordering::SeqCst),
        0,
        "no tap before any pointer"
    );

    // A secondary-button (right-click) down + up fires on_secondary_tap.
    laid.dispatch_secondary_down(50.0, 50.0);
    laid.dispatch_secondary_up(50.0, 50.0);

    assert_eq!(
        secondary_taps.load(Ordering::SeqCst),
        1,
        "a secondary down+up fires on_secondary_tap exactly once",
    );
    assert_eq!(
        primary_taps.load(Ordering::SeqCst),
        0,
        "a secondary tap must NOT fire on_tap",
    );
}

/// Flutter parity (tag `3.44.0`): `widgets/gesture_detector.dart`'s
/// `onHorizontalDrag*` family — an axis-constrained recognizer distinct from
/// `onPan*`, exercised end to end (down/start/update/end) here for the first
/// time in this crate. `DrawerController`'s own `_handleDragDown`/`_move`/
/// `_settle` (`material/drawer.dart`) is the parity seam this family exists
/// for.
///
/// Red-check: swap `DragAxis::Horizontal` for `DragAxis::Vertical` in
/// `GestureDetectorState::init_state`'s `horizontal_drag` recognizer — this
/// test's horizontal move no longer crosses the (now-vertical) slop, and
/// `starts`/`updates`/`ends` all stay `0`.
#[test]
fn horizontal_drag_fires_down_start_update_end_for_horizontal_motion() {
    let downs = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let updates = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(AtomicUsize::new(0));
    let (down_cb, start_cb, update_cb, end_cb) = (
        Arc::clone(&downs),
        Arc::clone(&starts),
        Arc::clone(&updates),
        Arc::clone(&ends),
    );

    let laid = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_down(move |_cx, _details| {
                down_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_start(move |_cx, _details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_update(move |_cx, _details| {
                update_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_end(move |_cx, _details| {
                end_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );

    assert_eq!(
        downs.load(Ordering::SeqCst),
        0,
        "no down before any pointer"
    );

    // Down, then a horizontal move well past the drag slop (60px > 18px),
    // a second move, then up.
    laid.dispatch_pointer_down(20.0, 100.0);
    assert_eq!(
        downs.load(Ordering::SeqCst),
        1,
        "on_horizontal_drag_down fires immediately on contact",
    );

    laid.dispatch_pointer_move(80.0, 100.0);
    laid.dispatch_pointer_move(150.0, 100.0);
    laid.dispatch_pointer_up(150.0, 100.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "the horizontal drag started exactly once"
    );
    assert!(
        updates.load(Ordering::SeqCst) >= 1,
        "the horizontal drag reported at least one update",
    );
    assert_eq!(
        ends.load(Ordering::SeqCst),
        1,
        "the horizontal drag ended exactly once on up"
    );
}

/// With a free-axis drag competing in the same presentation arena, a purely
/// vertical move must resolve to that competitor rather than the horizontal
/// recognizer. A lone recognizer wins Flutter's deferred default after Down,
/// so axis disambiguation is observable only under real competition.
///
/// Red-check: change the recognizer's axis to `DragAxis::Free` — a vertical
/// move now crosses its (any-direction) slop and `starts` becomes `1`.
#[test]
fn horizontal_drag_does_not_fire_for_purely_vertical_motion() {
    let starts = Arc::new(AtomicUsize::new(0));
    let start_cb = Arc::clone(&starts);

    let laid = lay_out(
        GestureDetector::new().on_pan_start(|_cx, _| {}).child(
            GestureDetector::new()
                .on_horizontal_drag_start(move |_cx, _details| {
                    start_cb.fetch_add(1, Ordering::SeqCst);
                })
                .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        ),
        tight(200.0, 200.0),
    );

    laid.dispatch_pointer_down(100.0, 20.0);
    laid.dispatch_pointer_move(100.0, 90.0);
    laid.dispatch_pointer_up(100.0, 90.0);

    assert_eq!(
        starts.load(Ordering::SeqCst),
        0,
        "a purely vertical move must not start a horizontal drag",
    );
}

/// Flutter parity: once a drag has won its arena, `PointerCancel` follows
/// `didStopTrackingLastPointer`'s accepted branch and fires `onEnd`, not
/// `onCancel`. The terminal event must still leave the recognizer reusable.
#[test]
fn horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector() {
    let cancels = Arc::new(AtomicUsize::new(0));
    let ends = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (cancel_cb, end_cb, start_cb) =
        (Arc::clone(&cancels), Arc::clone(&ends), Arc::clone(&starts));

    let laid = lay_out(
        GestureDetector::new()
            .on_horizontal_drag_start(move |_cx, _details| {
                start_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_cancel(move |_cx| {
                cancel_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_horizontal_drag_end(move |_cx, _details| {
                end_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );

    laid.dispatch_pointer_down(20.0, 100.0);
    laid.dispatch_pointer_move(80.0, 100.0);
    assert_eq!(starts.load(Ordering::SeqCst), 1, "the drag started");

    laid.dispatch_pointer_cancel();
    assert_eq!(
        ends.load(Ordering::SeqCst),
        1,
        "a PointerCancel after arena acceptance ends the active drag"
    );
    assert_eq!(
        cancels.load(Ordering::SeqCst),
        0,
        "on_horizontal_drag_cancel is reserved for a sequence rejected before acceptance"
    );

    // A fresh contact afterward still completes normally.
    laid.dispatch_pointer_down(20.0, 100.0);
    laid.dispatch_pointer_move(80.0, 100.0);
    laid.dispatch_pointer_up(80.0, 100.0);
    assert_eq!(
        starts.load(Ordering::SeqCst),
        2,
        "a drag after a cancel still starts (the cancel did not wedge the recognizer)",
    );
    assert_eq!(ends.load(Ordering::SeqCst), 2);
}

/// A cancel while a tap competitor still keeps the drag unaccepted follows
/// the possible branch and fires `on_horizontal_drag_cancel`.
#[test]
fn horizontal_drag_cancel_before_acceptance_fires_cancel() {
    let cancels = Arc::new(AtomicUsize::new(0));
    let cancels_for_callback = Arc::clone(&cancels);
    let laid = lay_out(
        GestureDetector::new()
            .on_tap(|_cx| {})
            .on_horizontal_drag_cancel(move |_cx| {
                cancels_for_callback.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(200.0, 200.0),
    );

    laid.dispatch_pointer_down(20.0, 100.0);
    laid.dispatch_pointer_cancel();

    assert_eq!(cancels.load(Ordering::SeqCst), 1);
}

#[test]
fn primary_tap_does_not_fire_on_secondary_tap() {
    let primary_taps = Arc::new(AtomicUsize::new(0));
    let secondary_taps = Arc::new(AtomicUsize::new(0));
    let (primary_cb, secondary_cb) = (Arc::clone(&primary_taps), Arc::clone(&secondary_taps));

    let laid = lay_out(
        GestureDetector::new()
            .on_tap(move |_cx| {
                primary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .on_secondary_tap(move |_cx| {
                secondary_cb.fetch_add(1, Ordering::SeqCst);
            })
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );

    // A primary-button tap fires on_tap and must NOT fire on_secondary_tap.
    laid.dispatch_pointer_down(50.0, 50.0);
    laid.dispatch_pointer_up(50.0, 50.0);

    assert_eq!(
        primary_taps.load(Ordering::SeqCst),
        1,
        "a primary down+up fires on_tap exactly once",
    );
    assert_eq!(
        secondary_taps.load(Ordering::SeqCst),
        0,
        "a primary tap must NOT fire on_secondary_tap",
    );
}

// ============================================================================
// Event context (ADR-0086): every callback receives `&mut EventCx<'_>` and
// writes a signal through it; the write rebuilds the signal's reader.
// ============================================================================

mod event_cx {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use crate::common::harness::{
        PostFrameCapability, TextInputCapability, mount_with_capabilities,
    };
    use crate::common::{LaidOut, ProbeSignals, SignalProbe, lay_out, tight};
    use flui_interaction::{DragEndDetails, DragUpdateDetails};
    use flui_rendering::pipeline::PipelineCell;
    use flui_testing::{A11yTree, Action, ActionRequest, TreeId, invoke_semantics_action};
    use flui_types::Color;
    use flui_view::prelude::*;
    use flui_widgets::{ColoredBox, GestureDetector, Semantics, Text};

    fn target() -> ColoredBox {
        ColoredBox::new(Color::rgb(10, 20, 30))
    }

    /// A tap at the centre of a 100x100 detector.
    fn tap(app: &LaidOut) {
        app.dispatch_pointer_down(50.0, 50.0);
        app.dispatch_pointer_up(50.0, 50.0);
    }

    #[test]
    fn a_tap_writes_a_signal_and_rebuilds_its_reader() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            GestureDetector::new()
                .on_tap(move |cx| count.update(cx, |n| *n += 1))
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));

        tap(&app);
        assert_eq!(probe.value(), Ok(1));
        app.tick();

        assert_eq!(
            probe.reads(),
            [0, 1],
            "the reader rebuilt once, with the new value"
        );
    }

    #[test]
    fn a_long_press_on_the_virtual_clock_writes_a_signal() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            GestureDetector::new()
                .on_long_press(move |cx| count.set(cx, 5))
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));

        app.dispatch_pointer_down(50.0, 50.0);
        app.pump_for(Duration::from_millis(300));
        assert_eq!(probe.value(), Ok(0), "not before the hold deadline");
        app.pump_for(Duration::from_millis(300));

        assert_eq!(probe.value(), Ok(5));
        app.tick();
        assert_eq!(probe.reads().last(), Some(&5), "the reader rebuilt");
    }

    #[test]
    fn a_double_tap_down_writes_the_position_it_carries() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            GestureDetector::new()
                .on_double_tap_down(move |cx, details| {
                    count.set(cx, details.local_position.dx.get() as u32)
                })
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));

        app.dispatch_pointer_down(40.0, 50.0);
        app.dispatch_pointer_up(40.0, 50.0);
        app.pump_for(Duration::from_millis(50));
        app.dispatch_pointer_down(40.0, 50.0);

        assert_eq!(probe.value(), Ok(40));
        app.tick();
        assert_eq!(probe.reads().last(), Some(&40));
    }

    #[test]
    fn a_pan_update_writes_a_signal_per_update() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            GestureDetector::new()
                .on_pan_update(move |cx, details: DragUpdateDetails| {
                    count.update(cx, |n| *n += details.delta.dy.get() as u32)
                })
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));

        app.dispatch_pointer_down(50.0, 10.0);
        app.dispatch_pointer_move(50.0, 50.0);
        app.dispatch_pointer_move(50.0, 90.0);
        app.dispatch_pointer_up(50.0, 90.0);

        let moved = probe.value().expect("the probe's signal is live");
        assert!(moved > 0, "each update added its delta, got {moved}");
        app.tick();
        assert_eq!(probe.reads(), [0, moved], "one rebuild after the drag");
    }

    #[test]
    fn a_horizontal_drag_end_writes_a_signal() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            GestureDetector::new()
                .on_horizontal_drag_end(move |cx, _details: DragEndDetails| count.set(cx, 1))
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(200.0, 200.0));

        app.dispatch_pointer_down(20.0, 100.0);
        app.dispatch_pointer_move(80.0, 100.0);
        app.dispatch_pointer_move(150.0, 100.0);
        assert_eq!(probe.value(), Ok(0), "nothing before the drag ends");
        app.dispatch_pointer_up(150.0, 100.0);

        assert_eq!(probe.value(), Ok(1));
        app.tick();
        assert_eq!(probe.reads(), [0, 1]);
    }

    #[test]
    fn a_let_bound_pan_callback_compiles_through_callback_with() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            let on_update =
                callback_with(move |cx, _details: DragUpdateDetails| count.update(cx, |n| *n += 1));
            GestureDetector::new()
                .on_pan_update(on_update)
                .child(target())
        });
        let app = lay_out(probe.view(), tight(100.0, 100.0));

        app.dispatch_pointer_down(50.0, 10.0);
        app.dispatch_pointer_move(50.0, 90.0);
        app.dispatch_pointer_up(50.0, 90.0);

        assert!(probe.value().expect("live") >= 1);
    }

    #[test]
    fn a_refused_write_in_a_tap_is_reported_not_panicked() {
        let probe = SignalProbe::new(|ProbeSignals { released, .. }| {
            GestureDetector::new()
                .on_tap(move |cx| released.set(cx, 1))
                .child(target())
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));

        let ((), log) = flui_testing::log_capture::capture(|| tap(&app));

        assert!(
            log.contains("an event callback's signal write was refused"),
            "the refusal is logged at the dispatch boundary: {log}"
        );
        app.tick();
        assert_eq!(probe.value(), Ok(0), "other state is intact");
    }

    /// The detector wrapped so that its semantics actions merge into one
    /// node labelled `Tap`.
    fn labelled(detector: GestureDetector) -> Semantics {
        Semantics::new()
            .container(true)
            .child(detector.child(Text::new("Tap")))
    }

    fn click(pipeline_owner: &PipelineCell, tree: &A11yTree) {
        invoke_labelled_action(pipeline_owner, tree, Action::Click);
    }

    fn invoke_labelled_action(pipeline_owner: &PipelineCell, tree: &A11yTree, action: Action) {
        let id = tree
            .find_by_label("Tap")
            .unwrap_or_else(|error| {
                panic!("one node labelled \"Tap\": {error}\n{}", tree.describe())
            })
            .id();
        invoke_semantics_action(
            pipeline_owner,
            ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: None,
            },
        )
        .expect("a click on a node advertising one resolves");
    }

    #[test]
    fn an_assistive_tap_writes_after_the_frame() {
        let probe = SignalProbe::new(|ProbeSignals { count, .. }| {
            labelled(GestureDetector::new().on_tap(move |cx| count.update(cx, |n| *n += 1)))
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));
        app.enable_semantics();
        app.pump();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");

        click(&app.pipeline_owner(), &tree);
        assert_eq!(
            probe.value(),
            Ok(0),
            "the request is recorded, not performed inline"
        );
        app.tick();

        assert_eq!(
            probe.value(),
            Ok(1),
            "the tap ran after the frame, outside any build, so the write was accepted"
        );
        app.tick();
        assert_eq!(probe.reads().last(), Some(&1), "and its reader rebuilt");
    }

    #[test]
    fn accepted_assistive_taps_preserve_the_multiplicity_of_pointer_taps() {
        let calls = Rc::new(Cell::new(0));
        let callback_calls = Rc::clone(&calls);
        let mut app = lay_out(
            labelled(GestureDetector::new().on_tap(move |_cx| {
                callback_calls.set(callback_calls.get() + 1);
            })),
            tight(100.0, 100.0),
        );

        tap(&app);
        tap(&app);
        assert_eq!(
            calls.get(),
            2,
            "two ordinary pointer activations invoke the callback twice"
        );

        app.enable_semantics();
        app.pump();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");
        click(&app.pipeline_owner(), &tree);
        click(&app.pipeline_owner(), &tree);
        assert_eq!(
            calls.get(),
            2,
            "accepted semantics actions are deferred until the next frame"
        );

        app.tick();

        assert_eq!(
            calls.get(),
            4,
            "two accepted semantic Click actions are two activations, just like two pointer taps"
        );
    }

    #[test]
    fn accepted_assistive_actions_preserve_ingress_order_across_action_kinds() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let tap_calls = Rc::clone(&calls);
        let long_press_calls = Rc::clone(&calls);
        let mut app = lay_out(
            labelled(
                GestureDetector::new()
                    .on_tap(move |_cx| tap_calls.borrow_mut().push("tap"))
                    .on_long_press(move |_cx| long_press_calls.borrow_mut().push("long-press")),
            ),
            tight(100.0, 100.0),
        );
        app.enable_semantics();
        app.pump();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");
        let owner = app.pipeline_owner();

        invoke_labelled_action(&owner, &tree, Action::Click);
        invoke_labelled_action(&owner, &tree, Action::ShowContextMenu);
        invoke_labelled_action(&owner, &tree, Action::Click);
        app.tick();

        assert_eq!(
            calls.borrow().as_slice(),
            ["tap", "long-press", "tap"],
            "accepted commands retain their cross-action ingress order"
        );
    }

    #[test]
    fn a_panicking_assistive_action_does_not_discard_the_fifo_tail() {
        let long_press_calls = Rc::new(Cell::new(0));
        let observed_long_press = Rc::clone(&long_press_calls);
        let mut app = lay_out(
            labelled(
                GestureDetector::new()
                    .on_tap(|_cx| -> () { panic!("intentional assistive tap panic") })
                    .on_long_press(move |_cx| {
                        observed_long_press.set(observed_long_press.get() + 1);
                    }),
            ),
            tight(100.0, 100.0),
        );
        app.enable_semantics();
        app.pump();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");
        let owner = app.pipeline_owner();
        invoke_labelled_action(&owner, &tree, Action::Click);
        invoke_labelled_action(&owner, &tree, Action::ShowContextMenu);

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.tick()));
        assert!(
            panicked.is_err(),
            "the user callback panic still propagates"
        );
        assert_eq!(
            long_press_calls.get(),
            0,
            "the tail has not run out of order"
        );

        app.tick();
        assert_eq!(
            long_press_calls.get(),
            1,
            "scheduler recovery retains the next accepted command"
        );
    }

    #[test]
    fn an_assistive_tap_uses_the_replacement_callback() {
        let old = Rc::new(Cell::new(0));
        let old_callback = Rc::clone(&old);
        let mut app = lay_out(
            labelled(
                GestureDetector::new().on_tap(move |_cx| old_callback.set(old_callback.get() + 1)),
            ),
            tight(100.0, 100.0),
        );
        app.enable_semantics();
        app.pump();
        click(
            &app.pipeline_owner(),
            &app.a11y_tree().expect("semantics tree"),
        );
        let current = Rc::new(Cell::new(0));
        let current_callback = Rc::clone(&current);
        app.pump_widget(labelled(GestureDetector::new().on_tap(move |_cx| {
            current_callback.set(current_callback.get() + 1);
        })));
        assert_eq!(old.get(), 0);
        assert_eq!(current.get(), 1);
    }

    #[test]
    fn an_assistive_tap_is_cancelled_when_its_handler_is_removed() {
        let calls = Rc::new(Cell::new(0));
        let callback_calls = Rc::clone(&calls);
        let mut app = lay_out(
            labelled(
                GestureDetector::new()
                    .on_tap(move |_cx| callback_calls.set(callback_calls.get() + 1)),
            ),
            tight(100.0, 100.0),
        );
        app.enable_semantics();
        app.pump();
        click(
            &app.pipeline_owner(),
            &app.a11y_tree().expect("semantics tree"),
        );
        app.pump_widget(labelled(GestureDetector::new()));
        assert_eq!(calls.get(), 0);
    }

    /// Without a local post-frame lane there is no moment after the frame to
    /// run the activation in. Running it at once would run it inside the
    /// detector's `build`, where its writes are refused; the detector drops it
    /// with a warning instead.
    #[test]
    fn without_a_local_post_frame_lane_an_assistive_tap_is_dropped_not_run_in_build() {
        let ran = Rc::new(Cell::new(0_u32));
        let ran_in_tap = Rc::clone(&ran);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            let ran = Rc::clone(&ran_in_tap);
            labelled(GestureDetector::new().on_tap(move |cx| {
                ran.set(ran.get() + 1);
                count.set(cx, 1)
            }))
        });
        let mut app = mount_with_capabilities(
            probe.view(),
            PostFrameCapability::Absent,
            TextInputCapability::Absent,
        );
        app.enable_semantics();
        app.tick();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");
        click(&app.pipeline_owner(), &tree);

        let ((), log) = flui_testing::log_capture::capture(|| app.tick());

        assert_eq!(ran.get(), 0, "the callback never ran: {log}");
        assert!(
            log.contains("dropping an assistive-technology activation"),
            "the drop is logged: {log}"
        );
        assert!(
            !log.contains("signal write was refused"),
            "nothing was written inside build: {log}"
        );
        assert_eq!(probe.value(), Ok(0));
    }
}
