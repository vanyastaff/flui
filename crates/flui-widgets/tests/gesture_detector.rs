//! End-to-end gesture recognition: a `GestureDetector`'s `on_tap` fires when the
//! user taps (pointer down then up) its child. Drives the real recognizer +
//! presentation-owned arena through the hit-test + binding dispatch path. The
//! binding alone closes and sweeps the shared arena.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{lay_out, tight};
use flui_painting::styling::Color;
use flui_widgets::{ColoredBox, GestureDetector};

#[derive(Clone, flui_view::prelude::StatefulView)]
struct ConfiguredGesture {
    settings: flui_interaction::GestureSettings,
    detector: GestureDetector,
}

struct ConfiguredGestureState {
    arena: Option<flui_interaction::GestureArena>,
}

impl flui_view::StatefulView for ConfiguredGesture {
    type State = ConfiguredGestureState;

    fn create_state(&self) -> Self::State {
        ConfiguredGestureState { arena: None }
    }
}

impl flui_view::ViewState<ConfiguredGesture> for ConfiguredGestureState {
    fn init_state(&mut self, ctx: &dyn flui_view::LifecycleContext) {
        self.arena = Some(flui_widgets::GestureArenaScope::of(ctx));
    }

    fn build(
        &self,
        view: &ConfiguredGesture,
        _: &dyn flui_view::BuildContext,
    ) -> impl flui_view::IntoView {
        flui_widgets::GestureArenaScope::new(
            self.arena
                .as_ref()
                .expect("mounted presentation arena")
                .clone(),
            view.detector.clone(),
        )
        .settings(view.settings.clone())
    }
}

pub(crate) fn scoped_settings_control_touch_recognition_thresholds() {
    use std::{cell::Cell, rc::Rc};

    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{GestureSettings, PointerId};

    for family in ["tap", "pan", "horizontal drag"] {
        for configured in [false, true] {
            let callbacks = Rc::new(Cell::new(0));
            let observed = Rc::clone(&callbacks);
            let detector = match family {
                "tap" => GestureDetector::new().on_tap(move |_| observed.set(observed.get() + 1)),
                "pan" => GestureDetector::new()
                    .on_pan_start(move |_, _| observed.set(observed.get() + 1)),
                _ => GestureDetector::new()
                    .on_horizontal_drag_start(move |_, _| observed.set(observed.get() + 1)),
            }
            .child(ColoredBox::new(Color::rgb(10, 20, 30)));
            let settings = if configured {
                GestureSettings::default()
                    .try_with_touch_slop(60.0)
                    .expect("valid touch slop")
                    .try_with_pan_slop(10.0)
                    .expect("valid pan slop")
                    .try_with_pan_slop_horizontal(10.0)
                    .expect("valid horizontal slop")
            } else {
                GestureSettings::default()
            };
            let laid = lay_out(
                ConfiguredGesture { settings, detector },
                tight(150.0, 100.0),
            );
            let pointer = PointerId::try_from(1_u64).expect("authored touch contact");
            let start = Offset::new(40.0, 40.0);
            let end = Offset::new(if family == "tap" { 70.0 } else { 52.0 }, 40.0);
            for event in [
                make_down_event_for_id(pointer, start, PointerKind::Touch).expect("finite Down"),
                make_move_event_for_id(pointer, end, PointerKind::Touch).expect("finite Move"),
                make_up_event_for_id(pointer, end, PointerKind::Touch).expect("finite Up"),
            ] {
                laid.dispatch_pointer_event(&event);
            }
            assert_eq!(
                callbacks.get(),
                usize::from(configured),
                "{family}, configured={configured}"
            );
        }
    }
}

