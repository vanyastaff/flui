//! `Draggable`'s callbacks run inside a write the widget's `WriterSource`
//! opens (ADR-0086): each receives an `EventCx`, writes signals, and a
//! refused write is reported rather than panicked. An unmount mid-drag
//! cancels from `finalize_tree`, outside any build, so its callbacks' writes
//! land too.

use std::cell::Cell;
use std::rc::Rc;

use flui_interaction::DragUpdateDetails;
use flui_painting::styling::Color;
use flui_view::prelude::*;
use flui_widgets::{
    ColoredBox, DragTarget, DragTargetDetails, Draggable, DraggableCanceledDetails,
    DraggableDetails, InsertPosition, Overlay, OverlayEntry, OverlayHandle, SizedBox, Text,
};

use crate::common::{LaidOut, ProbeSignals, SignalProbe, lay_out, tight};

/// How a test wires the draggable's callbacks to the probe's signals.
type Configure = Rc<dyn Fn(Draggable<u32>, ProbeSignals) -> Draggable<u32>>;

pub(crate) fn draggable_reads_admission_profiles_and_retires_authored_owners() {
    crate::common::cases::run_cases(
        "mounted draggable policy",
        &[
            (
                "live admission snapshot",
                draggable_retains_down_profile_and_refreshes_next_contact,
            ),
            (
                "authored replacement",
                draggable_cancels_authored_replacement_before_stale_terminal,
            ),
        ],
    );
}

fn touch(laid: &LaidOut, id: u64, y: f64, phase: u8) {
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    let pointer = flui_interaction::PointerId::try_from(id).expect("nonzero touch");
    let position = Offset::new(50.0, y);
    let event = match phase {
        0 => make_down_event_for_id(pointer, position, PointerKind::Touch),
        1 => make_move_event_for_id(pointer, position, PointerKind::Touch),
        2 => make_up_event_for_id(pointer, position, PointerKind::Touch),
        _ => unreachable!("scripted touch phase"),
    }
    .expect("finite touch fixture");
    laid.dispatch_pointer_event(&event);
}

fn draggable_retains_down_profile_and_refreshes_next_contact() {
    use flui_interaction::{GestureSettings, GestureSettingsSource};
    use flui_widgets::GestureDetector;
    let profile = |slop| {
        GestureSettings::default()
            .try_with_touch_slop(slop)
            .expect("finite slop")
    };
    let source = GestureSettingsSource::new(profile(20.0));
    let starts = Rc::new(Cell::new(0));
    let started = starts.clone();
    let laid = lay_out(
        crate::gesture_settings::SettingsScope::new(
            source.provider(),
            GestureDetector::new().on_tap(|_| {}).child(
                Draggable::new(ColoredBox::new(Color::RED))
                    .data(7_u32)
                    .on_drag_started(move |_| started.set(started.get() + 1)),
            ),
        ),
        tight(400.0, 400.0),
    );
    touch(&laid, 1, 50.0, 0);
    source.replace(profile(100.0));
    touch(&laid, 1, 90.0, 1);
    assert_eq!(
        starts.get(),
        1,
        "active contact retains its admitted short slop"
    );
    touch(&laid, 1, 90.0, 2);
    touch(&laid, 2, 50.0, 0);
    touch(&laid, 2, 90.0, 1);
    assert_eq!(starts.get(), 1, "next contact reads the updated large slop");
    touch(&laid, 2, 170.0, 1);
    assert_eq!(
        starts.get(),
        2,
        "new threshold still permits a deliberate drag"
    );
    touch(&laid, 2, 170.0, 2);
}

