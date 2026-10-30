//! Observation is optional and cannot become an animation failure boundary.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::{
    AnimatedValue, Animation, AnimationController, MotionSpec, MotionUpdate, REFUSAL_TARGET,
    RepeatMode, SpringDescription, TwoWayConverter, Vsync,
};
#[cfg(feature = "diagnostics")]
use flui_foundation::Listenable;
use tracing::{Event, Subscriber};
use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static OBSERVED: RefCell<Option<AnimationController>> = const { RefCell::new(None) };
    #[cfg(feature = "diagnostics")]
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static OBSERVED_OWNER: RefCell<Option<flui_animation::DrivenController>> = const { RefCell::new(None) };
    #[cfg(feature = "diagnostics")]
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static OBSERVED_VSYNC: RefCell<Option<Vsync>> = const { RefCell::new(None) };
    #[cfg(feature = "diagnostics")]
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ADMITTED: RefCell<Vec<(flui_animation::DrivenController, flui_animation::AnimationRunFuture)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct Fields(Vec<(String, String)>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }
}

impl Fields {
    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hook {
    New,
    Enter,
    Event,
    Exit,
    Close,
}

#[derive(Clone, Copy, Debug)]
struct Fault {
    hook: Hook,
    span: &'static str,
    opaque: bool,
}

struct OpaqueFault(Arc<AtomicBool>);
impl Drop for OpaqueFault {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
        panic!("opaque subscriber payload must not retire");
    }
}

struct Observer {
    records: Arc<Mutex<Vec<Fields>>>,
    armed: Arc<AtomicBool>,
    borrow_failures: Arc<AtomicUsize>,
    optional_spans: Arc<AtomicUsize>,
    payload_retired: Arc<AtomicBool>,
    fault: Option<Fault>,
}

impl Observer {
    fn touch(&self, hook: Hook, span: &str) {
        OBSERVED.with(|observed| {
            let controller = observed.borrow().clone();
            if let Some(controller) = controller {
                let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (
                        controller.value(),
                        controller.status(),
                        controller.is_animating(),
                    )
                }));
                if read.is_err() {
                    self.borrow_failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        if let Some(fault) = self.fault
            && fault.hook == hook
            && fault.span == span
            && self.armed.swap(false, Ordering::Relaxed)
        {
            if fault.opaque {
                std::panic::panic_any(OpaqueFault(Arc::clone(&self.payload_retired)));
            }
            panic!("subscriber failure");
        }
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Observer {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _: &tracing::span::Id,
        _: Context<'_, S>,
    ) {
        if attrs.metadata().target().starts_with("flui_animation") {
            self.optional_spans.fetch_add(1, Ordering::Relaxed);
            self.touch(Hook::New, attrs.metadata().name());
        }
    }
    fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && span.metadata().target().starts_with("flui_animation")
        {
            self.touch(Hook::Enter, span.name());
        }
    }
    fn on_exit(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && span.metadata().target().starts_with("flui_animation")
        {
            self.touch(Hook::Exit, span.name());
        }
    }
    fn on_close(&self, id: tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(&id)
            && span.metadata().target().starts_with("flui_animation")
        {
            self.touch(Hook::Close, span.name());
        }
    }
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        if event.metadata().target() == "flui_animation::diagnostics" {
            self.touch(Hook::Event, "animation transition");
        }
        self.records.lock().expect("observer records").push(fields);
    }
}

fn transitions_follow_real_run_outcomes() {
    flui_testing::log_capture::disarm_interest_cache();
    let records = Arc::new(Mutex::new(Vec::new()));
    let optional_spans = Arc::new(AtomicUsize::new(0));
    let observer = Observer {
        records: Arc::clone(&records),
        armed: Arc::new(AtomicBool::new(false)),
        borrow_failures: Arc::new(AtomicUsize::new(0)),
        optional_spans: Arc::clone(&optional_spans),
        payload_retired: Arc::new(AtomicBool::new(false)),
        fault: None,
    };
    tracing::subscriber::with_default(tracing_subscriber::registry().with(observer), || {
        tracing::debug!(marker = "capture control");
        let registry = Vsync::new();
        let mut owner =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
        let controller = owner.controller();
        let original = controller.forward().expect("forward");
        controller.tick_at(Duration::from_millis(50));
        assert!((controller.value() - 0.5).abs() < 1e-9);
        let replacement = controller
            .animate_to(0.75, Some(Duration::from_millis(100)))
            .expect("replacement");
        assert!(original.is_canceled());
        controller.stop().expect("cancel replacement");
        assert!(replacement.is_canceled());
        let completed = controller.forward().expect("new run");
        controller.tick_at(Duration::from_millis(100));
        assert!(completed.is_complete());
        owner.dispose();
        let peer =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
        let peer_run = peer.controller().forward().expect("independent run");
        peer.controller().tick_at(Duration::from_millis(100));
        assert!(peer_run.is_complete());
    });
    let records = records.lock().expect("observer records");
    assert!(
        records
            .iter()
            .any(|record| record.get("marker") == Some("capture control"))
    );
    let transitions: Vec<_> = records
        .iter()
        .filter(|record| record.get("transition").is_some())
        .collect();
    if cfg!(feature = "diagnostics") {
        let kinds: Vec<_> = transitions
            .iter()
            .map(|record| record.get("transition").expect("transition field"))
            .collect();
        assert_eq!(
            &kinds[..6],
            &["clock", "start", "replace", "cancel", "start", "complete"]
        );
        assert_eq!(transitions[0].get("clock"), Some("bound"));
        assert_eq!(transitions[0].get("run_generation"), Some("0"));
        assert_eq!(transitions[0].get("active"), Some("false"));
        assert_eq!(
            transitions[0].get("controller"),
            transitions[1].get("controller")
        );
        assert!(kinds.contains(&"dispose"));
        let replacement = transitions
            .iter()
            .find(|record| record.get("transition") == Some("replace"))
            .expect("replacement receipt");
        assert_eq!(replacement.get("run_generation"), Some("2"));
        assert_eq!(replacement.get("outcome_generation"), Some("1"));
        assert_eq!(replacement.get("outcome"), Some("cancelled"));
        let starts: Vec<_> = transitions
            .iter()
            .filter(|record| record.get("transition") == Some("start"))
            .collect();
        assert_ne!(
            starts.first().expect("first controller").get("controller"),
            starts.last().expect("peer controller").get("controller")
        );
    } else {
        assert!(
            transitions.is_empty(),
            "feature-off animation must emit no optional receipts"
        );
        assert_eq!(optional_spans.load(Ordering::Relaxed), 0);
    }
}

fn filtered_diagnostics_do_not_emit() {
    flui_testing::log_capture::disarm_interest_cache();
    let records = Arc::new(Mutex::new(Vec::new()));
    let optional_spans = Arc::new(AtomicUsize::new(0));
    let observer = Observer {
        records: Arc::clone(&records),
        armed: Arc::new(AtomicBool::new(false)),
        borrow_failures: Arc::new(AtomicUsize::new(0)),
        optional_spans: Arc::clone(&optional_spans),
        payload_retired: Arc::new(AtomicBool::new(false)),
        fault: None,
    };
    let subscriber = tracing_subscriber::registry()
        .with(observer.with_filter(tracing_subscriber::filter::LevelFilter::WARN));
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(marker = "filtered capture control");
        let registry = Vsync::new();
        let owner =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
        let controller = owner.controller();
        let run = controller.forward().expect("real run");
        controller.tick_at(Duration::from_millis(100));
        assert!(run.is_complete());
        assert_eq!(controller.value(), 1.0);
    });
    let records = records.lock().expect("observer records");
    assert!(
        records
            .iter()
            .any(|record| record.get("marker") == Some("filtered capture control"))
    );
    assert!(
        records
            .iter()
            .all(|record| record.get("transition").is_none())
    );
    assert_eq!(optional_spans.load(Ordering::Relaxed), 0);
}

