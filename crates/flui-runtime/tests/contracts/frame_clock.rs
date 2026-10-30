//! The host-selected timestamp owns all animation samples in one pump.

use flui_animation::{Animation, AnimationController, Vsync};
use flui_foundation::{Listenable, ManualClock, MonotonicClock};
use flui_runtime::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport, PanicText, SamplingFailureOrigin, SamplingStage,
};
use flui_runtime::pump::{FrameClockSource, SampledClock};
use flui_runtime::sink::{FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_view::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Duration;

struct AdvancingHostClock {
    selected: web_time::Instant,
    host: ManualClock,
    reads: usize,
}

impl FrameClockSource for AdvancingHostClock {
    fn frame_time(&mut self) -> web_time::Instant {
        self.reads += 1;
        self.host.advance(Duration::from_secs(2));
        self.selected
    }
}

struct Sink;

#[derive(Debug, PartialEq)]
struct RecordedSampling {
    first: (SamplingStage, SamplingFailureOrigin),
    blocking: Option<(SamplingStage, SamplingFailureOrigin)>,
    admitted_attempt: bool,
    message: PanicText,
    internal_invariant: bool,
}

#[derive(Debug, PartialEq)]
struct RecordedFailure {
    address: flui_foundation::PresentationAddress,
    disposition: FailureDisposition,
    consecutive_failures: u32,
    sampling: Option<RecordedSampling>,
}

fn record_failure(report: &FrameFailureReport) -> RecordedFailure {
    let sampling = match &report.kind {
        FrameFailureKind::SamplingPanic {
            first,
            blocking,
            admitted_attempt,
            message,
            internal_invariant,
            ..
        } => Some(RecordedSampling {
            first: (first.stage, first.origin),
            blocking: blocking.map(|blocker| (blocker.stage, blocker.origin)),
            admitted_attempt: *admitted_attempt,
            message: message.clone(),
            internal_invariant: *internal_invariant,
        }),
        _ => None,
    };
    RecordedFailure {
        address: report.address,
        disposition: report.disposition,
        consecutive_failures: report.consecutive_failures,
        sampling,
    }
}

#[derive(Clone, StatefulView)]
struct SamplingView {
    mounted: Rc<RefCell<Option<MountedAnimation>>>,
    built_samples: Rc<RefCell<Vec<f64>>>,
}

struct MountedAnimation {
    controller: AnimationController,
    peer: AnimationController,
    completion: flui_scheduler::PostFrameHandle,
    arena: flui_interaction::gesture::GestureArena,
}

struct SamplingState {
    driven: Option<[flui_animation::DrivenController; 2]>,
    mounted: Rc<RefCell<Option<MountedAnimation>>>,
}

impl StatefulView for SamplingView {
    type State = SamplingState;

    fn create_state(&self) -> Self::State {
        SamplingState {
            driven: None,
            mounted: Rc::clone(&self.mounted),
        }
    }
}

impl ViewState<SamplingView> for SamplingState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let registry = flui_widgets::VsyncScope::maybe_of(ctx)
            .expect("mounted animation receives its presentation registry");
        let driven = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let peer = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
        let completion = ctx
            .post_frame_handle()
            .expect("presentation completion lane");
        *self.mounted.borrow_mut() = Some(MountedAnimation {
            controller: driven.controller().clone(),
            peer: peer.controller().clone(),
            completion,
            arena: flui_widgets::GestureArenaScope::of(ctx),
        });
        self.driven = Some([driven, peer]);
    }

    fn build(&self, view: &SamplingView, _: &dyn BuildContext) -> impl IntoView {
        let controller = self.driven.as_ref().expect("mounted animation owners")[0]
            .controller()
            .clone();
        let read = controller.clone();
        let built_samples = Rc::clone(&view.built_samples);
        flui_widgets::AnimatedBuilder::new(Rc::new(controller), move || {
            built_samples.borrow_mut().push(read.value());
            flui_widgets::SizedBox::square(10.0)
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Healthy,
    Animation,
    FinalStatus,
    Completion,
    Deadline,
}

#[test]
fn presentation_delivery_failure_keeps_healthy_sampling_build_and_completion() {
    crate::table_test::run_table(
        "presentation sampling delivery",
        &[
            ("healthy sampling", healthy_sampling_builds_and_completes),
            (
                "value delivery failure",
                value_delivery_failure_keeps_healthy_frame_work,
            ),
            (
                "deadline delivery failure",
                deadline_delivery_failure_keeps_healthy_frame_work,
            ),
            (
                "final status delivery failure",
                final_status_delivery_failure_preserves_completion,
            ),
            (
                "run completion delivery failure",
                run_completion_delivery_failure_preserves_completion,
            ),
            (
                "late font diagnostic delivery failure",
                late_font_diagnostic_failure_preserves_fresh_sibling_frame,
            ),
            (
                "late report admits new authored input",
                a_late_report_rebuilds_fresh_text_before_geometry_publication,
            ),
        ],
    );
}

fn healthy_sampling_builds_and_completes() {
    delivery_case(Delivery::Healthy);
}

fn value_delivery_failure_keeps_healthy_frame_work() {
    delivery_case(Delivery::Animation);
}

fn deadline_delivery_failure_keeps_healthy_frame_work() {
    delivery_case(Delivery::Deadline);
}

fn final_status_delivery_failure_preserves_completion() {
    delivery_case(Delivery::FinalStatus);
}

fn run_completion_delivery_failure_preserves_completion() {
    delivery_case(Delivery::Completion);
}

#[derive(Default)]
struct FontWidthSink(Vec<f64>);

impl FrameSink for FontWidthSink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }

    fn submit(&mut self, scene: flui_layer::Scene) -> SubmitVerdict {
        self.0 = scene
            .tree()
            .iter()
            .filter_map(|(_, node)| match node.layer() {
                flui_layer::Layer::Picture(picture) => Some(picture),
                _ => None,
            })
            .flat_map(|picture| picture.picture().iter())
            .filter_map(|command| match &command.op {
                flui_painting::DrawOp::Paragraph { paragraph, .. } => Some(paragraph.size().width),
                _ => None,
            })
            .collect();
        SubmitVerdict::Presented
    }
}

