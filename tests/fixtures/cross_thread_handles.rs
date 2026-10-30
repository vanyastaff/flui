//! The handles meant to cross a thread stay `Send` (and `Sync` where they are
//! now): a worker's signal sender, a rebuild request, the frame waker, the
//! application handle, the host's IO and compute work and pools, and the dev
//! hooks the event loop starts. Each is moved into `std::thread::spawn`, so a
//! handle that stops being `Send` stops this consumer from compiling.

use std::sync::Arc;

use flui::app::{AppHandle, ComputeJob, HostComputePool, HostIoPool, IoFuture};
use flui::view::dev_agent::DevAgentHook;
use flui::view::dev_reload::DevReloadHook;
use flui::view::{FrameWaker, RebuildHandle, SignalSender};

fn across<T: Send + 'static>(value: T) {
    std::thread::spawn(move || drop(value))
        .join()
        .expect("the worker drops the handle");
}

fn shared<T: ?Sized + Send + Sync + 'static>(value: Arc<T>) {
    let worker = Arc::clone(&value);
    across(worker);
}

fn handles(
    sender: SignalSender<u32>,
    rebuild: RebuildHandle,
    waker: FrameWaker,
    app: AppHandle,
    io: IoFuture,
    compute: ComputeJob,
    io_pool: Arc<dyn HostIoPool>,
    compute_pool: Arc<dyn HostComputePool>,
    reload: Box<dyn DevReloadHook>,
    agent: Box<dyn DevAgentHook>,
) {
    across(sender);
    across(rebuild);
    across(waker);
    across(app);
    across(io);
    across(compute);
    shared(io_pool);
    shared(compute_pool);
    across(reload);
    across(agent);
}

#[test]
fn cross_thread_handles_stay_sendable() {
    // Compiling `handles` is the check; nothing here crosses a thread at run time.
    let _ = handles;
}
