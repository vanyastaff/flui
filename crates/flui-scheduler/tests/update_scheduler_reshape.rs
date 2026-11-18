//! Frame ids, frame telemetry and frame pace through the public scheduler API.

use std::{cell::Cell, rc::Rc, time::Duration};

use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::{
    DemandKind, FrameClock, FrameId, FramePhase, FramePhaseDurations, FrameTimes, Instant,
    PresentOutcome, PresentationAddress, PresentationId, UiRuntimeId, UpdateScheduler, frame_pace,
};

#[test]
fn update_scheduler_bounds_matrix() {
    crate::run_table(
        "update_scheduler_bounds_matrix",
        &[
            (
                "frame_ids_count_per_scheduler",
                frame_ids_count_per_scheduler as fn(),
            ),
            (
                "a_frame_recorded_after_its_cut_keeps_its_id_and_its_inputs",
                a_frame_recorded_after_its_cut_keeps_its_id_and_its_inputs as fn(),
            ),
            (
                "a_frame_recorded_out_of_order_reaches_a_consumer_past_the_later_one",
                a_frame_recorded_out_of_order_reaches_a_consumer_past_the_later_one as fn(),
            ),
            (
                "an_evicted_cursor_resumes_in_recording_order_and_repeats_none",
                an_evicted_cursor_resumes_in_recording_order_and_repeats_none as fn(),
            ),
            (
                "a_late_frame_names_its_slowest_phase",
                a_late_frame_names_its_slowest_phase as fn(),
            ),
            (
                "frame_pace_ignores_idle_gaps",
                frame_pace_ignores_idle_gaps as fn(),
            ),
        ],
    );
}

/// Frame numbers belong to their scheduler: every scheduler's first frame is
/// frame 1, however many frames other schedulers ran.
fn frame_ids_count_per_scheduler() {
    for _ in 0..2 {
        let scheduler = UpdateScheduler::new();
        let observed = Rc::new(Cell::new(None));
        let now = Instant::now();
        let scheduler_in_frame = scheduler.clone();
        let observed_in_frame = Rc::clone(&observed);
        flui_scheduler::OwnerFrame::try_new(&scheduler)
            .expect("the scheduler has no live owner frame")
            .drive_frame(
                now,
                || {},
                move || {
                    observed_in_frame.set(scheduler_in_frame.current_frame().map(|frame| frame.id));
                },
            )
            .expect("live owner frame");
        assert_eq!(observed.get(), Some(FrameId::FIRST));
    }
}

/// A snapshot names the phase that made a late frame late, including the
/// submit the segment's end leaves to the backend.
fn a_late_frame_names_its_slowest_phase() {
    let clock = FrameClock::new();
    clock.mark_demand(DemandKind::Dirty);
    let start = Instant::now();
    assert!(
        clock
            .poll(start, flui_scheduler::FramesInFlight::NONE)
            .should_run_segment()
    );
    let ms = Duration::from_millis;
    let snapshot = clock.record_frame(
        clock.telemetry_cut(),
        address(),
        FrameTimes {
            clock_timestamp: start,
            segment_start: start,
            segment_end: start + ms(9),
            submit_at: start + ms(30),
            phases: FramePhaseDurations::new(ms(2), ms(3), ms(4)),
        },
        PresentOutcome::Presented,
    );
    assert_eq!(snapshot.phase_duration(FramePhase::Paint), ms(4));
    assert_eq!(snapshot.phase_duration(FramePhase::Composite), ms(21));
    assert_eq!(
        snapshot.over_budget_phase(ms(16)),
        Some(FramePhase::Composite)
    );
    assert_eq!(snapshot.over_budget_phase(ms(40)), None);
}

/// The one presentation these frames belong to.
fn address() -> PresentationAddress {
    PresentationAddress {
        ui_runtime_id: UiRuntimeId::new(1),
        presentation_id: PresentationId::new(1),
    }
}

/// Idle time between bursts of frames is neither a slow frame nor a low
/// frame rate.
fn frame_pace_ignores_idle_gaps() {
    let clock = FrameClock::new();
    let start = Instant::now();
    let ms = Duration::from_millis;
    let snapshots: Vec<_> = [0, 16, 5_016, 5_032]
        .into_iter()
        .map(|at| {
            clock.mark_demand(DemandKind::Dirty);
            let begun = start + ms(at);
            assert!(
                clock
                    .poll(begun, flui_scheduler::FramesInFlight::NONE)
                    .should_run_segment()
            );
            clock.record_frame(
                clock.telemetry_cut(),
                address(),
                FrameTimes {
                    clock_timestamp: begun,
                    segment_start: begun,
                    segment_end: begun + ms(3),
                    submit_at: begun + ms(4),
                    phases: FramePhaseDurations::default(),
                },
                PresentOutcome::Presented,
            )
        })
        .collect();

    let pace = frame_pace(&snapshots);
    assert!((pace.frames_per_second - 62.5).abs() < 1e-6, "{pace:?}");
    assert_eq!(pace.frame_work, ms(4));
}

