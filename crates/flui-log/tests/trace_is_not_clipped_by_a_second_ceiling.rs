//! A `trace` directive really delivers TRACE.
//!
//! The historical logger stacked a `LevelFilter` seeded from a `Level` field
//! that defaulted to INFO next to the `EnvFilter`, so `RUST_LOG=…=trace`
//! selected events that the ceiling then discarded. Nothing in this crate can
//! reintroduce that: there is no level knob, and every native backend is pinned
//! wide open.

mod support;

use flui_log::{LogConfig, PlatformLayer, SubscriberOwnership, SubscriberPolicy};
use support::CaptureLayer;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn a_module_trace_directive_reaches_the_backend() {
    let capture = CaptureLayer::new();

    // `without_env_var` so an ambient RUST_LOG in the developer's shell cannot
    // change what this test measures.
    let config = LogConfig::builder()
        .filter(
            flui_log::FilterConfig::new("info,trace_is_not_clipped_by_a_second_ceiling=trace")
                .without_env_var(),
        )
        .build();

    let subscriber = Registry::default()
        .with(config.env_filter().expect("the directives parse"))
        .with(PlatformLayer::platform_default(&config))
        .with(capture.clone());

    let ownership = flui_log::install_subscriber(subscriber, SubscriberPolicy::Install)
        .expect("the slot is empty on entry to this binary");
    assert_eq!(ownership, SubscriberOwnership::Installed);

    tracing::trace!(reached = true, "trace must survive the filter");
    tracing::debug!(reached = true, "debug must survive the filter");
    tracing::info!(reached = true, "info must survive the filter");

    let levels: Vec<String> = capture
        .events()
        .into_iter()
        .map(|event| event.level)
        .collect();

    assert_eq!(
        levels,
        vec!["TRACE".to_owned(), "DEBUG".to_owned(), "INFO".to_owned()],
        "a `=trace` directive must not be narrowed by anything downstream"
    );
}

#[test]
fn the_platform_layer_states_no_opinion_about_the_maximum_level() {
    // The structural half of the same guarantee: whatever the filter allows,
    // the backend does not subtract from it.
    let config = LogConfig::default();
    let layer: PlatformLayer<Registry> = PlatformLayer::platform_default(&config);

    assert_eq!(
        tracing_subscriber::Layer::<Registry>::max_level_hint(&layer),
        None,
        "the platform sink must not advertise a ceiling of its own"
    );
}
