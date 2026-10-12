//! The host-selected timestamp owns all animation samples in one pump.

use flui_animation::{Animation, AnimationController, Vsync};
use flui_foundation::{Listenable, ManualClock, MonotonicClock};
use flui_runtime::frame_failure::{FailureDisposition, FrameFailureHandler};
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

#[derive(Clone, StatefulView)]
struct SamplingView {
    mounted: Rc<RefCell<Option<MountedAnimation>>>,
    built_samples: Rc<RefCell<Vec<f64>>>,
}

struct MountedAnimation {
    controller: AnimationController,
    peer: AnimationController,
    completion: flui_scheduler::PostFrameHandle,
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

fn delivery_case(delivery: Delivery) {
    let mut clock = ManualClock::new();
    let mut runtime = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime");
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        reported.lock().expect("failure observations").push((
            report.address,
            report.disposition,
            report.consecutive_failures,
        ));
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
            flui_platform_api::PointerId::new(std::num::NonZeroU64::MIN),
            flui_foundation::geometry::Offset::new(5.0, 5.0),
            flui_platform_api::PointerKind::Touch,
            flui_platform_api::PointerButton::PRIMARY,
        )
        .expect("finite touch");
        runtime.enter(|runtime| {
            runtime.handle_input_addressed(
                runtime.presentation_id(),
                flui_platform_api::PlatformInput::Pointer(down),
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
        let mut outcome = primary_run.clone();
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
                &[(
                    flui_foundation::PresentationAddress {
                        ui_runtime_id: runtime.id(),
                        presentation_id: runtime.presentation_id(),
                    },
                    FailureDisposition::Contained,
                    0,
                )],
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
    let mut runtime = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock.clone()),
        ),
    )
    .expect("runtime");
    let reports = Arc::new(Mutex::new(Vec::new()));
    let reported = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        reported.lock().expect("failure observations").push((
            report.address,
            report.disposition,
            report.consecutive_failures,
        ));
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
            &[(
                flui_foundation::PresentationAddress {
                    ui_runtime_id: runtime.id(),
                    presentation_id: runtime.presentation_id(),
                },
                FailureDisposition::FrameDropped,
                1,
            )],
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
    let mut runtime = UiRuntime::new(
        crate::owner_publication::window(),
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
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
