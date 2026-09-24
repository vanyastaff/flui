//! The one thread that touches the desktop.
//!
//! UI Automation objects belong to the COM apartment of the thread that
//! created them, and synthesized input must not interleave, so every desktop
//! call runs here, in arrival order. Async tool handlers send a closure and
//! await its result; no tokio thread ever touches UIA.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::desktop::Desktop;
use crate::error::{ToolError, ToolResult};

type Job = Box<dyn FnOnce(&mut Desktop) + Send>;

/// A job's state, moved on by exactly one compare-and-swap.
const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const CANCELLED: u8 = 2;

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
        let (reply, mut result) = oneshot::channel();
        // Queued, then either running or cancelled: one compare-and-swap
        // decides, so a job cannot start after its caller was told it never
        // would, nor be reported unstarted while it runs.
        let state = Arc::new(AtomicU8::new(QUEUED));
        let job_state = Arc::clone(&state);
        self.jobs
            .try_send(Box::new(move |desktop| {
                let claimed = job_state
                    .compare_exchange(QUEUED, RUNNING, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
                if !claimed || reply.is_closed() {
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
        let panicked = |_| ToolError::platform("desktop thread", "the call panicked");
        tokio::select! {
            reply = &mut result => reply.map_err(panicked)?,
            () = ct.cancelled() => {
                // Still queued: it will never run. Already running: it runs
                // to the end on the desktop thread either way, so its real
                // outcome is the answer, not "nothing was done".
                let cancelled = state
                    .compare_exchange(QUEUED, CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
                if cancelled {
                    Err(ToolError::Cancelled)
                } else {
                    result.await.map_err(panicked)?
                }
            }
        }
    }
}

impl Worker {
    /// Runs `f` after everything already queued, waiting for room in the
    /// queue rather than refusing when it is full: the release of held input
    /// at shutdown must not be the call a busy queue turns away.
    pub async fn run_at_shutdown<R, F>(&self, f: F) -> ToolResult<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut Desktop) -> ToolResult<R> + Send + 'static,
    {
        let (reply, result) = oneshot::channel();
        let jobs = self.jobs.clone();
        let job: Job = Box::new(move |desktop| {
            let _ = reply.send(f(desktop));
        });
        // A plain thread, not the runtime's blocking pool: if the queue stays
        // full past the caller's timeout, the runtime does not wait for this
        // send at teardown, and the process still exits.
        let (queued, sent) = oneshot::channel();
        thread::Builder::new()
            .name("desktop-shutdown".into())
            .spawn(move || {
                let _ = queued.send(jobs.send(job).is_ok());
            })
            .map_err(|e| ToolError::platform("desktop thread", e))?;
        if !sent.await.unwrap_or(false) {
            return Err(ToolError::platform("desktop thread", "it has stopped"));
        }
        result
            .await
            .map_err(|_| ToolError::platform("desktop thread", "the call panicked"))?
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;

    /// A call cancelled after it started is not reported as never run: its
    /// real result comes back, since it runs to the end regardless.
    #[tokio::test(flavor = "current_thread")]
    async fn a_call_cancelled_while_running_reports_its_result() {
        let worker = Worker::spawn().expect("BUG: the thread starts");
        let (running, started) = std::sync::mpsc::channel::<()>();
        let (release, hold) = std::sync::mpsc::channel::<()>();
        let ct = CancellationToken::new();
        let call = {
            let (worker, ct) = (worker.clone(), ct.clone());
            tokio::spawn(async move {
                worker
                    .run(&ct, move |_| {
                        let _ = running.send(());
                        let _ = hold.recv();
                        Ok(7)
                    })
                    .await
            })
        };
        tokio::task::spawn_blocking(move || started.recv())
            .await
            .expect("BUG: the wait completes")
            .expect("BUG: the job starts");
        ct.cancel();
        tokio::task::yield_now().await;
        release.send(()).expect("BUG: the job waits");
        let outcome = call.await.expect("BUG: the task completes");
        assert_eq!(
            outcome.ok(),
            Some(7),
            "the call's own result, not Cancelled"
        );
    }

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
