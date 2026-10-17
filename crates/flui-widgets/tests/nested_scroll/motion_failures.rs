//! Completion-time producer failures cannot strand accepted end delivery.

use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
struct DerivativeFailure(Arc<AtomicBool>);

impl flui_widgets::ScrollPhysics for DerivativeFailure {
    fn apply_boundary_conditions(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        proposed: f64,
    ) -> f64 {
        proposed.clamp(metrics.min_scroll_extent, metrics.max_scroll_extent)
    }
    fn create_ballistic_simulation(
        &self,
        metrics: &flui_widgets::ScrollMetrics,
        velocity: f64,
    ) -> Option<Box<dyn flui_animation::Simulation>> {
        (velocity != 0.0).then(|| {
            Box::new(FiniteMotion {
                initial: metrics.pixels,
                fail: self.0.clone(),
            }) as Box<dyn flui_animation::Simulation>
        })
    }
}

struct FiniteMotion {
    initial: f64,
    fail: Arc<AtomicBool>,
}
impl flui_animation::Simulation for FiniteMotion {
    fn x(&self, time: f64) -> f64 {
        self.initial + 100.0 * time.clamp(0.0, 0.05)
    }
    fn dx(&self, time: f64) -> f64 {
        assert!(
            time < 0.05 || !self.fail.swap(false, Ordering::SeqCst),
            "ballistic derivative failure"
        );
        if time < 0.05 { 100.0 } else { 0.0 }
    }
    fn is_done(&self, time: f64) -> bool {
        time >= 0.05
    }
}

struct EndFailure(bool);
impl NestedScrollConnection for EndFailure {
    fn sequence_ended(&self, _: InputSequenceId, _: ScrollEndReason) {
        assert!(!self.0, "competing motion end failure");
    }
}

pub(crate) fn a_failing_final_derivative_completes_cleanup_and_preserves_first_failure() {
    for competing in [false, true] {
        let log = Rc::new(RefCell::new(Vec::new()));
        let controller = ScrollController::new();
        let vsync = flui_animation::Vsync::new();
        let fail = Arc::new(AtomicBool::new(true));
        let content = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(Rc::new(EndFailure(competing))).child(
                NestedScroll::new(Rc::new(MotionProbe(log.clone()))).child(
                    Scrollable::new()
                        .controller(controller.clone())
                        .physics(Arc::new(DerivativeFailure(fail.clone())))
                        .child(SizedBox::new(100.0, 5100.0)),
                ),
            ),
        );
        let mut laid = lay_out(content, tight(100.0, 100.0));
        laid.adopt_vsync(vsync).expect("owner clock");
        laid.dispatch_pointer_down(50.0, 80.0);
        for y in [60.0, 40.0, 20.0] {
            laid.dispatch_pointer_move_after(50.0, y, Duration::from_millis(20));
        }
        laid.dispatch_pointer_up(50.0, 20.0);
        let released_pixels = controller.pixels();
        let ((), reports) = flui_testing::log_capture::capture(|| {
            for _ in 0..3 {
                laid.pump_for(Duration::from_millis(100));
            }
        });
        assert!(
            !fail.load(Ordering::SeqCst),
            "the real final derivative was sampled"
        );
        let failures: Vec<_> = reports
            .records()
            .iter()
            .filter(|record| record.message == "presentation sampling failure contained")
            .collect();
        assert_eq!(failures.len(), 1, "one failed admitted frame: {reports}");
        assert!(
            failures[0]
                .field("panic_message")
                .is_some_and(|message| message.contains("ballistic derivative failure")),
            "first producer failure survives cleanup: {reports}"
        );
        assert_eq!(
            controller.pixels(),
            released_pixels,
            "a failed final derivative cannot publish through a direct value-listener tail"
        );
        assert_eq!(
            log.borrow()
                .iter()
                .filter(|event| matches!(event, MotionEvent::End(_, ScrollEndReason::Cancelled)))
                .count(),
            1,
            "cleanup completes even if end handlers compete"
        );
        assert!(
            !controller.position().is_scrolling(),
            "completion cannot leave an activity live"
        );
        let before = controller.pixels();
        laid.dispatch_pointer_down(50.0, 80.0);
        laid.dispatch_pointer_move(50.0, 40.0);
        assert_eq!(
            controller.pixels(),
            before + 40.0,
            "a later source remains usable"
        );
        let (cancelled, _) = flui_testing::log_capture::capture(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                laid.dispatch_pointer_cancel();
            }))
        });
        assert_eq!(
            cancelled.is_err(),
            competing,
            "the configured end handler still has its explicit policy on the next sequence"
        );
        if let Err(payload) = cancelled {
            assert_eq!(
                flui_foundation::panic::payload_text(payload.as_ref()),
                Some("competing motion end failure")
            );
        }
        laid.pump_for(Duration::from_millis(16));
        assert_eq!(
            log.borrow()
                .iter()
                .filter(|event| matches!(event, MotionEvent::End(..)))
                .count(),
            2,
            "no accepted terminal event is replayed after recovery"
        );
    }
}
