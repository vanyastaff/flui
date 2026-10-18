//! Frame phase ordering through the public owning scheduler.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use flui_scheduler::scheduler::UpdateScheduler;

#[test]
fn test_full_frame_lifecycle() {
    // Test complete frame lifecycle: schedule -> begin -> callbacks -> end
    let scheduler = UpdateScheduler::new();

    let transient_called = Arc::new(AtomicU32::new(0));
    let persistent_called = Arc::new(AtomicU32::new(0));
    let post_frame_called = Arc::new(AtomicU32::new(0));

    // Register all callback types
    let t = Arc::clone(&transient_called);
    scheduler.schedule_frame_callback(Box::new(move |_timestamp| {
        t.fetch_add(1, Ordering::SeqCst);
    }));

    let p = Arc::clone(&persistent_called);
    scheduler.add_persistent_frame_callback(std::rc::Rc::new(move |_timing| {
        p.fetch_add(1, Ordering::SeqCst);
    }));

    let pf = Arc::clone(&post_frame_called);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        pf.fetch_add(1, Ordering::SeqCst);
    }));

    // Execute frame
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");

    // Verify all callbacks were called
    assert_eq!(transient_called.load(Ordering::SeqCst), 1);
    assert_eq!(persistent_called.load(Ordering::SeqCst), 1);
    assert_eq!(post_frame_called.load(Ordering::SeqCst), 1);

    // Execute another frame - only persistent should be called again
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");

    assert_eq!(transient_called.load(Ordering::SeqCst), 1); // Still 1
    assert_eq!(persistent_called.load(Ordering::SeqCst), 2); // Now 2
    assert_eq!(post_frame_called.load(Ordering::SeqCst), 1); // Still 1
}

/// The frame span's `period_us` field is read by the profiler's jank line, so
/// it must carry the budget's period, not the 60 Hz the scheduler starts with.
#[test]
fn frame_span_carries_the_budget_period() {
    use std::sync::Mutex;
    use std::time::Duration;

    use flui_scheduler::duration::{FrameDuration, InvalidDurationConfig};
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

    assert_eq!(
        FrameDuration::try_from_period(Duration::ZERO),
        Err(InvalidDurationConfig::ZeroPeriod)
    );
    // 59.94 Hz stays exact: it is not rounded to 60 frames per second.
    let ntsc = FrameDuration::try_from_period(Duration::from_nanos(16_683_333)).expect("period");
    assert_eq!(ntsc.as_micros(), 16_683);

    let seen = Arc::new(Mutex::new(Vec::new()));
    flui_testing::log_capture::disarm_interest_cache();
    tracing::subscriber::with_default(Periods(Arc::clone(&seen)), || {
        let scheduler = UpdateScheduler::new();
        let drive = || {
            flui_scheduler::OwnerFrame::new(&scheduler)
                .expect("the scheduler has no live owner frame")
                .drive_frame(
                    flui_scheduler::Instant::now(),
                    flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
                    || {},
                    || {},
                )
                .expect("frame");
        };
        drive();
        scheduler.set_budget_frame_duration(
            FrameDuration::try_from_period(Duration::from_micros(8_333)).expect("period"),
        );
        drive();
    });
    let seen = seen.lock().expect("period log");
    assert_eq!(seen.len(), 2);
    assert!(
        (16_600..=17_000).contains(&seen[0]),
        "the first frame reports the default 60 Hz-class budget, got {} us",
        seen[0]
    );
    assert_eq!(
        seen[1], 8_333,
        "the span reports the period the budget judged the frame against"
    );
}