struct LateFontDiagnostic {
    armed: AtomicBool,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

#[derive(Clone, StatefulView)]
struct ChangingProbeText {
    size: Arc<std::sync::atomic::AtomicU64>,
    rebuild: Arc<Mutex<Option<flui_view::RebuildHandle>>>,
}

struct ChangingProbeTextState(Arc<Mutex<Option<flui_view::RebuildHandle>>>);

impl StatefulView for ChangingProbeText {
    type State = ChangingProbeTextState;

    fn create_state(&self) -> Self::State {
        ChangingProbeTextState(Arc::clone(&self.rebuild))
    }
}

impl ViewState<ChangingProbeText> for ChangingProbeTextState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.0.lock().expect("probe rebuild capability") = Some(ctx.rebuild_handle());
    }

    fn build(&self, view: &ChangingProbeText, _: &dyn BuildContext) -> impl IntoView {
        flui_widgets::Text::new("AAAA").style(
            flui_painting::typography::TextStyle::default()
                .with_font_family("FLUI Probe Mono")
                .with_font_weight(flui_painting::typography::FontWeight::W100)
                .with_font_size(f64::from_bits(
                    view.size.load(std::sync::atomic::Ordering::SeqCst),
                )),
        )
    }
}

impl tracing::Subscriber for LateFontDiagnostic {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        struct FontChangeMessage(bool);

        impl tracing::field::Visit for FontChangeMessage {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}")
                        == "the font collection changed; laying out the text measured on it again";
                }
            }
        }

        let mut message = FontChangeMessage(false);
        event.record(&mut message);
        if event.metadata().target() == "flui_rendering::pipeline::owner::accessors"
            && message.0
            && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::panic::panic_any("late-font-diagnostic");
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn late_font_diagnostic_failure_preserves_fresh_sibling_frame() {
    late_font_diagnostic_case(false);
}

fn a_late_report_rebuilds_fresh_text_before_geometry_publication() {
    late_font_diagnostic_case(true);
}

fn late_font_diagnostic_case(change_on_report: bool) {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    let mut clock = ManualClock::new();
    let fonts = flui_painting::FontCollection::new();
    let mut runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &fonts,
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("late font runtime");
    runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let primary_id = runtime.presentation_id();
    let primary_mounted = Rc::new(RefCell::new(None));
    let primary_builds = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size(
            &SamplingView {
                mounted: Rc::clone(&primary_mounted),
                built_samples: Rc::clone(&primary_builds),
            },
            800.0,
            600.0,
        )
        .expect("primary late font root");
    let sibling_id = runtime
        .install_presentation(runtime.assemble_presentation(crate::owner_publication::window()));
    let sibling_mounted = Rc::new(RefCell::new(None));
    let sibling_builds = Rc::new(RefCell::new(Vec::new()));
    let size = Arc::new(AtomicU64::new(20.0_f64.to_bits()));
    let rebuild = Arc::new(Mutex::new(None));
    runtime
        .attach_root_widget_with_size_to(
            sibling_id,
            &flui_widgets::Column::new(vec![
                SamplingView {
                    mounted: Rc::clone(&sibling_mounted),
                    built_samples: Rc::clone(&sibling_builds),
                }
                .boxed(),
                flui_widgets::Center::new()
                    .child(ChangingProbeText {
                        size: Arc::clone(&size),
                        rebuild: Arc::clone(&rebuild),
                    })
                    .boxed(),
            ]),
            800.0,
            600.0,
        )
        .expect("fresh sibling with real text");
    let mut sink = FontWidthSink::default();
    assert!(runtime.pump(&mut clock, &mut sink).presented());
    assert_eq!(sink.0.len(), 1);
    assert!(
        (sink.0[0] - 80.0).abs() > 1.0,
        "the unregistered probe family paints a distinguishable fallback"
    );
    let primary = primary_mounted.take().expect("primary animation mounted");
    let sibling = sibling_mounted.take().expect("sibling animation mounted");
    let _primary_run = run_source(&primary.controller, SourceFailure::Curve);
    let _sibling_run = sibling.controller.forward().expect("sibling animation");
    let _ = runtime.pump(&mut clock, &mut sink);
    let primary_before = primary_builds.borrow().len();
    let sibling_before = sibling_builds.borrow().len();
    let primary_completed = Rc::new(Cell::new(0));
    let sibling_completed = Rc::new(Cell::new(0));
    for (handle, completed) in [
        (&primary.completion, &primary_completed),
        (&sibling.completion, &sibling_completed),
    ] {
        let observed = Rc::clone(completed);
        handle
            .schedule(move |_| observed.set(observed.get() + 1))
            .expect("completion admitted before font diagnostic");
    }
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    let registered = fonts;
    let register_once = AtomicBool::new(true);
    let authored_size = Arc::clone(&size);
    let rebuild = rebuild
        .lock()
        .expect("probe rebuild capability")
        .as_ref()
        .expect("mounted probe owns rebuild capability")
        .clone();
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        reported
            .lock()
            .expect("late font failure observations")
            .push(record_failure(report));
        if report.address.presentation_id == primary_id
            && matches!(report.kind, FrameFailureKind::SamplingPanic { .. })
            && register_once.swap(false, Ordering::SeqCst)
        {
            registered
                .register_font(include_bytes!(
                    "../../../flui-painting/assets/fonts/probe-mono-100.ttf"
                ))
                .expect("actual probe face registration");
        }
        if change_on_report
            && report.address.presentation_id == sibling_id
            && matches!(report.kind, FrameFailureKind::SamplingPanic { .. })
        {
            authored_size.store(22.75_f64.to_bits(), Ordering::SeqCst);
            rebuild.schedule(flui_foundation::RebuildReason::StateChange);
        }
    })));
    clock.advance(Duration::from_millis(250));
    let diagnostic_calls = Arc::new(AtomicUsize::new(0));
    let attempt = {
        let _diagnostics = crate::scoped_diagnostics(LateFontDiagnostic {
            armed: AtomicBool::new(true),
            calls: Arc::clone(&diagnostic_calls),
        });
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.pump(&mut clock, &mut sink)
        }))
    };
    assert_eq!(diagnostic_calls.load(Ordering::SeqCst), 1);
    assert_eq!(sibling.controller.value(), 0.25);
    assert!(
        sibling_builds.borrow().len() > sibling_before,
        "a post-commit font diagnostic must not discard the sibling's coherent fresh frame"
    );
    assert_eq!(sibling_builds.borrow().last().copied(), Some(0.25));
    assert_eq!(
        sibling_completed.get(),
        1,
        "delivery cannot cancel completion"
    );
    assert!(attempt.expect("font diagnostic is contained").presented());
    assert_eq!(primary_builds.borrow().len(), primary_before);
    assert_eq!(primary_completed.get(), 0);
    assert_eq!(sink.0.len(), 1);
    let expected_width = if change_on_report { 91.0 } else { 80.0 };
    assert!(
        (sink.0[0] - expected_width).abs() < 0.01,
        "same-pump paint must reflect the latest accepted authored size: {:?}",
        sink.0
    );
    let address = |presentation_id| flui_foundation::PresentationAddress {
        ui_runtime_id: runtime.id(),
        presentation_id,
    };
    assert_eq!(
        reports
            .lock()
            .expect("late font failure observations")
            .as_slice(),
        &[
            RecordedFailure {
                address: address(primary_id),
                disposition: FailureDisposition::FrameDropped,
                consecutive_failures: 1,
                sampling: Some(RecordedSampling {
                    first: (SamplingStage::Animation, SamplingFailureOrigin::Computation),
                    blocking: Some((SamplingStage::Animation, SamplingFailureOrigin::Computation)),
                    admitted_attempt: true,
                    message: PanicText::Verbatim("curve computation".into()),
                    internal_invariant: false,
                }),
            },
            RecordedFailure {
                address: address(sibling_id),
                disposition: FailureDisposition::Contained,
                consecutive_failures: 0,
                sampling: Some(RecordedSampling {
                    first: (SamplingStage::Preparation, SamplingFailureOrigin::Delivery),
                    blocking: None,
                    admitted_attempt: true,
                    message: PanicText::Verbatim("late-font-diagnostic".into()),
                    internal_invariant: false,
                }),
            },
        ]
    );
    clock.advance(Duration::from_millis(250));
    let _ = runtime.pump(&mut clock, &mut sink);
    assert_eq!(sibling.controller.value(), 0.5);
    assert_eq!(sibling_builds.borrow().last().copied(), Some(0.5));
    assert_eq!(primary_completed.get(), 1);
    assert_eq!(sibling_completed.get(), 1);
    assert_eq!(
        reports
            .lock()
            .expect("late font failure observations")
            .len(),
        2
    );
}

