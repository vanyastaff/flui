//! A host may reserve the `log` facade while allowing FLUI to own tracing.

use std::sync::atomic::{AtomicUsize, Ordering};

use flui_log::{
    LogBridgePolicy, LogBridgeStatus, LogConfig, SubscriberOwnership, SubscriberPolicy,
};
use tracing_log::log::{self, Log, Metadata, Record};

struct HostLogger;

static HOST_LOGGER: HostLogger = HostLogger;
static RECORDS: AtomicUsize = AtomicUsize::new(0);

impl Log for HostLogger {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            RECORDS.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn flush(&self) {}
}

#[test]
fn inherit_installs_tracing_but_leaves_the_logger_slot_for_the_host() {
    let config = LogConfig::builder()
        .log_bridge_policy(LogBridgePolicy::Inherit)
        .build();

    let installation = flui_log::setup(&config, SubscriberPolicy::Install)
        .expect("this test binary owns the tracing subscriber slot");
    assert_eq!(installation.ownership, SubscriberOwnership::Installed);
    assert_eq!(installation.log_bridge, LogBridgeStatus::Inherited);

    log::set_logger(&HOST_LOGGER)
        .expect("the inherited logger slot must remain available to the host");
    log::set_max_level(log::LevelFilter::Trace);
    log::info!("host logger installed after FLUI tracing");

    assert_eq!(RECORDS.load(Ordering::Relaxed), 1);
}