#[cfg(feature = "diagnostics")]
fn subscriber_failures_preserve_delivery_and_recovery() {
    flui_testing::log_capture::disarm_interest_cache();
    let mut faults = Vec::new();
    for span in [
        "animation_tick",
        "animation_sampling",
        "animation_listeners",
        "animation_controller",
    ] {
        for hook in [Hook::New, Hook::Enter, Hook::Exit, Hook::Close] {
            if span == "animation_controller" && matches!(hook, Hook::Enter | Hook::Exit) {
                continue;
            }
            faults.push(Fault {
                hook,
                span,
                opaque: false,
            });
        }
    }
    faults.push(Fault {
        hook: Hook::Event,
        span: "animation transition",
        opaque: false,
    });
    faults.push(Fault {
        hook: Hook::Exit,
        span: "animation_sampling",
        opaque: true,
    });
    for fault in faults {
        for callback_fails in [false, true] {
            // Failed walks retain opaque controller snapshots under their
            // existing custody policy; their root span intentionally survives.
            if callback_fails && fault.span == "animation_controller" && fault.hook == Hook::Close {
                continue;
            }
            let armed = Arc::new(AtomicBool::new(
                fault.span == "animation_controller" && fault.hook == Hook::New,
            ));
            let borrow_failures = Arc::new(AtomicUsize::new(0));
            let payload_retired = Arc::new(AtomicBool::new(false));
            let observer = Observer {
                records: Arc::new(Mutex::new(Vec::new())),
                armed: Arc::clone(&armed),
                borrow_failures: Arc::clone(&borrow_failures),
                optional_spans: Arc::new(AtomicUsize::new(0)),
                payload_retired: Arc::clone(&payload_retired),
                fault: Some(fault),
            };
            tracing::subscriber::with_default(
                tracing_subscriber::registry().with(observer),
                || {
                    let registry = Vsync::new();
                    let owner = AnimationController::builder(Duration::from_millis(100))
                        .build_on(Some(&registry));
                    let controller = owner.controller();
                    OBSERVED.with(|observed| *observed.borrow_mut() = Some(controller.clone()));
                    let mut clock = flui_animation::MotionClock::new();
                    let throws = Rc::new(std::cell::Cell::new(callback_fails));
                    controller.add_listener(Rc::new(move || {
                        let value = OBSERVED.with(|observed| {
                            observed
                                .borrow()
                                .as_ref()
                                .expect("observed controller")
                                .value()
                        });
                        assert!(
                            value != 1.0 || !throws.replace(false),
                            "real listener failure"
                        );
                    }));
                    let healthy = Rc::new(std::cell::Cell::new(0));
                    let tail = Rc::clone(&healthy);
                    controller.add_listener(Rc::new(move || tail.set(tail.get() + 1)));
                    let original = controller.forward().expect("original run");
                    if fault.span != "animation_controller" {
                        armed.store(true, Ordering::Relaxed);
                    }
                    let sampled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        registry.tick_all(&clock.frame(Duration::ZERO));
                        registry.tick_all(&clock.frame(Duration::from_millis(100)));
                    }));
                    if callback_fails {
                        let failure =
                            sampled.expect_err("the real listener failure stays authoritative");
                        assert_eq!(
                            failure.downcast_ref::<&str>(),
                            Some(&"real listener failure"),
                            "{fault:?}"
                        );
                    } else {
                        sampled.expect("subscriber failure cannot fail animation");
                    }
                    registry.tick_all(&clock.frame(Duration::from_millis(100)));
                    assert!(original.is_complete(), "{fault:?}");
                    let next = controller.reverse().expect("healthy next run");
                    registry.tick_all(&clock.frame(Duration::from_millis(100)));
                    registry.tick_all(&clock.frame(Duration::from_millis(200)));
                    assert!(next.is_complete(), "{fault:?}");
                    assert_eq!(controller.value(), 0.0);
                    assert!(
                        healthy.get() >= 4,
                        "healthy delivery must continue: {fault:?}"
                    );
                    if fault.span == "animation_controller" && fault.hook == Hook::Close {
                        armed.store(true, Ordering::Relaxed);
                    }
                    OBSERVED.with(|observed| *observed.borrow_mut() = None);
                },
            );
            assert!(
                !armed.load(Ordering::Relaxed),
                "the intended subscriber callout must occur: {fault:?}"
            );
            assert_eq!(
                borrow_failures.load(Ordering::Relaxed),
                0,
                "subscriber reentry must not cross a borrow: {fault:?}"
            );
            assert!(
                !payload_retired.load(Ordering::Relaxed),
                "opaque payload custody: {fault:?}"
            );
        }
    }
}

#[cfg(feature = "diagnostics")]
fn subscriber_commands_preserve_committed_receipts() {
    struct CancelOnce(AtomicBool);
    impl<S: Subscriber> Layer<S> for CancelOnce {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            if fields.get("transition") == Some("start") && self.0.swap(false, Ordering::Relaxed) {
                let controller = OBSERVED.with(|observed| observed.borrow().clone());
                controller
                    .expect("published controller")
                    .stop()
                    .expect("subscriber cancellation");
            }
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let records = Arc::new(Mutex::new(Vec::new()));
    let observer = Observer {
        records: Arc::clone(&records),
        armed: Arc::new(AtomicBool::new(false)),
        borrow_failures: Arc::new(AtomicUsize::new(0)),
        optional_spans: Arc::new(AtomicUsize::new(0)),
        payload_retired: Arc::new(AtomicBool::new(false)),
        fault: None,
    };
    tracing::subscriber::with_default(
        tracing_subscriber::registry()
            .with(observer)
            .with(CancelOnce(AtomicBool::new(true))),
        || {
            let controller = AnimationController::builder(Duration::from_millis(100)).build();
            OBSERVED.with(|observed| *observed.borrow_mut() = Some(controller.clone()));
            let canceled = controller.forward().expect("accepted then canceled run");
            assert!(canceled.is_canceled());
            assert!(!controller.is_animating());
            let next = controller.forward().expect("healthy subsequent run");
            controller.tick_at(Duration::from_millis(100));
            assert!(next.is_complete());
            OBSERVED.with(|observed| *observed.borrow_mut() = None);
        },
    );
    let records = records.lock().expect("receipts");
    let transitions: Vec<_> = records
        .iter()
        .filter(|record| record.get("transition").is_some())
        .collect();
    assert_eq!(transitions[0].get("transition"), Some("start"));
    assert_eq!(
        transitions[0].get("active"),
        Some("true"),
        "start describes its committed state, not the subscriber's later cancellation"
    );
    assert_eq!(transitions[1].get("transition"), Some("cancel"));
    assert_eq!(transitions[1].get("active"), Some("false"));
    assert_eq!(transitions[1].get("outcome_generation"), Some("1"));
}

#[cfg(feature = "diagnostics")]
fn playback_and_motion_policy_changes_are_observable() {
    flui_testing::log_capture::disarm_interest_cache();
    let records = Arc::new(Mutex::new(Vec::new()));
    let observer = Observer {
        records: Arc::clone(&records),
        armed: Arc::new(AtomicBool::new(false)),
        borrow_failures: Arc::new(AtomicUsize::new(0)),
        optional_spans: Arc::new(AtomicUsize::new(0)),
        payload_retired: Arc::new(AtomicBool::new(false)),
        fault: None,
    };
    tracing::subscriber::with_default(tracing_subscriber::registry().with(observer), || {
        let configured = AnimationController::builder(Duration::from_millis(100)).build();
        configured.set_playback_rate(flui_animation::PlaybackRate::new(2.0).expect("initial rate"));
        assert!(
            records.lock().expect("receipts").iter().any(|record| {
                record.get("transition") == Some("playback")
                    && record.get("run_generation") == Some("0")
                    && record.get("active") == Some("false")
                    && record
                        .get("playback_rate")
                        .and_then(|value| value.parse::<f64>().ok())
                        == Some(2.0)
            }),
            "the first playback change before a run must be observable"
        );
        let registry = Vsync::new();
        let owner =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
        let controller = owner.controller();
        let run = controller
            .repeat(RepeatMode::Restart)
            .expect("infinite repeat");
        let mut clock = flui_animation::MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        controller.set_playback_rate(flui_animation::PlaybackRate::new(2.0).expect("rate"));
        registry.tick_all(&clock.frame(Duration::from_millis(20)));
        clock.set_system_motion(flui_platform::MotionPreference::Reduce);
        registry.tick_all(&clock.frame(Duration::from_millis(40)));
        assert!(!registry.has_running());
        assert!(!run.is_complete() && !run.is_canceled());
        clock.set_system_motion(flui_platform::MotionPreference::NoPreference);
        registry.tick_all(&clock.frame(Duration::from_millis(60)));
        assert!(registry.has_running());
        controller.stop().expect("cancel repeat");
        assert!(run.is_canceled());
    });
    let records = records.lock().expect("receipts");
    let kinds: Vec<_> = records
        .iter()
        .filter_map(|record| record.get("transition"))
        .collect();
    for expected in ["playback", "park", "resume", "cancel"] {
        assert!(kinds.contains(&expected), "missing {expected}: {kinds:?}");
    }
}

#[cfg(feature = "diagnostics")]
fn subscriber_retirement_after_failed_close() {
    struct ClosingSubscriber(Arc<AtomicBool>);
    impl tracing::Subscriber for ClosingSubscriber {
        fn register_callsite(
            &self,
            _: &'static tracing::Metadata<'static>,
        ) -> tracing::subscriber::Interest {
            tracing::subscriber::Interest::sometimes()
        }
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.name() == "animation_listeners"
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {}
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {
            panic!("observer exit failed");
        }
        fn try_close(&self, _: tracing::span::Id) -> bool {
            panic!("observer close failed");
        }
    }
    impl Drop for ClosingSubscriber {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
            panic!("observer subscriber dropped");
        }
    }

    flui_testing::log_capture::disarm_interest_cache();
    let dropped = Arc::new(AtomicBool::new(false));
    let dispatcher = tracing::Dispatch::new(ClosingSubscriber(Arc::clone(&dropped)));
    let guard = tracing::dispatcher::set_default(&dispatcher);
    drop(dispatcher);
    let guard = Rc::new(RefCell::new(Some(guard)));
    let controller = AnimationController::builder(Duration::from_millis(100)).build();
    controller.add_listener(Rc::new(move || {
        drop(guard.borrow_mut().take());
        panic!("real listener failure");
    }));
    let run = controller.forward().expect("real run");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        controller.tick_at(Duration::from_millis(100));
    }));
    let payload = outcome.expect_err("real listener failure must remain authoritative");
    assert_eq!(
        payload.downcast_ref::<&str>(),
        Some(&"real listener failure")
    );
    assert!(run.is_complete());
    assert!(
        !dropped.load(Ordering::Relaxed),
        "subscriber retirement must retain first-failure custody"
    );
}