struct BackgroundDeadline {
    due: Cell<Option<flui_platform::EventTime>>,
    effects: Rc<Cell<usize>>,
    panic_after_effect: bool,
}

impl flui_interaction::gesture::GestureArenaMember for BackgroundDeadline {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}

    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}

    fn deadline(&self) -> Option<flui_platform::EventTime> {
        self.due.get()
    }

    fn poll_deadline(&self, _: flui_platform::EventTime) {
        self.effects.set(self.effects.get() + 1);
        if self.panic_after_effect {
            std::panic::panic_any("unknown background deadline progress");
        }
        self.due.set(None);
    }
}

#[test]
fn background_deadlines_progress_without_visual_frames_or_unknown_replay() {
    crate::table_test::run_table(
        "background presentation sampling",
        &[
            (
                "healthy timers while frames are disabled",
                healthy_background_deadlines_do_not_build_or_complete,
            ),
            (
                "unknown timer preserves healthy progress without replay",
                unknown_background_deadline_keeps_healthy_progress_once,
            ),
        ],
    );
}

fn healthy_background_deadlines_do_not_build_or_complete() {
    background_deadline_case(false);
}

fn unknown_background_deadline_keeps_healthy_progress_once() {
    background_deadline_case(true);
}

fn background_deadline_case(unknown: bool) {
    let mut clock = ManualClock::new();
    let mut runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("background runtime");
    runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        reported
            .lock()
            .expect("background failure observations")
            .push(record_failure(report));
    })));
    let primary_mounted = Rc::new(RefCell::new(None));
    let primary_builds = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size(
            &SamplingView {
                mounted: Rc::clone(&primary_mounted),
                built_samples: Rc::clone(&primary_builds),
            },
            800.0,
            600.0,
        )
        .expect("primary background root");
    let sibling_id = runtime
        .install_presentation(runtime.assemble_presentation(crate::owner_publication::window()));
    let sibling_mounted = Rc::new(RefCell::new(None));
    let sibling_builds = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size_to(
            sibling_id,
            &SamplingView {
                mounted: Rc::clone(&sibling_mounted),
                built_samples: Rc::clone(&sibling_builds),
            },
            800.0,
            600.0,
        )
        .expect("sibling background root");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let primary = primary_mounted.take().expect("primary animation mounted");
    let sibling = sibling_mounted.take().expect("sibling animation mounted");
    let _primary_run = primary.controller.forward().expect("primary animation");
    let _sibling_run = sibling.controller.forward().expect("sibling animation");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let primary_before = primary_builds.borrow().len();
    let sibling_before = sibling_builds.borrow().len();
    let primary_sample = primary.controller.value();
    let sibling_sample = sibling.controller.value();
    let primary_completed = Rc::new(Cell::new(0));
    let sibling_completed = Rc::new(Cell::new(0));
    for (handle, completed) in [
        (&primary.completion, &primary_completed),
        (&sibling.completion, &sibling_completed),
    ] {
        let observed = Rc::clone(completed);
        handle
            .schedule(move |_| observed.set(observed.get() + 1))
            .expect("completion awaiting a visual frame");
    }
    runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Paused);
    assert!(!runtime.scheduler().frames_enabled());

    // Pausing retires old native contacts. An owner-side producer admits
    // these new deadlines afterwards, while visual frames remain disabled.
    let due = clock.now() + Duration::from_millis(50);
    let primary_effects = Rc::new(Cell::new(0));
    let sibling_effects = Rc::new(Cell::new(0));
    let unknown_effects = Rc::new(Cell::new(0));
    let primary_timer = Rc::new(BackgroundDeadline {
        due: Cell::new(Some(crate::event_at(&clock, due))),
        effects: Rc::clone(&primary_effects),
        panic_after_effect: false,
    });
    let sibling_timer = Rc::new(BackgroundDeadline {
        due: Cell::new(Some(crate::event_at(&clock, due))),
        effects: Rc::clone(&sibling_effects),
        panic_after_effect: false,
    });
    let unknown_timer = Rc::new(BackgroundDeadline {
        due: Cell::new(Some(crate::event_at(&clock, due))),
        effects: Rc::clone(&unknown_effects),
        panic_after_effect: true,
    });
    let contact =
        |value| flui_platform::PointerId::new(std::num::NonZeroU64::new(value).expect("contact"));
    if unknown {
        let _ = primary.arena.add(contact(71), &unknown_timer);
    }
    let _ = primary.arena.add(contact(72), &primary_timer);
    let _ = sibling.arena.add(contact(73), &sibling_timer);
    clock.advance(Duration::from_millis(50));
    let background = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.pump_background();
    }));
    assert_eq!(
        primary_effects.get(),
        1,
        "a background turn polls a healthy due deadline in the faulty presentation"
    );
    assert_eq!(
        sibling_effects.get(),
        1,
        "a healthy presentation does not need a visual frame to service its timer"
    );
    background.expect("background deadline failures are presentation-local");
    assert_eq!(unknown_effects.get(), usize::from(unknown));
    for delay in [0, 49, 1, 100, 100] {
        clock.advance(Duration::from_millis(delay));
        runtime.pump_background();
    }
    assert_eq!(primary_effects.get(), 1, "completed timer remains retired");
    assert_eq!(sibling_effects.get(), 1);
    assert_eq!(
        unknown_effects.get(),
        usize::from(unknown),
        "an unknown partial effect is never automatically polled again"
    );
    assert_eq!(primary.controller.value(), primary_sample);
    assert_eq!(sibling.controller.value(), sibling_sample);
    assert_eq!(primary_builds.borrow().len(), primary_before);
    assert_eq!(sibling_builds.borrow().len(), sibling_before);
    assert_eq!(primary_completed.get(), 0);
    assert_eq!(sibling_completed.get(), 0);
    let expected = if unknown {
        vec![RecordedFailure {
            address: flui_foundation::PresentationAddress {
                ui_runtime_id: runtime.id(),
                presentation_id: runtime.presentation_id(),
            },
            disposition: FailureDisposition::Contained,
            consecutive_failures: 0,
            sampling: Some(RecordedSampling {
                first: (
                    SamplingStage::GestureDeadline,
                    SamplingFailureOrigin::Unknown,
                ),
                blocking: Some((
                    SamplingStage::GestureDeadline,
                    SamplingFailureOrigin::Unknown,
                )),
                admitted_attempt: false,
                message: PanicText::Verbatim("unknown background deadline progress".into()),
                internal_invariant: false,
            }),
        }]
    } else {
        Vec::new()
    };
    assert_eq!(
        reports
            .lock()
            .expect("background failure observations")
            .as_slice(),
        expected
    );

    runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Resumed);
    let _ = runtime.pump(&mut clock, &mut Sink);
    clock.advance(Duration::from_millis(250));
    let _ = runtime.pump(&mut clock, &mut Sink);
    assert!(primary_builds.borrow().len() > primary_before);
    assert!(sibling_builds.borrow().len() > sibling_before);
    assert_eq!(primary_completed.get(), 1);
    assert_eq!(sibling_completed.get(), 1);
    assert_eq!(unknown_effects.get(), usize::from(unknown));
    assert_eq!(
        reports
            .lock()
            .expect("background failure observations")
            .as_slice(),
        expected
    );
}

