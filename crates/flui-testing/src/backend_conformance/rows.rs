//! The conformance table's rows.

use std::time::Duration;

use flui_platform::Availability;
use flui_platform::PlatformInput;
use flui_platform::pointer::{
    CancelReason, PanZoomPhase, PenTool, PointerEvent, PointerKind, PointerRole, ScrollPhase,
    ScrollPrecision,
};
use flui_runtime::HostInput;

use super::drag_rows::{
    a_drag_source_starts_when_the_backend_declares,
    a_leaving_backend_escalates_when_the_pointer_leaves_the_window,
    an_at_start_backend_never_escalates_mid_drag, outgoing_drag_is_delivered_as_claimed,
    start_decision_includes_sibling_presentation_with_identity_only_offer,
};
use super::haptic_rows::{
    a_backend_never_queues, a_stale_source_plays_on_the_window_actuator,
    capabilities_match_what_plays, semantic_feedback_reports_a_tier_one_outcome,
};
use super::key_rows::{
    a_key_during_composition_is_the_input_methods, menu_keys_arrive_as_the_bindings_name_them,
};
use super::{
    BackendUnderTest, ContactAction, ContactStep, PanStep, PinchChange, PinchStep, RowGroup,
    RowOutcome, Stamp, Step, StepPhase,
};

pub(super) struct Row<B> {
    pub(super) name: &'static str,
    pub(super) group: RowGroup,
    pub(super) check: fn(&mut B) -> Outcome,
}

pub(super) type Outcome = Result<RowOutcome, String>;

const HELD: Outcome = Ok(RowOutcome::Held);

macro_rules! row {
    ($group:ident, $name:ident) => {
        Row {
            name: stringify!($name),
            group: RowGroup::$group,
            check: $name::<B>,
        }
    };
}

// Every row, in running order.
pub(super) fn rows<B: BackendUnderTest>() -> Vec<Row<B>> {
    vec![
        row!(PointerAssembly, a_contact_is_one_pointer_from_down_to_up),
        row!(PointerAssembly, a_reused_touch_label_is_a_new_pointer),
        row!(PointerAssembly, a_second_touch_is_additional),
        row!(
            PointerAssembly,
            a_named_device_is_announced_before_its_first_contact
        ),
        row!(Clocks, event_times_follow_the_reading_clock),
        row!(Clocks, event_times_never_go_back),
        row!(Clocks, the_coarse_clock_wraps_forward),
        row!(Clocks, precise_readings_are_stamped_from_the_counter),
        row!(Clocks, a_sequence_keeps_the_clock_it_started_on),
        row!(
            Clocks,
            a_device_whose_reading_lacked_the_counter_stays_coarse
        ),
        row!(
            ScrollGrammar,
            two_finger_pan_is_phased_scroll_on_every_backend
        ),
        row!(ScrollGrammar, pan_zoom_carries_only_scale_and_rotation),
        row!(
            InvalidInput,
            an_undecodable_reading_cancels_its_contact_as_invalid_input
        ),
        row!(Availability, pen_is_delivered_as_claimed),
        row!(Availability, precise_scroll_is_delivered_as_claimed),
        row!(Availability, trackpad_gestures_are_delivered_as_claimed),
        row!(Ownership, no_backend_cancels_a_contact_as_claimed),
        row!(DragAndDrop, outgoing_drag_is_delivered_as_claimed),
        row!(DragAndDrop, a_drag_source_starts_when_the_backend_declares),
        row!(DragAndDrop, an_at_start_backend_never_escalates_mid_drag),
        row!(
            DragAndDrop,
            start_decision_includes_sibling_presentation_with_identity_only_offer
        ),
        row!(
            DragAndDrop,
            a_leaving_backend_escalates_when_the_pointer_leaves_the_window
        ),
        row!(Haptics, a_backend_never_queues),
        row!(Haptics, semantic_feedback_reports_a_tier_one_outcome),
        row!(Haptics, a_stale_source_plays_on_the_window_actuator),
        row!(Haptics, capabilities_match_what_plays),
        row!(Keys, menu_keys_arrive_as_the_bindings_name_them),
        row!(Ime, a_key_during_composition_is_the_input_methods),
    ]
}

