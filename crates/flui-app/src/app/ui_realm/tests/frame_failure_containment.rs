use std::sync::Mutex as StdMutex;

use flui_widgets::SizedBox;

use super::*;
use crate::app::frame_failure::{FrameFailureHandler, FrameFailureKind, PanicText};

/// Failure reports collected through the real registered-handler
/// route — the same `FrameFailureHandler` an embedder registers via
/// `AppConfig::with_frame_failure_handler`. Flattened to owned
/// fields because `FrameFailureReport` itself is delivered by
/// reference and deliberately not `Clone`.
#[derive(Debug, Clone, PartialEq)]
struct SeenFailure {
    presentation: PresentationId,
    realm: RealmId,
    disposition: FailureDisposition,
    consecutive: u32,
    kind: SeenKind,
}

#[derive(Debug, Clone, PartialEq)]
enum SeenKind {
    SegmentPanic {
        message: PanicText,
        phase: SegmentPhase,
        internal_invariant: bool,
    },
    Pipeline {
        error: String,
    },
    RecoveredPanic {
        hook: flui_view::LifecycleHook,
    },
}

fn install_collecting_handler(realm: &UiRealm) -> Arc<StdMutex<Vec<SeenFailure>>> {
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        let kind = match &report.kind {
            FrameFailureKind::SegmentPanic {
                message,
                phase,
                internal_invariant,
            } => SeenKind::SegmentPanic {
                message: message.clone(),
                phase: *phase,
                internal_invariant: *internal_invariant,
            },
            FrameFailureKind::Pipeline { error } => SeenKind::Pipeline {
                error: error.to_string(),
            },
            FrameFailureKind::RecoveredPanic { hook, .. } => {
                SeenKind::RecoveredPanic { hook: *hook }
            }
        };
        sink.lock().expect("handler mutex").push(SeenFailure {
            presentation: report.address.presentation_id,
            realm: report.address.realm_id,
            disposition: report.disposition,
            consecutive: report.consecutive_failures,
            kind,
        });
    })));
    seen
}