fn delivery_case(delivery: Delivery) {
    let mut clock = ManualClock::new();
    let mut runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime");
    runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        reported
            .lock()
            .expect("failure observations")
            .push(record_failure(report));
    })));
    let armed = Rc::new(Cell::new(true));
    let deadline_armed = Rc::clone(&armed);
    let primary_mounted = Rc::new(RefCell::new(None));
    let primary_builds = Rc::new(RefCell::new(Vec::new()));
    let root = flui_widgets::GestureDetector::new()
        .behavior(flui_widgets::HitTestBehavior::Opaque)
        .on_long_press(move |_| {
            if delivery == Delivery::Deadline && deadline_armed.replace(false) {
                std::panic::panic_any("primary deadline delivery");
            }
        })
        .child(SamplingView {
            mounted: Rc::clone(&primary_mounted),
            built_samples: Rc::clone(&primary_builds),
        });
    runtime
        .attach_root_widget_with_size(&root, 800.0, 600.0)
        .expect("primary root");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let sibling = runtime.assemble_presentation(crate::owner_publication::window());
    let sibling_id = runtime.install_presentation(sibling);
    let sibling_mounted = Rc::new(RefCell::new(None));
    let sibling_builds = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size_to(
            sibling_id,
            &SamplingView {
                mounted: Rc::clone(&sibling_mounted),
                built_samples: Rc::clone(&sibling_builds),
            },
            800.0,
            600.0,
        )
        .expect("sibling root");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let primary = primary_mounted.take().expect("primary animation mounted");
    let sibling = sibling_mounted.take().expect("sibling animation mounted");
    let primary_run = primary.controller.forward().expect("primary run");
    let _sibling_run = sibling.controller.forward().expect("sibling run");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let primary_before = primary_builds.borrow().len();
    let sibling_before = sibling_builds.borrow().len();
    let fail_once = Rc::clone(&armed);
    let listener = primary.controller.add_listener(Rc::new(move || {
        if delivery == Delivery::Animation && fail_once.replace(false) {
            std::panic::panic_any("primary value delivery");
        }
    }));
    let fail_once = Rc::clone(&armed);
    let status_listener = primary.controller.subscribe_status(Rc::new(move |status| {
        if delivery == Delivery::FinalStatus
            && status == flui_animation::AnimationStatus::ForwardSettled
            && fail_once.replace(false)
        {
            std::panic::panic_any("primary final status delivery");
        }
    }));
    let fail_once = Rc::clone(&armed);
    primary_run.when_complete_or_cancel(move |_| {
        if delivery == Delivery::Completion && fail_once.replace(false) {
            std::panic::panic_any("primary run completion delivery");
        }
    });
    let later_samples = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&later_samples);
    let read = primary.controller.clone();
    let later_listener = primary.controller.add_listener(Rc::new(move || {
        recorded.borrow_mut().push(read.value());
    }));
    let primary_completed = Rc::new(Cell::new(0));
    let sibling_completed = Rc::new(Cell::new(0));
    for (completion, count) in [
        (&primary.completion, Rc::clone(&primary_completed)),
        (&sibling.completion, Rc::clone(&sibling_completed)),
    ] {
        completion
            .schedule(move |_| count.set(count.get() + 1))
            .expect("completion admitted to the presentation");
    }
    if delivery == Delivery::Deadline {
        let down = flui_interaction::events::make_down_event_for_id_with_button(
            flui_platform::PointerId::new(std::num::NonZeroU64::MIN),
            flui_foundation::geometry::Offset::new(5.0, 5.0),
            flui_platform::PointerKind::Touch,
            flui_platform::PointerButton::PRIMARY,
        )
        .expect("finite touch");
        runtime.enter(|runtime| {
            runtime.handle_input_addressed(
                runtime.presentation_id(),
                flui_platform::PlatformInput::Pointer(down),
            );
        });
    }
    let (elapsed, sample, recovered) = match delivery {
        Delivery::Deadline => (Duration::from_millis(600), 0.6, 0.85),
        Delivery::FinalStatus | Delivery::Completion => (Duration::from_secs(1), 1.0, 1.0),
        Delivery::Healthy | Delivery::Animation => (Duration::from_millis(250), 0.25, 0.5),
    };
    clock.advance(elapsed);
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.pump(&mut clock, &mut Sink)
    }));
    assert!(
        (sibling.controller.value() - sample).abs() < 1e-9,
        "healthy sibling samples the admitted tick: expected {sample}, got {}",
        sibling.controller.value()
    );
    let _ = attempt.expect("a contained delivery failure cannot escape the runtime pump");
    assert!((primary.controller.value() - sample).abs() < 1e-9);
    assert_eq!(later_samples.borrow().as_slice(), &[sample]);
    for (name, built, before) in [
        ("primary", &primary_builds, primary_before),
        ("sibling", &sibling_builds, sibling_before),
    ] {
        assert!(built.borrow().len() > before, "{name} actually rebuilds");
        assert_eq!(built.borrow().last().copied(), Some(sample));
    }
    assert_eq!(primary_completed.get(), 1, "valid primary frame completes");
    assert_eq!(
        sibling_completed.get(),
        1,
        "healthy sibling frame completes"
    );
    if matches!(delivery, Delivery::FinalStatus | Delivery::Completion) {
        use std::future::Future as _;
        use std::pin::Pin;
        use std::task::{Context, Poll, Waker};
        let mut outcome = primary_run;
        assert!(
            matches!(
                Pin::new(&mut outcome).poll(&mut Context::from_waker(Waker::noop())),
                Poll::Ready(Ok(()))
            ),
            "the successful run outcome was published before delivery"
        );
        assert_eq!(
            primary.controller.status(),
            flui_animation::AnimationStatus::ForwardSettled
        );
    }
    {
        let reports = reports.lock().expect("failure observations");
        if delivery == Delivery::Healthy {
            assert!(reports.is_empty());
        } else {
            assert!(!armed.get(), "the real failing callback ran");
            assert_eq!(
                reports.as_slice(),
                &[RecordedFailure {
                    address: flui_foundation::PresentationAddress {
                        ui_runtime_id: runtime.id(),
                        presentation_id: runtime.presentation_id(),
                    },
                    disposition: FailureDisposition::Contained,
                    consecutive_failures: 0,
                    sampling: Some(RecordedSampling {
                        first: (
                            if delivery == Delivery::Deadline {
                                SamplingStage::GestureDeadline
                            } else {
                                SamplingStage::Animation
                            },
                            SamplingFailureOrigin::Delivery,
                        ),
                        blocking: None,
                        admitted_attempt: true,
                        message: PanicText::Verbatim(
                            match delivery {
                                Delivery::Animation => "primary value delivery",
                                Delivery::FinalStatus => "primary final status delivery",
                                Delivery::Completion => "primary run completion delivery",
                                Delivery::Deadline => "primary deadline delivery",
                                Delivery::Healthy => unreachable!("healthy delivery has no report"),
                            }
                            .into(),
                        ),
                        internal_invariant: false,
                    }),
                }],
                "one addressed delivery report does not increment the dropped-frame streak"
            );
        }
    }
    clock.advance(Duration::from_millis(250));
    let _ = runtime.pump(&mut clock, &mut Sink);
    assert!((primary.controller.value() - recovered).abs() < 1e-9);
    assert!((sibling.controller.value() - recovered).abs() < 1e-9);
    assert_eq!(primary_completed.get(), 1, "completion is not replayed");
    assert_eq!(
        sibling_completed.get(),
        1,
        "sibling completion is not replayed"
    );
    primary.controller.remove_listener(listener);
    primary.controller.remove_listener(later_listener);
    drop(status_listener);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceFailure {
    Curve,
    SimulationPosition,
    SimulationCompletion,
    DeliveryThenComputation,
    ComputationThenDelivery,
}

