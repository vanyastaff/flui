//! Installing FLUI tracing must not replace a logger already owned by the host.

use std::sync::atomic::{AtomicUsize, Ordering};

use flui_log::{InstallPolicy, LogBridgePolicy, LogBridgeStatus};
use tracing_log::log::{self, Log, Metadata, Record};
use tracing_subscriber::Registry;

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
fn an_existing_log_logger_is_preserved() {
    log::set_logger(&HOST_LOGGER).expect("this test binary owns the logger slot");
    log::set_max_level(log::LevelFilter::Trace);

    let installation = flui_log::install_subscriber(
        Registry::default(),
        InstallPolicy::Install,
        LogBridgePolicy::Auto,
    )
    .expect("this test binary owns the tracing subscriber slot");

    assert_eq!(
        installation.log_bridge,
        LogBridgeStatus::ExistingLoggerPreserved
    );

    log::info!("host logger remains active");
    assert_eq!(RECORDS.load(Ordering::Relaxed), 1);
}
