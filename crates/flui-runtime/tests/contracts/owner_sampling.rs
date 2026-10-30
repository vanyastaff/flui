//! Scheduling observations never execute an application's deadline query.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::{Rc, Weak};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock, PresentationAddress, UiRuntimeId};
use flui_interaction::{
    GestureDeadlineError, GestureDeadlineUpdate, gesture::GestureArenaMember, ids::PointerId,
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
    due: flui_platform::EventTime,
    fail: Cell<bool>,
}

impl GestureArenaMember for DeadlineQuery {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}

    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}

    fn deadline(&self) -> Option<flui_platform::EventTime> {
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
    queries_before_drain: usize,
    queries_after_drain: usize,
    queries_before: usize,
    queries_after_wake: usize,
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

    fn resize_surface(&self, _: PresentationAddress, _: flui_foundation::geometry::WindowMetrics) {}

    fn frame(&self, _: PresentationAddress, runtime: &mut UiRuntime) {
        self.frames.set(self.frames.get() + 1);
        if self.fail_operation.replace(false) {
            std::panic::panic_any("primary owner operation");
        }
        // The native host services geometry before consulting its wake gate.
        let queries_before_drain = self.query_calls.get();
        runtime.enter(UiRuntime::drain_owner_inbox);
        let queries_after_drain = self.query_calls.get();
        if !self.probe_runtime.get() {
            return;
        }
        let queries_before = self.query_calls.get();
        let wakes = (0..3).map(|_| runtime.next_wake()).collect();
        let queries_after_wake = self.query_calls.get();
        let pending = (0..3).map(|_| runtime.has_pending_work()).collect();
        self.runtime_observation.replace(Some(RuntimeObservation {
            queries_before_drain,
            queries_after_drain,
            queries_before,
            queries_after_wake,
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
    NestedDrain,
}

fn scheduling_observation_case(scope: ObservationScope) {
    let clock = ManualClock::new();
    let due = clock.now() + Duration::from_secs(1);
    let (runtime, _window) = manual_runtime(clock.clone());
    assert!(
        !runtime.has_pending_work(),
        "the empty presentation has no unrelated work before deadline admission"
    );
    let query_calls = Rc::new(Cell::new(0));
    let member = Rc::new(DeadlineQuery {
        calls: Rc::clone(&query_calls),
        due: crate::event_at(&clock, due),
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
        probe_runtime: Cell::new(matches!(scope, ObservationScope::NestedDrain)),
        runtime_observation: RefCell::new(None),
    };

    // The completed owner operation may refresh its scheduling observation.
    // Getters must use that observation without executing the query again.
    assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
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
            assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
            let observed = effects
                .runtime_observation
                .take()
                .expect("runtime getters observed during owner execution");
            assert_eq!(
                observed.queries_after_wake, observed.queries_before,
                "runtime wake observation executed the external deadline query"
            );
            assert_eq!(
                observed.queries_after, observed.queries_after_wake,
                "runtime pending observation executed the external deadline query"
            );
            assert_eq!(observed.wakes, [Some(due); 3]);
            assert_eq!(observed.pending, [true; 3]);
        }
        ObservationScope::NestedDrain => {
            let observed = effects
                .runtime_observation
                .take()
                .expect("nested owner drain observation");
            assert_eq!(
                observed.queries_after_drain, observed.queries_before_drain,
                "a nested native gate drain must wait for the admitted owner transaction"
            );
            assert!(
                query_calls.get() > observed.queries_after_drain,
                "lease return performs the deferred discovery"
            );
            observe_owner_deadline(&owner, &query_calls, Some(due));
        }
    }
}

/// A runtime's native window and the handler it delivers to.
struct ManualWindow {
    _native: Rc<dyn flui_platform::PlatformWindow>,
    handler: Rc<flui_testing::window_handler::RecordingWindowHandler>,
}

fn manual_runtime(clock: ManualClock) -> (UiRuntime, ManualWindow) {
    let handler = flui_testing::window_handler::RecordingWindowHandler::new();
    let window = crate::owner_publication::window_with(Rc::clone(&handler));
    // A native host owns the window; the runtime intentionally holds it weakly.
    let native_window = ManualWindow {
        _native: Rc::clone(window.window()),
        handler,
    };
    let runtime = UiRuntime::new(
        window,
        1.0,
        RuntimeHostServices::new(
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform::InMemoryClipboard::new()),
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

fn nested_owner_drain_defers_discovery_until_lease_return() {
    scheduling_observation_case(ObservationScope::NestedDrain);
}

struct RetryDeadline {
    due: Cell<Option<flui_platform::EventTime>>,
    queries: Cell<usize>,
    polls: Cell<usize>,
    fail_query: Cell<bool>,
}

impl GestureArenaMember for RetryDeadline {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}
    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}

    fn deadline(&self) -> Option<flui_platform::EventTime> {
        self.queries.set(self.queries.get() + 1);
        assert!(!self.fail_query.get(), "native gate deadline query");
        self.due.get()
    }

    fn poll_deadline(&self, _: flui_platform::EventTime) {
        self.polls.set(self.polls.get() + 1);
        self.due.set(None);
    }
}

struct PendingDeadlineEffects {
    clock: ManualClock,
    pending: Cell<bool>,
    pumps: Cell<usize>,
}

impl OwnerEffects for PendingDeadlineEffects {
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
    fn resize_surface(&self, _: PresentationAddress, _: flui_foundation::geometry::WindowMetrics) {}
    fn frame(&self, _: PresentationAddress, runtime: &mut UiRuntime) {
        // Like the native driver, drain inside the owner lease before reading
        // the public pending gate. No other dirty or scheduled work opens it.
        let pending = runtime.enter(|runtime| {
            assert!(!runtime.drain_owner_inbox(), "no unrelated inbox redraw");
            assert!(!runtime.needs_redraw(), "no unrelated redraw demand");
            assert!(
                !runtime.scheduler().is_frame_scheduled(),
                "no scheduled ticker"
            );
            runtime.has_pending_work()
        });
        self.pending.set(pending);
        if pending {
            self.pumps.set(self.pumps.get() + 1);
            let mut clock = self.clock.clone();
            let _ = runtime.pump(&mut clock, &mut SamplingSink);
        }
    }
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: RecoveryState) {}
    fn request_continuation(&self) -> bool {
        true
    }
}

fn query_retry_participates_in_the_pending_gate_and_then_settles() {
    for (registered, repaired) in [(false, true), (false, false), (true, true), (true, false)] {
        let clock = ManualClock::new();
        let now = clock.now();
        let retry_due = now + Duration::from_millis(100);
        let (runtime, _window) = manual_runtime(clock.clone());
        let reports = Arc::new(AtomicUsize::new(0));
        let observed_reports = Arc::clone(&reports);
        runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            let FrameFailureKind::SamplingPanic { first, .. } = &report.kind else {
                panic!("expected a deadline discovery report");
            };
            assert_eq!(first.stage, SamplingStage::DeadlineDiscovery);
            assert_eq!(first.origin, SamplingFailureOrigin::Unknown);
            assert_eq!(report.disposition, FailureDisposition::Contained);
            observed_reports.fetch_add(1, Ordering::Relaxed);
        })));
        let member = Rc::new(RetryDeadline {
            due: Cell::new(Some(crate::event_at(
                &clock,
                now + Duration::from_millis(50),
            ))),
            queries: Cell::new(0),
            polls: Cell::new(0),
            fail_query: Cell::new(true),
        });
        let _entry = runtime
            .gestures()
            .arena()
            .add(PointerId::new(NonZeroU64::MIN), &member);
        let _registration = registered.then(|| {
            let erased: Rc<dyn GestureArenaMember> = member.clone();
            runtime
                .gestures()
                .arena()
                .register_deadline_member(PointerId::new(NonZeroU64::MIN), &erased)
                .expect("register query-derived deadline")
        });
        runtime.enter(UiRuntime::drain_owner_inbox);
        assert_eq!(member.queries.get(), 1, "one original query failure");
        assert_eq!(reports.load(Ordering::Relaxed), 1);
        assert_eq!(runtime.next_wake(), Some(retry_due));

        let owner = OwnerHost::new();
        let address = owner
            .publication(owner.prepare_runtime(runtime))
            .expect("publish runtime")
            .commit();
        let frame = owner.frame_dispatcher(address).expect("frame authority");
        let effects = PendingDeadlineEffects {
            clock: clock.clone(),
            pending: Cell::new(false),
            pumps: Cell::new(0),
        };
        clock.advance(Duration::from_millis(99));
        assert_eq!(
            owner
                .runtime_dispatcher(address.ui_runtime_id)
                .expect("background authority")
                .deliver(flui_runtime::owner::RuntimeOperation::Background, &effects),
            Ok(Delivery::Driven)
        );
        assert_eq!(
            member.queries.get(),
            1,
            "an early owner turn cannot re-probe"
        );
        assert_eq!(
            member.polls.get(),
            0,
            "the stale deadline cannot poll early"
        );
        assert_eq!(owner.next_wake(), Ok(Some(retry_due)));
        assert_eq!(reports.load(Ordering::Relaxed), 1);

        clock.advance(Duration::from_millis(1));
        member.fail_query.set(!repaired);
        assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
        assert_eq!(
            member.polls.get(),
            usize::from(repaired),
            "the due owner opportunity delivers an already-due recovered deadline"
        );
        assert!(
            effects.pending.get(),
            "the due recovery wake must participate in the native pending gate"
        );
        assert_eq!(effects.pumps.get(), 1, "the gate admits its due owner pump");
        assert_eq!(
            owner.next_wake(),
            Ok(None),
            "settled or parked debt cannot spin"
        );
        assert_eq!(
            reports.load(Ordering::Relaxed),
            1,
            "retry never repeats its report"
        );
        if !repaired {
            assert_eq!(member.queries.get(), 2, "one failed automatic re-probe");
        } else if registered {
            assert_eq!(
                member.queries.get(),
                3,
                "the registered query-derived poll resamples changed timing once"
            );
        }

        let after_retry = member.queries.get();
        for _ in 0..3 {
            clock.advance(Duration::from_millis(100));
            assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
            assert!(!effects.pending.get(), "completed recovery closes the gate");
            assert_eq!(owner.next_wake(), Ok(None));
        }
        assert_eq!(
            effects.pumps.get(),
            1,
            "later idle opportunities do not pump"
        );
        assert_eq!(
            member.polls.get(),
            usize::from(repaired),
            "settled work is not replayed"
        );
        if registered || !repaired {
            assert_eq!(
                member.queries.get(),
                after_retry,
                "completed explicit discovery or parked query awaits changed input"
            );
        }
        assert_eq!(reports.load(Ordering::Relaxed), 1);
    }
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
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
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
    let queued = probe.frame.deliver(crate::VSYNC, probe.effects.as_ref());
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
        due: crate::event_at(&clock, due),
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
    assert_eq!(
        frame.deliver(crate::VSYNC, effects.as_ref()),
        Ok(Delivery::Driven)
    );
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
    let diagnostics = fail_diagnostics
        .then(|| crate::scoped_diagnostics(FailingDiagnostics(Arc::clone(&diagnostic_calls))));
    member.fail.set(true);
    effects.fail_operation.set(fail_operation);
    let delivered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        frame.deliver(crate::VSYNC, effects.as_ref())
    }));
    drop(diagnostics);
    if fail_operation {
        // The operation's own panic is contained (ADR-0258 §1), and it is the
        // one reported: query, tracing and handler failures cannot replace it.
        assert_eq!(
            delivered.expect("the owner operation's panic is contained"),
            Ok(Delivery::Driven)
        );
        assert_eq!(
            *probe.callback_observations.borrow(),
            [(
                address,
                PanicText::Verbatim("primary owner operation".into())
            )]
        );
        // What its sampling report admitted runs at the next native
        // opportunity, with the normal FIFO budget.
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
    assert_eq!(
        frame.deliver(crate::VSYNC, effects.as_ref()),
        Ok(Delivery::Driven)
    );
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
    assert_eq!(
        frame.deliver(crate::VSYNC, effects.as_ref()),
        Ok(Delivery::Driven)
    );
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
    due: Cell<Option<flui_platform::EventTime>>,
    queries: Rc<Cell<usize>>,
}