#[derive(Debug)]
struct FailingCurve {
    armed: AtomicBool,
}

impl flui_animation::Curve for FailingCurve {
    fn transform(&self, time: f64) -> f64 {
        if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            std::panic::panic_any("curve computation");
        }
        time
    }
}

struct FailingSimulation {
    completion: bool,
    armed: Cell<bool>,
}

impl flui_animation::simulation::Simulation for FailingSimulation {
    fn x(&self, time: f64) -> f64 {
        if !self.completion && time > 0.0 && self.armed.replace(false) {
            std::panic::panic_any("simulation position computation");
        }
        time
    }

    fn dx(&self, _: f64) -> f64 {
        1.0
    }

    fn is_done(&self, time: f64) -> bool {
        if self.completion && time > 0.0 && self.armed.replace(false) {
            std::panic::panic_any("simulation completion computation");
        }
        time >= 1.0
    }
}

#[test]
fn presentation_computation_failure_rejects_its_frame_and_preserves_healthy_work() {
    crate::table_test::run_table(
        "presentation sampling computation",
        &[
            (
                "curve computation",
                curve_computation_preserves_healthy_work,
            ),
            (
                "simulation position computation",
                simulation_position_computation_preserves_healthy_work,
            ),
            (
                "simulation completion computation",
                simulation_completion_computation_preserves_healthy_work,
            ),
            (
                "delivery then computation",
                delivery_then_computation_rejects_the_frame,
            ),
            (
                "computation then delivery",
                computation_then_delivery_rejects_the_frame,
            ),
        ],
    );
}