pub(crate) fn clearing_pan_callbacks_mid_drag_still_finishes_the_drag() {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_view::SignalWriteExt;
    use std::{cell::Cell, rc::Rc};

    let enabled = Rc::new(Cell::new(true));
    let starts = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let (gate, started, updated, ended) = (
        Rc::clone(&enabled),
        Rc::clone(&starts),
        Rc::clone(&updates),
        Rc::clone(&ends),
    );
    let signal = Rc::new(Cell::new(None));
    let remembered = Rc::clone(&signal);
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        let detector = GestureDetector::new();
        let detector = if gate.get() {
            let (started, updated, ended) =
                (Rc::clone(&started), Rc::clone(&updated), Rc::clone(&ended));
            detector
                .on_pan_start(move |_, _| started.set(started.get() + 1))
                .on_pan_update(move |_, _| updated.set(updated.get() + 1))
                .on_pan_end(move |_, _| ended.set(ended.get() + 1))
        } else {
            detector
        };
        detector.child(ColoredBox::new(Color::rgb(10, 20, 30)))
    });
    let mut laid = lay_out(probe.view(), tight(100.0, 100.0));
    let contacts = flui_testing::widgets::PointerContacts::new();
    let pointer = contacts.begin();
    laid.dispatch_pointer_event(
        &make_down_event_for_id(pointer, Offset::new(50.0, 10.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    laid.dispatch_pointer_event(
        &make_move_event_for_id(pointer, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(starts.get(), 1);
    enabled.set(false);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("write");
    laid.pump();
    laid.dispatch_pointer_event(
        &make_up_event_for_id(pointer, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(ends.get(), 0, "removed callbacks are not invoked");
    enabled.set(true);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 2))
        .expect("write");
    laid.pump();
    let before = updates.get();
    // Deliberately replay a stale sample for the released identity through the
    // public host boundary; the convenience Move helper requires a live Down.
    laid.dispatch_pointer_event(
        &make_move_event_for_id(pointer, Offset::new(50.0, 60.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(
        updates.get(),
        before,
        "the released contact cannot resume when callbacks return"
    );
    let fresh = contacts.begin();
    laid.dispatch_pointer_event(
        &make_down_event_for_id(fresh, Offset::new(50.0, 10.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    laid.dispatch_pointer_event(
        &make_move_event_for_id(fresh, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    laid.dispatch_pointer_event(
        &make_up_event_for_id(fresh, Offset::new(50.0, 50.0), PointerKind::Mouse)
            .expect("finite pointer fixture"),
    );
    assert_eq!(starts.get(), 2);
    assert_eq!(ends.get(), 1);
}

pub(crate) fn unmount_mid_drag_cancels_once_and_hands_the_arena_to_the_rival() {
    use crate::common::{ProbeSignals, SignalProbe};
    use flui_view::{IntoView, SignalWriteExt, ViewExt};
    use std::{cell::Cell, rc::Rc};

    let mounted = Rc::new(Cell::new(true));
    let cancelled = Rc::new(Cell::new(0));
    let rival_starts = Rc::new(Cell::new(0));
    let rival_ends = Rc::new(Cell::new(0));
    let signal = Rc::new(Cell::new(None));
    let (gate, cancels, remembered) = (
        Rc::clone(&mounted),
        Rc::clone(&cancelled),
        Rc::clone(&signal),
    );
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        let child = ColoredBox::new(Color::rgb(10, 20, 30));
        if gate.get() {
            let cancels = Rc::clone(&cancels);
            GestureDetector::new()
                .on_horizontal_drag_start(|_, _| -> () {
                    panic!("the retired contender must not start")
                })
                .on_horizontal_drag_cancel(move |_| cancels.set(cancels.get() + 1))
                .child(child)
                .into_view()
                .boxed()
        } else {
            child.into_view().boxed()
        }
    });
    let (started, ended) = (Rc::clone(&rival_starts), Rc::clone(&rival_ends));
    let mut laid = lay_out(
        GestureDetector::new()
            .on_pan_start(move |_, _| started.set(started.get() + 1))
            .on_pan_end(move |_, _| ended.set(ended.get() + 1))
            .child(probe.view()),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_down(10.0, 50.0);
    mounted.set(false);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("write");
    laid.pump();
    assert_eq!(
        cancelled.get(),
        1,
        "unmount explicitly cancels the admitted contender"
    );
    laid.dispatch_pointer_move(60.0, 50.0);
    laid.dispatch_pointer_up(60.0, 50.0);
    assert_eq!(
        rival_starts.get(),
        1,
        "the remaining live recognizer wins the contact"
    );
    assert_eq!(rival_ends.get(), 1);
    assert_eq!(
        cancelled.get(),
        1,
        "the cached terminal route cannot cancel the retired owner twice"
    );
}

pub(crate) fn gesture_detector_fires_on_tap_for_a_down_up_on_the_child() {
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

/// The gesture arena's rule is that the first member to accept, or the last
/// member not to reject, wins. A drag past the slop makes the tap recognizer
/// reject itself, leaving the pan recognizer as the last remaining (and thus
/// winning) member.
pub(crate) fn gesture_detector_recognizes_a_pan_and_suppresses_the_tap() {
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

/// Once a drag has won its arena, `PointerCancel` takes the accepted branch
/// and fires `onEnd`, not `onCancel`. The terminal event must still leave the recognizer reusable.
pub(crate) fn horizontal_drag_pointer_cancel_after_acceptance_ends_and_does_not_wedge_the_detector()
{
    let reasons = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let recorded = std::rc::Rc::clone(&reasons);
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
            .on_horizontal_drag_end(move |_cx, details| {
                recorded.borrow_mut().push(details.reason);
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
    assert_eq!(
        *reasons.borrow(),
        [
            flui_widgets::GestureEndReason::Cancelled,
            flui_widgets::GestureEndReason::Completed
        ]
    );
}

// ============================================================================
// Event context (ADR-0086): every callback receives `&mut EventCx<'_>` and
// writes a signal through it; the write rebuilds the signal's reader.
// ============================================================================

pub(crate) mod event_cx {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::common::{LaidOut, ProbeSignals, SignalProbe, lay_out, tight};

    use flui_painting::styling::Color;
    use flui_testing::{A11yTree, Action, ActionRequest, TreeId};
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

    pub(crate) fn a_tap_writes_a_signal_and_rebuilds_its_reader() {
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

    /// The detector wrapped so that its semantics actions merge into one
    /// node labelled `Tap`.
    fn labelled(detector: GestureDetector) -> Semantics {
        Semantics::new()
            .container(true)
            .child(detector.child(Text::new("Tap")))
    }

    fn invoke_labelled_action(app: &LaidOut, tree: &A11yTree, action: Action) {
        let id = tree
            .find_by_label("Tap")
            .unwrap_or_else(|error| {
                panic!("one node labelled \"Tap\": {error}\n{}", tree.describe())
            })
            .id();
        app.invoke_semantics_action(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: id,
            data: None,
        })
        .expect("a click on a node advertising one resolves");
    }

    pub(crate) fn repeated_assistive_taps_are_delivered_once_each_and_keep_making_progress() {
        assert_repeated_actions_are_delivered_once_each(Action::Click);
    }

    pub(crate) fn repeated_assistive_long_presses_are_delivered_once_each_and_keep_making_progress()
    {
        assert_repeated_actions_are_delivered_once_each(Action::ShowContextMenu);
    }

    /// Two accepted `action`s of one kind run their handler twice on the next
    /// frame, never again on a later one, and a third still arrives after the
    /// batch drains. The detector advertises both kinds, so only the handler
    /// `action` reaches counts.
    fn assert_repeated_actions_are_delivered_once_each(action: Action) {
        let long_press = matches!(action, Action::ShowContextMenu);
        let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
            let detector = GestureDetector::new();
            let detector = if long_press {
                detector
                    .on_tap(|_cx| {})
                    .on_long_press(move |cx| count.update(cx, |n| *n += 1))
            } else {
                detector
                    .on_tap(move |cx| count.update(cx, |n| *n += 1))
                    .on_long_press(|_cx| {})
            };
            labelled(detector)
        });
        let mut app = lay_out(probe.view(), tight(100.0, 100.0));
        app.enable_semantics();
        app.pump();
        let tree = app.a11y_tree().expect("semantics enabled before the frame");

        invoke_labelled_action(&app, &tree, action);
        invoke_labelled_action(&app, &tree, action);
        assert_eq!(probe.value(), Ok(0), "accepted actions are deferred");

        app.tick();
        assert_eq!(
            probe.value(),
            Ok(2),
            "coalescing wake demand must not coalesce two accepted activations"
        );
        app.tick();
        assert_eq!(probe.reads().last(), Some(&2), "the signal reader rebuilt");
        assert_eq!(
            probe.value(),
            Ok(2),
            "a later frame must not replay either action"
        );

        let tree = app.a11y_tree().expect("semantics remains available");
        invoke_labelled_action(&app, &tree, action);
        assert_eq!(probe.value(), Ok(2), "the next activation is deferred too");
        app.tick();
        assert_eq!(
            probe.value(),
            Ok(3),
            "delivery remains live after draining a batch"
        );
        app.tick();
        assert_eq!(
            probe.reads().last(),
            Some(&3),
            "the later write also rebuilds"
        );
        assert_eq!(
            probe.value(),
            Ok(3),
            "the later action is delivered only once"
        );
    }

    pub(crate) fn a_panicking_assistive_action_does_not_discard_the_fifo_tail() {
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
        invoke_labelled_action(&app, &tree, Action::Click);
        invoke_labelled_action(&app, &tree, Action::ShowContextMenu);

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
}

pub(crate) fn viewer_reports_cancelled_then_completed_interactions() {
    use flui_widgets::{GestureEndReason, InteractiveViewer};
    use std::cell::RefCell;
    use std::rc::Rc;
    let reasons = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&reasons);
    let laid = lay_out(
        InteractiveViewer::new()
            .on_interaction_end(move |_cx, details| recorded.borrow_mut().push(details.reason))
            .child(target_for_viewer()),
        tight(200.0, 200.0),
    );
    laid.dispatch_pointer_down(30.0, 80.0);
    laid.dispatch_pointer_move(100.0, 80.0);
    laid.dispatch_pointer_cancel();
    laid.dispatch_pointer_down(30.0, 80.0);
    laid.dispatch_pointer_move(100.0, 80.0);
    laid.dispatch_pointer_up(100.0, 80.0);
    assert_eq!(
        *reasons.borrow(),
        [GestureEndReason::Cancelled, GestureEndReason::Completed]
    );
}

fn target_for_viewer() -> ColoredBox {
    ColoredBox::new(Color::rgb(10, 20, 30))
}