impl GestureArenaMember for PublishedDeadline {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}

    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}

    fn deadline(&self) -> Option<flui_platform::EventTime> {
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
    let (runtime, window) = manual_runtime(clock.clone());
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_count = Arc::clone(&wakes);
    window.handler.react_to_frame(move |_| {
        wake_count.fetch_add(1, Ordering::Relaxed);
    });
    let queries = Rc::new(Cell::new(0));
    let member = Rc::new(PublishedDeadline {
        due: Cell::new(Some(crate::event_at(&clock, initial))),
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
        .publish(Some(crate::event_at(&clock, initial)))
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
    assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
    observe_owner_deadline(&owner, &queries, Some(initial));

    let before = wakes.load(Ordering::Relaxed);
    member.due.set(Some(crate::event_at(&clock, earlier)));
    publisher
        .publish(Some(crate::event_at(&clock, earlier)))
        .expect("retained authority publishes earlier timing");
    assert!(
        wakes.load(Ordering::Relaxed) > before,
        "earlier timing wakes without a manual poke"
    );
    observe_owner_deadline(&owner, &queries, Some(earlier));
    assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
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
        publisher.publish(Some(crate::event_at(&clock, earlier))),
        Err(GestureDeadlineError::Retired)
    );
    assert_eq!(publisher.refresh(), Err(GestureDeadlineError::Retired));
    assert_eq!(wakes.load(Ordering::Relaxed), before);
    observe_owner_deadline(&owner, &queries, None);

    let successor_registration = arena
        .register_deadline_member(pointer, &erased)
        .expect("register successor under the reused pointer");
    let successor = successor_registration.update_handle();
    member.due.set(Some(crate::event_at(&clock, successor_due)));
    successor
        .publish(Some(crate::event_at(&clock, successor_due)))
        .expect("successor timing");
    observe_owner_deadline(&owner, &queries, Some(successor_due));
    let before = wakes.load(Ordering::Relaxed);
    assert_eq!(publisher.publish(None), Err(GestureDeadlineError::Retired));
    assert_eq!(
        publisher.publish(Some(crate::event_at(&clock, earlier))),
        Err(GestureDeadlineError::Retired)
    );
    assert_eq!(publisher.refresh(), Err(GestureDeadlineError::Retired));
    assert_eq!(wakes.load(Ordering::Relaxed), before);
    observe_owner_deadline(&owner, &queries, Some(successor_due));
    member.due.set(None);
    successor.publish(None).expect("cancel successor timing");
    observe_owner_deadline(&owner, &queries, None);
}

fn registered_query_revision_case(fail_first_query: bool) {
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
        due: crate::event_at(&clock, due),
        fail: Cell::new(fail_first_query),
    });
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = runtime
        .gestures()
        .arena()
        .register_deadline_member(PointerId::new(NonZeroU64::MIN), &erased)
        .expect("register explicit deadline source");
    let publisher = registration.update_handle();
    publisher
        .publish(Some(crate::event_at(&clock, due)))
        .expect("publish initial timing");
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
        probe_runtime: Cell::new(false),
        runtime_observation: RefCell::new(None),
    };

    publisher.refresh().expect("request one query revision");
    assert_eq!(
        queries.get(),
        0,
        "refresh admission does not call the member"
    );
    assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
    assert_eq!(
        queries.get(),
        1,
        "the owner observes the admitted query revision"
    );
    if fail_first_query {
        {
            let observed = reports.lock().expect("reports");
            assert_eq!(observed.len(), 1, "one failed revision report");
            let report = &observed[0];
            assert_eq!(report.address, address);
            assert_eq!(report.first.stage, SamplingStage::DeadlineDiscovery);
            assert_eq!(report.first.origin, SamplingFailureOrigin::Unknown);
            assert!(!report.admitted_attempt);
            assert_eq!(report.disposition, FailureDisposition::Contained);
            assert_eq!(report.consecutive_failures, 0);
        }
        let retry_due = now + Duration::from_millis(100);
        observe_owner_deadline(&owner, &queries, Some(retry_due));
        clock.advance(Duration::from_millis(99));
        assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
        assert_eq!(
            queries.get(),
            1,
            "the retry query waits for its due opportunity"
        );
        observe_owner_deadline(&owner, &queries, Some(retry_due));
        member.fail.set(false);
        clock.advance(Duration::from_millis(1));
        assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
        assert_eq!(queries.get(), 2, "one successful automatic re-probe");
    }
    let observed_queries = queries.get();
    let expected_reports = usize::from(fail_first_query);
    observe_owner_deadline(&owner, &queries, Some(due));
    assert_eq!(reports.lock().expect("reports").len(), expected_reports);

    // An explicit source changes its query revision through its publisher.
    // Unrelated owner work cannot rediscover or fail the completed revision.
    member.fail.set(true);
    for _ in 0..3 {
        assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
        assert_eq!(
            queries.get(),
            observed_queries,
            "the completed explicit query revision remains cached on the next owner turn"
        );
        assert_eq!(reports.lock().expect("reports").len(), expected_reports);
        observe_owner_deadline(&owner, &queries, Some(due));
    }

    member.fail.set(false);
    publisher.refresh().expect("request a new query revision");
    assert_eq!(queries.get(), observed_queries);
    assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
    assert_eq!(
        queries.get(),
        observed_queries + 1,
        "a new revision observes even an unchanged numerical deadline"
    );
    observe_owner_deadline(&owner, &queries, Some(due));
    assert_eq!(reports.lock().expect("reports").len(), expected_reports);

    publisher.publish(None).expect("cancel explicit timing");
    member.fail.set(true);
    assert_eq!(frame.deliver(crate::VSYNC, &effects), Ok(Delivery::Driven));
    assert_eq!(
        queries.get(),
        observed_queries + 1,
        "cancellation needs no query"
    );
    observe_owner_deadline(&owner, &queries, None);
    assert_eq!(reports.lock().expect("reports").len(), expected_reports);
}

