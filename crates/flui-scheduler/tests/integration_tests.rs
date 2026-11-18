//! Frame phase ordering through the public owning scheduler.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use flui_scheduler::__runtime::DriveFrame as _;
use flui_scheduler::UpdateScheduler;

/// The pipeline runs in every frame; a post-frame callback runs once, after
/// the pipeline of the frame it was registered for.
#[test]
fn a_post_frame_callback_runs_once_after_the_pipeline() {
    let scheduler = UpdateScheduler::new();
    let owner = flui_scheduler::OwnerFrame::try_new(&scheduler)
        .expect("the scheduler has no live owner frame");
    let pipeline_runs = Arc::new(AtomicU32::new(0));
    let post_frame_saw = Arc::new(AtomicU32::new(0));

    let runs = Arc::clone(&pipeline_runs);
    let saw = Arc::clone(&post_frame_saw);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        saw.store(runs.load(Ordering::SeqCst), Ordering::SeqCst);
    }));

    for _ in 0..2 {
        owner
            .drive_frame(
                flui_scheduler::Instant::now(),
                || {},
                || {
                    pipeline_runs.fetch_add(1, Ordering::SeqCst);
                },
            )
            .expect("live owner frame");
    }

    assert_eq!(pipeline_runs.load(Ordering::SeqCst), 2);
    assert_eq!(
        post_frame_saw.load(Ordering::SeqCst),
        1,
        "the callback ran once, after the first frame's pipeline"
    );
}

/// The frame span's `period_us` field is read by the profiler's jank line, so
/// it must carry the display period the runtime set, not the 60 Hz the
/// scheduler starts with.
#[test]
fn frame_span_carries_the_display_period() {
    use std::sync::Mutex;
    use std::time::Duration;

    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};

    #[derive(Default)]
    struct PeriodVisitor(Option<u64>);
    impl Visit for PeriodVisitor {
        fn record_u64(&mut self, field: &Field, value: u64) {
            if field.name() == "period_us" {
                self.0 = Some(value);
            }
        }
        fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
    }
    /// Collects every `period_us` value recorded on any span.
    struct Periods(Arc<Mutex<Vec<u64>>>);
    impl tracing::Subscriber for Periods {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, values: &Record<'_>) {
            let mut visitor = PeriodVisitor::default();
            values.record(&mut visitor);
            if let Some(period) = visitor.0 {
                self.0.lock().expect("period log").push(period);
            }
        }
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, _: &tracing::Event<'_>) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    flui_testing::log_capture::disarm_interest_cache();
    tracing::subscriber::with_default(Periods(Arc::clone(&seen)), || {
        let scheduler = UpdateScheduler::new();
        let drive = || {
            flui_scheduler::OwnerFrame::try_new(&scheduler)
                .expect("the scheduler has no live owner frame")
                .drive_frame(flui_scheduler::Instant::now(), || {}, || {})
                .expect("frame");
        };
        drive();
        // 119.88 Hz: the span reports whole microseconds.
        scheduler.set_display_period(Duration::from_nanos(8_341_675));
        drive();
    });
    let seen = seen.lock().expect("period log");
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0], 16_666,
        "the first frame reports the default 60 Hz period"
    );
    assert_eq!(
        seen[1], 8_341,
        "the span reports the period the runtime set"
    );
}
