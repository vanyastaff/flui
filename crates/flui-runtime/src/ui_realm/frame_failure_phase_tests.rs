use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};

use flui_widgets::SizedBox;

use super::{FrameFailureHandler, FrameFailureKind, SegmentPhase, UiRealm};
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

fn mount() -> UiRealm {
    let realm = UiRealm::for_test();
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("root attaches");
    realm
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
        let realm = mount();
        let reported_phases = Arc::new(StdMutex::new(Vec::new()));
        let reported_phases_for_handler = Arc::clone(&reported_phases);
        realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
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
            realm.presentations.primary().arm_finalize_phase_panic();
        } else {
            realm.presentations.primary().set_segment_probe(
                phase,
                Some(Box::new(move || {
                    assert!(
                        !armed.replace(false),
                        "phase probe — intentional one-shot panic"
                    );
                })),
            );
        }
        let mut backend = ScriptedSink::always_presents();

        let first_presented = with_quiet_panics(|| {
            catch_unwind(AssertUnwindSafe(|| realm.render_frame(&mut backend)))
        })
        .expect("the phase panic must be contained");
        assert!(!first_presented, "{phase:?} failure must not present");
        assert_eq!(
            realm.presentations.primary().segment_phase(),
            phase,
            "the unwound phase must remain stored"
        );
        assert_eq!(
            *reported_phases.lock().expect("reported-phase mutex"),
            vec![phase],
            "the typed report must carry the exact unwound phase"
        );

        assert!(
            realm.render_frame(&mut backend),
            "{phase:?} must recover on the automatic retry"
        );
        assert_eq!(
            realm.presentations.primary().segment_phase(),
            SegmentPhase::Scene,
            "a clean retry must reach Scene"
        );
    }
}
