//! Each frame a UI runtime hands its sink carries the next epoch of its
//! presentation, the stamp a frame's settlement is matched by (ADR-0305 §4).

use std::sync::Arc;

use flui_foundation::FrameEpoch;
use flui_runtime::sink::{FrameSink, SubmitVerdict};

/// A sink that keeps each frame's epoch and answers with the next verdict of
/// its script, then `Presented`.
struct Stamps {
    epochs: Vec<FrameEpoch>,
    script: Vec<SubmitVerdict>,
}

impl FrameSink for Stamps {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(&mut self, epoch: FrameEpoch, _: flui_layer::ScenePublication) -> SubmitVerdict {
        self.epochs.push(epoch);
        if self.script.is_empty() {
            SubmitVerdict::Presented
        } else {
            self.script.remove(0)
        }
    }
}

#[test]
fn each_submitted_frame_takes_the_next_epoch_of_its_presentation() {
    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(|| {}));
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("stamped"), 400.0, 400.0)
        .expect("mount text");
    // Two frames the sink does not show, each retried, then one it presents.
    let mut sink = Stamps {
        epochs: Vec::new(),
        script: vec![SubmitVerdict::Retry, SubmitVerdict::SurfaceStale],
    };
    let mut now = web_time::Instant::now();
    for _ in 0..3 {
        now += std::time::Duration::from_millis(16);
        let _ = runtime.pump(&mut flui_runtime::pump::SampledClock(now), &mut sink);
    }
    let first = FrameEpoch::ZERO.next();
    assert_eq!(
        sink.epochs,
        vec![first, first.next(), first.next().next()],
        "a retried frame is a new frame with the next epoch, never the same one again"
    );
}