// --- Steps ------------------------------------------------------------------

const PEN: PointerKind = PointerKind::Pen { tool: PenTool::Tip };

const fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

const fn us(value: u64) -> Duration {
    Duration::from_micros(value)
}

const fn contact(kind: PointerKind, label: u32, action: ContactAction, at: Stamp) -> ContactStep {
    ContactStep {
        contact: label,
        device: None,
        kind,
        action,
        at,
        x: 10.0,
        y: 20.0,
    }
}

const fn on_device(step: ContactStep, device: u32) -> ContactStep {
    ContactStep {
        device: Some(device),
        ..step
    }
}

const fn moved_to(step: ContactStep, x: f64, y: f64) -> ContactStep {
    ContactStep { x, y, ..step }
}

const fn press(kind: PointerKind, label: u32, at: Stamp) -> Step {
    Step::Contact(contact(kind, label, ContactAction::Press, at))
}

const fn movement(kind: PointerKind, label: u32, at: Stamp) -> Step {
    Step::Contact(moved_to(
        contact(kind, label, ContactAction::Move, at),
        15.0,
        25.0,
    ))
}

const fn release(kind: PointerKind, label: u32, at: Stamp) -> Step {
    Step::Contact(contact(kind, label, ContactAction::Release, at))
}

const fn pan(phase: StepPhase, momentum: bool, dy: f64, at: Stamp) -> Step {
    Step::Pan(PanStep {
        phase,
        momentum,
        dx: 0.0,
        dy,
        at,
    })
}

const fn pinch(phase: StepPhase, change: PinchChange, x: f64, y: f64, at: Stamp) -> Step {
    Step::Pinch(PinchStep {
        phase,
        change,
        at,
        x,
        y,
    })
}

// --- Driving ----------------------------------------------------------------

/// The pointer events `steps` decode to, in order. Fails when the backend has
/// no reading for a step, and on any `CancelReason::Claimed`.
fn feed<B: BackendUnderTest>(backend: &mut B, steps: &[Step]) -> Result<Vec<PointerEvent>, String> {
    let mut events = Vec::new();
    for step in steps {
        let reading = backend
            .reading(*step)
            .ok_or_else(|| format!("the backend has no reading for {step:?}"))?;
        for input in backend.decode(reading) {
            if let HostInput::Platform(PlatformInput::Pointer(event)) = input {
                if let PointerEvent::Cancel(cancel) = &event
                    && cancel.reason == CancelReason::Claimed
                {
                    return Err(format!(
                        "{step:?} cancelled its contact as Claimed, which only the arena may"
                    ));
                }
                events.push(event);
            }
        }
    }
    Ok(events)
}

/// The events that make up contact sequences and gestures, without the
/// device, hover and boundary signals around them.
fn sequence(events: &[PointerEvent]) -> Vec<&PointerEvent> {
    events
        .iter()
        .filter(|event| {
            !matches!(
                event,
                PointerEvent::DeviceAdded(_)
                    | PointerEvent::DeviceRemoved(_)
                    | PointerEvent::Enter(_)
                    | PointerEvent::Leave(_)
            )
        })
        .collect()
}

/// Whether the backend has a reading for `step`; probing commits nothing.
fn supports<B: BackendUnderTest>(backend: &mut B, step: Step) -> bool {
    let supported = backend.reading(step).is_some();
    backend.restart();
    supported
}

/// The first of `kinds` the backend has contacts of.
fn contact_kind<B: BackendUnderTest>(
    backend: &mut B,
    kinds: &[PointerKind],
) -> Option<PointerKind> {
    kinds
        .iter()
        .copied()
        .find(|kind| supports(backend, press(*kind, 1, Stamp::coarse(ms(10)))))
}

fn time_of(event: &PointerEvent) -> Option<Duration> {
    let time = match event {
        PointerEvent::Down(press) => press.sample.time,
        PointerEvent::Up(release) => release.sample.time,
        PointerEvent::Move(movement) => movement.current().time,
        PointerEvent::Cancel(cancel) => cancel.time,
        PointerEvent::Scroll(scroll) => scroll.time,
        PointerEvent::PanZoom(pan_zoom) => pan_zoom.time,
        _ => return None,
    };
    Some(Duration::from_nanos(time.as_nanos()))
}

