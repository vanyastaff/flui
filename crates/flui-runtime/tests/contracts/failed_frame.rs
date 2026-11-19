//! A frame whose sink answers `Failed` is painted again after the dropped-frame
//! backoff, which grows while the frames keep failing; one answered `Refused`
//! waits for its window's next change (ADR-0334).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_foundation::{ManualClock, MonotonicClock};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::UiRuntime;

/// A sink that answers `verdict` to the next `remaining` frames, then presents.
struct Failing {
    verdict: SubmitVerdict,
    remaining: usize,
    submits: usize,
    /// Set when a frame fails, so the platform wake that follows panics.
    panic_the_next_wake: Option<Arc<AtomicBool>>,
}

impl Failing {
    fn new(verdict: SubmitVerdict, remaining: usize) -> Self {
        Self {
            verdict,
            remaining,
            submits: 0,
            panic_the_next_wake: None,
        }
    }
}

impl FrameSink for Failing {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(
        &mut self,
        _epoch: flui_foundation::FrameEpoch,
        _: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        self.submits += 1;
        if self.remaining > 0 {
            self.remaining -= 1;
            if let Some(panics) = &self.panic_the_next_wake {
                panics.store(true, Ordering::SeqCst);
            }
            self.verdict
        } else {
            SubmitVerdict::Presented
        }
    }
}

/// One host turn at the clock's reading: the frame timestamp the runtime samples is the instant
/// the test has moved the clock to.
fn host_turn(runtime: &mut UiRuntime, sink: &mut Failing, clock: &ManualClock) -> bool {
    runtime.pump(&mut clock.clone(), sink).presented()
}

/// How many wakes before a retry deadline a host turn may leave standing before the wait is
/// taken as stuck.
const MOST_WAKES_BEFORE_THE_DEADLINE: usize = 16;

/// Runs the host's loop until the dropped frame's retry deadline: the clock moves to each wake
/// `next_wake` reports and the host pumps, and no frame is submitted before the deadline. Returns
/// how long the wait was from the clock's reading on entry, when the frame failed.
fn wait_out_the_backoff(
    runtime: &mut UiRuntime,
    sink: &mut Failing,
    clock: &ManualClock,
) -> std::time::Duration {
    let failed_at = clock.now();
    let deadline = runtime
        .failure_retry_deadline()
        .expect("a failed frame waits out the dropped-frame backoff");
    let submits = sink.submits;
    let mut reached_the_deadline = false;
    for _ in 0..MOST_WAKES_BEFORE_THE_DEADLINE {
        let wake = runtime
            .next_wake()
            .expect("the host is woken for the retry deadline");
        assert!(wake <= deadline, "the host wakes no later than the retry");
        clock.advance(wake.saturating_duration_since(clock.now()));
        if wake == deadline {
            reached_the_deadline = true;
            break;
        }
        assert!(!host_turn(runtime, sink, clock));
        assert_eq!(
            sink.submits, submits,
            "no frame is submitted before the backoff runs out"
        );
    }
    assert!(
        reached_the_deadline,
        "the wakes before the retry deadline end: a turn clears each one"
    );
    assert!(runtime.has_pending_work(), "the frame is due again");
    deadline.saturating_duration_since(failed_at)
}

#[test]
fn a_failed_frame_is_painted_again_after_a_backoff_that_grows() {
    let clock = ManualClock::new();
    let mut runtime = crate::owner_publication::runtime_on_manual_clock(&clock);
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("refused"), 400.0, 400.0)
        .expect("mount text");
    let mut sink = Failing::new(SubmitVerdict::Failed, 3);
    assert!(
        !host_turn(&mut runtime, &mut sink, &clock),
        "the first frame fails"
    );
    assert_eq!(sink.submits, 1);
    assert!(
        !runtime.has_pending_work(),
        "the repaint waits for the deadline"
    );
    assert!(!host_turn(&mut runtime, &mut sink, &clock));
    assert_eq!(
        sink.submits, 1,
        "no frame is submitted before the backoff runs out"
    );

    let mut waits = Vec::new();
    for attempt in 2..=4 {
        waits.push(wait_out_the_backoff(&mut runtime, &mut sink, &clock));
        let presented = host_turn(&mut runtime, &mut sink, &clock);
        assert_eq!(
            sink.submits, attempt,
            "the retried frame paints and submits the tree"
        );
        assert_eq!(
            presented,
            attempt == 4,
            "the sink fails three frames, then presents"
        );
    }
    assert!(
        waits[0] < waits[1],
        "the backoff grows while the frames keep failing: {waits:?}"
    );
    assert_eq!(
        runtime.failure_retry_deadline(),
        None,
        "a presented frame ends the wait"
    );
    assert!(!host_turn(&mut runtime, &mut sink, &clock));
    assert_eq!(sink.submits, 4, "a presented tree is not painted again");
}

