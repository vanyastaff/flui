//! Scheduling observations never execute an application's deadline query.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock, PresentationAddress, UiRuntimeId};
use flui_interaction::{
    GestureDeadlineError, GestureDeadlineUpdate, arena::GestureArenaMember, ids::PointerId,
};
use flui_runtime::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport, PanicText, SamplingFailure, SamplingFailureOrigin, SamplingStage,
};
use flui_runtime::owner::{
    Delivery, DispatchError, FrameDispatcher, InitializationOutcome, InstallInitialization,
    InstallToken, OwnerEffects, OwnerHost, RecoveryState,
};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};

struct DeadlineQuery {
    calls: Rc<Cell<usize>>,
    due: web_time::Instant,
    fail: Cell<bool>,
}

impl GestureArenaMember for DeadlineQuery {
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, _: PointerId) {}

    fn deadline(&self) -> Option<web_time::Instant> {
        self.calls.set(self.calls.get() + 1);
        if self.fail.get() {
            std::panic::panic_any("external deadline query");
        }
        Some(self.due)
    }
}

struct Effects {
    query_calls: Rc<Cell<usize>>,
    frames: Cell<usize>,
    fail_operation: Cell<bool>,
    probe_runtime: Cell<bool>,
    runtime_observation: RefCell<Option<RuntimeObservation>>,
}

struct RuntimeObservation {
    queries_before: usize,
    queries_after: usize,
    wakes: Vec<Option<web_time::Instant>>,
    pending: Vec<bool>,
}

impl OwnerEffects for Effects {
    fn runtime_lifecycle(&self, _: UiRuntimeId, _: flui_scheduler::AppLifecycleState) {}

    fn runtimes_stopped(&self, _: RecoveryState) {}

    fn retire_host(&self, _: &[PresentationAddress], _: RecoveryState) {}

    fn commit_install(&self, _: InstallToken, _: RecoveryState) -> Option<InstallInitialization> {
        panic!("no pending installation");
    }

    fn finish_install(&self, _: PresentationAddress, _: InitializationOutcome, _: RecoveryState) {}

    fn cancel_install(&self, _: InstallToken, _: RecoveryState) {
        panic!("no pending installation");
    }

    fn resize_surface(
        &self,
        _: PresentationAddress,
        _: flui_foundation::geometry::Size<f64>,
        _: f64,
    ) {
    }

    fn frame(&self, _: PresentationAddress, runtime: &mut UiRuntime) {
        self.frames.set(self.frames.get() + 1);
        if self.fail_operation.replace(false) {
            std::panic::panic_any("primary owner operation");
        }
        // The native host services geometry before consulting its wake gate.
        runtime.enter(UiRuntime::drain_owner_inbox);
        if !self.probe_runtime.get() {
            return;
        }
        let queries_before = self.query_calls.get();
        let mut wakes = Vec::new();
        let mut pending = Vec::new();
        for _ in 0..3 {
            wakes.push(runtime.next_wake());
            pending.push(runtime.has_pending_work());
        }
        self.runtime_observation.replace(Some(RuntimeObservation {
            queries_before,
            queries_after: self.query_calls.get(),
            wakes,
            pending,
        }));
    }

    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}

    fn after_turn(&self, _: RecoveryState) {}

    fn request_continuation(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy)]
enum ObservationScope {
    Owner,
    Runtime,
}