#[cfg(feature = "diagnostics")]
#[test]
fn diagnostic_subscriber_retirement_contract() {
    crate::child_process::run_single(
        "diagnostics::diagnostic_subscriber_retirement_contract",
        subscriber_retirement_after_failed_close,
    );
}

#[cfg(feature = "diagnostics")]
fn source_failure_survives_failed_observation_and_keeps_peers_deliverable() {
    struct CurveOnce(std::cell::Cell<bool>);
    impl flui_animation::Curve for CurveOnce {
        fn transform(&self, t: f64) -> f64 {
            if self.0.replace(false) {
                std::panic::panic_any("real source failure");
            }
            t.clamp(0.0, 1.0)
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let armed = Arc::new(AtomicBool::new(false));
    let observer = Observer {
        records: Arc::new(Mutex::new(Vec::new())),
        armed: Arc::clone(&armed),
        borrow_failures: Arc::new(AtomicUsize::new(0)),
        optional_spans: Arc::new(AtomicUsize::new(0)),
        payload_retired: Arc::new(AtomicBool::new(false)),
        fault: Some(Fault {
            hook: Hook::Close,
            span: "animation_sampling",
            opaque: false,
        }),
    };
    tracing::subscriber::with_default(tracing_subscriber::registry().with(observer), || {
        let registry = Vsync::new();
        let owner =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
        let controller = owner.controller();
        let original = controller
            .animate_to_curved(1.0, None, CurveOnce(std::cell::Cell::new(true)))
            .expect("curved run");
        let peer =
            AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
        let peer_run = peer.controller().forward().expect("healthy peer");
        let mut clock = flui_animation::MotionClock::new();
        registry.tick_all(&clock.frame(Duration::ZERO));
        armed.store(true, Ordering::Relaxed);
        let sampled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            registry.tick_all(&clock.frame(Duration::from_millis(50)));
        }));
        let payload = sampled.expect_err("the source failure remains authoritative");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"real source failure"));
        assert!(
            !armed.load(Ordering::Relaxed),
            "the failing sampling close actually ran"
        );
        assert_eq!(
            controller.value(),
            0.0,
            "failed computation cannot publish a position"
        );
        assert!(
            (peer.controller().value() - 0.5).abs() < 1e-9,
            "healthy peers still sample this frame"
        );
        registry.tick_all(&clock.frame(Duration::from_millis(50)));
        assert!(
            (controller.value() - 0.5).abs() < 1e-9,
            "the interrupted position remains deliverable"
        );
        registry.tick_all(&clock.frame(Duration::from_millis(100)));
        assert!(original.is_complete() && peer_run.is_complete());
    });
}

#[test]
fn animation_observation_contract() {
    crate::run_table(&[
        (
            "run_outcomes_and_feature_boundary",
            transitions_follow_real_run_outcomes,
        ),
        ("filtered_diagnostics", filtered_diagnostics_do_not_emit),
        (
            "rate_command_during_sampling",
            a_rate_command_during_sampling_remains_pending,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "subscriber_failure_matrix",
            subscriber_failures_preserve_delivery_and_recovery,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "subscriber_commands",
            subscriber_commands_preserve_committed_receipts,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "motion_policy",
            playback_and_motion_policy_changes_are_observable,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "source_failure_and_observer_close",
            source_failure_survives_failed_observation_and_keeps_peers_deliverable,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "subscriber_cancels_before_curve_sampling",
            subscriber_cancellation_silences_the_displaced_curve,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "subscriber_cancels_before_simulation_sampling",
            subscriber_cancellation_silences_the_displaced_simulation,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "subscriber_disposes_before_value_delivery",
            subscriber_disposal_silences_value_delivery,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "subscriber_listener_admission",
            subscriber_listener_admission_waits_for_the_next_value,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "subscriber_registry_admission",
            subscriber_registry_admission_waits_for_the_next_tick,
        ),
        #[cfg(feature = "diagnostics")]
        (
            "controller_dispatcher_identity",
            controller_events_keep_their_owning_dispatcher,
        ),
    ]);
}

#[cfg(feature = "diagnostics")]
#[derive(Clone, Copy, Debug)]
enum Admission {
    Listener,
    Controller,
    Child,
}

#[cfg(feature = "diagnostics")]
struct AdmitFromSpan {
    admission: Admission,
    hook: Hook,
    armed: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

#[cfg(feature = "diagnostics")]
impl AdmitFromSpan {
    fn admit(&self, hook: Hook, name: &str) {
        let expected = match self.admission {
            Admission::Listener => "animation_listeners",
            Admission::Controller | Admission::Child => "animation_tick",
        };
        if hook != self.hook || name != expected || !self.armed.swap(false, Ordering::Relaxed) {
            return;
        }
        let calls = Arc::clone(&self.calls);
        let listener = Rc::new(move || {
            calls.fetch_add(1, Ordering::Relaxed);
        });
        match self.admission {
            Admission::Listener => {
                let controller = OBSERVED
                    .with(|slot| slot.borrow().clone())
                    .expect("observed controller");
                controller.add_listener(listener);
            }
            Admission::Controller | Admission::Child => {
                let registry = OBSERVED_VSYNC
                    .with(|slot| slot.borrow().clone())
                    .expect("observed registry");
                let destination = if matches!(self.admission, Admission::Child) {
                    let child = Vsync::new();
                    registry.attach_child(&child).expect("new child admission");
                    child
                } else {
                    registry
                };
                let owner = AnimationController::builder(Duration::from_millis(100))
                    .build_on(Some(&destination));
                owner.controller().add_listener(listener);
                let run = owner
                    .controller()
                    .forward()
                    .expect("subscriber-admitted run");
                ADMITTED.with(|slot| slot.borrow_mut().push((owner, run)));
            }
        }
    }
}

#[cfg(feature = "diagnostics")]
impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for AdmitFromSpan {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _: &tracing::span::Id,
        _: Context<'_, S>,
    ) {
        self.admit(Hook::New, attrs.metadata().name());
    }
    fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            self.admit(Hook::Enter, span.name());
        }
    }
}

#[cfg(feature = "diagnostics")]
fn subscriber_listener_admission_waits_for_the_next_value() {
    observer_admission_round(&[Admission::Listener]);
}