fn draggable_cancels_authored_replacement_before_stale_terminal() {
    use flui_interaction::GestureSettings;
    use flui_widgets::GestureDetector;
    let threshold = Rc::new(Cell::new(20.0));
    let starts = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let cancels = Rc::new(Cell::new(0));
    let signal = Rc::new(Cell::new(None));
    let (profile, started, ended, cancelled, remembered) = (
        threshold.clone(),
        starts.clone(),
        ends.clone(),
        cancels.clone(),
        signal.clone(),
    );
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        let (started, ended, cancelled) = (started.clone(), ended.clone(), cancelled.clone());
        crate::gesture_settings::SettingsScope::new(
            GestureSettings::default()
                .try_with_touch_slop(profile.get())
                .expect("finite slop"),
            GestureDetector::new().on_tap(|_| {}).child(
                Draggable::new(ColoredBox::new(Color::RED))
                    .data(7_u32)
                    .on_drag_started(move |_| started.set(started.get() + 1))
                    .on_drag_end(move |_, _| ended.set(ended.get() + 1))
                    .on_draggable_canceled(move |_, _| cancelled.set(cancelled.get() + 1)),
            ),
        )
    });
    let mut laid = lay_out(probe.view(), tight(400.0, 400.0));
    touch(&laid, 1, 50.0, 0);
    touch(&laid, 1, 90.0, 1);
    assert_eq!(starts.get(), 1);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("publish equal authored draggable policy");
    laid.pump();
    assert_eq!(
        (ends.get(), cancels.get()),
        (0, 0),
        "equal policy keeps the accepted drag"
    );
    threshold.set(100.0);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 2))
        .expect("replace authored draggable policy");
    laid.pump();
    assert_eq!(
        (ends.get(), cancels.get()),
        (1, 1),
        "outgoing drag is cancelled once"
    );
    touch(&laid, 1, 90.0, 2);
    assert_eq!(ends.get(), 1, "stale release cannot end a replacement");
    touch(&laid, 2, 50.0, 0);
    touch(&laid, 2, 90.0, 1);
    assert_eq!(starts.get(), 1, "replacement keeps the authored threshold");
    touch(&laid, 2, 170.0, 1);
    touch(&laid, 2, 170.0, 2);
    assert_eq!((starts.get(), ends.get(), cancels.get()), (2, 2, 2));
}

/// A probe whose child is an `Overlay` hosting one entry: the draggable
/// under test (so its feedback has an overlay to go into), or, once `show`
/// is cleared and the entry rebuilt, nothing.
struct Rig {
    probe: SignalProbe,
    show: Rc<Cell<bool>>,
    entry: OverlayEntry,
}

fn rig(configure: impl Fn(Draggable<u32>, ProbeSignals) -> Draggable<u32> + 'static) -> Rig {
    rig_over_target(configure, None)
}

/// How a test wires a drag target's callbacks to the probe's signals.
type ConfigureTarget = Rc<dyn Fn(DragTarget<u32>, ProbeSignals) -> DragTarget<u32>>;

/// [`rig`] with the draggable built inside a `DragTarget<u32>`, so the target
/// is on the hit path under every point of the drag, the drop included.
fn target_rig(
    configure: impl Fn(Draggable<u32>, ProbeSignals) -> Draggable<u32> + 'static,
    configure_target: impl Fn(DragTarget<u32>, ProbeSignals) -> DragTarget<u32> + 'static,
) -> Rig {
    rig_over_target(configure, Some(Rc::new(configure_target)))
}

fn rig_over_target(
    configure: impl Fn(Draggable<u32>, ProbeSignals) -> Draggable<u32> + 'static,
    configure_target: Option<ConfigureTarget>,
) -> Rig {
    let configure: Configure = Rc::new(configure);
    let show = Rc::new(Cell::new(true));
    let signals: Rc<Cell<Option<ProbeSignals>>> = Rc::new(Cell::new(None));

    let entry = {
        let show = Rc::clone(&show);
        let signals = Rc::clone(&signals);
        OverlayEntry::new(move |_ctx| {
            if !show.get() {
                return SizedBox::shrink().into_view().boxed();
            }
            let signals = signals.get().expect("the probe built before its overlay");
            let configure = Rc::clone(&configure);
            let draggable = move || {
                let draggable = Draggable::new(ColoredBox::new(Color::rgb(10, 20, 30)))
                    .data(7_u32)
                    .feedback(|| Text::new("feedback").into_view().boxed());
                configure(draggable, signals).into_view().boxed()
            };
            match &configure_target {
                None => draggable(),
                Some(configure_target) => {
                    let target = DragTarget::new(move |_candidates, _rejected| draggable());
                    configure_target(target, signals).into_view().boxed()
                }
            }
        })
    };
    let handle = OverlayHandle::new();
    handle.insert(&entry, &InsertPosition::Top);

    let probe = SignalProbe::new(move |probe_signals| {
        signals.set(Some(probe_signals));
        Overlay::new(handle.clone())
    });
    Rig { probe, show, entry }
}

