//! Devtools-shaped composition: an extra layer stacks over FLUI's default
//! backend without the framework crates changing dependencies.
//!
//! This is the mechanical half of "devtools layers compose": the only crates
//! involved are `flui-log` and `tracing-subscriber`, and the layer being
//! stacked knows nothing about FLUI.

mod support;

use flui_log::{LogConfig, PlatformLayer, SubscriberOwnership, SubscriberPolicy};
use support::CaptureLayer;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn an_additional_layer_sees_the_events_the_backend_sees() {
    let timeline = CaptureLayer::new();

    let config = LogConfig::builder()
        .filter(flui_log::FilterConfig::new("info").without_env_var())
        .build();

    let subscriber = Registry::default()
        .with(config.env_filter().expect("`info` parses"))
        .with(PlatformLayer::platform_default(&config))
        .with(timeline.clone());

    let ownership = flui_log::install_subscriber(subscriber, SubscriberPolicy::Auto)
        .expect("`Auto` never fails");
    assert_eq!(ownership, SubscriberOwnership::Installed);

    tracing::info!(frame_id = 9_u64, "frame rasterized");
    tracing::debug!("below the configured filter");

    let events = timeline.events();
    assert_eq!(
        events.len(),
        1,
        "the stacked layer must see exactly the events the filter admits; got {events:?}"
    );
    assert_eq!(events[0].field("frame_id"), Some("9"));
}
