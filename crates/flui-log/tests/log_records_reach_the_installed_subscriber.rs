//! Crates that emit through `log` remain visible after FLUI installs tracing.
//!
//! `wgpu`, `naga`, and other dependencies still use the `log` facade. Installing
//! only a tracing subscriber silently drops those records, so the compatibility
//! bridge is part of a successful managed installation rather than an optional
//! formatting detail.

mod support;

use flui_log::{InstallPolicy, LogBridgePolicy, LogBridgeStatus};
use support::CaptureLayer;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt as _;

#[test]
fn a_log_record_is_forwarded_into_the_installed_tracing_stack() {
    let capture = CaptureLayer::new();
    let subscriber = Registry::default().with(capture.clone());

    let installation =
        flui_log::install_subscriber(subscriber, InstallPolicy::Install, LogBridgePolicy::Auto)
            .expect("the subscriber and logger slots are empty on entry to this binary");
    assert_eq!(installation.log_bridge, LogBridgeStatus::Installed);

    tracing_log::log::info!(target: "dependency_using_log", "adapter selected");

    let events = capture.events();
    assert_eq!(
        events.len(),
        1,
        "the log record must reach tracing: {events:?}"
    );
    assert_eq!(events[0].target, "log");
    assert_eq!(events[0].field("log.target"), Some("dependency_using_log"));
    assert_eq!(events[0].field("message"), Some("adapter selected"));

    // What the native sinks tag with. A bridged record's raw target is the
    // literal `"log"`, so a sink reading `event.metadata().target()` files
    // every `wgpu` and `naga` line under the tag `log` — destroying exactly the
    // grouping `adb logcat wgpu:S` and `log stream --predicate` rely on. The
    // logcat sink normalises for this reason; this pins the upstream behaviour
    // that makes normalisation both necessary and sufficient.
    assert_eq!(events[0].normalized_target, "dependency_using_log");

    // Same doctrine as `trace_is_not_clipped_by_a_second_ceiling`, applied to
    // the other entrance. `log`'s global max level is a hard gate in front of
    // the facade: a record above it is dropped by the `log!` macro before any
    // subscriber is consulted, so no `EnvFilter` directive can pull it back.
    // Seeding that ceiling from the configured directives looks like an
    // optimisation and reintroduces exactly the bug this crate was rebuilt to
    // remove.
    //
    // Asserted here rather than in its own test because the ceiling is set by
    // installing the bridge, and the runner gives each test its own process.
    assert_eq!(
        tracing_log::log::max_level(),
        tracing_log::log::LevelFilter::Trace,
        "the `log` ceiling must stay open so the EnvFilter remains the only gate"
    );
}
