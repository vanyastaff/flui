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

use crate::desktop::Desktop;
use crate::error::{ToolError, ToolResult};

type Job = Box<dyn FnOnce(&mut Desktop) + Send>;

/// A handle to the desktop thread; cheap to clone.
#[derive(Debug, Clone)]
pub struct Worker {
    jobs: mpsc::Sender<Job>,
}

impl Worker {
    /// Starts the thread. The [`Desktop`] is created on it, so its COM
    /// apartment is that thread's.
    pub fn spawn() -> std::io::Result<Self> {
        let (jobs, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("desktop".into())
            .spawn(move || {
                let mut desktop = Desktop::new();
                while let Ok(job) = rx.recv() {
                    // A panicking job drops its reply sender, which reports
                    // the failure to its caller; the thread keeps serving.
                    if catch_unwind(AssertUnwindSafe(|| job(&mut desktop))).is_err() {
                        tracing::error!("a desktop job panicked");
                    }
                }
            })?;
        Ok(Self { jobs })
    }

    /// Runs `f` on the desktop thread and returns its result.
    pub async fn run<R, F>(&self, f: F) -> ToolResult<R>
    where
        R: Send + 'static,
        F: FnOnce(&mut Desktop) -> ToolResult<R> + Send + 'static,
    {
        let (reply, result) = oneshot::channel();
        self.jobs
            .send(Box::new(move |desktop| {
                let _ = reply.send(f(desktop));
            }))
            .map_err(|_| ToolError::platform("desktop thread", "it has stopped"))?;
        result
            .await
            .map_err(|_| ToolError::platform("desktop thread", "the call panicked"))?
    }
}