/// Silence the default panic hook for the duration of `f` so the
/// intentional panics these tests throw do not spam captured
/// output; the panics themselves still unwind normally.
fn with_quiet_panics<R>(f: impl FnOnce() -> R) -> R {
    // RAII, not a trailing `set_hook`: the panic hook is
    // process-global, so if `f` itself panics out of this helper
    // (an assertion failure inside the closure), a non-guarded
    // restore would be skipped and every LATER test in the same
    // process would run with silenced panic diagnostics. nextest
    // is process-per-test, but plain `cargo test` shares one
    // process — restore must survive the unwind.
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

// ====================================================================
// The frame-transaction boundary itself: whatever escapes a
// presentation's build+layout+paint segment — of any origin — must
// be contained at `UiRealm::draw_frame_entered`'s per-presentation
// `catch_unwind`, not unwind out of the realm pump. Before that
// boundary existed, ANY panic reaching this deep (a bug in FLUI's
// own pipeline, a user callback nothing else had caught) unwound
// straight through the realm's per-presentation loop and killed the
// process via the runner's `resume_unwind`.
//
// User lifecycle hooks now have narrower per-child boundaries,
// including dense reconcile insertion and GlobalKey retake. They do
// not exercise this outer presentation boundary. The headline test
// therefore uses the dedicated segment probe to inject an unwind at
// precisely this boundary without depending on a lifecycle escape.
// ====================================================================

/// The headline containment claim, driven through a one-shot segment
/// probe: the panic is contained to presentation A's own frame, the
/// pump returns, sibling B's segment still runs and presents in the
/// same pump, the typed report names A with causal detail, a retry is
/// armed, and the next pump recovers A cleanly. The probe runs before
/// `WidgetsBinding::draw_frame`, so this test intentionally makes no
/// building-flag claim; lifecycle insertion panics are now bounded by
/// the dense reconciler itself.
#[test]
fn an_escaped_segment_panic_is_contained_to_its_own_presentation_and_the_sibling_still_frames() {
    let mut realm = UiRealm::for_test();
    let a_id = realm.presentation_id();
    let b_id = realm.install_second_presentation_for_test();
    let seen = install_collecting_handler(&realm);
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("A attaches");
    realm
        .attach_root_widget_to_for_test(b_id, &SizedBox::new(20.0, 20.0))
        .expect("B attaches");

    let mut backend = TestRasterBackend::always_presents();

    // Pump 1: both presentations mount and frame cleanly.
    assert!(
        realm.render_frame_entered(&mut backend),
        "pump 1: a clean two-presentation frame must present"
    );
    assert!(seen.lock().expect("mutex").is_empty(), "no failures yet");
    let a_frames_after_pump_1 = realm.presentations.primary().frames_rendered();
    let b_flushes_after_pump_1 = realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .flush_count();

    // Inject exactly one segment failure for A. The closure remains
    // installed for pump 3 but disarms itself before panicking, so
    // the clean retry also proves the presentation can make progress.
    let probe_armed = Rc::new(Cell::new(true));
    let armed = Rc::clone(&probe_armed);
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(move || {
            assert!(
                !armed.replace(false),
                "segment probe — intentional test panic"
            );
        })),
    );
    realm.request_redraw();
    // Give B real work too, so this same pump proves B's segment
    // still runs AFTER A's failure (A is primary and iterates
    // first).
    realm.enter(|realm| {
        let b = realm.presentations.get(b_id).expect("B installed");
        b.pipeline().with_mut(|owner| {
            if let Some(root_id) = owner.root_id() {
                owner.mark_needs_paint(root_id);
            }
        });
    });
    realm.mark_rendered();

    // Pump 2: A's segment probe panics. The pump must return and B
    // must still frame after A's earlier failure.
    let outcome = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    });
    let presented = outcome.expect(
        "a panic escaping one presentation's segment must be contained at the \
         frame-transaction boundary, not unwind out of the realm pump",
    );
    assert!(
        presented,
        "sibling B's own segment must still produce and present in the same pump"
    );
    assert_eq!(
        realm.presentations.primary().frames_rendered(),
        a_frames_after_pump_1,
        "A's failed segment must not be counted as rendered"
    );
    assert_eq!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .flush_count(),
        b_flushes_after_pump_1 + 1,
        "B's segment must run despite A's earlier failure in the same loop"
    );
    assert!(
        realm.needs_redraw(),
        "a contained frame failure must arm a retry, not settle as rendered"
    );
    {
        let seen = seen.lock().expect("mutex");
        assert_eq!(seen.len(), 1, "exactly one failure report: {seen:?}");
        assert_eq!(seen[0].presentation, a_id, "the report must name A");
        assert_eq!(seen[0].realm, realm.realm_id());
        assert_eq!(seen[0].disposition, FailureDisposition::FrameDropped);
        assert_eq!(seen[0].consecutive, 1);
        match &seen[0].kind {
            SeenKind::SegmentPanic {
                message,
                phase,
                internal_invariant,
            } => {
                #[cfg(debug_assertions)]
                {
                    let PanicText::Verbatim(message) = message else {
                        panic!("debug-default report must retain the panic text: {message:?}");
                    };
                    assert!(
                        message.contains("segment probe"),
                        "causal detail (the panic message) must reach the report; got \
                         {message:?}"
                    );
                }
                #[cfg(not(debug_assertions))]
                assert_eq!(
                    message,
                    &PanicText::Redacted,
                    "release-default reports must retain no panic text"
                );
                assert_eq!(*phase, SegmentPhase::Build);
                assert!(
                    !internal_invariant,
                    "an application panic carries no BUG: prefix"
                );
            }
            other => panic!("expected SegmentPanic, got {other:?}"),
        }
    }

    // Pump 3: the probe is now disarmed. Give A real pipeline work so
    // the retry proves that presentation resumes normal progress.
    realm.enter(|realm| {
        let a = realm.presentations.get(a_id).expect("A installed");
        a.pipeline().with_mut(|owner| {
            if let Some(root_id) = owner.root_id() {
                owner.mark_needs_paint(root_id);
            }
        });
    });
    realm.request_redraw();
    let presented = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("the recovery pump must not panic");
    assert!(presented, "the recovery pump must present");
    assert_eq!(
        realm.presentations.primary().frames_rendered(),
        a_frames_after_pump_1 + 1,
        "A's clean retry must resume rendering"
    );
    assert_eq!(
        seen.lock().expect("mutex").len(),
        1,
        "the one-shot probe must not produce a second failure report"
    );
}