/// Mounts a runtime whose platform wake panics while `panics` is set.
fn runtime_whose_wake_panics(panics: &Arc<AtomicBool>, clock: &ManualClock) -> UiRuntime {
    let armed = Arc::clone(panics);
    let runtime = crate::owner_publication::runtime_with_wake_on(
        Arc::new(move || {
            assert!(
                !armed.load(Ordering::SeqCst),
                "platform wake — intentional test panic"
            );
        }),
        flui_scheduler::ClockSource::Manual(clock.clone()),
    );
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("refused"), 400.0, 400.0)
        .expect("mount text");
    runtime
}

/// A platform wake that panics while a failed or device-lost frame asks for its repaint leaves
/// the frame's backoff armed: the frame waits for the deadline, neither tried at once nor
/// forgotten, and the retry after it presents.
///
/// Fails when the repaint's wake runs before the streak and the wait are committed, before the
/// dropped frame's telemetry is recorded, or when the aborted frame leaves its presentation's
/// completed segment unable to begin the next.
#[test]
fn a_dropped_frame_whose_wake_panics_still_waits_out_the_backoff() {
    for verdict in [SubmitVerdict::Failed, SubmitVerdict::DeviceLost] {
        let panics = Arc::new(AtomicBool::new(false));
        let clock = ManualClock::new();
        let mut runtime = runtime_whose_wake_panics(&panics, &clock);
        let mut sink = Failing::new(verdict, 1);
        sink.panic_the_next_wake = Some(Arc::clone(&panics));
        let (turn, log) = flui_testing::log_capture::capture(|| {
            catch_unwind(AssertUnwindSafe(|| {
                host_turn(&mut runtime, &mut sink, &clock)
            }))
        });
        panics.store(false, Ordering::SeqCst);
        assert!(
            turn.is_err(),
            "{verdict:?}: the wake's panic reaches the host"
        );
        assert_eq!(sink.submits, 1, "{verdict:?}");
        assert!(
            runtime.failure_retry_deadline().is_some(),
            "{verdict:?}: the frame waits out the dropped-frame backoff"
        );
        assert!(
            log.records().iter().any(|record| {
                record.target == "flui.frame"
                    && record.field("event") == Some("frame_telemetry")
                    && record.field("present_outcome") == Some("errored")
            }),
            "{verdict:?}: the dropped frame's telemetry is recorded"
        );
        wait_out_the_backoff(&mut runtime, &mut sink, &clock);
        assert!(
            host_turn(&mut runtime, &mut sink, &clock),
            "{verdict:?}: the retried frame presents after the aborted one"
        );
        assert_eq!(sink.submits, 2, "{verdict:?}");
    }
}

/// A frame its sink answers `Refused` arms no retry and asks for no frame: the window waits for
/// its next change, whose frame presents.
///
/// Fails when a refused frame is retried on the backoff or at once.
#[test]
fn a_refused_frame_waits_for_its_windows_next_change() {
    let clock = ManualClock::new();
    let mut runtime = crate::owner_publication::runtime_on_manual_clock(&clock);
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("refused"), 400.0, 400.0)
        .expect("mount text");
    let mut sink = Failing::new(SubmitVerdict::Refused, 1);
    assert!(
        !host_turn(&mut runtime, &mut sink, &clock),
        "the frame is refused"
    );
    assert_eq!(runtime.failure_retry_deadline(), None, "no retry is armed");
    assert!(!runtime.has_pending_work(), "no frame is asked for");
    assert!(!host_turn(&mut runtime, &mut sink, &clock));
    assert_eq!(sink.submits, 1, "the refused frame is not painted again");

    // The window changes: its host asks for the frame again, as a new surface does.
    runtime.mark_primary_needs_full_repaint();
    assert!(
        host_turn(&mut runtime, &mut sink, &clock),
        "the window's next change presents"
    );
    assert_eq!(sink.submits, 2);
}