fn registered_query_revision_is_observed_once() {
    registered_query_revision_case(false);
}

fn registered_query_reprobe_success_stays_cached() {
    registered_query_revision_case(true);
}

struct CompletionRefresh {
    member: Rc<PublishedDeadline>,
    publisher: GestureDeadlineUpdate,
    due: flui_platform::EventTime,
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
    fn resize_surface(&self, _: PresentationAddress, _: flui_foundation::geometry::WindowMetrics) {}
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
    let (runtime, _window) = manual_runtime(clock.clone());
    let queries = Rc::new(Cell::new(0));
    let member = Rc::new(PublishedDeadline {
        due: Cell::new(Some(crate::event_at(&clock, initial))),
        queries: Rc::clone(&queries),
    });
    let erased: Rc<dyn GestureArenaMember> = member.clone();
    let registration = runtime
        .gestures()
        .arena()
        .register_deadline_member(PointerId::new(NonZeroU64::MIN), &erased)
        .expect("register source");
    let publisher = registration.update_handle();
    publisher
        .publish(Some(crate::event_at(&clock, initial)))
        .expect("initial timing");
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
            due: crate::event_at(&clock, completed),
        })),
        observation: RefCell::new(None),
    };
    assert_eq!(
        owner
            .frame_dispatcher(address)
            .expect("frame authority")
            .deliver(crate::VSYNC, &effects),
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
        due: crate::event_at(&clock, due),
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

struct SamplingSink;

impl flui_runtime::sink::FrameSink for SamplingSink {
    fn surface_size(&mut self) -> (u32, u32) {
        (800, 600)
    }

    fn submit(&mut self, _: flui_layer::Scene) -> flui_runtime::sink::SubmitVerdict {
        flui_runtime::sink::SubmitVerdict::Presented
    }
}

thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static PENDING_RUNTIME: RefCell<Option<Weak<UiRuntime>>> = const { RefCell::new(None) };
}

