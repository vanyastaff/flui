//! The execution services live in `flui-runtime`, an internal crate; an
//! embedder reaches their host-injection seam only through `flui_app::…`.
//! These names are that public surface, so this module names nothing else.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flui_app::{AppConfig, ComputeJob, DeterministicExecutors, HostComputePool, SpawnError};

#[test]
fn host_executors_route_work_through_flui_app_paths() {
    let det = DeterministicExecutors::new();

    let config = AppConfig::default().with_executors(det.host_executors());
    assert!(config.executors.is_some());

    let ran = Arc::new(AtomicBool::new(false));
    let ran_in_job = Arc::clone(&ran);
    let job: ComputeJob = Box::new(move || ran_in_job.store(true, Ordering::Release));
    assert_eq!(HostComputePool::spawn_job(&det, job), Ok(()));

    assert_eq!(det.run_until_idle(), 1);
    assert!(ran.load(Ordering::Acquire), "the injected job ran");
    assert_ne!(SpawnError::Saturated, SpawnError::ShuttingDown);
}