/// The times of the sequence events, in order.
fn times(events: &[PointerEvent]) -> Result<Vec<Duration>, String> {
    sequence(events)
        .into_iter()
        .map(|event| time_of(event).ok_or_else(|| format!("{event:?} carries no time")))
        .collect()
}

/// `later - earlier` must be `expected` within `tolerance`.
fn interval(
    earlier: Duration,
    later: Duration,
    expected: Duration,
    tolerance: Duration,
) -> Result<(), String> {
    let Some(actual) = later.checked_sub(earlier) else {
        return Err(format!(
            "{later:?} precedes {earlier:?}; expected {expected:?} after it"
        ));
    };
    let error = actual.abs_diff(expected);
    if error > tolerance {
        return Err(format!(
            "events {actual:?} apart; the readings were {expected:?} apart (tolerance {tolerance:?})"
        ));
    }
    Ok(())
}

// --- Pointer assembly -------------------------------------------------------

/// A press, a move and a release of one contact are `Down`, `Move`, `Up` of
/// one pointer of the contact's kind, for touch and for the mouse.
fn a_contact_is_one_pointer_from_down_to_up<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let mut checked = false;
    for kind in [PointerKind::Touch, PointerKind::Mouse] {
        if !supports(backend, press(kind, 1, Stamp::coarse(ms(10)))) {
            continue;
        }
        checked = true;
        let events = feed(
            backend,
            &[
                press(kind, 1, Stamp::coarse(ms(10))),
                movement(kind, 1, Stamp::coarse(ms(12))),
                release(kind, 1, Stamp::coarse(ms(14))),
            ],
        )?;
        let [
            PointerEvent::Down(down),
            PointerEvent::Move(moved),
            PointerEvent::Up(up),
        ] = sequence(&events).as_slice()
        else {
            return Err(format!("{kind:?}: expected Down, Move, Up; got {events:?}"));
        };
        if moved.pointer.id != down.pointer.id || up.pointer.id != down.pointer.id {
            return Err(format!("{kind:?}: one contact changed pointer: {events:?}"));
        }
        if down.pointer.kind != kind {
            return Err(format!(
                "{kind:?}: the pointer reports {:?}",
                down.pointer.kind
            ));
        }
        backend.restart();
    }
    if checked {
        HELD
    } else {
        Err("the backend has neither touch nor mouse contacts".to_owned())
    }
}

/// A touch label the OS reuses after release starts a new pointer.
fn a_reused_touch_label_is_a_new_pointer<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let touch = PointerKind::Touch;
    if !supports(backend, press(touch, 7, Stamp::coarse(ms(10)))) {
        return Ok(RowOutcome::NotApplicable("the backend has no touch"));
    }
    let events = feed(
        backend,
        &[
            press(touch, 7, Stamp::coarse(ms(10))),
            release(touch, 7, Stamp::coarse(ms(12))),
            press(touch, 7, Stamp::coarse(ms(14))),
        ],
    )?;
    let downs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            PointerEvent::Down(down) => Some(down.pointer.id),
            _ => None,
        })
        .collect();
    match downs.as_slice() {
        [first, second] if first != second => HELD,
        _ => Err(format!(
            "expected two Downs of different pointers; got {events:?}"
        )),
    }
}

/// A second touch while the first is down is an additional pointer; the
/// first is primary.
fn a_second_touch_is_additional<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let touch = PointerKind::Touch;
    if !supports(backend, press(touch, 1, Stamp::coarse(ms(10)))) {
        return Ok(RowOutcome::NotApplicable("the backend has no touch"));
    }
    let events = feed(
        backend,
        &[
            press(touch, 1, Stamp::coarse(ms(10))),
            press(touch, 2, Stamp::coarse(ms(11))),
        ],
    )?;
    match sequence(&events).as_slice() {
        [PointerEvent::Down(first), PointerEvent::Down(second)]
            if first.pointer.id != second.pointer.id
                && first.pointer.role == PointerRole::Primary
                && second.pointer.role == PointerRole::Additional =>
        {
            HELD
        }
        _ => Err(format!(
            "expected a primary and an additional Down; got {events:?}"
        )),
    }
}