struct PendingProbeScope;

impl PendingProbeScope {
    fn install(runtime: &Rc<UiRuntime>) -> Self {
        let previous = PENDING_RUNTIME.with(|slot| slot.replace(Some(Rc::downgrade(runtime))));
        assert!(previous.is_none(), "pending probe is not nested");
        Self
    }
}

impl Drop for PendingProbeScope {
    fn drop(&mut self) {
        let retired = PENDING_RUNTIME.with(RefCell::take);
        drop(retired);
    }
}

#[derive(Default)]
struct TraceCalls {
    events: AtomicUsize,
    reentries: AtomicUsize,
    refused_reentries: AtomicUsize,
    inside_event: AtomicBool,
}

struct PendingDiagnostics(Arc<TraceCalls>);

impl tracing::Subscriber for PendingDiagnostics {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, _: &tracing::Event<'_>) {
        self.0.events.fetch_add(1, Ordering::Relaxed);
        if self.0.inside_event.swap(true, Ordering::Relaxed) {
            return;
        }
        let runtime = PENDING_RUNTIME
            .with(|slot| slot.borrow().clone())
            .and_then(|owner| owner.upgrade());
        if let Some(runtime) = runtime {
            self.0.reentries.fetch_add(1, Ordering::Relaxed);
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.has_pending_work()))
                .is_err()
            {
                self.0.refused_reentries.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.0.inside_event.store(false, Ordering::Relaxed);
    }

    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