/// Last-good retention, distinguished from zero-value fake
/// recovery: a failed frame submits NOTHING — `render_scene` is
/// never called with a blank/empty stand-in scene — so whatever the
/// surface last presented stays on screen rather than being replaced
/// by a zero-value fake recovery.
#[test]
fn a_failed_frame_submits_nothing_rather_than_a_blank_scene() {
    let realm = UiRealm::for_test();
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("attaches");
    let mut backend = TestRasterBackend::always_presents();

    assert!(
        realm.render_frame_entered(&mut backend),
        "pump 1 presents real content"
    );
    assert_eq!(backend.render_scene_calls, 1);

    // Inject a segment failure and give the presentation demand so
    // its segment genuinely runs (a skipped segment would prove
    // nothing).
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(|| {
            panic!("segment probe — intentional test panic");
        })),
    );
    realm.request_redraw();
    realm.mark_rendered();

    let presented = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("the failure must be contained");
    assert!(!presented, "a failed frame must never present");
    assert_eq!(
        backend.render_scene_calls, 1,
        "the failed frame must not reach render_scene at all — retention means \
         the previous submission stands, not that a zero/blank scene replaced it"
    );
}

/// The consecutive-failure streak counts uninterrupted failures and
/// resets on the next cleanly completed segment — the field an
/// embedder keys escalation off.
#[test]
fn consecutive_failures_count_up_and_reset_on_a_clean_segment() {
    let realm = UiRealm::for_test();
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("attaches");
    let seen = install_collecting_handler(&realm);
    let mut backend = TestRasterBackend::always_presents();

    let probe_armed = Rc::new(Cell::new(true));
    let armed = Rc::clone(&probe_armed);
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(move || {
            assert!(!armed.get(), "segment probe — intentional test panic");
        })),
    );

    let pump = |realm: &UiRealm, backend: &mut TestRasterBackend| {
        realm.request_redraw();
        realm.mark_rendered();
        with_quiet_panics(|| catch_unwind(AssertUnwindSafe(|| realm.render_frame_entered(backend))))
            .expect("every failure must be contained")
    };

    pump(&realm, &mut backend); // fails: streak 1
    pump(&realm, &mut backend); // fails: streak 2
    probe_armed.set(false);
    pump(&realm, &mut backend); // clean: streak resets
    probe_armed.set(true);
    pump(&realm, &mut backend); // fails: streak restarts at 1

    let consecutive: Vec<u32> = seen
        .lock()
        .expect("mutex")
        .iter()
        .map(|failure| failure.consecutive)
        .collect();
    assert_eq!(
        consecutive,
        vec![1, 2, 1],
        "two uninterrupted failures count 1,2; a clean segment resets; the next \
         failure restarts at 1"
    );
}