#[cfg(feature = "diagnostics")]
fn subscriber_registry_admission_waits_for_the_next_tick() {
    observer_admission_round(&[Admission::Controller, Admission::Child]);
}

#[cfg(feature = "diagnostics")]
fn observer_admission_round(admissions: &[Admission]) {
    let mut failures = Vec::new();
    for &admission in admissions {
        for hook in [Hook::New, Hook::Enter] {
            flui_testing::log_capture::disarm_interest_cache();
            let armed = Arc::new(AtomicBool::new(false));
            let calls = Arc::new(AtomicUsize::new(0));
            let observer = AdmitFromSpan {
                admission,
                hook,
                armed: Arc::clone(&armed),
                calls: Arc::clone(&calls),
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                tracing::subscriber::with_default(
                    tracing_subscriber::registry().with(observer),
                    || {
                        let registry = Vsync::new();
                        OBSERVED_VSYNC.with(|slot| *slot.borrow_mut() = Some(registry.clone()));
                        let owner = AnimationController::builder(Duration::from_millis(100))
                            .build_on(Some(&registry));
                        let controller = owner.controller();
                        OBSERVED.with(|slot| *slot.borrow_mut() = Some(controller.clone()));
                        let run = controller.forward().expect("resident run");
                        let mut clock = flui_animation::MotionClock::new();
                        armed.store(true, Ordering::Relaxed);
                        if matches!(admission, Admission::Listener) {
                            controller.tick_at(Duration::from_millis(50));
                            assert!((controller.value() - 0.5).abs() < 1e-9);
                        } else {
                            registry.tick_all(&clock.frame(Duration::ZERO));
                        }
                        assert!(!armed.load(Ordering::Relaxed), "subscriber admission ran");
                        assert_eq!(
                            calls.load(Ordering::Relaxed),
                            0,
                            "new membership belongs to the next call: {admission:?}/{hook:?}"
                        );
                        if matches!(admission, Admission::Listener) {
                            controller.tick_at(Duration::from_millis(100));
                        } else {
                            registry.tick_all(&clock.frame(Duration::from_millis(100)));
                            assert_eq!(
                                calls.load(Ordering::Relaxed),
                                1,
                                "next tick admits the new controller"
                            );
                            registry.tick_all(&clock.frame(Duration::from_millis(200)));
                            ADMITTED.with(|slot| {
                                for (owner, admitted_run) in slot.borrow().iter() {
                                    assert_eq!(owner.controller().value(), 1.0);
                                    assert!(admitted_run.is_complete());
                                }
                            });
                        }
                        assert!(run.is_complete(), "resident work remains deliverable");
                        assert!(
                            calls.load(Ordering::Relaxed) > 0,
                            "admitted work is delivered later"
                        );
                    },
                );
            }));
            OBSERVED.with(|slot| {
                slot.borrow_mut().take();
            });
            let owners = ADMITTED.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
            drop(owners);
            OBSERVED_VSYNC.with(|slot| {
                slot.borrow_mut().take();
            });
            if result.is_err() {
                failures.push((admission, hook));
            }
        }
    }
    assert!(failures.is_empty(), "admission hooks failed: {failures:?}");
}

#[cfg(feature = "diagnostics")]
fn controller_events_keep_their_owning_dispatcher() {
    type Receipt = (Fields, Option<&'static str>);
    struct Capture(Arc<Mutex<Vec<Receipt>>>);
    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
        fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
            let target = event.metadata().target();
            if target == "flui_animation::diagnostics"
                || target == REFUSAL_TARGET
                || target == flui_animation::NEVER_SETTLES_TARGET
            {
                let mut fields = Fields::default();
                event.record(&mut fields);
                fields.0.push(("target".to_owned(), target.to_owned()));
                let parent = event
                    .parent()
                    .and_then(|id| ctx.span(id))
                    .map(|span| span.name());
                self.0
                    .lock()
                    .expect("dispatcher capture")
                    .push((fields, parent));
            }
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let first = Arc::new(Mutex::new(Vec::new()));
    let second = Arc::new(Mutex::new(Vec::new()));
    let dispatcher_a =
        tracing::Dispatch::new(tracing_subscriber::registry().with(Capture(Arc::clone(&first))));
    let dispatcher_b =
        tracing::Dispatch::new(tracing_subscriber::registry().with(Capture(Arc::clone(&second))));
    tracing::dispatcher::with_default(&dispatcher_a, || {
        let _context = tracing::debug_span!("owning_context").entered();
        let controller = AnimationController::builder(Duration::from_millis(100)).build();
        let endless = AnimationController::builder(Duration::from_secs(1)).build();
        tracing::dispatcher::with_default(&dispatcher_b, || {
            let _unrelated = tracing::debug_span!("unrelated_controller").entered();
            let run = controller.forward().expect("run under another dispatcher");
            controller.tick_at(Duration::from_millis(100));
            assert!(run.is_complete());
            assert!(controller.animate_to(f64::NAN, None).is_err());
            let _endless = endless.animate_with(Endless).expect("finite start");
            for seconds in [0, 61] {
                endless.tick_at(Duration::from_secs(seconds));
            }
        });
    });
    assert!(
        second.lock().expect("foreign dispatcher").is_empty(),
        "foreign subscriber must not receive another subscriber's identity"
    );
    let first = first.lock().expect("owning dispatcher");
    for transition in ["start", "complete"] {
        let (fields, parent) = first
            .iter()
            .find(|(fields, _)| fields.get("transition") == Some(transition))
            .expect("owning dispatcher receives the transition");
        assert_eq!(*parent, Some("animation_controller"));
        assert!(fields.get("controller").is_some());
    }
    for target in [REFUSAL_TARGET, flui_animation::NEVER_SETTLES_TARGET] {
        let (_, parent) = first
            .iter()
            .find(|(fields, _)| fields.get("target") == Some(target))
            .expect("owning dispatcher receives the warning");
        assert_eq!(*parent, Some("animation_controller"), "{target}");
    }
}

#[cfg(feature = "diagnostics")]
fn subscriber_disposal_silences_value_delivery() {
    struct DisposeListeners {
        armed: Arc<AtomicBool>,
        hook: Hook,
    }
    impl DisposeListeners {
        fn dispose(&self, hook: Hook, name: &str) {
            if self.hook == hook
                && name == "animation_listeners"
                && self.armed.swap(false, Ordering::Relaxed)
            {
                let mut owner = OBSERVED_OWNER
                    .with(|slot| slot.borrow_mut().take())
                    .expect("owning controller");
                owner.dispose();
                OBSERVED_OWNER.with(|slot| *slot.borrow_mut() = Some(owner));
            }
        }
    }
    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for DisposeListeners {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _: &tracing::span::Id,
            _: Context<'_, S>,
        ) {
            self.dispose(Hook::New, attrs.metadata().name());
        }
        fn on_enter(&self, id: &tracing::span::Id, ctx: Context<'_, S>) {
            if let Some(span) = ctx.span(id) {
                self.dispose(Hook::Enter, span.name());
            }
        }
    }
    let mut failures = Vec::new();
    for hook in [Hook::New, Hook::Enter] {
        flui_testing::log_capture::disarm_interest_cache();
        let armed = Arc::new(AtomicBool::new(false));
        let observer = DisposeListeners {
            armed: Arc::clone(&armed),
            hook,
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracing::subscriber::with_default(
                tracing_subscriber::registry().with(observer),
                || {
                    let registry = Vsync::new();
                    let owner = AnimationController::builder(Duration::from_millis(100))
                        .build_on(Some(&registry));
                    let controller = owner.controller().clone();
                    OBSERVED_OWNER.with(|slot| *slot.borrow_mut() = Some(owner));
                    let notifications = Rc::new(std::cell::Cell::new(0));
                    let delivered = Rc::clone(&notifications);
                    controller.add_listener(Rc::new(move || delivered.set(delivered.get() + 1)));
                    let original = controller.forward().expect("owned run");
                    let peer = AnimationController::builder(Duration::from_millis(100))
                        .build_on(Some(&registry));
                    let peer_run = peer.controller().forward().expect("peer run");
                    let mut clock = flui_animation::MotionClock::new();
                    registry.tick_all(&clock.frame(Duration::ZERO));
                    let before_disposal = notifications.get();
                    armed.store(true, Ordering::Relaxed);
                    registry.tick_all(&clock.frame(Duration::from_millis(50)));
                    assert!(!armed.load(Ordering::Relaxed), "{hook:?} disposal ran");
                    assert!(original.is_canceled());
                    assert_eq!(
                        notifications.get(),
                        before_disposal,
                        "disposed listeners stay silent"
                    );
                    assert_eq!(
                        controller.forward().expect_err("retired controller"),
                        flui_animation::AnimationError::Disposed
                    );
                    assert!((peer.controller().value() - 0.5).abs() < 1e-9);
                    registry.tick_all(&clock.frame(Duration::from_millis(100)));
                    assert!(peer_run.is_complete());
                    let fresh = AnimationController::builder(Duration::from_millis(100))
                        .build_on(Some(&registry));
                    let next = fresh.controller().forward().expect("fresh owner");
                    registry.tick_all(&clock.frame(Duration::from_millis(100)));
                    registry.tick_all(&clock.frame(Duration::from_millis(200)));
                    assert!(next.is_complete());
                },
            );
        }));
        OBSERVED_OWNER.with(|slot| drop(slot.borrow_mut().take()));
        if result.is_err() {
            failures.push(hook);
        }
    }
    assert!(failures.is_empty(), "disposal hooks failed: {failures:?}");
}