fn mounted_root_invalidation(
    runtime: &UiRuntime,
) -> flui_rendering::pipeline::RenderInvalidationHandle {
    runtime
        .widgets()
        .pipeline_owner()
        .expect("mounted pipeline")
        .with(|owner| {
            owner
                .render_invalidation_handle(owner.root_id().expect("mounted root render object"))
                .expect("attached root has an invalidation capability")
        })
}

fn queued_render_invalidation_is_observed_without_foreign_work() {
    let mut clock = ManualClock::new();
    let (mut runtime, window) = manual_runtime(clock.clone());
    runtime
        .attach_root_widget_with_size(&flui_widgets::SizedBox::square(10.0), 800.0, 600.0)
        .expect("first root");
    let _ = runtime.pump(&mut clock, &mut SamplingSink);
    assert!(
        !runtime.has_pending_work(),
        "a real frame settles the first tree"
    );
    let retired = mounted_root_invalidation(&runtime);
    runtime.enter(|runtime| runtime.widgets().detach_root_widget());
    runtime
        .attach_root_widget_with_size(&flui_widgets::SizedBox::square(20.0), 800.0, 600.0)
        .expect("replacement root");
    clock.advance(Duration::from_millis(20));
    let _ = runtime.pump(&mut clock, &mut SamplingSink);
    assert!(
        !runtime.has_pending_work(),
        "replacement frame settles unrelated work"
    );
    let live = mounted_root_invalidation(&runtime);
    let wakes = Arc::new(AtomicUsize::new(0));
    let observed_wakes = Arc::clone(&wakes);
    window.handler.react_to_frame(move |_| {
        observed_wakes.fetch_add(1, Ordering::Relaxed);
    });
    retired
        .mark_needs_paint()
        .expect("the live pipeline accepts the queued old attachment");
    let wakes_before_getter = wakes.load(Ordering::Relaxed);
    let mut runtime = Rc::new(runtime);
    let scope = PendingProbeScope::install(&runtime);
    let calls = Arc::new(TraceCalls::default());
    let diagnostics = crate::scoped_diagnostics(PendingDiagnostics(Arc::clone(&calls)));
    let pending: Vec<_> = (0..3).map(|_| runtime.has_pending_work()).collect();
    drop(diagnostics);
    drop(scope);
    assert_eq!(
        calls.events.load(Ordering::Relaxed),
        0,
        "pending observation must not drain or trace stale work"
    );
    assert_eq!(
        calls.reentries.load(Ordering::Relaxed),
        0,
        "a getter invokes no subscriber reentry"
    );
    assert_eq!(
        wakes.load(Ordering::Relaxed),
        wakes_before_getter,
        "a getter invokes no native wake callback"
    );
    assert_eq!(
        pending, [true; 3],
        "queued work remains pending until a real drain"
    );

    let runtime = Rc::get_mut(&mut runtime).expect("the native host exclusively owns its runtime");
    clock.advance(Duration::from_millis(20));
    let _ = runtime.pump(&mut clock, &mut SamplingSink);
    assert!(
        !runtime.has_pending_work(),
        "the real frame retires the stale request"
    );

    live.mark_needs_layout()
        .expect("live attachment admits an invalidation");
    let wakes_before_getter = wakes.load(Ordering::Relaxed);
    let calls = Arc::new(TraceCalls::default());
    let diagnostics = crate::scoped_diagnostics(PendingDiagnostics(Arc::clone(&calls)));
    let pending: Vec<_> = (0..3).map(|_| runtime.has_pending_work()).collect();
    drop(diagnostics);
    assert_eq!(calls.events.load(Ordering::Relaxed), 0);
    assert_eq!(wakes.load(Ordering::Relaxed), wakes_before_getter);
    assert_eq!(
        pending, [true; 3],
        "fresh accepted work is visible without executing its notifier"
    );
    clock.advance(Duration::from_millis(20));
    let _ = runtime.pump(&mut clock, &mut SamplingSink);
    assert!(
        !runtime.has_pending_work(),
        "the next real frame completes fresh work"
    );
}