/// A structured pipeline error (here: a root render object whose
/// layout panics, surfaced by the pipeline as
/// `RenderError::Poisoned`) travels the SAME typed report route as
/// a boundary-caught panic — not only `tracing`.
#[test]
fn a_pipeline_error_reaches_the_typed_report_route() {
    let realm = UiRealm::for_test();
    let seen = install_collecting_handler(&realm);
    realm.pipeline_for_test().with_mut(|owner| {
        let root_id = owner.insert(Box::new(PanicOnLayoutForReportBox)
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        owner.set_root_id(Some(root_id));
    });

    let mut backend = TestRasterBackend::always_presents();
    let presented = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("a pipeline error is contained (pre-existing) and reported (this test)");
    assert!(!presented);

    let seen = seen.lock().expect("mutex");
    assert_eq!(seen.len(), 1, "one pipeline failure report: {seen:?}");
    assert_eq!(seen[0].presentation, realm.presentation_id());
    assert_eq!(seen[0].disposition, FailureDisposition::FrameDropped);
    assert_eq!(seen[0].consecutive, 1);
    match &seen[0].kind {
        SeenKind::Pipeline { error } => {
            assert!(
                error.contains("panicked during layout"),
                "the typed report must carry the pipeline's own error; got {error:?}"
            );
        }
        other => panic!("expected Pipeline, got {other:?}"),
    }
}

/// Root render box whose layout panics — local twin of the sibling
/// module's private helper, for the pipeline-error report test.
#[derive(Debug)]
struct PanicOnLayoutForReportBox;

impl flui_foundation::Diagnosticable for PanicOnLayoutForReportBox {}

impl flui_rendering::traits::RenderBox for PanicOnLayoutForReportBox {
    type Arity = flui_rendering::prelude::Leaf;
    type ParentData = flui_rendering::prelude::BoxParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_types::Size {
        panic!("PanicOnLayoutForReportBox::perform_layout -- intentional test panic");
    }
}

/// A failed pump must not actively re-probe the tree either: the
/// stationary-device re-hit-test that normally follows a frame
/// (mouse-tracker hover maintenance) reads whatever geometry the
/// failed segment left mid-commit, so it is skipped for that pump —
/// hover state holds the last cleanly committed version. (Pointer
/// EVENTS arriving before the retry still hit-test the live tree;
/// that residual gap is named in ADR-0048, not claimed closed.)
#[test]
fn a_failed_pump_skips_the_stationary_device_re_hit_test() {
    let (realm, hits) = super::super::frame_commit_state_tests::mount_hit_counting_root();

    let mut backend = TestRasterBackend::always_presents();
    let _ = realm.render_frame_entered(&mut backend);
    let hits_after_clean_pump = hits.load(Ordering::Relaxed);
    assert!(
        hits_after_clean_pump > 0,
        "precondition: a clean pump re-hit-tests the tracked stationary device"
    );

    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(|| {
            panic!("segment probe — intentional test panic");
        })),
    );
    realm.request_redraw();
    let _ = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("contained");
    assert_eq!(
        hits.load(Ordering::Relaxed),
        hits_after_clean_pump,
        "a failed pump must not re-hit-test stationary devices against the \
         mid-commit tree"
    );
}

/// `docs/PANIC-POLICY.md`'s `BUG:` convention is classified, not
/// blended into application failures: a `BUG:`-prefixed payload
/// reports `internal_invariant = true`.
#[test]
fn a_bug_prefixed_panic_is_reported_as_an_internal_invariant() {
    let realm = UiRealm::for_test();
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("attaches");
    let seen = install_collecting_handler(&realm);
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(|| {
            panic!("BUG: intentional invariant-violation payload for this test");
        })),
    );
    realm.request_redraw();

    let mut backend = TestRasterBackend::always_presents();
    let _ = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("contained");

    let seen = seen.lock().expect("mutex");
    assert_eq!(seen.len(), 1);
    match &seen[0].kind {
        SeenKind::SegmentPanic {
            internal_invariant, ..
        } => {
            assert!(
                internal_invariant,
                "a BUG:-prefixed payload must be classified as an internal invariant"
            );
        }
        other => panic!("expected SegmentPanic, got {other:?}"),
    }
}