fn a_rate_command_during_sampling_remains_pending() {
    struct RateOnce(std::cell::Cell<bool>);
    impl flui_animation::Curve for RateOnce {
        fn transform(&self, t: f64) -> f64 {
            if self.0.replace(false) {
                let controller = OBSERVED.with(|observed| observed.borrow().clone());
                controller
                    .expect("sampling controller")
                    .set_playback_rate(flui_animation::PlaybackRate::new(3.0).expect("newer rate"));
            }
            t.clamp(0.0, 1.0)
        }
    }
    let controller = AnimationController::builder(Duration::from_secs(1)).build();
    OBSERVED.with(|observed| *observed.borrow_mut() = Some(controller.clone()));
    let original = controller
        .animate_to_curved(1.0, None, RateOnce(std::cell::Cell::new(true)))
        .expect("curved run");
    controller.set_playback_rate(flui_animation::PlaybackRate::new(2.0).expect("captured rate"));
    controller.tick_at(Duration::from_millis(100));
    assert!((controller.value() - 0.1).abs() < 1e-9);
    controller.tick_at(Duration::from_millis(200));
    let applied = controller.playback_rate();
    OBSERVED.with(|observed| *observed.borrow_mut() = None);
    assert_eq!(
        applied,
        flui_animation::PlaybackRate::new(3.0).expect("newer rate"),
        "a captured rate must not erase a later command from the curve"
    );
    controller.tick_at(Duration::from_millis(300));
    assert!((controller.value() - 0.6).abs() < 1e-9);
    controller.tick_at(Duration::from_secs(1));
    assert!(original.is_complete());
}

#[cfg(feature = "diagnostics")]
fn subscriber_cancellation_silences_the_displaced_curve() {
    subscriber_cancellation_silences_the_displaced_source(false);
}

#[cfg(feature = "diagnostics")]
fn subscriber_cancellation_silences_the_displaced_simulation() {
    subscriber_cancellation_silences_the_displaced_source(true);
}

#[cfg(feature = "diagnostics")]
fn subscriber_cancellation_silences_the_displaced_source(use_simulation: bool) {
    struct CancelSampling(Arc<AtomicBool>);
    impl<S: Subscriber> Layer<S> for CancelSampling {
        fn on_new_span(
            &self,
            attrs: &tracing::span::Attributes<'_>,
            _: &tracing::span::Id,
            _: Context<'_, S>,
        ) {
            if attrs.metadata().name() == "animation_sampling"
                && self.0.swap(false, Ordering::Relaxed)
            {
                let controller = OBSERVED.with(|observed| observed.borrow().clone());
                controller
                    .expect("observed controller")
                    .stop()
                    .expect("cancel before sampling");
            }
        }
    }
    struct DisplacedCurve(Arc<AtomicUsize>);
    impl flui_animation::Curve for DisplacedCurve {
        fn transform(&self, _: f64) -> f64 {
            self.0.fetch_add(1, Ordering::Relaxed);
            std::panic::panic_any("displaced curve sampled");
        }
    }
    struct DisplacedSimulation(Arc<AtomicUsize>);
    impl flui_animation::Simulation for DisplacedSimulation {
        fn x(&self, time: f64) -> f64 {
            if time == 0.0 {
                return 0.0;
            }
            self.0.fetch_add(1, Ordering::Relaxed);
            std::panic::panic_any("displaced simulation sampled");
        }
        fn dx(&self, _: f64) -> f64 {
            0.0
        }
        fn is_done(&self, _: f64) -> bool {
            false
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let armed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(
        tracing_subscriber::registry().with(CancelSampling(Arc::clone(&armed))),
        || {
            let registry = Vsync::new();
            let owner =
                AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
            let controller = owner.controller();
            OBSERVED.with(|observed| *observed.borrow_mut() = Some(controller.clone()));
            let original = if use_simulation {
                controller.animate_with(DisplacedSimulation(Arc::clone(&calls)))
            } else {
                controller.animate_to_curved(1.0, None, DisplacedCurve(Arc::clone(&calls)))
            }
            .expect("source run");
            let peer =
                AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
            let peer_run = peer.controller().forward().expect("peer run");
            let mut clock = flui_animation::MotionClock::new();
            registry.tick_all(&clock.frame(Duration::ZERO));
            armed.store(true, Ordering::Relaxed);
            let sampled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                registry.tick_all(&clock.frame(Duration::from_millis(50)));
            }));
            OBSERVED.with(|observed| *observed.borrow_mut() = None);
            assert!(
                sampled.is_ok(),
                "a subscriber-canceled source must not be invoked"
            );
            assert!(!armed.load(Ordering::Relaxed));
            assert_eq!(calls.load(Ordering::Relaxed), 0);
            assert!(original.is_canceled());
            assert_eq!(controller.value(), 0.0);
            assert!((peer.controller().value() - 0.5).abs() < 1e-9);
            let next = controller
                .forward()
                .expect("fresh run without the displaced curve");
            registry.tick_all(&clock.frame(Duration::from_millis(50)));
            registry.tick_all(&clock.frame(Duration::from_millis(150)));
            assert!(next.is_complete() && peer_run.is_complete());
        },
    );
}

/// The events a command emits on [`REFUSAL_TARGET`]: level, operation, kind.
type Refusal = (tracing::Level, String, String);

struct RefusalCapture(Arc<Mutex<Vec<Refusal>>>);

impl<S: Subscriber> Layer<S> for RefusalCapture {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != REFUSAL_TARGET {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let field = |name| fields.get(name).unwrap_or_default().to_owned();
        self.0.lock().expect("refusal records").push((
            *event.metadata().level(),
            field("operation"),
            field("kind"),
        ));
    }
}

fn refusals(command: impl FnOnce()) -> Vec<Refusal> {
    flui_testing::log_capture::disarm_interest_cache();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(RefusalCapture(Arc::clone(&captured)));
    tracing::subscriber::with_default(subscriber, command);
    captured.lock().expect("refusal records").clone()
}

/// `command` is refused, and says so exactly once at `warn`.
fn refuses(operation: &str, kind: &str, command: impl FnOnce()) {
    assert_eq!(
        refusals(command),
        [(tracing::Level::WARN, operation.to_owned(), kind.to_owned())]
    );
}

fn unbounded() -> flui_animation::DrivenController {
    AnimationController::builder(Duration::from_secs(1))
        .unbounded()
        .build_on(None)
}

fn a_disposed_controller_refuses_a_run() {
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let controller = owner.controller().clone();
    owner.dispose();
    refuses("AnimationController::forward", "disposed", || {
        assert!(controller.forward().is_err());
    });
}

fn a_mutation_during_build_is_refused() {
    let vsync = Vsync::new();
    let phase = flui_foundation::BuildPhase::new();
    vsync.set_build_phase(Some(phase.clone()));
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync));
    let controller = owner.controller().clone();
    let _build = phase.enter();
    refuses(
        "AnimationController::forward",
        "mutated_during_build",
        || {
            assert!(controller.forward().is_err());
        },
    );
}