struct HostedDeadline {
    due: Cell<Option<flui_platform::EventTime>>,
    polls: Rc<Cell<usize>>,
    queries: Rc<Cell<usize>>,
    fail_after_effect: bool,
}

impl GestureArenaMember for HostedDeadline {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}
    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}

    fn deadline(&self) -> Option<flui_platform::EventTime> {
        self.queries.set(self.queries.get() + 1);
        self.due.get()
    }

    fn poll_deadline(&self, _: flui_platform::EventTime) {
        self.polls.set(self.polls.get() + 1);
        if self.fail_after_effect {
            std::panic::panic_any("hosted unknown deadline progress");
        }
        self.due.set(None);
    }
}

fn hosted_background_services_deadlines_after_its_admitted_transaction() {
    let clock = ManualClock::new();
    let (runtime, _window) = manual_runtime(clock.clone());
    runtime.update_host_lifecycle(flui_scheduler::AppLifecycleState::Paused);
    runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
    runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(observe_sampling_report)));
    let arena = runtime.gestures().arena().clone();
    let due = clock.now() + Duration::from_millis(50);
    let queries = Rc::new(Cell::new(0));
    let unknown_polls = Rc::new(Cell::new(0));
    let healthy_polls = Rc::new(Cell::new(0));
    let unknown = Rc::new(HostedDeadline {
        due: Cell::new(Some(crate::event_at(&clock, due))),
        polls: Rc::clone(&unknown_polls),
        queries: Rc::clone(&queries),
        fail_after_effect: true,
    });
    let healthy = Rc::new(HostedDeadline {
        due: Cell::new(Some(crate::event_at(&clock, due))),
        polls: Rc::clone(&healthy_polls),
        queries: Rc::clone(&queries),
        fail_after_effect: false,
    });
    // The paused lifecycle has already cancelled preceding contacts. Admit
    // these owner-side timers afterwards, while visual frames stay disabled.
    let _unknown_entry = arena.add(PointerId::new(NonZeroU64::MIN), &unknown);
    let _healthy_entry = arena.add(
        PointerId::new(NonZeroU64::new(2).expect("contact")),
        &healthy,
    );
    let owner = OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish runtime")
        .commit();
    assert!(
        !owner
            .runtime_status(address.ui_runtime_id)
            .expect("runtime status")
            .frames_enabled
    );
    let effects = Rc::new(Effects {
        query_calls: Rc::clone(&queries),
        frames: Cell::new(0),
        fail_operation: Cell::new(false),
        probe_runtime: Cell::new(false),
        runtime_observation: RefCell::new(None),
    });
    let probe = Rc::new(ReportProbe {
        owner: owner.clone(),
        frame: owner.frame_dispatcher(address).expect("frame authority"),
        effects: Rc::clone(&effects),
        fail_handler: false,
        observations: RefCell::new(Vec::new()),
        callback_observations: RefCell::new(Vec::new()),
    });
    let _scope = ScopedReportProbe::install(Rc::clone(&probe));
    let background = owner
        .runtime_dispatcher(address.ui_runtime_id)
        .expect("runtime authority");
    clock.advance(Duration::from_millis(50));
    assert_eq!(
        background.deliver(
            flui_runtime::owner::RuntimeOperation::Background,
            effects.as_ref()
        ),
        Ok(Delivery::Driven)
    );
    assert_eq!(
        unknown_polls.get(),
        1,
        "the hosted background operation services its due source"
    );
    assert_eq!(
        healthy_polls.get(),
        1,
        "a healthy deadline progresses after the unknown poll fails"
    );
    {
        let observations = probe.observations.borrow();
        assert_eq!(observations.len(), 1, "one contained unknown poll report");
        let report = &observations[0];
        assert_eq!(report.address, address);
        assert_eq!(report.first.stage, SamplingStage::GestureDeadline);
        assert_eq!(report.first.origin, SamplingFailureOrigin::Unknown);
        assert!(!report.admitted_attempt);
        assert_eq!(report.disposition, FailureDisposition::Contained);
        assert_eq!(report.consecutive_failures, 0);
        assert_eq!(
            report.message,
            PanicText::Verbatim("hosted unknown deadline progress".into())
        );
        assert_eq!(report.executing, Ok(false));
        assert_eq!(report.runtimes, Ok(vec![address.ui_runtime_id]));
        assert_eq!(
            report.wake,
            Ok(None),
            "unknown side effects have no automatic replay wake"
        );
        assert_eq!(report.queued, Ok(Delivery::Queued));
        assert_eq!(report.frames_before, report.frames_after);
        assert_eq!(report.queries_before, report.queries_after);
    }
    assert_eq!(
        effects.frames.get(),
        1,
        "the report's accepted reentry reaches the owner FIFO"
    );
    assert!(probe.callback_observations.borrow().is_empty());

    let next_polls = Rc::new(Cell::new(0));
    let next = Rc::new(HostedDeadline {
        due: Cell::new(Some(crate::event_at(
            &clock,
            clock.now() + Duration::from_millis(20),
        ))),
        polls: Rc::clone(&next_polls),
        queries: Rc::clone(&queries),
        fail_after_effect: false,
    });
    let _next_entry = arena.add(
        PointerId::new(NonZeroU64::new(3).expect("next contact")),
        &next,
    );
    clock.advance(Duration::from_millis(20));
    assert_eq!(
        background.deliver(
            flui_runtime::owner::RuntimeOperation::Background,
            effects.as_ref()
        ),
        Ok(Delivery::Driven)
    );
    assert_eq!(
        next_polls.get(),
        1,
        "the next hosted background turn services new accepted work"
    );
    clock.advance(Duration::from_millis(100));
    assert_eq!(
        background.deliver(
            flui_runtime::owner::RuntimeOperation::Background,
            effects.as_ref()
        ),
        Ok(Delivery::Driven)
    );
    assert_eq!(
        unknown_polls.get(),
        1,
        "an unknown poll cannot replay the same revision"
    );
    assert_eq!(healthy_polls.get(), 1);
    assert_eq!(next_polls.get(), 1);
    assert_eq!(probe.observations.borrow().len(), 1);
    observe_owner_deadline(&owner, &queries, None);
}

