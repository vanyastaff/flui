use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};

use flui_widgets::SizedBox;

use super::{SegmentPhase, UiRealm};
use crate::app::raster_test_support::TestRasterBackend;

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

fn capturing_backend(submitted: Arc<StdMutex<Vec<String>>>) -> TestRasterBackend {
    TestRasterBackend::new(move |_, scene| {
        submitted
            .lock()
            .expect("scene capture mutex")
            .push(format!("{:?}", scene.layer_tree()));
        Ok(true)
    })
}

/// A panic after the pipeline has consumed paint dirtiness must re-dirty
/// that exact presentation at the containment boundary. A wake alone opens
/// the segment gate but gives the pipeline nothing to reproduce.
#[test]
fn a_tail_panic_repaints_and_presents_on_the_automatic_retry() {
    let realm = mount();
    let probe_armed = Rc::new(Cell::new(true));
    let armed = Rc::clone(&probe_armed);
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Tail,
        Some(Box::new(move || {
            assert!(
                !armed.replace(false),
                "tail probe — intentional one-shot panic"
            );
        })),
    );
    let submitted = Arc::new(StdMutex::new(Vec::new()));
    let mut backend = capturing_backend(Arc::clone(&submitted));

    let first_presented = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("the Tail panic must be contained");
    assert!(!first_presented, "the failed attempt must not present");
    assert_eq!(backend.render_scene_calls, 0);
    assert_eq!(realm.frames_rendered(), 0);
    assert_eq!(
        realm.presentations.primary().segment_phase(),
        SegmentPhase::Tail,
        "the last-entered phase must survive unwind"
    );
    assert!(realm.needs_redraw(), "the failure must arm a retry");

    let second_presented = realm.render_frame_entered(&mut backend);
    assert!(
        second_presented,
        "the automatic retry must repaint and present without external dirtiness"
    );
    assert_eq!(backend.render_scene_calls, 1);
    assert_eq!(realm.frames_rendered(), 1);
    assert_eq!(
        realm.presentations.primary().segment_phase(),
        SegmentPhase::Scene
    );

    let fresh = mount();
    let expected = Arc::new(StdMutex::new(Vec::new()));
    let mut fresh_backend = capturing_backend(Arc::clone(&expected));
    assert!(fresh.render_frame_entered(&mut fresh_backend));
    assert_eq!(
        *submitted.lock().expect("scene capture mutex"),
        *expected.lock().expect("fresh scene capture mutex"),
        "the retried scene must equal an identical fresh realm's scene"
    );
}

/// Every segment publishes its identity before either its probe or work
/// begins. The value survives unwind, and a clean retry reaches Scene.
#[test]
fn every_segment_phase_survives_unwind_and_retries_to_scene() {
    let phases = [
        SegmentPhase::Build,
        SegmentPhase::Finalize,
        SegmentPhase::Pipeline,
        SegmentPhase::Tail,
        SegmentPhase::Scene,
    ];

    for phase in phases {
        let realm = mount();
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
        let mut backend = TestRasterBackend::always_presents();

        let first_presented = with_quiet_panics(|| {
            catch_unwind(AssertUnwindSafe(|| {
                realm.render_frame_entered(&mut backend)
            }))
        })
        .expect("the phase panic must be contained");
        assert!(!first_presented, "{phase:?} failure must not present");
        assert_eq!(
            realm.presentations.primary().segment_phase(),
            phase,
            "the unwound phase must remain stored"
        );

        assert!(
            realm.render_frame_entered(&mut backend),
            "{phase:?} must recover on the automatic retry"
        );
        assert_eq!(
            realm.presentations.primary().segment_phase(),
            SegmentPhase::Scene,
            "a clean retry must reach Scene"
        );
    }
}

/// Exact-addressing discriminator: a later sibling may become the pump's
/// producer after A fails, but must not receive A's repaint.
#[test]
fn a_tail_failure_repaints_a_even_when_later_b_paints_in_the_same_pump() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("A attaches");
    realm
        .attach_root_widget_to_for_test(b_id, &SizedBox::new(20.0, 20.0))
        .expect("B attaches");

    let probe_armed = Rc::new(Cell::new(true));
    let armed = Rc::clone(&probe_armed);
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Tail,
        Some(Box::new(move || {
            assert!(
                !armed.replace(false),
                "A Tail probe — intentional one-shot panic"
            );
        })),
    );
    let mut backend = TestRasterBackend::always_presents();

    assert!(
        with_quiet_panics(|| {
            catch_unwind(AssertUnwindSafe(|| {
                realm.render_frame_entered(&mut backend)
            }))
        })
        .expect("A's Tail panic must be contained"),
        "later B must still paint and present in the failing pump"
    );
    assert_eq!(backend.render_scene_calls, 1, "only B submits in pump 1");
    assert_eq!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .frames_rendered(),
        0
    );
    assert_eq!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .frames_rendered(),
        1
    );

    assert!(
        realm.render_frame_entered(&mut backend),
        "A must repaint and present on the automatic retry"
    );
    assert_eq!(backend.render_scene_calls, 2, "pump 2 submits only A");
    assert_eq!(
        realm
            .presentations
            .get(a_id)
            .expect("A installed")
            .frames_rendered(),
        1
    );
    assert_eq!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .frames_rendered(),
        1,
        "B must not receive A's repaint mark"
    );
}
