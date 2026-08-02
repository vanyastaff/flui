//! Devtools-shaped composition: an extra layer stacks over FLUI's default
//! backend without the framework crates changing dependencies.
//!
//! This is the mechanical half of "devtools layers compose": the only crates
//! involved are `flui-log` and `tracing-subscriber`, and the layer being
//! stacked knows nothing about FLUI.

mod support;

use flui_log::{InstallPolicy, LogConfig, PlatformLayer, SubscriberOwnership};
use support::CaptureLayer;
use tracing_subscriber::Layer as _;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn an_additional_layer_can_use_a_more_verbose_filter_than_the_native_sink() {
    let timeline = CaptureLayer::new();

    let config = LogConfig::builder()
        .filter(flui_log::FilterConfig::new("info").without_env_var())
        .build();

    let subscriber = Registry::default()
        .with(
            PlatformLayer::platform_default(&config)
                .with_filter(config.env_filter().expect("`info` parses")),
        )
        .with(timeline.clone().with_filter(LevelFilter::TRACE));

    let installation = flui_log::install_subscriber(
        subscriber,
        InstallPolicy::Auto,
        flui_log::LogBridgePolicy::Auto,
    )
    .expect("`Auto` never fails");
    assert_eq!(installation.ownership, SubscriberOwnership::Installed);

    tracing::info!(frame_id = 9_u64, "frame rasterized");
    tracing::debug!("below the native sink's configured filter");

    let events = timeline.events();
    assert_eq!(
        events.len(),
        2,
        "the stacked layer must apply its own filter independently; got {events:?}"
    );
    assert_eq!(events[0].field("frame_id"), Some("9"));
}
