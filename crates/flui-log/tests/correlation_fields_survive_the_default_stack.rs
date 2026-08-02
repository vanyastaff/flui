//! Structured correlation fields survive FLUI's default subscriber stack.
//!
//! Runtime, realm, presentation, frame, and worker identity are the vocabulary
//! the framework crates emit for cross-subsystem correlation. A backend that
//! dropped them would leave every event technically present and practically
//! unusable.

mod support;

use flui_foundation::diagnostics;
use flui_log::{LogConfig, PlatformLayer, SubscriberPolicy};
use support::CaptureLayer;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn every_correlation_field_reaches_a_layer_stacked_on_the_default() {
    let capture = CaptureLayer::new();

    let config = LogConfig::builder()
        .filter(flui_log::FilterConfig::new("trace").without_env_var())
        .build();

    let subscriber = Registry::default()
        .with(config.env_filter().expect("`trace` parses"))
        .with(PlatformLayer::platform_default(&config))
        .with(capture.clone());

    flui_log::install_subscriber(subscriber, SubscriberPolicy::Install)
        .expect("the slot is empty on entry to this binary");

    tracing::info!(
        runtime_id = 1_u64,
        realm_id = 2_u64,
        presentation_id = 3_u64,
        frame_id = 4_u64,
        worker_id = 5_u64,
        "presentation committed"
    );

    let events = capture.events();
    let event = events.first().expect("exactly one event was emitted");

    for (index, name) in diagnostics::CORRELATION_FIELDS.iter().enumerate() {
        let expected = (index + 1).to_string();
        assert_eq!(
            event.field(name),
            Some(expected.as_str()),
            "correlation field `{name}` was lost or altered; captured {:?}",
            event.fields
        );
    }

    assert_eq!(
        event.field("message"),
        Some("presentation committed"),
        "the message must survive alongside the correlation fields"
    );
}
