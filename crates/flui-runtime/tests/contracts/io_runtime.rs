//! The IO runtime the execution services lend the native host (ADR-0327 §3):
//! FLUI's own IO pool with its drivers, none with host-injected pools, and a
//! refusal once shutdown began.

use std::time::{Duration, Instant};

use flui_runtime::execution::{DeterministicExecutors, ExecutionServices, SpawnError};

/// The default services lend their IO pool, started on first use, with its
/// time driver: a task sleeps and answers on a `flui-io` thread.
fn the_default_services_lend_their_io_pool() {
    let services = ExecutionServices::with_defaults();
    let runtime = services
        .io_runtime()
        .expect("the IO pool starts")
        .expect("the default services run their own IO pool");
    let (answer, answered) = std::sync::mpsc::channel();
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(1)).await;
        let _ = answer.send(std::thread::current().name().map(str::to_owned));
    });
    let thread = answered
        .recv_timeout(Duration::from_secs(10))
        .expect("a task on the lent runtime runs");
    assert_eq!(thread.as_deref(), Some("flui-io"));
    services.shutdown(Instant::now() + Duration::from_secs(5));
}

/// Host-injected pools mean FLUI runs no runtime, so none is lent.
fn host_pools_lend_no_runtime() {
    let services = ExecutionServices::with_host(DeterministicExecutors::new().host_executors());
    assert!(matches!(services.io_runtime(), Ok(None)));
}

/// Once shutdown began no runtime is lent.
fn a_shut_down_service_lends_no_runtime() {
    let services = ExecutionServices::with_defaults();
    services.shutdown(Instant::now());
    assert!(matches!(
        services.io_runtime(),
        Err(SpawnError::ShuttingDown)
    ));
}

#[test]
fn the_execution_services_lend_the_host_their_io_runtime() {
    crate::table_test::run_table(
        "the_execution_services_lend_the_host_their_io_runtime",
        &[
            (
                "the_default_services_lend_their_io_pool",
                the_default_services_lend_their_io_pool,
            ),
            ("host_pools_lend_no_runtime", host_pools_lend_no_runtime),
            (
                "a_shut_down_service_lends_no_runtime",
                a_shut_down_service_lends_no_runtime,
            ),
        ],
    );
}