fn mounted(rig: &Rig) -> LaidOut {
    lay_out(rig.probe.view(), tight(400.0, 400.0))
}

/// Down, then far enough past the slop that the drag has certainly started
/// and moved.
fn start_drag(app: &LaidOut) {
    app.dispatch_pointer_down(50.0, 50.0);
    app.dispatch_pointer_move(50.0, 90.0);
    app.dispatch_pointer_move(50.0, 130.0);
}

/// `started` adds 1, each update 10, `end` 100 and `canceled` 1000, so the
/// final value says exactly which callbacks ran and how often.
#[test]
fn drag_callbacks_write_signals_and_rebuild_their_reader() {
    let rig = rig(|draggable, ProbeSignals { count, .. }| {
        draggable
            .on_drag_started(move |cx| count.update(cx, |n| *n += 1))
            .on_drag_update(move |cx, _details| count.update(cx, |n| *n += 10))
            .on_drag_end(move |cx, details: DraggableDetails| {
                assert!(!details.was_accepted, "nothing accepts a drop over nothing");
                count.update(cx, |n| *n += 100)
            })
            .on_draggable_canceled(move |cx, _details| count.update(cx, |n| *n += 1000))
            .on_drag_completed(move |cx| count.set(cx, 0))
    });
    let mut app = mounted(&rig);

    start_drag(&app);
    let during = rig.probe.value().expect("the probe's signal is live");
    assert_eq!(during % 10, 1, "on_drag_started wrote once: {during}");
    let updates = during / 10;
    assert!(updates >= 1, "on_drag_update wrote per move: {during}");

    app.dispatch_pointer_up(50.0, 130.0);
    let value = rig.probe.value().expect("the probe's signal is live");
    assert_eq!(
        value,
        1 + 10 * updates + 100 + 1000,
        "on_drag_end and on_draggable_canceled each wrote once, on_drag_completed never"
    );

    app.tick();
    assert_eq!(
        rig.probe.reads().last(),
        Some(&value),
        "the reader rebuilt with the drag's writes"
    );
}

#[test]
fn a_refused_drag_write_is_reported_not_panicked() {
    let rig = rig(|draggable, ProbeSignals { count, released }| {
        draggable
            .on_drag_update(move |cx, _details| released.set(cx, 1))
            .on_drag_end(move |cx, _details| count.set(cx, 1))
    });
    let mut app = mounted(&rig);

    let ((), log) = flui_testing::log_capture::capture(|| start_drag(&app));
    assert!(
        log.contains("an event callback's signal write was refused"),
        "the refusal is logged at the dispatch boundary: {log}"
    );

    app.dispatch_pointer_up(50.0, 130.0);
    assert_eq!(rig.probe.value(), Ok(1), "the drag still ended normally");
    app.tick();
    assert_eq!(rig.probe.reads().last(), Some(&1));
}

