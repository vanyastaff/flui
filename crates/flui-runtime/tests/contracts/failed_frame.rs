//! A frame whose sink answers `Failed` is painted again after the dropped-frame
//! backoff, which grows while the frames keep failing (ADR-0334).

use std::sync::Arc;

use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::UiRuntime;

/// A sink that fails the next `failing` frames, then presents.
struct Refusing {
    failing: usize,
    submits: usize,
}

impl FrameSink for Refusing {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(
        &mut self,
        _epoch: flui_foundation::FrameEpoch,
        _: flui_layer::ScenePublication,
    ) -> SubmitVerdict {
        self.submits += 1;
        if self.failing > 0 {
            self.failing -= 1;
            SubmitVerdict::Failed
        } else {
            SubmitVerdict::Presented
        }
    }
}

fn host_turn(runtime: &mut UiRuntime, sink: &mut Refusing) -> bool {
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
    sink: &mut Refusing,
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
    let mut sink = Refusing {
        failing: 3,
        submits: 0,
    };
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
