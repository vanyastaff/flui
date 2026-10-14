//! Contracts exercised through the runtime's host interface.

#[path = "contracts/frame_clock.rs"]
mod frame_clock;

#[path = "contracts/input_tap.rs"]
mod input_tap;
#[path = "contracts/numeric_text_answers.rs"]
mod numeric_text_answers;
#[path = "contracts/owner_close.rs"]
mod owner_close;
#[path = "contracts/owner_delivery.rs"]
mod owner_delivery;
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
#[path = "../src/table_test.rs"]
mod table_test;
#[path = "contracts/text_frame.rs"]
mod text_frame;

/// The vsync these contracts deliver frames at. None of them resamples
/// pointer motion, the only consumer of a frame's vsync.
pub(crate) const VSYNC: flui_platform_api::EventTime = flui_platform_api::EventTime::from_nanos(0);

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
        input: flui_platform_api::PlatformInput,
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
        input: flui_platform_api::PlatformInput,
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
) -> flui_platform_api::EventTime {
    use flui_foundation::MonotonicClock as _;
    let elapsed = instant.saturating_duration_since(clock.epoch());
    flui_platform_api::EventTime::from_nanos(
        u64::try_from(elapsed.as_nanos()).expect("test time fits"),
    )
}
