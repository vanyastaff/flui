//! A frame whose sink answers `Failed` is painted again after the dropped-frame
//! backoff, which grows while the frames keep failing; one answered `Refused`
//! waits for its window's next change (ADR-0334).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

fn host_turn(runtime: &mut UiRuntime, sink: &mut Failing) -> bool {
    runtime
        .pump(
            &mut flui_runtime::pump::SampledClock(web_time::Instant::now()),
            sink,
        )
        .presented()
}

/// Runs the host's loop until the dropped frame's retry deadline: it sleeps until each wake
/// `next_wake` reports and pumps, and no frame is submitted before the deadline. Returns how long
/// the wait was from when the frame failed.
fn wait_out_the_backoff(
    runtime: &mut UiRuntime,
    sink: &mut Failing,
    failed_at: web_time::Instant,
) -> std::time::Duration {
    let deadline = runtime
        .failure_retry_deadline()
        .expect("a failed frame waits out the dropped-frame backoff");
    let submits = sink.submits;
    loop {
        let wake = runtime
            .next_wake()
            .expect("the host is woken for the retry deadline");
        assert!(wake <= deadline, "the host wakes no later than the retry");
        std::thread::sleep(wake.saturating_duration_since(web_time::Instant::now()));
        if wake == deadline {
            break;
        }
        assert!(!host_turn(runtime, sink));
        assert_eq!(
            sink.submits, submits,
            "no frame is submitted before the backoff runs out"
        );
    }
    assert!(runtime.has_pending_work(), "the frame is due again");
    deadline.saturating_duration_since(failed_at)
}

#[test]
fn a_failed_frame_is_painted_again_after_a_backoff_that_grows() {
    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(|| {}));
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("refused"), 400.0, 400.0)
        .expect("mount text");
    let mut sink = Failing::new(SubmitVerdict::Failed, 3);
    let mut failed_at = web_time::Instant::now();
    assert!(!host_turn(&mut runtime, &mut sink), "the first frame fails");
    assert_eq!(sink.submits, 1);
    assert!(
        !runtime.has_pending_work(),
        "the repaint waits for the deadline"
    );
    assert!(!host_turn(&mut runtime, &mut sink));
    assert_eq!(
        sink.submits, 1,
        "no frame is submitted before the backoff runs out"
    );

    let mut waits = Vec::new();
    for attempt in 2..=4 {
        waits.push(wait_out_the_backoff(&mut runtime, &mut sink, failed_at));
        failed_at = web_time::Instant::now();
        let presented = host_turn(&mut runtime, &mut sink);
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
    assert!(!host_turn(&mut runtime, &mut sink));
    assert_eq!(sink.submits, 4, "a presented tree is not painted again");
}

/// Mounts a runtime whose platform wake panics while `panics` is set.
fn runtime_whose_wake_panics(panics: &Arc<AtomicBool>) -> UiRuntime {
    let armed = Arc::clone(panics);
    let runtime = crate::owner_publication::runtime_with_wake(Arc::new(move || {
        assert!(
            !armed.load(Ordering::SeqCst),
            "platform wake — intentional test panic"
        );
    }));
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("refused"), 400.0, 400.0)
        .expect("mount text");
    runtime
}

/// A platform wake that panics while a failed or device-lost frame asks for its repaint leaves
/// the frame's backoff armed: the frame waits for the deadline, neither tried at once nor
/// forgotten.
///
/// Fails when the repaint's wake runs before the streak and the wait are committed, or before the
/// dropped frame's telemetry is recorded.
#[test]
fn a_dropped_frame_whose_wake_panics_still_waits_out_the_backoff() {
    for verdict in [SubmitVerdict::Failed, SubmitVerdict::DeviceLost] {
        let panics = Arc::new(AtomicBool::new(false));
        let mut runtime = runtime_whose_wake_panics(&panics);
        let mut sink = Failing::new(verdict, 1);
        sink.panic_the_next_wake = Some(Arc::clone(&panics));
        let (turn, log) = flui_testing::log_capture::capture(|| {
            catch_unwind(AssertUnwindSafe(|| host_turn(&mut runtime, &mut sink)))
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
    }
}

/// A frame its sink answers `Refused` arms no retry and asks for no frame: the window waits for
/// its next change, whose frame presents.
///
/// Fails when a refused frame is retried on the backoff or at once.
#[test]
fn a_refused_frame_waits_for_its_windows_next_change() {
    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(|| {}));
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("refused"), 400.0, 400.0)
        .expect("mount text");
    let mut sink = Failing::new(SubmitVerdict::Refused, 1);
    assert!(!host_turn(&mut runtime, &mut sink), "the frame is refused");
    assert_eq!(runtime.failure_retry_deadline(), None, "no retry is armed");
    assert!(!runtime.has_pending_work(), "no frame is asked for");
    assert!(!host_turn(&mut runtime, &mut sink));
    assert_eq!(sink.submits, 1, "the refused frame is not painted again");

    // The window changes: its host asks for the frame again, as a new surface does.
    runtime.mark_primary_needs_full_repaint();
    assert!(
        host_turn(&mut runtime, &mut sink),
        "the window's next change presents"
    );
    assert_eq!(sink.submits, 2);
}
