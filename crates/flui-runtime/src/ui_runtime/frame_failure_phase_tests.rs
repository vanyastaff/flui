use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};

use flui_widgets::SizedBox;

use super::{FrameFailureHandler, FrameFailureKind, SegmentPhase, UiRuntime};
use crate::testing::ScriptedSink;

fn with_quiet_panics<R>(f: impl FnOnce() -> R) -> R {
    type PanicHook = Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;

    struct HookRestore(Option<PanicHook>);

    impl Drop for HookRestore {
        fn drop(&mut self) {
            if let Some(hook) = self.0.take() {
                std::panic::set_hook(hook);
            }
        }
    }

    let _restore = HookRestore(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_| {}));
    f()
}

fn mount() -> UiRuntime {
    let ui_runtime = UiRuntime::for_test();
    ui_runtime
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("root attaches");
    ui_runtime
}

/// Every segment publishes its identity before either its probe or work
/// begins. The value survives unwind, and a clean retry reaches Scene.
pub(crate) fn every_segment_phase_survives_unwind_and_retries_to_scene() {
    let phases = [
        SegmentPhase::Build,
        SegmentPhase::Finalize,
        SegmentPhase::Pipeline,
        SegmentPhase::Tail,
        SegmentPhase::Scene,
    ];

    for phase in phases {
        let ui_runtime = mount();
        let reported_phases = Arc::new(StdMutex::new(Vec::new()));
        let reported_phases_for_handler = Arc::clone(&reported_phases);
        ui_runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            let FrameFailureKind::SegmentPanic { phase, .. } = &report.kind else {
                panic!("phase probe must produce a SegmentPanic report");
            };
            reported_phases_for_handler
                .lock()
                .expect("reported-phase mutex")
                .push(*phase);
        })));
        let probe_armed = Rc::new(Cell::new(true));
        let armed = Rc::clone(&probe_armed);
        if phase == SegmentPhase::Finalize {
            ui_runtime
                .presentations
                .primary()
                .arm_finalize_phase_panic();
        } else {
            ui_runtime.presentations.primary().set_segment_probe(
                phase,
                Some(Box::new(move |_| {
                    assert!(
                        !armed.replace(false),
                        "phase probe — intentional one-shot panic"
                    );
                })),
            );
        }
        let mut backend = ScriptedSink::always_presents();

        let first_presented = with_quiet_panics(|| {
            catch_unwind(AssertUnwindSafe(|| ui_runtime.render_frame(&mut backend)))
        })
        .expect("the phase panic must be contained");
        assert!(!first_presented, "{phase:?} failure must not present");
        assert_eq!(
            ui_runtime.presentations.primary().segment_phase(),
            phase,
            "the unwound phase must remain stored"
        );
        assert_eq!(
            *reported_phases.lock().expect("reported-phase mutex"),
            vec![phase],
            "the typed report must carry the exact unwound phase"
        );

        assert!(
            ui_runtime.wait_out_failure_retry_for_test(),
            "{phase:?} failure must arm its retry"
        );
        assert!(
            ui_runtime.render_frame(&mut backend),
            "{phase:?} must recover on the automatic retry"
        );
        assert_eq!(
            ui_runtime.presentations.primary().segment_phase(),
            SegmentPhase::Scene,
            "a clean retry must reach Scene"
        );
    }
}

/// Panics once, as the frame's paint span opens.
struct PanicsAsPaintOpens {
    armed: std::sync::atomic::AtomicBool,
}

impl tracing::Subscriber for PanicsAsPaintOpens {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        Some(tracing::level_filters::LevelFilter::TRACE)
    }
    fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let metadata = attributes.metadata();
        if metadata.target() == "flui.frame"
            && metadata.name() == "paint"
            && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            panic!("a subscriber that cannot open the paint span");
        }
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A panic out of a phase inside the render pipeline's frame, here a `tracing` subscriber as
/// the paint span opens, is a Pipeline segment panic. The presentation keeps its render tree
/// and the work the frame had not finished, so the automatic retry, at unchanged constraints,
/// reaches Scene and presents.
pub(crate) fn a_pipeline_frame_that_unwinds_keeps_its_tree_for_the_retry() {
    let ui_runtime = mount();
    let reported_phases = Arc::new(StdMutex::new(Vec::new()));
    let reported_phases_for_handler = Arc::clone(&reported_phases);
    ui_runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        let FrameFailureKind::SegmentPanic { phase, .. } = &report.kind else {
            panic!("the subscriber's panic must produce a SegmentPanic report");
        };
        reported_phases_for_handler
            .lock()
            .expect("reported-phase mutex")
            .push(*phase);
    })));
    let mut backend = ScriptedSink::always_presents();

    let subscriber = PanicsAsPaintOpens {
        armed: std::sync::atomic::AtomicBool::new(true),
    };
    // A second dispatcher keeps the callsites' interest from being cached against another
    // thread's empty default.
    let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let first_presented = tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        with_quiet_panics(|| {
            catch_unwind(AssertUnwindSafe(|| ui_runtime.render_frame(&mut backend)))
        })
    });
    tracing::callsite::rebuild_interest_cache();
    let first_presented = first_presented.expect("the pipeline panic must be contained");
    assert!(!first_presented, "the unwound frame must not present");
    assert_eq!(
        *reported_phases.lock().expect("reported-phase mutex"),
        vec![SegmentPhase::Pipeline],
        "the subscriber's panic is a Pipeline segment panic"
    );

    assert!(
        ui_runtime.wait_out_failure_retry_for_test(),
        "the pipeline failure must arm its retry"
    );
    assert!(
        ui_runtime.render_frame(&mut backend),
        "the retry runs on the presentation's own tree and presents"
    );
    assert_eq!(
        ui_runtime.presentations.primary().segment_phase(),
        SegmentPhase::Scene,
        "the retry reaches Scene"
    );
}