fn a_disposed_controller_tears_down_quietly() {
    let mut owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let controller = owner.controller().clone();
    owner.dispose();
    let refused = refusals(|| {
        assert!(controller.stop().is_err());
        assert!(controller.reset().is_err());
    });
    let teardown = |operation: &str| {
        (
            tracing::Level::DEBUG,
            operation.to_owned(),
            "disposed".to_owned(),
        )
    };
    assert_eq!(
        refused,
        [
            teardown("AnimationController::stop"),
            teardown("AnimationController::reset"),
        ]
    );
}

fn an_infinite_repeat_has_no_endpoint_to_finish() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let _run = owner.controller().repeat(RepeatMode::Restart);
    refuses("AnimationController::finish", "no_finite_endpoint", || {
        assert!(owner.controller().finish().is_err());
    });
}

fn a_repeat_without_a_finite_range_is_refused() {
    let owner = unbounded();
    refuses(
        "AnimationController::repeat",
        "invalid_repeat_range",
        || {
            assert!(owner.controller().repeat(RepeatMode::Alternate).is_err());
        },
    );
}

fn a_zero_gesture_extent_is_refused() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    refuses(
        "AnimationController::fling_across",
        "invalid_extent",
        || {
            assert!(owner.controller().fling_across(1.0, 0.0).is_err());
        },
    );
}

fn an_overflowing_gesture_velocity_is_refused() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    refuses(
        "AnimationController::fling_across",
        "velocity_overflow",
        || {
            let controller = owner.controller();
            assert!(
                controller
                    .fling_across(f64::MAX, f64::MIN_POSITIVE)
                    .is_err()
            );
        },
    );
}

fn an_oscillating_fling_spring_is_refused() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    let spring = SpringDescription::with_damping_ratio(1.0, 100.0, 0.5).expect("spring");
    refuses(
        "AnimationController::fling_with",
        "underdamped_fling_spring",
        || assert!(owner.controller().fling_with(1.0, Some(spring)).is_err()),
    );
}

fn an_unrepresentable_fling_is_refused() {
    let bounds = flui_animation::ValueRange::new(1e308, 1.1e308).expect("finite range");
    let owner = AnimationController::builder(Duration::from_secs(1))
        .bounds(bounds)
        .initial_value(1.1e308)
        .build_on(None);
    let _run = owner.controller().reverse().expect("reverse run");
    owner.controller().tick_at(Duration::ZERO);
    owner.controller().tick_at(Duration::from_millis(100));
    let spring = SpringDescription::with_damping_ratio(1.0, 1e308, 1.0).expect("spring");
    refuses("AnimationController::fling_with", "simulation", || {
        assert!(owner.controller().fling_with(1.0, Some(spring)).is_err());
    });
}

fn a_nan_target_is_refused() {
    let owner = AnimationController::builder(Duration::from_secs(1)).build_on(None);
    refuses(
        "AnimationController::animate_to",
        "non_finite_input",
        || {
            assert!(owner.controller().animate_to(f64::NAN, None).is_err());
        },
    );
}

fn a_run_toward_an_infinite_bound_is_refused() {
    let owner = unbounded();
    refuses("AnimationController::reverse", "unbounded_target", || {
        assert!(owner.controller().reverse().is_err());
    });
}

fn an_overflowing_span_is_refused() {
    let owner = unbounded();
    owner
        .controller()
        .set_value(-f64::MAX)
        .expect("no build is running");
    refuses("AnimationController::animate_to", "span_overflow", || {
        assert!(owner.controller().animate_to(f64::MAX, None).is_err());
    });
}

/// A converted value whose vector conversion runs a hook.
#[derive(Clone)]
struct Hooked {
    position: f64,
    hook: Rc<dyn Fn()>,
}

impl TwoWayConverter for Hooked {
    type Vector = [f64; 1];
    fn rest_thresholds() -> Self::Vector {
        [0.001]
    }
    fn to_vector(&self) -> Self::Vector {
        (self.hook)();
        [self.position]
    }
    fn from_vector([position]: Self::Vector) -> Self {
        Self {
            position,
            hook: Rc::new(|| {}),
        }
    }
}

/// A value whose spring motion has no valid rest distance.
#[derive(Clone)]
struct NegativeThreshold(f64);

impl TwoWayConverter for NegativeThreshold {
    type Vector = [f64; 1];
    fn rest_thresholds() -> Self::Vector {
        [-1.0]
    }
    fn to_vector(&self) -> Self::Vector {
        [self.0]
    }
    fn from_vector([value]: Self::Vector) -> Self {
        Self(value)
    }
}

fn linear() -> MotionSpec {
    MotionSpec::Curve {
        duration: Duration::from_secs(1),
        curve: flui_animation::SharedCurve::new(flui_animation::curves::LINEAR),
    }
}

fn a_non_finite_initial_value_is_refused() {
    refuses("AnimatedValue::new", "non_finite_component", || {
        assert!(AnimatedValue::new(f64::NAN, linear(), None).is_err());
    });
}

fn an_invalid_rest_distance_is_refused() {
    let mut value = AnimatedValue::new(NegativeThreshold(0.0), linear(), None).expect("value");
    let spring =
        MotionSpec::Spring(SpringDescription::with_damping_ratio(1.0, 100.0, 1.0).expect("spring"));
    refuses("AnimatedValue::retarget", "component_simulation", || {
        assert!(value.retarget(NegativeThreshold(1.0), spring).is_err());
    });
}

/// A later member's conversion ticks the frame and moves an earlier
/// member's published seam.
fn a_group_invalidated_while_preparing_is_refused() {
    let registry = Vsync::new();
    let mut clock = flui_animation::MotionClock::new();
    let mut left = AnimatedValue::new(0.0, linear(), Some(&registry)).expect("left");
    let _left = left.animate_to(1.0).expect("left run");
    registry.tick_all(&clock.frame(Duration::ZERO));
    let tick = clock.frame(Duration::from_millis(250));
    let mut moved = AnimatedValue::new(
        Hooked {
            position: 0.0,
            hook: Rc::new(|| {}),
        },
        linear(),
        Some(&registry),
    )
    .expect("moved");
    let target = Hooked {
        position: 4.0,
        hook: Rc::new(move || registry.tick_all(&tick)),
    };
    refuses("MotionUpdate::run", "reentrant_motion", || {
        let result = MotionUpdate::run(|update| {
            update.retarget(&mut left, 3.0, linear())?;
            update.retarget(&mut moved, target, linear())
        });
        assert!(result.is_err());
    });
}

fn a_group_whose_preparation_panicked_is_refused() {
    let mut left = AnimatedValue::new(0.0, linear(), None).expect("left");
    let mut right = AnimatedValue::new(
        Hooked {
            position: 0.0,
            hook: Rc::new(|| {}),
        },
        linear(),
        None,
    )
    .expect("right");
    let target = Hooked {
        position: 1.0,
        hook: Rc::new(|| panic!("preparation failure")),
    };
    refuses("MotionUpdate::run", "admission_aborted", || {
        let result = MotionUpdate::run(|update| {
            update.retarget(&mut left, 1.0, linear())?;
            let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                update.retarget(&mut right, target, linear())
            }));
            assert!(prepared.is_err());
            Ok(())
        });
        assert!(result.is_err());
    });
}

/// One row per [`AnimationError::kind`] a public command can produce:
/// each refusal reports once, on [`REFUSAL_TARGET`], naming its operation.
/// `identity_exhausted` needs 2^64 runs and is not reachable here.
/// A subscriber that panics on a refusal cannot replace the command's typed
/// error, whether the event goes to the current dispatcher or, with
/// `diagnostics`, to the dispatcher that owns the controller's span.
fn a_panicking_refusal_subscriber_keeps_the_typed_error() {
    struct PanicOnRefusal;
    impl<S: Subscriber> Layer<S> for PanicOnRefusal {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            assert!(
                event.metadata().target() != REFUSAL_TARGET,
                "subscriber panic"
            );
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let owning = tracing::Dispatch::new(tracing_subscriber::registry().with(PanicOnRefusal));
    tracing::dispatcher::with_default(&owning, || {
        let controller = AnimationController::builder(Duration::from_secs(1)).build();
        let refuse = || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                controller.animate_to(f64::NAN, None).is_err()
            }))
        };
        assert!(
            matches!(refuse(), Ok(true)),
            "the current dispatcher's panic is contained"
        );
        let foreign = tracing::Dispatch::new(tracing_subscriber::registry());
        let from_foreign = tracing::dispatcher::with_default(&foreign, refuse);
        assert!(
            matches!(from_foreign, Ok(true)),
            "the owning dispatcher's panic is contained"
        );
        let subjectless =
            std::panic::catch_unwind(|| AnimatedValue::new(f64::NAN, linear(), None).is_err());
        assert!(
            matches!(subjectless, Ok(true)),
            "a refusal without a controller is contained"
        );
    });
    #[cfg(feature = "diagnostics")]
    a_panicking_span_clone_keeps_the_typed_error();
}