/// A contact from a device the OS names announces the device first, and its
/// events carry the announced id.
fn a_named_device_is_announced_before_its_first_contact<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PEN, PointerKind::Mouse]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let first = Step::Contact(on_device(
        contact(kind, 1, ContactAction::Press, Stamp::coarse(ms(10))),
        3,
    ));
    if !supports(backend, first) {
        return Ok(RowOutcome::NotApplicable("the backend names no devices"));
    }
    let events = feed(backend, &[first])?;
    match events.as_slice() {
        [
            PointerEvent::DeviceAdded(added),
            PointerEvent::Down(down),
            ..,
        ] if down.pointer.device == Some(added.device) => HELD,
        _ => Err(format!(
            "expected DeviceAdded, then a Down carrying its id; got {events:?}"
        )),
    }
}

// --- Clocks -----------------------------------------------------------------

/// Event times are as far apart as the readings were.
fn event_times_follow_the_reading_clock<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PointerKind::Mouse, PEN]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let tolerance = backend.clock().resolution;
    let events = feed(
        backend,
        &[
            press(kind, 1, Stamp::coarse(ms(10))),
            movement(kind, 1, Stamp::coarse(ms(18))),
            release(kind, 1, Stamp::coarse(ms(30))),
        ],
    )?;
    let [down, moved, up] = times(&events)?[..] else {
        return Err(format!("expected Down, Move, Up; got {events:?}"));
    };
    interval(down, moved, ms(8), tolerance)?;
    interval(down, up, ms(20), tolerance)?;
    HELD
}

/// A reading stamped before the one it follows never moves its contact's
/// time back.
fn event_times_never_go_back<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PointerKind::Mouse, PEN]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let events = feed(
        backend,
        &[
            press(kind, 1, Stamp::coarse(ms(20))),
            movement(kind, 1, Stamp::coarse(ms(15))),
            release(kind, 1, Stamp::coarse(ms(12))),
        ],
    )?;
    let times = times(&events)?;
    if times.len() != 3 {
        return Err(format!("expected Down, Move, Up; got {events:?}"));
    }
    if times.windows(2).all(|pair| pair[0] <= pair[1]) {
        HELD
    } else {
        Err(format!("event times went back: {times:?}"))
    }
}

/// Readings either side of the coarse stamp's wrap stay in order and as far
/// apart as they were.
fn the_coarse_clock_wraps_forward<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let clock = backend.clock();
    let Some(wrap) = clock.wraps_after else {
        return Ok(RowOutcome::NotApplicable(
            "the backend's coarse stamp does not wrap",
        ));
    };
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PointerKind::Mouse, PEN]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let events = feed(
        backend,
        &[
            press(kind, 1, Stamp::coarse(wrap.saturating_sub(ms(2)))),
            movement(kind, 1, Stamp::coarse(wrap + ms(3))),
        ],
    )?;
    let [down, moved] = times(&events)?[..] else {
        return Err(format!("expected Down, Move; got {events:?}"));
    };
    interval(down, moved, ms(5), clock.resolution)?;
    HELD
}

/// Readings that carry the counter are stamped from it, below the coarse
/// resolution (`QueryPerformanceCounter` on Win32).
fn precise_readings_are_stamped_from_the_counter<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let Some(counter) = backend.clock().counter else {
        return Ok(RowOutcome::NotApplicable(
            "the backend has no high-resolution counter",
        ));
    };
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PEN, PointerKind::Mouse]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let events = feed(
        backend,
        &[
            press(kind, 1, Stamp::precise(ms(10) + us(300))),
            movement(kind, 1, Stamp::precise(ms(10) + us(900))),
        ],
    )?;
    let [down, moved] = times(&events)?[..] else {
        return Err(format!("expected Down, Move; got {events:?}"));
    };
    interval(down, moved, us(600), counter)?;
    HELD
}