fn scheduling_observation_case(scope: ObservationScope) {
    let clock = ManualClock::new();
    let due = clock.now() + Duration::from_secs(1);
    let (runtime, _window) = manual_runtime(clock);
    let query_calls = Rc::new(Cell::new(0));
    let member = Rc::new(DeadlineQuery {
        calls: Rc::clone(&query_calls),
        due,
        fail: Cell::new(false),
    });
    let _entry = runtime
        .gestures()
        .arena()
        .add(PointerId::new(NonZeroU64::MIN), &member);
    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish runtime")
        .commit();
    let frame = owner.frame_dispatcher(address).expect("frame authority");
    let effects = Effects {
        query_calls: Rc::clone(&query_calls),
        frames: Cell::new(0),
        fail_operation: Cell::new(false),
        probe_runtime: Cell::new(false),
        runtime_observation: RefCell::new(None),
    };

    // The completed owner operation may refresh its scheduling observation.
    // Getters must use that observation without executing the query again.
    assert_eq!(frame.deliver(&effects), Ok(Delivery::Driven));
    match scope {
        ObservationScope::Owner => {
            let queries_before = query_calls.get();
            let wakes: Vec<_> = (0..3).map(|_| owner.next_wake()).collect();
            assert_eq!(
                query_calls.get(),
                queries_before,
                "owner wake observation executed the external deadline query"
            );
            assert_eq!(wakes, [Ok(Some(due)); 3]);
        }
        ObservationScope::Runtime => {
            effects.probe_runtime.set(true);
            assert_eq!(frame.deliver(&effects), Ok(Delivery::Driven));
            let observed = effects
                .runtime_observation
                .take()
                .expect("runtime getters observed during owner execution");
            assert_eq!(
                observed.queries_after, observed.queries_before,
                "runtime wake or pending observation executed the external deadline query"
            );
            assert_eq!(observed.wakes, [Some(due); 3]);
            assert_eq!(observed.pending, [true; 3]);
        }
    }
}

fn manual_runtime(clock: ManualClock) -> (UiRuntime, Arc<dyn flui_platform_api::PlatformWindow>) {
    let window = crate::owner_publication::window();
    // A native host owns the window; the runtime intentionally holds it weakly.
    let native_window = Arc::clone(window.window());
    let runtime = UiRuntime::new(
        window,
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Manual(clock),
        ),
    )
    .expect("manual-clock runtime");
    (runtime, native_window)
}

fn owner_wake_uses_the_committed_scheduling_observation() {
    scheduling_observation_case(ObservationScope::Owner);
}

fn runtime_wake_and_pending_use_the_committed_scheduling_observation() {
    scheduling_observation_case(ObservationScope::Runtime);
}

struct ReportProbe {
    owner: OwnerHost,
    frame: FrameDispatcher,
    effects: Rc<Effects>,
    fail_handler: bool,
    observations: RefCell<Vec<ReportObservation>>,
    callback_observations: RefCell<Vec<(PresentationAddress, PanicText)>>,
}

#[derive(Debug)]
struct ReportObservation {
    address: PresentationAddress,
    first: SamplingFailure,
    blocking: Option<SamplingFailure>,
    admitted_attempt: bool,
    message: PanicText,
    disposition: FailureDisposition,
    consecutive_failures: u32,
    executing: Result<bool, DispatchError>,
    runtimes: Result<Vec<UiRuntimeId>, DispatchError>,
    wake: Result<Option<web_time::Instant>, DispatchError>,
    queued: Result<Delivery, DispatchError>,
    queries_before: usize,
    queries_after: usize,
    frames_before: usize,
    frames_after: usize,
}

thread_local! {
    // The report API is Send + Sync, while its owner is thread-affine.
    // Only this table installs a context, and every callback releases its
    // TLS borrow before invoking the public owner interface.
    static REPORT_PROBE: RefCell<Option<Rc<ReportProbe>>> = const { RefCell::new(None) };
}

struct ScopedReportProbe;

impl ScopedReportProbe {
    fn install(probe: Rc<ReportProbe>) -> Self {
        let previous = REPORT_PROBE.with(|slot| slot.replace(Some(probe)));
        assert!(previous.is_none(), "report probe is not nested");
        Self
    }
}

impl Drop for ScopedReportProbe {
    fn drop(&mut self) {
        let retired = REPORT_PROBE.with(RefCell::take);
        drop(retired);
    }
}