/// Naming the controller clones its span, which calls the subscriber: a
/// subscriber that panics there is contained like one that panics on the
/// event itself.
#[cfg(feature = "diagnostics")]
fn a_panicking_span_clone_keeps_the_typed_error() {
    use tracing::span::{Attributes, Id, Record};
    struct PanicOnClone;
    impl Subscriber for PanicOnClone {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, _: &Event<'_>) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
        fn clone_span(&self, _: &Id) -> Id {
            panic!("subscriber clone_span panic")
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    tracing::subscriber::with_default(PanicOnClone, || {
        let controller = AnimationController::builder(Duration::from_secs(1)).build();
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            controller.animate_to(f64::NAN, None).is_err()
        }));
        assert!(
            matches!(refused, Ok(true)),
            "a panic while naming the controller is contained"
        );
    });
}

#[test]
fn refusal_diagnostic_contract() {
    crate::run_table(&[
        (
            "a panicking subscriber keeps the typed error",
            a_panicking_refusal_subscriber_keeps_the_typed_error,
        ),
        ("disposed", a_disposed_controller_refuses_a_run),
        ("mutated_during_build", a_mutation_during_build_is_refused),
        (
            "disposed teardown",
            a_disposed_controller_tears_down_quietly,
        ),
        (
            "no_finite_endpoint",
            an_infinite_repeat_has_no_endpoint_to_finish,
        ),
        (
            "invalid_repeat_range",
            a_repeat_without_a_finite_range_is_refused,
        ),
        ("invalid_extent", a_zero_gesture_extent_is_refused),
        (
            "velocity_overflow",
            an_overflowing_gesture_velocity_is_refused,
        ),
        (
            "underdamped_fling_spring",
            an_oscillating_fling_spring_is_refused,
        ),
        ("simulation", an_unrepresentable_fling_is_refused),
        ("non_finite_input", a_nan_target_is_refused),
        (
            "unbounded_target",
            a_run_toward_an_infinite_bound_is_refused,
        ),
        ("span_overflow", an_overflowing_span_is_refused),
        (
            "non_finite_component",
            a_non_finite_initial_value_is_refused,
        ),
        ("component_simulation", an_invalid_rest_distance_is_refused),
        (
            "reentrant_motion",
            a_group_invalidated_while_preparing_is_refused,
        ),
        (
            "admission_aborted",
            a_group_whose_preparation_panicked_is_refused,
        ),
    ]);
}

/// Controller spans and the refusals parented to them.
#[cfg(feature = "diagnostics")]
#[derive(Default)]
struct Origins {
    /// `(span id, label, created_at)` of each `animation_controller` span.
    spans: Vec<(tracing::Id, String, String)>,
    /// `(parent span id, label, created_at)` of each refusal.
    refusals: Vec<(Option<tracing::Id>, String, String)>,
}

#[cfg(feature = "diagnostics")]
struct OriginCapture(Arc<Mutex<Origins>>);

#[cfg(feature = "diagnostics")]
impl<S: Subscriber> Layer<S> for OriginCapture {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        _: Context<'_, S>,
    ) {
        if attrs.metadata().name() != "animation_controller" {
            return;
        }
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        let field = |name| fields.get(name).unwrap_or_default().to_owned();
        self.0.lock().expect("origins").spans.push((
            id.clone(),
            field("label"),
            field("created_at"),
        ));
    }
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != REFUSAL_TARGET {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let field = |name| fields.get(name).unwrap_or_default().to_owned();
        self.0.lock().expect("origins").refusals.push((
            event.parent().cloned(),
            field("label"),
            field("created_at"),
        ));
    }
}

/// A labeled controller, and the site `build` records for it.
#[cfg(feature = "diagnostics")]
#[track_caller]
fn twin() -> (
    flui_animation::DrivenController,
    &'static std::panic::Location<'static>,
) {
    let owner = AnimationController::builder(Duration::from_secs(1))
        .debug_label("twin")
        .build_on(None);
    (owner, std::panic::Location::caller())
}

/// Two controllers sharing a label stay distinct: each span records its own
/// creation site, and a refusal names its controller and is parented to that
/// controller's span.
#[cfg(feature = "diagnostics")]
#[test]
fn controllers_carry_their_label_and_creation_site() {
    flui_testing::log_capture::disarm_interest_cache();
    let captured = Arc::new(Mutex::new(Origins::default()));
    let subscriber = tracing_subscriber::registry().with(OriginCapture(Arc::clone(&captured)));
    tracing::subscriber::with_default(subscriber, || {
        let (mut first, first_site) = twin();
        let (second, second_site) = twin();
        assert_ne!(first_site.line(), second_site.line());
        first.dispose();
        assert!(first.controller().forward().is_err());
        let origins = captured.lock().expect("origins");
        let sites = [first_site.to_string(), second_site.to_string()];
        assert_eq!(origins.spans.len(), 2);
        assert_ne!(
            origins.spans[0].0, origins.spans[1].0,
            "identity is the span"
        );
        for ((_, label, created_at), site) in origins.spans.iter().zip(&sites) {
            assert_eq!((label.as_str(), created_at), ("twin", site));
        }
        assert_eq!(
            origins.refusals,
            [(
                Some(origins.spans[0].0.clone()),
                "twin".to_owned(),
                sites[0].clone()
            )]
        );
        drop(second);
    });
}

/// The `settle_reason` of every `complete` receipt `scenario` produces.
#[cfg(feature = "diagnostics")]
fn settle_reasons(scenario: impl FnOnce()) -> Vec<String> {
    flui_testing::log_capture::disarm_interest_cache();
    let records = Arc::new(Mutex::new(Vec::new()));
    let observer = Observer {
        records: Arc::clone(&records),
        armed: Arc::new(AtomicBool::new(false)),
        borrow_failures: Arc::new(AtomicUsize::new(0)),
        optional_spans: Arc::new(AtomicUsize::new(0)),
        payload_retired: Arc::new(AtomicBool::new(false)),
        fault: None,
    };
    tracing::subscriber::with_default(tracing_subscriber::registry().with(observer), scenario);
    let records = records.lock().expect("receipts");
    records
        .iter()
        .filter(|record| record.get("transition") == Some("complete"))
        .map(|record| record.get("settle_reason").unwrap_or_default().to_owned())
        .collect()
}

/// A forward run on a registry, after its first frame.
#[cfg(feature = "diagnostics")]
fn bound_run() -> (
    Vsync,
    flui_animation::MotionClock,
    flui_animation::DrivenController,
) {
    let registry = Vsync::new();
    let mut clock = flui_animation::MotionClock::new();
    let owner = AnimationController::builder(Duration::from_millis(100)).build_on(Some(&registry));
    let _run = owner.controller().forward().expect("forward run");
    registry.tick_all(&clock.frame(Duration::ZERO));
    (registry, clock, owner)
}

#[cfg(feature = "diagnostics")]
fn a_run_ticked_to_its_end_settles_on_its_clock() {
    let reasons = settle_reasons(|| {
        let (registry, mut clock, _owner) = bound_run();
        registry.tick_all(&clock.frame(Duration::from_millis(100)));
    });
    assert_eq!(reasons, ["clock"]);
}

#[cfg(feature = "diagnostics")]
fn a_run_settled_by_reduced_motion_says_so() {
    let reasons = settle_reasons(|| {
        let (registry, mut clock, _owner) = bound_run();
        clock.set_system_motion(flui_platform::MotionPreference::Reduce);
        registry.tick_all(&clock.frame(Duration::from_millis(20)));
    });
    assert_eq!(reasons, ["reduced_motion"]);
}

#[cfg(feature = "diagnostics")]
fn a_run_settled_by_an_exhausted_clock_says_so() {
    let reasons = settle_reasons(|| {
        let (registry, mut clock, _owner) = bound_run();
        registry.tick_all(&clock.frame(Duration::MAX));
    });
    assert_eq!(reasons, ["exhausted_clock"]);
}

