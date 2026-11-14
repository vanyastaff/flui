//! Each frame a UI runtime hands its sink carries the next epoch of the
//! runtime, the stamp a frame's settlement is matched by (ADR-0305 §4).

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
fn each_submitted_frame_takes_the_next_epoch_of_its_runtime() {
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

/// A sink that queues every frame and keeps each frame's epoch, as a threaded raster side does.
struct Queues {
    epochs: Vec<FrameEpoch>,
}

impl FrameSink for Queues {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn submit(&mut self, epoch: FrameEpoch, _: flui_layer::ScenePublication) -> SubmitVerdict {
        self.epochs.push(epoch);
        SubmitVerdict::Queued
    }
}

#[test]
fn a_queued_frame_settles_by_its_stamp_and_a_stale_settlement_is_ignored() {
    use flui_foundation::PresentationAddress;
    use flui_runtime::sink::{FrameSettlement, Settled};

    let mut runtime = crate::owner_publication::runtime_with_wake(Arc::new(|| {}));
    runtime
        .attach_root_widget_with_size(&flui_widgets::Text::new("queued"), 400.0, 400.0)
        .expect("mount text");
    let mut sink = Queues { epochs: Vec::new() };
    let mut now = web_time::Instant::now();
    now += std::time::Duration::from_millis(16);
    let outcome = runtime.pump(&mut flui_runtime::pump::SampledClock(now), &mut sink);
    assert!(
        !outcome.presented(),
        "a queued frame has not reached the screen yet"
    );
    let [epoch] = sink.epochs[..] else {
        panic!("one frame was submitted, got {:?}", sink.epochs);
    };
    assert!(
        !runtime.needs_redraw(),
        "the queued frame owes nothing until it settles"
    );
    let address = PresentationAddress {
        ui_runtime_id: runtime.id(),
        presentation_id: runtime.presentation_id(),
    };

    assert_eq!(
        runtime.settle_frame(FrameSettlement::new(
            address,
            epoch.next(),
            SubmitVerdict::Retry
        )),
        Settled::Stale,
        "no frame was submitted with that epoch"
    );
    assert!(!runtime.needs_redraw(), "a stale settlement arms nothing");
    assert_eq!(
        runtime.settle_frame(FrameSettlement::new(address, epoch, SubmitVerdict::Queued)),
        Settled::Stale,
        "a settlement never carries Queued"
    );

    assert_eq!(
        runtime.settle_frame(FrameSettlement::new(address, epoch, SubmitVerdict::Retry)),
        Settled::Applied
    );
    assert!(
        runtime.needs_redraw(),
        "the settled frame's verdict arms its retry"
    );
    assert_eq!(
        runtime.settle_frame(FrameSettlement::new(
            address,
            epoch,
            SubmitVerdict::Presented
        )),
        Settled::Stale,
        "a frame settles once"
    );

    now += std::time::Duration::from_millis(16);
    let _ = runtime.pump(&mut flui_runtime::pump::SampledClock(now), &mut sink);
    assert_eq!(
        sink.epochs,
        vec![epoch, epoch.next()],
        "the retry is a new frame with the next epoch"
    );
    let retried = epoch.next();
    assert_eq!(
        runtime.settle_frame(FrameSettlement::new(
            PresentationAddress {
                presentation_id: address.presentation_id,
                ui_runtime_id: crate::owner_publication::runtime_with_wake(Arc::new(|| {})).id(),
            },
            retried,
            SubmitVerdict::Presented,
        )),
        Settled::Stale,
        "another runtime's settlement names none of this runtime's frames"
    );
    assert_eq!(
        runtime.settle_frame(FrameSettlement::new(
            address,
            retried,
            SubmitVerdict::Presented
        )),
        Settled::Applied
    );
    assert!(!runtime.needs_redraw(), "a presented frame owes no retry");
}