fn observe_sampling_report(report: &FrameFailureReport) {
    let probe = REPORT_PROBE
        .with(|slot| slot.borrow().clone())
        .expect("report has a scoped owner probe");
    if let FrameFailureKind::CallbackPanic { message, .. } = &report.kind {
        // Existing callback reports keep their original delivery boundary.
        // Only sampling observation reports promise delivery after lease return.
        probe
            .callback_observations
            .borrow_mut()
            .push((report.address, message.clone()));
        if probe.fail_handler {
            std::panic::panic_any("owner callback report handler");
        }
        return;
    }
    let FrameFailureKind::SamplingPanic {
        first,
        blocking,
        admitted_attempt,
        message,
        ..
    } = &report.kind
    else {
        return;
    };
    let queries_before = probe.effects.query_calls.get();
    let executing = probe.owner.is_executing();
    let runtimes = probe.owner.runtime_ids();
    let wake = probe.owner.next_wake();
    let frames_before = probe.effects.frames.get();
    let queued = probe.frame.deliver(probe.effects.as_ref());
    probe.observations.borrow_mut().push(ReportObservation {
        address: report.address,
        first: *first,
        blocking: *blocking,
        admitted_attempt: *admitted_attempt,
        message: message.clone(),
        disposition: report.disposition,
        consecutive_failures: report.consecutive_failures,
        executing,
        runtimes,
        wake,
        queued,
        queries_before,
        queries_after: probe.effects.query_calls.get(),
        frames_before,
        frames_after: probe.effects.frames.get(),
    });
    if probe.fail_handler {
        std::panic::panic_any("sampling report handler");
    }
}

struct FailingDiagnostics(Arc<AtomicUsize>);

impl tracing::Subscriber for FailingDiagnostics {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() == tracing::Level::ERROR {
            self.0.fetch_add(1, Ordering::Relaxed);
            std::panic::panic_any("sampling diagnostics");
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

fn query_report_case(fail_handler: bool, fail_diagnostics: bool, fail_operation: bool) {
    let clock = ManualClock::new();
    let now = clock.now();
    let due = now + Duration::from_secs(1);
    let (runtime, _window) = manual_runtime(clock.clone());
    runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(observe_sampling_report)));
    let query_calls = Rc::new(Cell::new(0));
    let member = Rc::new(DeadlineQuery {
        calls: Rc::clone(&query_calls),
        due,
        fail: Cell::new(false),
    });
    let _entry = runtime
        .gestures()
        .arena()
        .add(PointerId::new(NonZeroU64::MIN), &member);
    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish runtime")
        .commit();
    let frame = owner.frame_dispatcher(address).expect("frame authority");
    let effects = Rc::new(Effects {
        query_calls: Rc::clone(&query_calls),
        frames: Cell::new(0),
        fail_operation: Cell::new(false),
        probe_runtime: Cell::new(false),
        runtime_observation: RefCell::new(None),
    });
    assert_eq!(frame.deliver(effects.as_ref()), Ok(Delivery::Driven));
    let queries_before_fault = query_calls.get();
    let frames_before_fault = effects.frames.get();
    let probe = Rc::new(ReportProbe {
        owner: owner.clone(),
        frame: frame.clone(),
        effects: Rc::clone(&effects),
        fail_handler,
        observations: RefCell::new(Vec::new()),
        callback_observations: RefCell::new(Vec::new()),
    });
    let _probe_scope = ScopedReportProbe::install(Rc::clone(&probe));
    let diagnostic_calls = Arc::new(AtomicUsize::new(0));
    let diagnostics = fail_diagnostics.then(|| {
        tracing::subscriber::set_default(FailingDiagnostics(Arc::clone(&diagnostic_calls)))
    });
    member.fail.set(true);
    effects.fail_operation.set(fail_operation);
    let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        frame.deliver(effects.as_ref())
    }));
    drop(diagnostics);
    if fail_operation {
        let payload = delivered.expect_err("the original owner operation still propagates");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"primary owner operation"),
            "query, tracing and handler failures cannot replace the owner cause"
        );
        assert_eq!(
            *probe.callback_observations.borrow(),
            [(
                address,
                PanicText::Verbatim("primary owner operation".into())
            )]
        );
        // The failed callback owes the work admitted by its sampling report.
        // A real next native opportunity supplies the normal FIFO budget.
        let callback = owner
            .begin_callback(effects.as_ref())
            .expect("next callback");
        owner.continue_work(effects.as_ref());
        drop(callback);
    } else {
        assert_eq!(
            delivered.expect("deadline discovery and its diagnostics are contained"),
            Ok(Delivery::Driven)
        );
        assert!(probe.callback_observations.borrow().is_empty());
    }

    {
        let observed = probe.observations.borrow();
        assert_eq!(
            observed.len(),
            1,
            "one query operation report: {observed:?}"
        );
        let observed = &observed[0];
        assert_eq!(observed.address, address);
        assert_eq!(observed.first.stage, SamplingStage::DeadlineDiscovery);
        assert_eq!(observed.first.origin, SamplingFailureOrigin::Unknown);
        let blocking = observed
            .blocking
            .expect("unknown query restricts publication");
        assert_eq!(blocking.stage, SamplingStage::DeadlineDiscovery);
        assert_eq!(blocking.origin, SamplingFailureOrigin::Unknown);
        assert!(!observed.admitted_attempt);
        assert_eq!(observed.disposition, FailureDisposition::Contained);
        assert_eq!(observed.consecutive_failures, 0);
        assert_eq!(
            observed.message,
            PanicText::Verbatim("external deadline query".into())
        );
        assert_eq!(observed.executing, Ok(false), "report follows lease return");
        assert_eq!(observed.runtimes, Ok(vec![address.ui_runtime_id]));
        assert_eq!(observed.wake, Ok(Some(now + Duration::from_millis(100))));
        assert_eq!(observed.queued, Ok(Delivery::Queued));
        assert_eq!(observed.queries_before, queries_before_fault + 1);
        assert_eq!(observed.queries_after, observed.queries_before);
        assert_eq!(observed.frames_after, observed.frames_before);
    }
    assert_eq!(query_calls.get(), queries_before_fault + 1);
    assert_eq!(effects.frames.get(), frames_before_fault + 2);
    assert_eq!(
        diagnostic_calls.load(Ordering::Relaxed) > 0,
        fail_diagnostics
    );

    clock.advance(Duration::from_millis(99));
    assert_eq!(frame.deliver(effects.as_ref()), Ok(Delivery::Driven));
    assert_eq!(
        query_calls.get(),
        queries_before_fault + 1,
        "early owner turn does not re-probe"
    );
    assert_eq!(probe.observations.borrow().len(), 1);
    assert_eq!(
        owner.next_wake(),
        Ok(Some(now + Duration::from_millis(100)))
    );

    member.fail.set(false);
    clock.advance(Duration::from_millis(1));
    assert_eq!(frame.deliver(effects.as_ref()), Ok(Delivery::Driven));
    assert!(
        query_calls.get() > queries_before_fault + 1,
        "the due retry observes the repaired source"
    );
    assert_eq!(owner.next_wake(), Ok(Some(due)));
    assert_eq!(
        probe.observations.borrow().len(),
        1,
        "successful retry emits no duplicate report"
    );
}