/// A sequence that started on the coarse stamp stays on it when a later
/// reading adds the counter, so its times never mix two clocks.
fn a_sequence_keeps_the_clock_it_started_on<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let clock = backend.clock();
    let Some(counter) = clock.counter else {
        return Ok(RowOutcome::NotApplicable(
            "the backend has no high-resolution counter",
        ));
    };
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PEN, PointerKind::Mouse]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let events = feed(
        backend,
        &[
            press(kind, 1, Stamp::coarse(ms(10) + us(300))),
            movement(kind, 1, Stamp::precise(ms(12) + us(700))),
        ],
    )?;
    let [down, moved] = times(&events)?[..] else {
        return Err(format!("expected Down, Move; got {events:?}"));
    };
    // Both on the coarse stamp: whole coarse ticks apart.
    interval(down, moved, ms(2), counter)?;
    HELD
}

/// A device whose reading once lacked the counter starts its later sequences
/// on the coarse stamp; a device that always carried it keeps the counter.
fn a_device_whose_reading_lacked_the_counter_stays_coarse<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let clock = backend.clock();
    let Some(counter) = clock.counter else {
        return Ok(RowOutcome::NotApplicable(
            "the backend has no high-resolution counter",
        ));
    };
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PEN, PointerKind::Mouse]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let step = |device, label, action, at| {
        Step::Contact(on_device(contact(kind, label, action, at), device))
    };
    if !supports(
        backend,
        step(4, 1, ContactAction::Press, Stamp::coarse(ms(10))),
    ) {
        return Ok(RowOutcome::NotApplicable("the backend names no devices"));
    }
    let events = feed(
        backend,
        &[
            step(4, 1, ContactAction::Press, Stamp::coarse(ms(10) + us(300))),
            step(4, 1, ContactAction::Release, Stamp::coarse(ms(11))),
            step(4, 1, ContactAction::Press, Stamp::precise(ms(20) + us(300))),
            step(4, 1, ContactAction::Move, Stamp::precise(ms(20) + us(900))),
            step(5, 2, ContactAction::Press, Stamp::precise(ms(30) + us(300))),
            step(5, 2, ContactAction::Move, Stamp::precise(ms(30) + us(900))),
        ],
    )?;
    let [_, _, coarse_down, coarse_move, fresh_down, fresh_move] = times(&events)?[..] else {
        return Err(format!(
            "expected two sequences of the first device and one of the second; got {events:?}"
        ));
    };
    interval(coarse_down, coarse_move, Duration::ZERO, counter)
        .map_err(|message| format!("the device that lacked the counter: {message}"))?;
    interval(fresh_down, fresh_move, us(600), counter)
        .map_err(|message| format!("the device that kept the counter: {message}"))?;
    HELD
}

// --- Scroll and PanZoom grammar ---------------------------------------------

/// A two-finger pan is a precise `Scroll` with `Began`, `Changed`, `Ended`,
/// then `Momentum*` phases; never a `PanZoom` (ADR-0217 §4).
fn two_finger_pan_is_phased_scroll_on_every_backend<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    if backend.capabilities().precise_scroll() == Availability::Unavailable {
        return Ok(RowOutcome::NotApplicable(
            "the backend claims no precise scrolling",
        ));
    }
    let events = feed(
        backend,
        &[
            pan(StepPhase::Began, false, 4.0, Stamp::coarse(ms(10))),
            pan(StepPhase::Changed, false, 8.0, Stamp::coarse(ms(18))),
            pan(StepPhase::Ended, false, 0.0, Stamp::coarse(ms(26))),
            pan(StepPhase::Began, true, 6.0, Stamp::coarse(ms(34))),
            pan(StepPhase::Changed, true, 3.0, Stamp::coarse(ms(42))),
            pan(StepPhase::Ended, true, 0.0, Stamp::coarse(ms(50))),
        ],
    )?;
    let mut phases = Vec::new();
    for event in sequence(&events) {
        let PointerEvent::Scroll(scroll) = event else {
            return Err(format!("a two-finger pan emitted {event:?}"));
        };
        if scroll.precision != ScrollPrecision::Precise {
            return Err(format!("a two-finger pan scrolled {:?}", scroll.precision));
        }
        phases.push(scroll.phase);
    }
    let expected = [
        ScrollPhase::Began,
        ScrollPhase::Changed,
        ScrollPhase::Ended,
        ScrollPhase::MomentumBegan,
        ScrollPhase::MomentumChanged,
        ScrollPhase::MomentumEnded,
    ]
    .map(Some);
    if phases == expected {
        HELD
    } else {
        Err(format!("scroll phases {phases:?}, expected {expected:?}"))
    }
}