fn curve_computation_preserves_healthy_work() {
    source_failure_case(SourceFailure::Curve);
}

fn simulation_position_computation_preserves_healthy_work() {
    source_failure_case(SourceFailure::SimulationPosition);
}

fn simulation_completion_computation_preserves_healthy_work() {
    source_failure_case(SourceFailure::SimulationCompletion);
}

fn delivery_then_computation_rejects_the_frame() {
    source_failure_case(SourceFailure::DeliveryThenComputation);
}

fn computation_then_delivery_rejects_the_frame() {
    source_failure_case(SourceFailure::ComputationThenDelivery);
}

fn run_source(
    controller: &AnimationController,
    failure: SourceFailure,
) -> flui_animation::AnimationRunFuture {
    match failure {
        SourceFailure::SimulationPosition | SourceFailure::SimulationCompletion => controller
            .animate_with(FailingSimulation {
                completion: failure == SourceFailure::SimulationCompletion,
                armed: Cell::new(true),
            })
            .expect("admitted finite simulation"),
        SourceFailure::Curve
        | SourceFailure::DeliveryThenComputation
        | SourceFailure::ComputationThenDelivery => controller
            .animate_to_curved(
                1.0,
                Some(Duration::from_secs(1)),
                FailingCurve {
                    armed: AtomicBool::new(true),
                },
            )
            .expect("admitted curved run"),
    }
}

fn source_failure_case(failure: SourceFailure) {
    let mut clock = ManualClock::new();
    let mut runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime");
    runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        reported
            .lock()
            .expect("failure observations")
            .push(record_failure(report));
    })));
    let primary_mounted = Rc::new(RefCell::new(None));
    let primary_builds = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size(
            &SamplingView {
                mounted: Rc::clone(&primary_mounted),
                built_samples: Rc::clone(&primary_builds),
            },
            800.0,
            600.0,
        )
        .expect("primary root");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let sibling = runtime.assemble_presentation(crate::owner_publication::window());
    let sibling_id = runtime.install_presentation(sibling);
    let sibling_mounted = Rc::new(RefCell::new(None));
    let sibling_builds = Rc::new(RefCell::new(Vec::new()));
    runtime
        .attach_root_widget_with_size_to(
            sibling_id,
            &SamplingView {
                mounted: Rc::clone(&sibling_mounted),
                built_samples: Rc::clone(&sibling_builds),
            },
            800.0,
            600.0,
        )
        .expect("sibling root");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let primary = primary_mounted.take().expect("primary animations mounted");
    let sibling = sibling_mounted.take().expect("sibling animations mounted");
    let (source, healthy) = if failure == SourceFailure::DeliveryThenComputation {
        (&primary.peer, &primary.controller)
    } else {
        (&primary.controller, &primary.peer)
    };
    let source_run = run_source(source, failure);
    let _healthy_run = healthy.forward().expect("healthy peer run");
    let _sibling_run = sibling.controller.forward().expect("sibling run");
    let _ = runtime.pump(&mut clock, &mut Sink);
    let primary_before = primary_builds.borrow().len();
    let sibling_before = sibling_builds.borrow().len();
    let mixed = matches!(
        failure,
        SourceFailure::DeliveryThenComputation | SourceFailure::ComputationThenDelivery
    );
    let armed = Rc::new(Cell::new(true));
    let fail_once = Rc::clone(&armed);
    let listener = healthy.add_listener(Rc::new(move || {
        if mixed && fail_once.replace(false) {
            std::panic::panic_any("value delivery");
        }
    }));
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&delivered);
    let read = healthy.clone();
    let later_listener = healthy.add_listener(Rc::new(move || {
        recorded.borrow_mut().push(read.value());
    }));
    let primary_completed = Rc::new(Cell::new(0));
    let sibling_completed = Rc::new(Cell::new(0));
    for (completion, count) in [
        (&primary.completion, Rc::clone(&primary_completed)),
        (&sibling.completion, Rc::clone(&sibling_completed)),
    ] {
        completion
            .schedule(move |_| count.set(count.get() + 1))
            .expect("completion admitted to the presentation");
    }
    clock.advance(Duration::from_millis(250));
    let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.pump(&mut clock, &mut Sink)
    }));
    assert_eq!(
        source.value(),
        0.0,
        "failed proposal does not publish its candidate"
    );
    assert_eq!(healthy.value(), 0.25, "valid peer commit is preserved");
    assert_eq!(delivered.borrow().as_slice(), &[0.25]);
    assert_eq!(
        sibling.controller.value(),
        0.25,
        "healthy sibling receives the admitted tick"
    );
    let _ = attempt.expect("computation failure cannot escape the runtime pump");
    assert_eq!(
        primary_builds.borrow().len(),
        primary_before,
        "invalid sampling cohort cannot run a primary build"
    );
    assert!(sibling_builds.borrow().len() > sibling_before);
    assert_eq!(sibling_builds.borrow().last().copied(), Some(0.25));
    assert_eq!(primary_completed.get(), 0, "rejected frame cannot complete");
    assert_eq!(sibling_completed.get(), 1, "healthy sibling completes");
    assert!(
        source_run.is_pending(),
        "source failure does not invent completion"
    );
    {
        let reports = reports.lock().expect("failure observations");
        assert_eq!(
            reports.as_slice(),
            &[RecordedFailure {
                address: flui_foundation::PresentationAddress {
                    ui_runtime_id: runtime.id(),
                    presentation_id: runtime.presentation_id(),
                },
                disposition: FailureDisposition::FrameDropped,
                consecutive_failures: 1,
                sampling: Some(RecordedSampling {
                    first: (
                        SamplingStage::Animation,
                        if failure == SourceFailure::DeliveryThenComputation {
                            SamplingFailureOrigin::Delivery
                        } else {
                            SamplingFailureOrigin::Computation
                        },
                    ),
                    blocking: Some((SamplingStage::Animation, SamplingFailureOrigin::Computation)),
                    admitted_attempt: true,
                    message: PanicText::Verbatim(
                        match failure {
                            SourceFailure::DeliveryThenComputation => "value delivery",
                            SourceFailure::Curve | SourceFailure::ComputationThenDelivery => {
                                "curve computation"
                            }
                            SourceFailure::SimulationPosition => "simulation position computation",
                            SourceFailure::SimulationCompletion =>
                                "simulation completion computation",
                        }
                        .into(),
                    ),
                    internal_invariant: false,
                }),
            }],
            "a competing failure still drops the admitted attempt only once"
        );
    }
    clock.advance(Duration::from_millis(250));
    let _ = runtime.pump(&mut clock, &mut Sink);
    assert_eq!(
        source.value(),
        0.5,
        "next owner turn recovers the original run time"
    );
    assert_eq!(healthy.value(), 0.5);
    assert_eq!(primary_builds.borrow().last().copied(), Some(0.5));
    assert_eq!(
        primary_completed.get(),
        1,
        "accepted completion survives rejection"
    );
    assert_eq!(sibling_completed.get(), 1, "old completion is not replayed");
    assert_eq!(reports.lock().expect("failure observations").len(), 1);
    if failure == SourceFailure::Curve {
        let _next_run = run_source(source, SourceFailure::Curve);
        let _ = runtime.pump(&mut clock, &mut Sink);
        clock.advance(Duration::from_millis(250));
        let _ = runtime.pump(&mut clock, &mut Sink);
        let reports = reports.lock().expect("failure observations");
        assert_eq!(reports.len(), 2, "a new rejected attempt has a new report");
        assert_eq!(
            reports[1].consecutive_failures, 1,
            "the intervening terminal clean frame reset only this presentation's streak"
        );
    }
    healthy.remove_listener(listener);
    healthy.remove_listener(later_listener);
}