/// A surface that withholds frames past the withheld-retry budget is tried again after the
/// dropped-frame backoff, through the host's loop, and the press held while the surface was
/// withdrawn is dispatched once a frame shows the tree (ADR-0335).
///
/// Fails when a withheld frame commits its tree (the press dispatches against a tree the screen
/// did not show), or when the budget parks the frame (no wait is reported, and nothing shows the
/// tree once the surface returns).
#[test]
fn a_withdrawn_surface_is_retried_after_the_backoff_and_then_takes_the_held_press() {
    let clock = ManualClock::new();
    let mut runtime = crate::owner_publication::runtime_on_manual_clock(&clock);
    let downs = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let seen = std::rc::Rc::clone(&downs);
    runtime
        .attach_root_widget_with_size(
            &flui_widgets::Listener::new()
                .behavior(flui_rendering::hit_testing::HitTestBehavior::Opaque)
                .on_pointer_down(move |_, _| seen.set(seen.get() + 1))
                .child(flui_widgets::SizedBox::new(400.0, 400.0)),
            400.0,
            400.0,
        )
        .expect("mount the press's handler");
    let mut sink = Failing::new(SubmitVerdict::NotShown, usize::MAX);

    // The budget's attempts follow each other, one per host turn, until the wait is armed.
    let mut turns = 0;
    while runtime.failure_retry_deadline().is_none() {
        assert!(turns < 1_000, "the withheld retry is bounded");
        turns += 1;
        assert!(
            !host_turn(&mut runtime, &mut sink, &clock),
            "the surface withholds"
        );
        assert_eq!(
            sink.submits, turns,
            "each withheld attempt is tried at once"
        );
    }
    assert!(
        turns > 1,
        "a short withdrawal is ridden out at the display rate"
    );

    // Past the budget, a turn before the deadline submits nothing; each attempt waits out the
    // backoff, and the surface still withholds.
    assert!(!host_turn(&mut runtime, &mut sink, &clock));
    assert_eq!(
        sink.submits, turns,
        "no frame is submitted before the backoff runs out"
    );
    let first_wait = wait_out_the_backoff(&mut runtime, &mut sink, &clock);
    assert!(
        first_wait > std::time::Duration::ZERO,
        "the budget's end arms a wait"
    );
    assert!(!host_turn(&mut runtime, &mut sink, &clock));
    assert_eq!(sink.submits, turns + 1, "the attempt after the wait");
    let second_deadline = runtime
        .failure_retry_deadline()
        .expect("the next attempt waits out the backoff too");
    assert!(
        second_deadline.saturating_duration_since(clock.now()) > first_wait,
        "the backoff grows while the surface keeps withholding"
    );

    // A press while the surface is withdrawn waits for a frame that shows the tree. The press
    // ends the wait early, and the surface has returned.
    let primary = runtime.presentation_id();
    runtime.handle_input_addressed(
        primary,
        flui_platform::PlatformInput::Pointer(
            flui_interaction::events::make_down_event(
                flui_foundation::geometry::Offset::new(10.0, 10.0),
                flui_interaction::events::PointerKind::Touch,
            )
            .expect("finite test position"),
        ),
    );
    assert_eq!(downs.get(), 0, "the press waits for a shown tree");
    sink.remaining = 0;
    assert!(
        host_turn(&mut runtime, &mut sink, &clock),
        "the returned surface shows the tree"
    );
    assert_eq!(
        downs.get(),
        1,
        "the shown frame's commit dispatches the press"
    );
    assert_eq!(runtime.failure_retry_deadline(), None);
}