/// A pinch and a rotation are one `PanZoom` gesture whose transform carries
/// only scale and rotation; translation is its focal point moving, carried by
/// an `Update` even when the transform stands still.
fn pan_zoom_carries_only_scale_and_rotation<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    if backend.capabilities().trackpad_gestures() == Availability::Unavailable {
        return Ok(RowOutcome::NotApplicable(
            "the backend claims no trackpad gestures",
        ));
    }
    let events = feed(
        backend,
        &[
            pinch(
                StepPhase::Began,
                PinchChange::Scale(1.5),
                100.0,
                100.0,
                Stamp::coarse(ms(10)),
            ),
            pinch(
                StepPhase::Began,
                PinchChange::Rotation(0.25),
                100.0,
                100.0,
                Stamp::coarse(ms(18)),
            ),
            pinch(
                StepPhase::Changed,
                PinchChange::Scale(1.0),
                130.0,
                90.0,
                Stamp::coarse(ms(26)),
            ),
            pinch(
                StepPhase::Ended,
                PinchChange::Rotation(0.0),
                130.0,
                90.0,
                Stamp::coarse(ms(34)),
            ),
            pinch(
                StepPhase::Ended,
                PinchChange::Scale(1.0),
                130.0,
                90.0,
                Stamp::coarse(ms(42)),
            ),
        ],
    )?;
    let mut seen = Vec::new();
    for event in sequence(&events) {
        let PointerEvent::PanZoom(gesture) = event else {
            return Err(format!("a pinch emitted {event:?}"));
        };
        let position = gesture.position.get();
        seen.push(match gesture.phase {
            PanZoomPhase::Start => ("start", 1.0, 0.0, position.x, position.y),
            PanZoomPhase::Update(transform) => (
                "update",
                transform.scale(),
                transform.rotation(),
                position.x,
                position.y,
            ),
            PanZoomPhase::End => ("end", 0.0, 0.0, position.x, position.y),
            _ => ("cancelled", 0.0, 0.0, position.x, position.y),
        });
    }
    let expected = [
        ("start", 1.0, 0.0, 100.0, 100.0),
        ("update", 1.5, 0.0, 100.0, 100.0),
        ("update", 1.5, 0.25, 100.0, 100.0),
        ("update", 1.5, 0.25, 130.0, 90.0),
        ("end", 0.0, 0.0, 130.0, 90.0),
    ];
    if seen == expected {
        HELD
    } else {
        Err(format!("gesture {seen:?}, expected {expected:?}"))
    }
}

// --- Invalid input ----------------------------------------------------------

/// An undecodable reading of an open contact ends it with one
/// `Cancel(InvalidInput)`, after which the contact is gone; an undecodable
/// reading of no open contact emits nothing (ADR-0217 §3).
fn an_undecodable_reading_cancels_its_contact_as_invalid_input<B: BackendUnderTest>(
    backend: &mut B,
) -> Outcome {
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PointerKind::Mouse, PEN]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let broken = |label, action, at| Step::Undecodable(contact(kind, label, action, at));
    let events = feed(
        backend,
        &[
            press(kind, 1, Stamp::coarse(ms(10))),
            broken(1, ContactAction::Move, Stamp::coarse(ms(12))),
        ],
    )?;
    let [PointerEvent::Down(down), PointerEvent::Cancel(cancel)] = sequence(&events)[..] else {
        return Err(format!("expected Down then one Cancel; got {events:?}"));
    };
    if cancel.pointer.id != down.pointer.id || cancel.reason != CancelReason::InvalidInput {
        return Err(format!(
            "the contact ended as {:?} of {:?}, expected InvalidInput of {:?}",
            cancel.reason, cancel.pointer.id, down.pointer.id
        ));
    }
    let orphans = feed(
        backend,
        &[
            release(kind, 1, Stamp::coarse(ms(14))),
            broken(9, ContactAction::Release, Stamp::coarse(ms(16))),
        ],
    )?;
    if sequence(&orphans).is_empty() {
        HELD
    } else {
        Err(format!("readings of no open contact emitted {orphans:?}"))
    }
}