fn query_failure_reports_after_lease_return_and_recovers() {
    query_report_case(false, false, false);
}

fn failing_query_report_handler_preserves_recovery() {
    query_report_case(true, false, false);
}

fn failing_query_diagnostics_preserve_report_and_recovery() {
    query_report_case(false, true, false);
}

fn competing_query_diagnostics_and_handler_preserve_recovery() {
    query_report_case(true, true, false);
}

fn original_owner_failure_survives_query_report_competition() {
    query_report_case(true, true, true);
}

struct PublishedDeadline {
    due: Cell<Option<web_time::Instant>>,
    queries: Rc<Cell<usize>>,
}

impl GestureArenaMember for PublishedDeadline {
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, _: PointerId) {}

    fn deadline(&self) -> Option<web_time::Instant> {
        self.queries.set(self.queries.get() + 1);
        self.due.get()
    }
}

fn observe_owner_deadline(
    owner: &OwnerHost,
    queries: &Cell<usize>,
    expected: Option<web_time::Instant>,
) {
    let before = queries.get();
    let wakes: Vec<_> = (0..3).map(|_| owner.next_wake()).collect();
    assert_eq!(
        queries.get(),
        before,
        "wake getters are not discovery opportunities"
    );
    assert_eq!(wakes, [Ok(expected); 3]);
}