struct ReentrantDeadlineQuery {
    runtime: Weak<UiRuntime>,
    due: flui_platform::EventTime,
    queries: Cell<usize>,
    reentries: Cell<usize>,
    enter_once: Cell<bool>,
    drained_redraw: Cell<bool>,
}

impl GestureArenaMember for ReentrantDeadlineQuery {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}
    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}

    fn deadline(&self) -> Option<flui_platform::EventTime> {
        self.queries.set(self.queries.get() + 1);
        if self.enter_once.replace(false) {
            self.reentries.set(self.reentries.get() + 1);
            let runtime = self
                .runtime
                .upgrade()
                .expect("the query's owner remains live");
            runtime.command_sender().request_redraw();
            self.drained_redraw
                .set(runtime.enter(UiRuntime::drain_owner_inbox));
        }
        Some(self.due)
    }
}

fn deadline_query_reentry_does_not_rediscover_its_active_source() {
    let clock = ManualClock::new();
    let due = clock.now() + Duration::from_secs(1);
    let (runtime, _window) = manual_runtime(clock.clone());
    let runtime = Rc::new(runtime);
    let member = Rc::new(ReentrantDeadlineQuery {
        runtime: Rc::downgrade(&runtime),
        due: crate::event_at(&clock, due),
        queries: Cell::new(0),
        reentries: Cell::new(0),
        enter_once: Cell::new(true),
        drained_redraw: Cell::new(false),
    });
    let _entry = runtime
        .gestures()
        .arena()
        .add(PointerId::new(NonZeroU64::MIN), &member);
    runtime.enter(UiRuntime::drain_owner_inbox);
    assert!(
        member.drained_redraw.get(),
        "the nested drain still receives accepted redraw demand"
    );
    assert_eq!(
        member.reentries.get(),
        1,
        "the accepted reentrant owner drain ran"
    );
    assert_eq!(
        member.queries.get(),
        1,
        "reentry must not rediscover the already active source"
    );
    let before = member.queries.get();
    for _ in 0..3 {
        assert_eq!(runtime.next_wake(), Some(due));
    }
    assert_eq!(
        member.queries.get(),
        before,
        "wake reads remain inert after query reentry"
    );

    runtime.enter(UiRuntime::drain_owner_inbox);
    assert_eq!(
        member.queries.get(),
        2,
        "the next ordinary owner frontier still observes its source"
    );
    assert_eq!(member.reentries.get(), 1);
    assert_eq!(runtime.next_wake(), Some(due));
    assert_eq!(member.queries.get(), 2);
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
                "nested owner drain defers discovery until lease return",
                nested_owner_drain_defers_discovery_until_lease_return,
            ),
            (
                "query retry opens the pending gate and then settles",
                query_retry_participates_in_the_pending_gate_and_then_settles,
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
                "registered query revision is observed once",
                registered_query_revision_is_observed_once,
            ),
            (
                "registered query re-probe success stays cached",
                registered_query_reprobe_success_stays_cached,
            ),
            (
                "completion refresh observes timing after runtime lease return",
                completion_refresh_observes_timing_after_the_runtime_lease_returns,
            ),
            (
                "standalone owner drain delivers query report and recovery",
                standalone_owner_drain_delivers_its_query_report_and_progresses,
            ),
            (
                "queued render invalidation observation executes no foreign work",
                queued_render_invalidation_is_observed_without_foreign_work,
            ),
            (
                "hosted background services deadlines after its admitted transaction",
                hosted_background_services_deadlines_after_its_admitted_transaction,
            ),
            (
                "deadline query reentry does not rediscover its active source",
                deadline_query_reentry_does_not_rediscover_its_active_source,
            ),
        ],
    );
}
