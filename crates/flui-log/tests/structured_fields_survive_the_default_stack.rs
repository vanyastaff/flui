//! Arbitrary structured fields survive FLUI's default subscriber stack intact.
//!
//! This is the guarantee `flui-log` actually makes: whatever an event producer
//! records, a layer stacked on the default stack sees the same names, the same
//! values, and the same order. The backend is a transport, and a transport that
//! mangles, reorders, drops, or renames fields is broken regardless of which
//! fields happen to be fashionable.
//!
//! The field names below are **invented for this test on purpose**. An earlier
//! version of this file asserted on a fixed correlation vocabulary
//! (`runtime_id`, `realm_id`, …), which quietly turned a transport test into a
//! lock on the runtime's internal topology — a rename in a completely different
//! crate would have failed a logging test, and the vocabulary would have become
//! a contract by being tested rather than by anyone deciding it should be.
//! Names this crate has never heard of keep the assertion about transport and
//! nothing else.
//!
//! One global-slot write, in its own binary, like every other scenario here.

mod support;

use flui_log::{InstallPolicy, LogConfig, PlatformLayer};
use support::CaptureLayer;
use tracing_subscriber::Layer as _;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn arbitrary_fields_reach_a_stacked_layer_unmangled() {
    let capture = CaptureLayer::new();

    let config = LogConfig::builder()
        .filter(flui_log::FilterConfig::new("trace").without_env_var())
        .build();

    let subscriber = Registry::default()
        .with(
            PlatformLayer::platform_default(&config)
                .with_filter(config.env_filter().expect("`trace` parses")),
        )
        .with(capture.clone());

    flui_log::install_subscriber(
        subscriber,
        InstallPolicy::Install,
        flui_log::LogBridgePolicy::Auto,
    )
    .expect("the slot is empty on entry to this binary");

    // Values are distinguishable from one another and from their own field
    // names, so a swap or a rename cannot pass by coincidence.
    tracing::info!(
        widget_serial = 11_u64,
        cache_generation = 22_u64,
        tessellation_pass = 33_u64,
        operator_name = "flatten",
        retry_budget = -44_i64,
        "unit of work completed"
    );

    let events = capture.events();
    assert_eq!(
        events.len(),
        1,
        "expected exactly one event; got {events:?}"
    );
    let event = &events[0];

    for (name, expected) in [
        ("widget_serial", "11"),
        ("cache_generation", "22"),
        ("tessellation_pass", "33"),
        ("operator_name", "flatten"),
        ("retry_budget", "-44"),
    ] {
        assert_eq!(
            event.field(name),
            Some(expected),
            "field `{name}` was dropped, renamed, or altered in transit; captured {:?}",
            event.fields
        );
    }

    assert_eq!(
        event.field("message"),
        Some("unit of work completed"),
        "the message must survive alongside the fields"
    );
    assert_eq!(
        event.level, "INFO",
        "the level must survive alongside the fields"
    );

    // Order matters to any collector that renders fields into a line or pairs
    // them positionally.
    let names: Vec<&str> = event
        .fields
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| *name != "message")
        .collect();
    assert_eq!(
        names,
        vec![
            "widget_serial",
            "cache_generation",
            "tessellation_pass",
            "operator_name",
            "retry_budget",
        ],
        "the producer's field order must survive the stack"
    );
}