fn retained_deadline_publisher_updates_cancels_and_refuses_stale_authority() {
    let clock = ManualClock::new();
    let now = clock.now();
    let initial = now + Duration::from_secs(1);
    let earlier = now + Duration::from_millis(250);
    let successor_due = now + Duration::from_millis(500);
    let (runtime, window) = manual_runtime(clock);
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    window.on_request_frame(Box::new(move || {
        wake_count.fetch_add(1, Ordering::Relaxed);
    }));
    let queries = Rc::new(Cell::new(0));
    let member = Rc::new(PublishedDeadline {
        due: Cell::new(Some(initial)),
        queries: Rc::clone(&queries),
    });
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let pointer = PointerId::new(NonZeroU64::MIN);
    let arena = runtime.gestures().arena().clone();
    let registration = arena
        .register_deadline_member(pointer, &erased)
        .expect("admit external deadline source");
    let publisher = registration.update_handle();
    let before = wakes.load(Ordering::Relaxed);
    publisher
        .publish(Some(initial))
        .expect("publish initial timing");
    assert!(
        wakes.load(Ordering::Relaxed) > before,
        "publication wakes its native owner"
    );
    assert_eq!(
        queries.get(),
        0,
        "publishing committed timing is not a query"
    );

    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish runtime")
        .commit();
    let frame = owner.frame_dispatcher(address).expect("frame authority");
    let effects = Effects {
        query_calls: Rc::clone(&queries),
        frames: Cell::new(0),
        fail_operation: Cell::new(false),
        probe_runtime: Cell::new(true),
        runtime_observation: RefCell::new(None),
    };
    assert_eq!(frame.deliver(&effects), Ok(Delivery::Driven));
    observe_owner_deadline(&owner, &queries, Some(initial));

    let before = wakes.load(Ordering::Relaxed);
    member.due.set(Some(earlier));
    publisher
        .publish(Some(earlier))
        .expect("retained authority publishes earlier timing");
    assert!(
        wakes.load(Ordering::Relaxed) > before,
        "earlier timing wakes without a manual poke"
    );
    observe_owner_deadline(&owner, &queries, Some(earlier));
    assert_eq!(frame.deliver(&effects), Ok(Delivery::Driven));
    let observed = effects
        .runtime_observation
        .take()
        .expect("runtime wake gate");
    assert_eq!(observed.queries_after, observed.queries_before);
    assert_eq!(observed.wakes, [Some(earlier); 3]);
    assert_eq!(observed.pending, [true; 3]);
    assert_eq!(
        queries.get(),
        0,
        "explicit publishers need no foreign discovery"
    );

    let before = wakes.load(Ordering::Relaxed);
    member.due.set(None);
    publisher.publish(None).expect("cancel timing");
    assert!(wakes.load(Ordering::Relaxed) > before);
    observe_owner_deadline(&owner, &queries, None);
    drop(registration);

    // Reuse the same pointer and member under a new registration. Old weak
    // authority must neither cancel nor rearm this successor.
    let before = wakes.load(Ordering::Relaxed);
    assert_eq!(
        publisher.publish(Some(earlier)),
        Err(GestureDeadlineError::Retired)
    );
    assert_eq!(publisher.refresh(), Err(GestureDeadlineError::Retired));
    assert_eq!(wakes.load(Ordering::Relaxed), before);
    observe_owner_deadline(&owner, &queries, None);

    let successor_registration = arena
        .register_deadline_member(pointer, &erased)
        .expect("register successor under the reused pointer");
    let successor = successor_registration.update_handle();
    member.due.set(Some(successor_due));
    successor
        .publish(Some(successor_due))
        .expect("successor timing");
    observe_owner_deadline(&owner, &queries, Some(successor_due));
    let before = wakes.load(Ordering::Relaxed);
    assert_eq!(publisher.publish(None), Err(GestureDeadlineError::Retired));
    assert_eq!(
        publisher.publish(Some(earlier)),
        Err(GestureDeadlineError::Retired)
    );
    assert_eq!(publisher.refresh(), Err(GestureDeadlineError::Retired));
    assert_eq!(wakes.load(Ordering::Relaxed), before);
    observe_owner_deadline(&owner, &queries, Some(successor_due));
    member.due.set(None);
    successor.publish(None).expect("cancel successor timing");
    observe_owner_deadline(&owner, &queries, None);
}

struct CompletionRefresh {
    member: Rc<PublishedDeadline>,
    publisher: GestureDeadlineUpdate,
    due: web_time::Instant,
}

