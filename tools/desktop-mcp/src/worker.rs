//! The one thread that touches the desktop.
//!
//! UI Automation objects belong to the COM apartment of the thread that
//! created them, and synthesized input must not interleave, so every desktop
//! call runs here, in arrival order. Async tool handlers send a closure and
//! await its result; no tokio thread ever touches UIA.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::thread;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::desktop::Desktop;
use crate::error::{ToolError, ToolResult};

type Job = Box<dyn FnOnce(&mut Desktop) + Send>;

/// How many calls may wait for the desktop thread at once. Past it a call is
/// refused rather than queued: a client retrying on its own timeouts would
/// otherwise pile up side-effecting calls that run long after it gave up.
const QUEUE: usize = 32;

/// A handle to the desktop thread; cheap to clone.
#[derive(Debug, Clone)]
pub struct Worker {
    jobs: mpsc::SyncSender<Job>,
}

impl Worker {
    /// Starts the thread. The [`Desktop`] is created on it, so its COM
    /// apartment is that thread's.
    pub fn spawn() -> std::io::Result<Self> {
        let (jobs, rx) = mpsc::sync_channel::<Job>(QUEUE);
        thread::Builder::new()
            .name("desktop".into())
            .spawn(move || {
                let mut desktop = Desktop::new();
                while let Ok(job) = rx.recv() {
                    // A panicking job drops its reply sender, which reports
                    // the failure to its caller; the thread keeps serving.
                    // It may have panicked with a button or a key down, so
                    // everything held is released before the next job.
                    if catch_unwind(AssertUnwindSafe(|| job(&mut desktop))).is_err() {
                        tracing::error!("a desktop job panicked; releasing held input");
                        let released = catch_unwind(AssertUnwindSafe(|| desktop.release_input()));
                        if released.is_err() {
                            tracing::error!("releasing held input panicked too");
                        }
                    }
                }
            })?;
        Ok(Self { jobs })
    }

    /// Runs `f` on the desktop thread and returns its result, unless `ct`
    /// (the request's cancellation) fires first: a call cancelled before it
    /// starts never runs, and its caller stops waiting. One already running
    /// finishes, since stopping an action halfway is worse than completing
    /// it.
    pub async fn run<R, F>(&self, ct: &CancellationToken, f: F) -> ToolResult<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut Desktop) -> ToolResult<R> + Send + 'static,
    {
        if ct.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let (reply, result) = oneshot::channel();
        let job_ct = ct.clone();
        self.jobs
            .try_send(Box::new(move |desktop| {
                if reply.is_closed() || job_ct.is_cancelled() {
                    return;
                }
                let _ = reply.send(f(desktop));
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => ToolError::NotSupported(format!(
                    "busy: {QUEUE} desktop calls are already waiting; retry once they finish"
                )),
                mpsc::TrySendError::Disconnected(_) => {
                    ToolError::platform("desktop thread", "it has stopped")
                }
            })?;
        tokio::select! {
            reply = result => {
                reply.map_err(|_| ToolError::platform("desktop thread", "the call panicked"))?
            }
            () = ct.cancelled() => Err(ToolError::Cancelled),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    /// A call cancelled while it waits behind another never runs: its side
    /// effect does not happen once the thread gets to it.
    #[tokio::test(flavor = "current_thread")]
    async fn a_call_cancelled_in_the_queue_never_runs() {
        let worker = Worker::spawn().expect("BUG: the thread starts");
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let first = {
            let worker = worker.clone();
            tokio::spawn(async move {
                worker
                    .run(&CancellationToken::new(), move |_| {
                        let _ = hold.recv();
                        Ok(())
                    })
                    .await
            })
        };
        tokio::task::yield_now().await;
        let ran = Arc::new(AtomicBool::new(false));
        let cancelled = CancellationToken::new();
        let second = {
            let (worker, ran, ct) = (worker.clone(), Arc::clone(&ran), cancelled.clone());
            tokio::spawn(async move {
                worker
                    .run(&ct, move |_| {
                        ran.store(true, Ordering::SeqCst);
                        Ok(())
                    })
                    .await
            })
        };
        tokio::task::yield_now().await;
        cancelled.cancel();
        let second = second.await.expect("BUG: the task completes");
        assert!(matches!(second, Err(ToolError::Cancelled)), "{second:?}");
        release.send(()).expect("BUG: the blocker waits");
        first
            .await
            .expect("BUG: the task completes")
            .expect("BUG: the first call finishes");
        // A third call runs after the cancelled one was skipped.
        worker
            .run(&CancellationToken::new(), |_| Ok(()))
            .await
            .expect("BUG: the thread still serves");
        assert!(
            !ran.load(Ordering::SeqCst),
            "the cancelled call did not run"
        );
    }
}
