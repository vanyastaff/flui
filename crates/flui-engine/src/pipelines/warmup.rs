//! The pipeline library of the first format a slot names, built on a worker
//! while the application builds its first frame (ADR-0305 §1).
//!
//! Driver compilation dominates a cold first frame: the eager set
//! `PipelineSet::new` builds takes tens of milliseconds to hundreds on a
//! software driver. `wgpu::Device` is `Send + Sync` on every native target, so
//! the library is built over a clone of the device's `Arc` and handed back
//! whole; the frame that needs it waits only for what is left of the build.
//! wasm32 has no thread to build on, and there the frame builds it.

use std::sync::mpsc;

use flui_foundation::panic::discard_payload;

use crate::pipelines::set::PipelineLibrary;

/// A pipeline library under construction on its own thread, and what the
/// worker does after handing it over (saving the pipeline cache).
///
/// The library crosses on a channel as soon as it is built, so the frame
/// that takes it never waits for what follows. Dropped (a device rebuild,
/// the context's retirement), it waits for the worker, so no build or save
/// outlives its owner on a device the owner has let go; a library nobody
/// took, or a panic nobody resumed, is discarded.
pub(crate) struct PipelineWarmup {
    built: Option<mpsc::Receiver<PipelineLibrary>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl PipelineWarmup {
    /// Run `build` on its own thread, then `after` once its library is
    /// handed over. `None` when the OS refuses the thread: the frame then
    /// builds the library itself.
    pub(crate) fn start(
        build: impl FnOnce() -> PipelineLibrary + Send + 'static,
        after: impl FnOnce() + Send + 'static,
    ) -> Option<Self> {
        let (hand_over, built) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("flui-pipeline-warmup".to_owned())
            .spawn(move || {
                // The receiver is gone only when the warm-up was dropped;
                // the library then goes with this thread.
                let _ = hand_over.send(build());
                after();
            })
            .ok()
            .map(|worker| Self {
                built: Some(built),
                worker: Some(worker),
            })
    }

    /// Whether the library is still to be taken.
    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.built.is_some()
    }

    /// The built library, waiting for what is left of the build; `None` once
    /// taken. A panic of the build (wgpu's error handler, a tracing
    /// subscriber) resumes here, on the frame that needed the library, with
    /// the worker joined.
    pub(crate) fn take(&mut self) -> Option<PipelineLibrary> {
        let built = self.built.take()?;
        if let Ok(library) = built.recv() {
            return Some(library);
        }
        // The sender went without sending: the build panicked.
        let worker = self
            .worker
            .take()
            .expect("BUG: a warm-up's worker is joined only after its library is taken");
        match worker.join() {
            Ok(()) => unreachable!("BUG: a build that returned sent its library"),
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

impl Drop for PipelineWarmup {
    fn drop(&mut self) {
        drop(self.built.take());
        if let Some(worker) = self.worker.take()
            && let Err(payload) = worker.join()
        {
            discard_payload(payload);
        }
    }
}