/// An unmount mid-drag cancels the drag from `dispose`, which runs in
/// `finalize_tree` after the build, so `on_draggable_canceled`'s write is
/// accepted rather than refused as a write during build. The feedback layer
/// the drag put into the overlay goes with it.
#[test]
fn unmounting_mid_drag_reports_cancel_through_cx() {
    let rig = rig(|draggable, ProbeSignals { count, .. }| {
        draggable.on_draggable_canceled(move |cx, details: DraggableCanceledDetails| {
            assert_eq!(details.velocity.pixels_per_second.dx, 0.0);
            count.set(cx, 5)
        })
    });
    let mut app = mounted(&rig);

    start_drag(&app);
    app.tick();
    assert!(
        app.find_text("feedback").is_some(),
        "premise: the drag's feedback is showing in the overlay"
    );

    rig.show.set(false);
    rig.entry.mark_needs_build();
    let ((), log) = flui_testing::log_capture::capture(|| app.tick());

    assert_eq!(
        rig.probe.value(),
        Ok(5),
        "the cancel wrote through its cx: {log}"
    );
    assert!(
        !log.contains("refused"),
        "the unmount-time write was not refused: {log}"
    );
    app.tick();
    assert!(
        app.find_text("feedback").is_none(),
        "the feedback layer left with the draggable"
    );
    assert_eq!(rig.probe.reads().last(), Some(&5), "the reader rebuilt");
}

/// Closures bound with `let` before they reach a setter name their value
/// through `callback_with`, including the single cancel value.
#[test]
fn a_let_bound_drag_callback_compiles_through_callback_with() {
    let rig = rig(|draggable, ProbeSignals { count, .. }| {
        let on_update =
            callback_with(move |cx, _details: DragUpdateDetails| count.update(cx, |n| *n += 1));
        let on_canceled = callback_with(move |cx, canceled: DraggableCanceledDetails| {
            count.update(cx, |n| *n += 100 + canceled.offset.dy as u32)
        });
        draggable
            .on_drag_update(on_update)
            .on_draggable_canceled(on_canceled)
    });
    let app = mounted(&rig);

    start_drag(&app);
    let updates = rig.probe.value().expect("live");
    assert!(updates >= 1, "the update closure ran");
    app.dispatch_pointer_up(50.0, 130.0);

    let value = rig.probe.value().expect("live");
    assert!(
        value > updates + 100,
        "the cancel closure ran with the drag's displacement: {value}"
    );
}

/// A drop onto a target writes through the target's `on_accept` `EventCx`,
/// and does so before the draggable's own completion callback. `on_accept`
/// sets 1 and `on_drag_completed` multiplies
/// by 10, so only that order, with both writes landing, leaves 10.
pub(crate) fn a_drop_writes_through_the_targets_on_accept_before_the_draggable_completes() {
    let rig = target_rig(
        |draggable, ProbeSignals { count, .. }| {
            draggable.on_drag_completed(move |cx| count.update(cx, |n| *n *= 10))
        },
        |target, ProbeSignals { count, .. }| {
            target.on_accept(move |cx, details: DragTargetDetails<u32>| {
                assert_eq!(details.data, 7, "the target receives the drag's data");
                count.set(cx, 1)
            })
        },
    );
    let mut app = mounted(&rig);

    start_drag(&app);
    app.dispatch_pointer_up(50.0, 130.0);
    assert_eq!(
        rig.probe.value(),
        Ok(10),
        "on_accept wrote, then on_drag_completed"
    );

    app.tick();
    assert_eq!(rig.probe.reads().last(), Some(&10), "the reader rebuilt");
}