// --- Availability -----------------------------------------------------------

/// The backend delivers `step` exactly when it claims `availability`, and
/// what it delivers is `delivered`.
fn delivered_as_claimed<B: BackendUnderTest>(
    backend: &mut B,
    availability: Availability,
    step: Step,
    delivered: fn(&PointerEvent) -> bool,
) -> Outcome {
    let has_path = supports(backend, step);
    match (availability, has_path) {
        (Availability::Unavailable, false) => HELD,
        (Availability::Unavailable, true) => Err(format!(
            "claims Unavailable, yet has a reading for {step:?}"
        )),
        (_, false) => Err(format!(
            "claims {availability:?}, yet has no reading for {step:?}"
        )),
        (_, true) => {
            let events = feed(backend, &[step])?;
            if sequence(&events)
                .first()
                .is_some_and(|event| delivered(event))
            {
                HELD
            } else {
                Err(format!(
                    "claims {availability:?}, yet {step:?} delivered {events:?}"
                ))
            }
        }
    }
}

/// A pen tip touching is a pen `Down` exactly when pen input is claimed.
fn pen_is_delivered_as_claimed<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let availability = backend.capabilities().pen();
    delivered_as_claimed(
        backend,
        availability,
        press(PEN, 1, Stamp::coarse(ms(10))),
        |event| matches!(event, PointerEvent::Down(down) if matches!(down.pointer.kind, PointerKind::Pen { .. })),
    )
}

/// A two-finger pan begins a phased scroll exactly when precise scrolling is
/// claimed.
fn precise_scroll_is_delivered_as_claimed<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let availability = backend.capabilities().precise_scroll();
    let step = pan(StepPhase::Began, false, 4.0, Stamp::coarse(ms(10)));
    delivered_as_claimed(
        backend,
        availability,
        step,
        |event| matches!(event, PointerEvent::Scroll(scroll) if scroll.phase == Some(ScrollPhase::Began)),
    )
}

/// A pinch starts a `PanZoom` exactly when trackpad gestures are claimed.
fn trackpad_gestures_are_delivered_as_claimed<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let availability = backend.capabilities().trackpad_gestures();
    let step = pinch(
        StepPhase::Began,
        PinchChange::Scale(1.5),
        50.0,
        50.0,
        Stamp::coarse(ms(10)),
    );
    delivered_as_claimed(
        backend,
        availability,
        step,
        |event| matches!(event, PointerEvent::PanZoom(gesture) if gesture.phase == PanZoomPhase::Start),
    )
}

// --- Ownership --------------------------------------------------------------

/// The OS ending a contact is a cancel with the platform's reason, never
/// `Claimed`: only the gesture arena hands a contact over (ADR-0208). The
/// shared assembly's readings cannot carry it; this catches a backend that
/// builds its cancels outside that decoder.
fn no_backend_cancels_a_contact_as_claimed<B: BackendUnderTest>(backend: &mut B) -> Outcome {
    let Some(kind) = contact_kind(backend, &[PointerKind::Touch, PEN, PointerKind::Mouse]) else {
        return Err("the backend has no pointer contacts".to_owned());
    };
    let events = feed(
        backend,
        &[
            press(kind, 1, Stamp::coarse(ms(10))),
            Step::Contact(contact(
                kind,
                1,
                ContactAction::Cancel,
                Stamp::coarse(ms(12)),
            )),
        ],
    )?;
    match sequence(&events).as_slice() {
        [PointerEvent::Down(_), PointerEvent::Cancel(_)] => HELD,
        _ => Err(format!("expected Down then one Cancel; got {events:?}")),
    }
}