#[cfg(feature = "diagnostics")]
fn a_finished_run_was_stopped() {
    let reasons = settle_reasons(|| {
        let (_registry, _clock, owner) = bound_run();
        owner.controller().finish().expect("finite run");
    });
    assert_eq!(reasons, ["stopped"]);
}

/// A completion receipt says why its run settled.
#[cfg(feature = "diagnostics")]
#[test]
fn completion_receipts_state_why_the_run_settled() {
    crate::run_table(&[
        ("clock", a_run_ticked_to_its_end_settles_on_its_clock),
        ("reduced_motion", a_run_settled_by_reduced_motion_says_so),
        (
            "exhausted_clock",
            a_run_settled_by_an_exhausted_clock_says_so,
        ),
        ("stopped", a_finished_run_was_stopped),
    ]);
}

/// `(phase, payload, label)` of each contained-panic report; the layer
/// itself panics on the first report when `fails`.
struct ContainedPanics {
    reports: Arc<Mutex<Vec<(String, String, String)>>>,
    fails: bool,
}

impl<S: Subscriber> Layer<S> for ContainedPanics {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() != flui_animation::CONTAINED_PANIC_TARGET {
            return;
        }
        let mut fields = Fields::default();
        event.record(&mut fields);
        let field = |name| fields.get(name).unwrap_or_default().to_owned();
        self.reports.lock().expect("reports").push((
            field("phase"),
            field("payload"),
            field("label"),
        ));
        assert!(!self.fails, "contained-panic subscriber failure");
    }
}

/// Two status listeners each panic once while a labeled controller starts
/// a run. Returns the payload that escaped `forward` and the reports.
fn competing_status_panics(fails: bool) -> (String, Vec<(String, String, String)>) {
    flui_testing::log_capture::disarm_interest_cache();
    let reports = Arc::new(Mutex::new(Vec::new()));
    let layer = ContainedPanics {
        reports: Arc::clone(&reports),
        fails,
    };
    let escaped =
        tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
            let registry = Vsync::new();
            let owner = AnimationController::builder(Duration::from_millis(100))
                .debug_label("pair")
                .build_on(Some(&registry));
            let controller = owner.controller();
            let once = |message: &'static str| {
                let armed = std::cell::Cell::new(true);
                Rc::new(move |_| assert!(!armed.replace(false), "{message}"))
                    as flui_animation::StatusCallback
            };
            let _first = controller.subscribe_status(once("first"));
            let _second = controller.subscribe_status(once("second"));
            let payload =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| controller.forward()))
                    .expect_err("the first listener's failure escapes");
            // The next frame still runs the admitted run to its end.
            let mut clock = flui_animation::MotionClock::new();
            registry.tick_all(&clock.frame(Duration::ZERO));
            registry.tick_all(&clock.frame(Duration::from_millis(100)));
            assert_eq!(controller.value(), 1.0);
            flui_foundation::panic::payload_text(payload.as_ref())
                .unwrap_or_default()
                .to_owned()
        });
    let reports = reports.lock().expect("reports").clone();
    (escaped, reports)
}

/// The first panic is resumed; a second one caught while recovering is
/// reported with its phase, payload and controller label, not dropped, and a
/// failing report cannot replace the first failure.
#[test]
fn competing_panics_are_reported_and_the_first_is_resumed() {
    let report = || {
        [(
            "controller delivery".to_owned(),
            "second".to_owned(),
            "pair".to_owned(),
        )]
    };
    let (escaped, reports) = competing_status_panics(false);
    assert_eq!(escaped, "first");
    assert_eq!(reports, report());
    let (escaped, reports) = competing_status_panics(true);
    assert_eq!(escaped, "first", "a failing report keeps the first failure");
    assert_eq!(reports, report());
}

/// The labels of every never-settles report `scenario` produces.
#[cfg(any(debug_assertions, feature = "diagnostics"))]
fn never_settles_reports(scenario: impl FnOnce()) -> Vec<String> {
    struct Reports(Arc<Mutex<Vec<String>>>);
    impl<S: Subscriber> Layer<S> for Reports {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            if event.metadata().target() == flui_animation::NEVER_SETTLES_TARGET {
                let mut fields = Fields::default();
                event.record(&mut fields);
                self.0
                    .lock()
                    .expect("reports")
                    .push(fields.get("label").unwrap_or_default().to_owned());
            }
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let reports = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Reports(Arc::clone(&reports)));
    tracing::subscriber::with_default(subscriber, scenario);
    reports.lock().expect("reports").clone()
}

/// A simulation whose `is_done` never holds.
#[cfg(any(debug_assertions, feature = "diagnostics"))]
struct Endless;

#[cfg(any(debug_assertions, feature = "diagnostics"))]
impl flui_animation::simulation::Simulation for Endless {
    fn x(&self, time: f64) -> f64 {
        (time * 0.01).sin() * 0.5 + 0.5
    }
    fn dx(&self, time: f64) -> f64 {
        (time * 0.01).cos() * 0.005
    }
    fn is_done(&self, _: f64) -> bool {
        false
    }
}

/// A finite run that never settles is reported once, after a minute of its
/// own time, with its controller's label; a run that settles is not. A
/// subscriber that panics on the report cannot fail the frame. The watchdog
/// exists in debug and `diagnostics` builds only.
#[cfg(any(debug_assertions, feature = "diagnostics"))]
#[test]
fn a_simulation_that_never_settles_is_reported_once() {
    let reports = never_settles_reports(|| {
        let controller = AnimationController::builder(Duration::from_secs(1))
            .debug_label("endless")
            .build();
        let _run = controller.animate_with(Endless).expect("finite start");
        for seconds in [0, 30, 59, 61, 120, 600] {
            controller.tick_at(Duration::from_secs(seconds));
        }
    });
    assert_eq!(reports, ["endless"]);
    let settled = never_settles_reports(|| {
        let controller = AnimationController::builder(Duration::from_secs(1)).build();
        let _run = controller.fling(1.0).expect("fling");
        for seconds in [0, 1, 61, 120] {
            controller.tick_at(Duration::from_secs(seconds));
        }
    });
    assert_eq!(settled, [""; 0]);

    struct PanicOnReport;
    impl<S: Subscriber> Layer<S> for PanicOnReport {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            assert!(
                event.metadata().target() != flui_animation::NEVER_SETTLES_TARGET,
                "subscriber panic"
            );
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    let subscriber = tracing_subscriber::registry().with(PanicOnReport);
    tracing::subscriber::with_default(subscriber, || {
        let controller = AnimationController::builder(Duration::from_secs(1)).build();
        let run = controller.animate_with(Endless).expect("finite start");
        for seconds in [0, 61, 62] {
            let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                controller.tick_at(Duration::from_secs(seconds));
            }));
            assert!(frame.is_ok(), "a diagnostic subscriber failed the frame");
        }
        assert!(controller.is_animating() && !run.is_complete());
    });
}

/// A subscriber that panics on a refusal and again while the reporting span
/// clone closes: the event's panic is contained before the clone closes, so
/// the second panic cannot abort the process mid-unwind.
#[cfg(feature = "diagnostics")]
fn a_refusal_and_a_failing_span_close_keep_the_typed_error() {
    use tracing::span::{Attributes, Id, Record};
    struct PanicOnRefusalAndClose;
    impl Subscriber for PanicOnRefusalAndClose {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            assert!(
                event.metadata().target() != REFUSAL_TARGET,
                "subscriber event panic"
            );
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
        fn try_close(&self, _: Id) -> bool {
            panic!("subscriber close panic")
        }
    }
    flui_testing::log_capture::disarm_interest_cache();
    tracing::subscriber::with_default(PanicOnRefusalAndClose, || {
        let controller = AnimationController::builder(Duration::from_secs(1)).build();
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            controller.animate_to(f64::NAN, None).is_err()
        }));
        assert!(
            matches!(refused, Ok(true)),
            "both subscriber panics are contained"
        );
    });
}

#[cfg(feature = "diagnostics")]
#[test]
fn a_refusal_and_a_failing_span_close_contract() {
    crate::child_process::run_single(
        "diagnostics::a_refusal_and_a_failing_span_close_contract",
        a_refusal_and_a_failing_span_close_keep_the_typed_error,
    );
}