impl FrameSink for Sink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }

    fn submit(&mut self, _: flui_layer::Scene) -> SubmitVerdict {
        SubmitVerdict::Presented
    }
}

#[test]
fn one_pump_shares_its_selected_time_across_parent_and_child() {
    let host = ManualClock::new();
    let mut runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(host.clone()),
        ),
    )
    .expect("manual-clock runtime");
    runtime
        .attach_root_widget_with_size(&flui_widgets::SizedBox::square(10.0), 800.0, 600.0)
        .expect("mounted root");
    let registry = runtime.vsync();
    let child = Vsync::new();
    registry.attach_child(&child).expect("nested registry");
    let parent_owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&registry));
    let child_owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&child));
    let parent_run = parent_owner.controller().forward().expect("parent run");
    let child_run = child_owner.controller().forward().expect("child run");
    let _ = runtime.pump(&mut SampledClock(host.now()), &mut Sink);

    // Represent host time spent in an earlier animation callback without a
    // wall-clock sleep. No new pump is admitted by this callback.
    let armed = Rc::new(Cell::new(true));
    let observed = Rc::clone(&armed);
    let callback_clock = host.clone();
    let listener = child_owner.controller().add_listener(Rc::new(move || {
        if observed.replace(false) {
            callback_clock.advance(Duration::from_secs(3));
        }
    }));
    host.advance(Duration::from_millis(250));
    let selected = host.now();
    let mut frame = AdvancingHostClock {
        selected,
        host: host.clone(),
        reads: 0,
    };
    let _ = runtime.pump(&mut frame, &mut Sink);
    assert_eq!(frame.reads, 1, "the pump selects its timestamp once");
    assert!(!armed.get(), "the child callback ran in this pump");
    assert_eq!(host.now().duration_since(selected), Duration::from_secs(5));
    for (name, controller, run) in [
        ("parent", parent_owner.controller(), &parent_run),
        ("child", child_owner.controller(), &child_run),
    ] {
        assert!(
            (controller.value() - 0.25).abs() < 1e-9,
            "{name} samples the selected frame time, not the later host time"
        );
        assert!(run.is_pending(), "{name} retains its unfinished run");
    }

    let _ = runtime.pump(
        &mut SampledClock(selected + Duration::from_millis(250)),
        &mut Sink,
    );
    assert!((parent_owner.controller().value() - 0.5).abs() < 1e-9);
    assert!((child_owner.controller().value() - 0.5).abs() < 1e-9);
    child_owner.controller().remove_listener(listener);
}

fn manual_frame_runtime(host: &ManualClock) -> UiRuntime {
    let runtime = UiRuntime::try_new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(host.clone()),
        ),
    )
    .expect("manual-clock runtime");
    runtime
        .attach_root_widget_with_size(&flui_widgets::SizedBox::square(10.0), 800.0, 600.0)
        .expect("mounted root");
    runtime
}

/// Pump `runtime` at `sampled` after the host delivered a frame with `vsync`, and read the
/// progress of its one-second animation.
fn animation_after_vsync(
    host: &ManualClock,
    vsync: web_time::Instant,
    sampled: web_time::Instant,
    runtime: &mut UiRuntime,
    controller: &AnimationController,
) -> f64 {
    let request = flui_platform::FrameRequest::new(crate::event_at(host, vsync));
    runtime.note_frame_request(runtime.presentation_id(), request);
    let _ = runtime.pump(&mut SampledClock(sampled), &mut Sink);
    controller.value()
}