struct CompletionEffects {
    frame_effects: Effects,
    owner: OwnerHost,
    refresh: RefCell<Option<CompletionRefresh>>,
    observation: RefCell<Option<CompletionObservation>>,
}

struct CompletionObservation {
    executing: Result<bool, DispatchError>,
    previous_wake: Result<Option<web_time::Instant>, DispatchError>,
    queries_before: usize,
    queries_after: usize,
    accepted: Result<(), GestureDeadlineError>,
}

impl OwnerEffects for CompletionEffects {
    fn runtime_lifecycle(&self, _: UiRuntimeId, _: flui_scheduler::AppLifecycleState) {}
    fn runtimes_stopped(&self, _: RecoveryState) {}
    fn retire_host(&self, _: &[PresentationAddress], _: RecoveryState) {}
    fn commit_install(&self, _: InstallToken, _: RecoveryState) -> Option<InstallInitialization> {
        panic!("no pending installation");
    }
    fn finish_install(&self, _: PresentationAddress, _: InitializationOutcome, _: RecoveryState) {}
    fn cancel_install(&self, _: InstallToken, _: RecoveryState) {
        panic!("no pending installation");
    }
    fn resize_surface(
        &self,
        _: PresentationAddress,
        _: flui_foundation::geometry::Size<f64>,
        _: f64,
    ) {
    }
    fn frame(&self, address: PresentationAddress, runtime: &mut UiRuntime) {
        self.frame_effects.frame(address, runtime);
    }
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: RecoveryState) {
        let Some(refresh) = self.refresh.take() else {
            return;
        };
        let executing = self.owner.is_executing();
        let previous_wake = self.owner.next_wake();
        let queries_before = refresh.member.queries.get();
        refresh.member.due.set(Some(refresh.due));
        let accepted = refresh.publisher.refresh();
        self.observation.replace(Some(CompletionObservation {
            executing,
            previous_wake,
            queries_before,
            queries_after: refresh.member.queries.get(),
            accepted,
        }));
    }
    fn request_continuation(&self) -> bool {
        true
    }
}

fn completion_refresh_observes_timing_after_the_runtime_lease_returns() {
    let clock = ManualClock::new();
    let now = clock.now();
    let initial = now + Duration::from_secs(1);
    let completed = now + Duration::from_millis(150);
    let (runtime, _window) = manual_runtime(clock);
    let queries = Rc::new(Cell::new(0));
    let member = Rc::new(PublishedDeadline {
        due: Cell::new(Some(initial)),
        queries: Rc::clone(&queries),
    });
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = runtime
        .gestures()
        .arena()
        .register_deadline_member(PointerId::new(NonZeroU64::MIN), &erased)
        .expect("register source");
    let publisher = registration.update_handle();
    publisher.publish(Some(initial)).expect("initial timing");
    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish runtime")
        .commit();
    let effects = CompletionEffects {
        frame_effects: Effects {
            query_calls: Rc::clone(&queries),
            frames: Cell::new(0),
            fail_operation: Cell::new(false),
            probe_runtime: Cell::new(true),
            runtime_observation: RefCell::new(None),
        },
        owner: owner.clone(),
        refresh: RefCell::new(Some(CompletionRefresh {
            member,
            publisher,
            due: completed,
        })),
        observation: RefCell::new(None),
    };
    assert_eq!(
        owner
            .frame_dispatcher(address)
            .expect("frame authority")
            .deliver(&effects),
        Ok(Delivery::Driven)
    );
    let before_completion = effects
        .frame_effects
        .runtime_observation
        .take()
        .expect("frame wake gate");
    assert_eq!(
        before_completion.wakes,
        [Some(initial); 3],
        "frame body still observes the old timing"
    );
    assert_eq!(
        before_completion.queries_before,
        before_completion.queries_after
    );
    let completion = effects.observation.take().expect("completion callback");
    assert_eq!(completion.executing, Ok(false));
    assert_eq!(completion.previous_wake, Ok(Some(initial)));
    assert_eq!(completion.accepted, Ok(()));
    assert_eq!(
        completion.queries_after, completion.queries_before,
        "refresh admission never executes the source"
    );
    assert!(
        queries.get() > completion.queries_after,
        "completion frontier performs the requested query"
    );
    observe_owner_deadline(&owner, &queries, Some(completed));
}