/// The boundary's own blind spot, closed: the registered handler is
/// EMBEDDER code invoked outside the per-presentation
/// `catch_unwind` (a segment-panic report is delivered from the
/// boundary's `Err` arm, after it returned) — an uncontained
/// handler panic reopened the exact process-fatal path this
/// boundary exists to close. The delivery is now contained
/// per call: siblings keep framing in the same pump, and the
/// handler stays registered for later reports.
#[test]
fn a_panicking_failure_handler_does_not_escape_the_frame_boundary() {
    use std::sync::atomic::AtomicU32;

    let mut realm = UiRealm::for_test();
    let b_id = realm.install_second_presentation_for_test();
    realm
        .attach_root_widget(&SizedBox::new(10.0, 10.0))
        .expect("A attaches");
    realm
        .attach_root_widget_to_for_test(b_id, &SizedBox::new(20.0, 20.0))
        .expect("B attaches");

    let handler_calls = Arc::new(AtomicU32::new(0));
    let calls = Arc::clone(&handler_calls);
    realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |_report| {
        calls.fetch_add(1, Ordering::Relaxed);
        panic!("FrameFailureHandler — intentional embedder-bug test panic");
    })));

    let mut backend = TestRasterBackend::always_presents();
    assert!(
        realm.render_frame_entered(&mut backend),
        "pump 1: both presentations frame cleanly"
    );
    let b_flushes_after_pump_1 = realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .flush_count();

    // A (primary, iterated FIRST) fails; B has real work, so this
    // same pump proves B's segment still ran after both A's
    // failure AND the handler's own panic during its delivery.
    realm.presentations.primary().set_segment_probe(
        SegmentPhase::Build,
        Some(Box::new(|| {
            panic!("segment probe — intentional test panic");
        })),
    );
    realm.request_redraw();
    realm.enter(|realm| {
        let b = realm.presentations.get(b_id).expect("B installed");
        b.pipeline().with_mut(|owner| {
            if let Some(root_id) = owner.root_id() {
                owner.mark_needs_paint(root_id);
            }
        });
    });

    let outcome = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    });
    let presented = outcome.expect(
        "a panicking FrameFailureHandler must be contained at its delivery site, \
         not unwind out of the realm pump",
    );
    assert!(
        presented,
        "sibling B must still produce and present despite the handler's panic"
    );
    assert_eq!(
        realm
            .presentations
            .get(b_id)
            .expect("B installed")
            .flush_count(),
        b_flushes_after_pump_1 + 1,
        "B's segment must run despite A's failure and the handler panic"
    );
    assert_eq!(
        handler_calls.load(Ordering::Relaxed),
        1,
        "exactly one delivery for one failure — never a retry loop"
    );

    // The handler stays registered: the next failure is delivered
    // (and contained) again.
    realm.request_redraw();
    let _ = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("second failing pump must also be contained");
    assert_eq!(
        handler_calls.load(Ordering::Relaxed),
        2,
        "a panicking handler stays registered and receives later reports"
    );
}

/// The pipeline-report variant of the handler blind spot: that
/// delivery happens INSIDE the segment (from
/// `draw_frame_for_presentation`'s `Err` arm), so before the fix
/// the boundary caught the HANDLER's panic as a segment panic and
/// re-reported it — invoking the same panicking handler a second
/// time, now outside any catch. Containment at the delivery site
/// means exactly one delivery, of the Pipeline kind, per failure.
#[test]
fn a_panicking_handler_during_a_pipeline_report_is_delivered_once_not_re_reported() {
    let realm = UiRealm::for_test();
    let delivered = Arc::new(StdMutex::new(Vec::new()));
    let sink = Arc::clone(&delivered);
    realm.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        sink.lock().expect("mutex").push(match &report.kind {
            FrameFailureKind::SegmentPanic { .. } => "segment_panic",
            FrameFailureKind::Pipeline { .. } => "pipeline",
            FrameFailureKind::RecoveredPanic { .. } => "recovered_panic",
        });
        panic!("FrameFailureHandler — intentional embedder-bug test panic");
    })));
    realm.pipeline_for_test().with_mut(|owner| {
        let root_id = owner.insert(Box::new(PanicOnLayoutForReportBox)
            as Box<
                dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>,
            >);
        owner.set_root_id(Some(root_id));
    });

    let mut backend = TestRasterBackend::always_presents();
    let presented = with_quiet_panics(|| {
        catch_unwind(AssertUnwindSafe(|| {
            realm.render_frame_entered(&mut backend)
        }))
    })
    .expect("a handler panic during a pipeline report must be contained");
    assert!(!presented);
    assert_eq!(
        delivered.lock().expect("mutex").as_slice(),
        ["pipeline"],
        "one failure, one delivery, of the pipeline kind — the handler's own \
         panic must not be re-reported as a segment panic (which would invoke \
         the panicking handler a second time)"
    );
}