#[test]
fn a_delivered_vsync_is_the_frames_time_held_between_the_last_frame_and_the_wake() {
    let host = ManualClock::new();
    let mut runtime = manual_frame_runtime(&host);
    let owner =
        AnimationController::builder(Duration::from_secs(1)).build_on(Some(&runtime.vsync()));
    let controller = owner.controller();
    let _run = controller.forward().expect("run");
    let start = host.now();
    let _ = runtime.pump(&mut SampledClock(start), &mut Sink);

    // The wake ran 50 ms after its vsync: the animation samples the vsync.
    let at = |millis: u64| start + Duration::from_millis(millis);
    let value = animation_after_vsync(&host, at(200), at(250), &mut runtime, controller);
    assert!(
        (value - 0.2).abs() < 1e-9,
        "the frame samples its vsync, not the wake: {value}"
    );

    // A vsync ahead of the wake's own reading is a defect, not a time.
    let value = animation_after_vsync(&host, at(900), at(300), &mut runtime, controller);
    assert!(
        (value - 0.3).abs() < 1e-9,
        "a vsync past the wake is held at the wake: {value}"
    );

    // A vsync behind the previous frame cannot run the clock backwards.
    let value = animation_after_vsync(&host, at(100), at(400), &mut runtime, controller);
    assert!(
        (value - 0.3).abs() < 1e-9,
        "a vsync before the last frame is held at it: {value}"
    );

    // A pump with no delivered frame samples the host's reading, as before.
    let _ = runtime.pump(&mut SampledClock(at(500)), &mut Sink);
    assert!((controller.value() - 0.5).abs() < 1e-9);
}

struct ObservedIdleDeadline {
    sampled: web_time::Instant,
    asked: bool,
}

impl FrameClockSource for ObservedIdleDeadline {
    fn frame_time(&mut self) -> web_time::Instant {
        self.sampled
    }

    fn idle_deadline(&mut self, frame_time: web_time::Instant) -> flui_scheduler::IdleDeadline {
        self.asked = true;
        flui_scheduler::IdleDeadline::far_future(frame_time)
    }
}

/// How many of two idle tasks run in one frame, the first taking 20 ms of host time, when the
/// frame was delivered with `target_present` (10 ms after the frame) or without one.
fn idle_tasks_run(target_present: Option<u64>) -> (usize, bool) {
    let host = ManualClock::new();
    let runtime = manual_frame_runtime(&host);
    let start = host.now();
    let ran = Rc::new(Cell::new(0_usize));
    for _ in 0..2 {
        let (ran, host) = (Rc::clone(&ran), host.clone());
        runtime
            .scheduler()
            .add_task(flui_scheduler::Priority::Idle, move || {
                ran.set(ran.get() + 1);
                host.advance(Duration::from_millis(20));
            });
    }
    let mut request = flui_platform::FrameRequest::new(crate::event_at(&host, start));
    if let Some(millis) = target_present {
        let shown = crate::event_at(&host, start + Duration::from_millis(millis));
        request = request.with_target_present(shown);
    }
    runtime.note_frame_request(runtime.presentation_id(), request);
    let mut clock = ObservedIdleDeadline {
        sampled: start,
        asked: false,
    };
    let mut runtime = runtime;
    let _ = runtime.pump(&mut clock, &mut Sink);
    (ran.get(), clock.asked)
}

/// How many of two idle tasks, each taking 20 ms of host time, run in a frame whose request names
/// no present time, delivered 16 ms after an earlier frame whose request carried a report
/// where `reported_ago` names one: a frame shown that long before the earlier frame's vsync, at
/// `refresh` where the platform named one.
fn idle_tasks_after_a_presentation(
    reported_ago: Option<Duration>,
    refresh: Option<Duration>,
) -> (usize, bool) {
    let host = ManualClock::new();
    host.advance(Duration::from_secs(1));
    let mut runtime = manual_frame_runtime(&host);
    let earlier = host.now();
    let mut request = flui_platform::FrameRequest::new(crate::event_at(&host, earlier));
    if let Some(ago) = reported_ago {
        let at = crate::event_at(&host, earlier).as_nanos()
            - u64::try_from(ago.as_nanos()).expect("a test age fits");
        let mut shown =
            flui_platform::FramePresented::new(flui_platform::EventTime::from_nanos(at));
        if let Some(refresh) = refresh {
            shown = shown.with_refresh(refresh);
        }
        request = request.with_presented(shown);
    }
    runtime.note_frame_request(runtime.presentation_id(), request);
    let _ = runtime.pump(&mut SampledClock(earlier), &mut Sink);
    host.advance(Duration::from_millis(16));
    let start = host.now();

    let ran = Rc::new(Cell::new(0_usize));
    for _ in 0..2 {
        let (ran, host) = (Rc::clone(&ran), host.clone());
        runtime
            .scheduler()
            .add_task(flui_scheduler::Priority::Idle, move || {
                ran.set(ran.get() + 1);
                host.advance(Duration::from_millis(20));
            });
    }
    let request = flui_platform::FrameRequest::new(crate::event_at(&host, start));
    runtime.note_frame_request(runtime.presentation_id(), request);
    let mut clock = ObservedIdleDeadline {
        sampled: start,
        asked: false,
    };
    let _ = runtime.pump(&mut clock, &mut Sink);
    (ran.get(), clock.asked)
}

#[test]
fn a_presentation_report_bounds_the_idle_work_of_a_later_frame() {
    assert_eq!(
        idle_tasks_after_a_presentation(
            Some(Duration::from_millis(5)),
            Some(Duration::from_millis(10))
        ),
        (1, false),
        "the frame's idle work ends at the next refresh the earlier report places (9 ms after \
         the vsync), and the host's own deadline is not asked"
    );
    assert_eq!(
        idle_tasks_after_a_presentation(Some(Duration::from_millis(5)), None),
        (2, true),
        "a report with no refresh interval places no refresh"
    );
    assert_eq!(
        idle_tasks_after_a_presentation(None, None),
        (2, true),
        "without a report the host's deadline stands"
    );
    assert_eq!(
        idle_tasks_after_a_presentation(
            Some(Duration::from_millis(200)),
            Some(Duration::from_millis(10))
        ),
        (2, true),
        "a report from twenty refreshes back bounds nothing: the window was idle, and the host's \
         deadline stands"
    );
}

#[test]
fn a_target_present_time_bounds_the_idle_work_of_its_frame() {
    assert_eq!(
        idle_tasks_run(Some(10)),
        (1, false),
        "the second idle task waits once the frame's present time has passed, and the host's own \
         deadline is not asked"
    );
    assert_eq!(
        idle_tasks_run(None),
        (2, true),
        "a frame that names no present time keeps the host's deadline"
    );
}