#[derive(Debug)]
struct StandaloneReport {
    address: PresentationAddress,
    first: SamplingFailure,
    admitted_attempt: bool,
    disposition: FailureDisposition,
    consecutive_failures: u32,
}

fn standalone_owner_drain_delivers_its_query_report_and_progresses() {
    let clock = ManualClock::new();
    let now = clock.now();
    let due = now + Duration::from_secs(1);
    let (runtime, _window) = manual_runtime(clock.clone());
    let reports = Arc::new(Mutex::new(Vec::new()));
    let collected = Arc::clone(&reports);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
        if let FrameFailureKind::SamplingPanic {
            first,
            admitted_attempt,
            ..
        } = &report.kind
        {
            collected.lock().expect("reports").push(StandaloneReport {
                address: report.address,
                first: *first,
                admitted_attempt: *admitted_attempt,
                disposition: report.disposition,
                consecutive_failures: report.consecutive_failures,
            });
        }
    })));
    let queries = Rc::new(Cell::new(0));
    let member = Rc::new(DeadlineQuery {
        calls: Rc::clone(&queries),
        due,
        fail: Cell::new(false),
    });
    let _entry = runtime
        .gestures()
        .arena()
        .add(PointerId::new(NonZeroU64::MIN), &member);
    runtime.enter(UiRuntime::drain_owner_inbox);
    let before = queries.get();
    member.fail.set(true);
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.enter(UiRuntime::drain_owner_inbox)
    }))
    .expect("standalone owner drain contains discovery failure");
    {
        let observed = reports.lock().expect("reports");
        assert_eq!(
            observed.len(),
            1,
            "the completed drain delivers its report: {observed:?}"
        );
        let observed = &observed[0];
        assert_eq!(
            observed.address,
            PresentationAddress {
                ui_runtime_id: runtime.id(),
                presentation_id: runtime.presentation_id()
            }
        );
        assert_eq!(observed.first.stage, SamplingStage::DeadlineDiscovery);
        assert_eq!(observed.first.origin, SamplingFailureOrigin::Unknown);
        assert!(!observed.admitted_attempt);
        assert_eq!(observed.disposition, FailureDisposition::Contained);
        assert_eq!(observed.consecutive_failures, 0);
    }
    assert_eq!(queries.get(), before + 1);
    assert_eq!(runtime.next_wake(), Some(now + Duration::from_millis(100)));
    member.fail.set(false);
    clock.advance(Duration::from_millis(100));
    runtime.enter(UiRuntime::drain_owner_inbox);
    assert!(queries.get() > before + 1);
    assert_eq!(runtime.next_wake(), Some(due));
    assert_eq!(reports.lock().expect("reports").len(), 1);
}

#[test]
fn owner_sampling_contract() {
    crate::table_test::run_table(
        "owner sampling",
        &[
            (
                "owner wake observation is inert",
                owner_wake_uses_the_committed_scheduling_observation,
            ),
            (
                "runtime scheduling observations are inert",
                runtime_wake_and_pending_use_the_committed_scheduling_observation,
            ),
            (
                "query report follows lease return and recovery",
                query_failure_reports_after_lease_return_and_recovers,
            ),
            (
                "query report handler failure keeps recovery",
                failing_query_report_handler_preserves_recovery,
            ),
            (
                "query diagnostics failure keeps report and recovery",
                failing_query_diagnostics_preserve_report_and_recovery,
            ),
            (
                "query diagnostics and handler competition keeps recovery",
                competing_query_diagnostics_and_handler_preserve_recovery,
            ),
            (
                "original owner failure survives query report competition",
                original_owner_failure_survives_query_report_competition,
            ),
            (
                "retained deadline publisher changes timing and refuses stale authority",
                retained_deadline_publisher_updates_cancels_and_refuses_stale_authority,
            ),
            (
                "completion refresh observes timing after runtime lease return",
                completion_refresh_observes_timing_after_the_runtime_lease_returns,
            ),
            (
                "standalone owner drain delivers query report and recovery",
                standalone_owner_drain_delivers_its_query_report_and_progresses,
            ),
        ],
    );
}