/// `on_move` fires for a drag over a target that refused it, and `on_leave`
/// when that drag ends there; both write through their `EventCx`. The veto
/// is an owner-local query: it captures an `Rc`.
pub(crate) fn a_drag_leaving_a_target_writes_through_on_leave_and_on_move() {
    let vetoes = Rc::new(Cell::new(0_u32));
    let asked = Rc::clone(&vetoes);
    let rig = target_rig(
        |draggable, _signals| draggable,
        move |target, ProbeSignals { count, .. }| {
            let asked = Rc::clone(&asked);
            target
                .on_will_accept(move |_details| {
                    asked.set(asked.get() + 1);
                    false
                })
                .on_move(move |cx, _details| count.update(cx, |n| *n += 1))
                .on_leave(move |cx, data: Option<u32>| {
                    assert_eq!(data, Some(7), "the leaving drag's data");
                    count.update(cx, |n| *n += 1000)
                })
        },
    );
    let mut app = mounted(&rig);

    start_drag(&app);
    assert!(
        vetoes.get() >= 1,
        "the veto was asked when the drag entered"
    );
    let moves = rig.probe.value().expect("the probe's signal is live");
    assert!(
        moves >= 1,
        "on_move wrote while the drag was over the target"
    );

    app.dispatch_pointer_up(50.0, 130.0);
    assert_eq!(
        rig.probe.value(),
        Ok(moves + 1000),
        "the refused drop left the target through on_leave"
    );
    app.tick();
    assert_eq!(rig.probe.reads().last(), Some(&(moves + 1000)));
}

/// A target callback whose write is refused is reported, and the drop is
/// still accepted.
pub(crate) fn a_refused_write_in_a_target_callback_is_reported_not_panicked() {
    let rig = target_rig(
        |draggable, ProbeSignals { count, .. }| {
            draggable.on_drag_end(move |cx, details: DraggableDetails| {
                count.set(cx, if details.was_accepted { 1 } else { 2 })
            })
        },
        |target, ProbeSignals { released, .. }| {
            target
                .on_move(move |cx, _details| released.set(cx, 1))
                .on_accept(move |cx, _details| released.set(cx, 1))
        },
    );
    let mut app = mounted(&rig);

    let ((), log) = flui_testing::log_capture::capture(|| {
        start_drag(&app);
        app.dispatch_pointer_up(50.0, 130.0);
    });
    assert!(
        log.contains("an event callback's signal write was refused"),
        "the refusal is logged at the dispatch boundary: {log}"
    );
    assert_eq!(rig.probe.value(), Ok(1), "the drop was still accepted");
    app.tick();
    assert_eq!(rig.probe.reads().last(), Some(&1));
}

/// Unmounting a target releases its slot from the owner lane, so the state
/// its callbacks capture is dropped with the target rather than kept until
/// the UI runtime closes.
pub(crate) fn unmounting_a_target_releases_its_slot() {
    let captured = Rc::new(());
    let witness = Rc::downgrade(&captured);
    let target = DragTarget::<u32>::new(|_candidates, _rejected| {
        SizedBox::new(40.0, 40.0).into_view().boxed()
    })
    .on_accept(move |_cx, _details| {
        std::hint::black_box(&captured);
    });
    let mut app = lay_out(target, tight(400.0, 400.0));
    assert!(
        witness.upgrade().is_some(),
        "the mounted target holds its callbacks"
    );

    app.pump_widget(SizedBox::shrink());
    assert!(
        witness.upgrade().is_none(),
        "the unmounted target's slot, and what its callbacks captured, must leave \
         the owner lane with the target"
    );
}

/// A drag callback that panics while the unmount cancels the drag must not
/// leave the drag's feedback layer in the overlay: `dispose` removes the
/// layer before the cancel runs any user code. Whether the unmount path
/// contains the panic or lets it escape the frame, the next frame proceeds
/// without the layer.
#[test]
fn a_panicking_cancel_during_unmount_does_not_leak_the_feedback_layer() {
    let rig = rig(|draggable, _signals| {
        draggable.on_drag_end(|_cx, _details| -> () { panic!("intentional on_drag_end panic") })
    });
    let mut app = mounted(&rig);

    start_drag(&app);
    app.tick();
    assert!(
        app.find_text("feedback").is_some(),
        "premise: the drag's feedback is showing in the overlay"
    );

    rig.show.set(false);
    rig.entry.mark_needs_build();
    let _contained_or_not = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.tick()));

    app.tick();
    assert!(
        app.find_text("feedback").is_none(),
        "the feedback layer was removed before the panicking callback ran"
    );
}