/// A frame recorded after later frames were produced, as a queued frame is once its sink settles
/// it, keeps the id of its own produce and takes only the inputs stamped before it was cut.
fn a_frame_recorded_after_its_cut_keeps_its_id_and_its_inputs() {
    let clock = FrameClock::new();
    let start = Instant::now();
    let times = FrameTimes {
        clock_timestamp: start,
        segment_start: start,
        segment_end: start,
        submit_at: start,
        phases: FramePhaseDurations::default(),
    };
    let early = clock.stamp_input_epoch(start);
    clock.mark_demand(DemandKind::Dirty);
    assert!(
        clock
            .poll(start, flui_scheduler::FramesInFlight::NONE)
            .should_run_segment()
    );
    let first = clock.telemetry_cut();
    let late = clock.stamp_input_epoch(start);
    clock.mark_demand(DemandKind::Dirty);
    assert!(
        clock
            .poll(start, flui_scheduler::FramesInFlight::NONE)
            .should_run_segment()
    );
    let second = clock.telemetry_cut();

    let one = clock.record_frame(first, address(), times, PresentOutcome::Presented);
    let two = clock.record_frame(second, address(), times, PresentOutcome::Presented);
    assert!(
        one.frame_id < two.frame_id,
        "each frame keeps its own produce's id"
    );
    let ids = |snapshot: &flui_scheduler::FrameSnapshot| {
        snapshot
            .input_epochs
            .iter()
            .map(|epoch| epoch.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&one),
        [early],
        "the first frame carries only the input before its cut"
    );
    assert_eq!(
        ids(&two),
        [late],
        "the later input waited for the later frame"
    );
}

/// A frame recorded after a later one, because its sink settled it later, still reaches a
/// consumer whose cursor is the later frame: the cursor follows the order of recording.
///
/// Fails when the cursor filters by id.
fn a_frame_recorded_out_of_order_reaches_a_consumer_past_the_later_one() {
    let clock = FrameClock::new();
    let start = Instant::now();
    let times = FrameTimes {
        clock_timestamp: start,
        segment_start: start,
        segment_end: start,
        submit_at: start,
        phases: FramePhaseDurations::default(),
    };
    clock.mark_demand(DemandKind::Dirty);
    assert!(
        clock
            .poll(start, flui_scheduler::FramesInFlight::NONE)
            .should_run_segment()
    );
    let queued = clock.telemetry_cut();
    clock.mark_demand(DemandKind::Dirty);
    assert!(
        clock
            .poll(start, flui_scheduler::FramesInFlight::NONE)
            .should_run_segment()
    );
    let failed = clock.telemetry_cut();

    let later = clock.record_frame(failed, address(), times, PresentOutcome::Errored);
    let seen = clock.frames_since(None);
    assert_eq!(
        seen.last().map(|frame| frame.frame_id),
        Some(later.frame_id)
    );

    let settled = clock.record_frame(queued, address(), times, PresentOutcome::Presented);
    assert!(
        settled.frame_id < later.frame_id,
        "the queued frame was produced first"
    );
    let next = clock.frames_since(Some(later.frame_id));
    assert_eq!(
        next.iter().map(|frame| frame.frame_id).collect::<Vec<_>>(),
        [settled.frame_id],
        "the frame recorded after the cursor is delivered"
    );
}

/// A consumer whose cursor left the history ring resumes in recording order: a frame recorded
/// late, after a later one, is delivered once, and the next pull from the last snapshot it got
/// repeats none.
///
/// Fails when the evicted cursor's pull is sorted by id.
fn an_evicted_cursor_resumes_in_recording_order_and_repeats_none() {
    let clock = FrameClock::new();
    let start = Instant::now();
    let times = FrameTimes {
        clock_timestamp: start,
        segment_start: start,
        segment_end: start,
        submit_at: start,
        phases: FramePhaseDurations::default(),
    };
    let cut = || {
        clock.mark_demand(DemandKind::Dirty);
        assert!(
            clock
                .poll(start, flui_scheduler::FramesInFlight::NONE)
                .should_run_segment()
        );
        clock.telemetry_cut()
    };
    let first = cut();
    let cursor = clock
        .record_frame(first, address(), times, PresentOutcome::Presented)
        .frame_id;
    let cuts = (0..=flui_scheduler::FRAME_HISTORY_CAPACITY)
        .map(|_| cut())
        .collect::<Vec<_>>();
    let (late, rest) = cuts.split_first().expect("cuts were taken");
    for each in rest {
        let _ = clock.record_frame(*each, address(), times, PresentOutcome::Presented);
    }
    let late = clock.record_frame(*late, address(), times, PresentOutcome::Presented);

    let pulled = clock.frames_since(Some(cursor));
    assert_eq!(
        pulled.last().map(|frame| frame.frame_id),
        Some(late.frame_id),
        "the late frame comes last, as it was recorded"
    );
    let next = pulled.last().map(|frame| frame.frame_id);
    assert!(
        clock.frames_since(next).is_empty(),
        "a pull from the last snapshot received repeats none"
    );
}
