//! Contracts exercised through the runtime's host interface.

#[path = "contracts/drag_drop.rs"]
mod drag_drop;

fn scoped_diagnostics(
    subscriber: impl tracing::Subscriber + Send + Sync + 'static,
) -> tracing::subscriber::DefaultGuard {
    // tracing-core's single-dispatch fast path can cache `never` when another
    // thread first touches a callsite: https://github.com/tokio-rs/tracing/issues/3611.
    // Keep a second dispatcher alive, without installing it or replacing the
    // real diagnostic subscriber. Dead dispatchers are pruned on registration.
    static SENTINEL: std::sync::OnceLock<tracing::Dispatch> = std::sync::OnceLock::new();
    SENTINEL.get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()));
    tracing::subscriber::set_default(subscriber)
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn scoped_diagnostics_observes_a_callsite_first_used_without_a_subscriber() {
    const CHILD: &str = "FLUI_SCOPED_DIAGNOSTICS_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Other contract tests also install scoped dispatchers. A fresh process
        // prevents those dispatchers from masking the single-dispatch defect.
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "scoped_diagnostics_observes_a_callsite_first_used_without_a_subscriber",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .expect("run scoped diagnostics regression in a fresh process");
        assert!(
            output.status.success(),
            "scoped diagnostics child failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct DiagnosticCalls(Arc<AtomicUsize>);

    impl tracing::Subscriber for DiagnosticCalls {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, _: &tracing::Event<'_>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    fn emit() {
        tracing::error!("scoped diagnostics cold callsite");
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let _diagnostics = scoped_diagnostics(DiagnosticCalls(Arc::clone(&calls)));
    std::thread::spawn(emit)
        .join()
        .expect("first use without a subscriber");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    emit();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the scoped subscriber must receive its diagnostic after another thread initializes the callsite"
    );
}

#[path = "contracts/frame_telemetry.rs"]
mod frame_telemetry;

#[path = "contracts/frame_clock.rs"]
mod frame_clock;

#[path = "contracts/error_view.rs"]
mod error_view;

#[path = "contracts/announce.rs"]
mod announce;
#[path = "contracts/display_timing.rs"]
mod display_timing;
#[path = "contracts/frame_rate_demand.rs"]
mod frame_rate_demand;
#[path = "contracts/input_tap.rs"]
mod input_tap;
#[path = "contracts/numeric_text_answers.rs"]
mod numeric_text_answers;
#[path = "contracts/owner_close.rs"]
mod owner_close;
#[path = "contracts/owner_delivery.rs"]
mod owner_delivery;
#[path = "contracts/owner_display.rs"]
mod owner_display;
#[path = "contracts/owner_input.rs"]
mod owner_input;
#[path = "contracts/owner_metrics.rs"]
mod owner_metrics;
#[path = "contracts/owner_publication.rs"]
mod owner_publication;
#[path = "contracts/owner_registry.rs"]
mod owner_registry;
#[path = "contracts/owner_sampling.rs"]
mod owner_sampling;
#[path = "contracts/render_view_lending.rs"]
mod render_view_lending;
#[path = "../src/table_test.rs"]
mod table_test;
#[path = "contracts/text_frame.rs"]
mod text_frame;

/// The vsync these contracts deliver frames at: past the last instant the clock represents, so the
/// pump runs each frame at its host's own reading, as when no vsync is reported. None of them
/// resamples pointer motion. `frame_clock` pins what a representable vsync does.
pub(crate) const VSYNC: flui_platform::EventTime = flui_platform::EventTime::from_nanos(u64::MAX);

/// Window metrics from a logical size and scale, as a platform reports them.
pub(crate) fn window_metrics(
    width: f64,
    height: f64,
    scale: f64,
) -> flui_foundation::geometry::WindowMetrics {
    flui_foundation::geometry::WindowMetrics::from_logical(
        flui_foundation::geometry::Size::new(width, height),
        scale,
    )
    .expect("valid window metrics")
}

/// The two host-input shapes these contracts admit most, spelled through the
/// one boundary, [`HostInput`](flui_runtime::HostInput).
pub(crate) trait HostInputExt {
    fn observe(
        &self,
        observation: flui_runtime::owner::WindowObservation,
        effects: &dyn flui_runtime::owner::OwnerEffects,
    ) -> Result<flui_runtime::owner::InputOutcome, flui_runtime::owner::DispatchError>;

    fn input(
        &self,
        input: flui_platform::PlatformInput,
        effects: &dyn flui_runtime::owner::OwnerEffects,
    ) -> Result<flui_runtime::owner::InputOutcome, flui_runtime::owner::DispatchError>;
}

impl HostInputExt for flui_runtime::owner::PresentationDispatcher {
    fn observe(
        &self,
        observation: flui_runtime::owner::WindowObservation,
        effects: &dyn flui_runtime::owner::OwnerEffects,
    ) -> Result<flui_runtime::owner::InputOutcome, flui_runtime::owner::DispatchError> {
        self.dispatch(flui_runtime::HostInput::Window(observation), effects)
    }

    fn input(
        &self,
        input: flui_platform::PlatformInput,
        effects: &dyn flui_runtime::owner::OwnerEffects,
    ) -> Result<flui_runtime::owner::InputOutcome, flui_runtime::owner::DispatchError> {
        self.dispatch(flui_runtime::HostInput::Platform(input), effects)
    }
}

/// `instant` on the event timeline of a gesture binding built over `clock`
/// (ADR-0207 §2): deadlines are event times, wakes are instants.
pub(crate) fn event_at(
    clock: &flui_foundation::ManualClock,
    instant: web_time::Instant,
) -> flui_platform::EventTime {
    use flui_foundation::MonotonicClock as _;
    let elapsed = instant.saturating_duration_since(clock.epoch());
    flui_platform::EventTime::from_nanos(u64::try_from(elapsed.as_nanos()).expect("test time fits"))
}
