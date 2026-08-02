//! A second construction carrying a *different* configuration is the case where
//! silently replacing the subscriber would be most tempting and most wrong.

mod support;

use flui_log::{InstallPolicy, LogConfig, PlatformLayer, SubscriberOwnership, SubscriberPolicy};
use support::CaptureLayer;
use tracing_subscriber::Layer as _;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn a_differently_configured_repeat_keeps_the_first_subscriber() {
    let capture = CaptureLayer::new();

    // First construction: `info`, plus a capture layer so the test can tell
    // which subscriber is actually in effect afterwards.
    let first_config = LogConfig::builder()
        .filter(flui_log::FilterConfig::new("info").without_env_var())
        .build();
    let first = Registry::default()
        .with(
            PlatformLayer::platform_default(&first_config)
                .with_filter(first_config.env_filter().expect("`info` parses")),
        )
        .with(capture.clone().with_filter(LevelFilter::INFO));

    assert_eq!(
        flui_log::install_subscriber(first, InstallPolicy::Auto, flui_log::LogBridgePolicy::Auto,)
            .expect("the slot is empty on entry to this binary")
            .ownership,
        SubscriberOwnership::Installed
    );

    // Second construction: `trace`, which would admit strictly more events.
    let second = flui_log::setup(
        &LogConfig::builder()
            .filter(flui_log::FilterConfig::new("trace").without_env_var())
            .build(),
        SubscriberPolicy::Auto,
    )
    .expect("`Auto` never fails on a taken slot");
    assert_eq!(second.ownership, SubscriberOwnership::Unchanged);

    tracing::trace!("below the first configuration's filter");
    tracing::info!("admitted by the first configuration");

    let levels: Vec<String> = capture
        .events()
        .into_iter()
        .map(|event| event.level)
        .collect();
    assert_eq!(
        levels,
        vec!["INFO".to_owned()],
        "the second, wider configuration must not have taken effect"
    );
}
