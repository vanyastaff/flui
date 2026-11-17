//! The pipeline library of the first format a slot names, built on a worker
//! while the application builds its first frame (ADR-0305 §1).
//!
//! Driver compilation dominates a cold first frame: the eager set
//! `PipelineSet::new` builds takes tens of milliseconds to hundreds on a
//! software driver. `wgpu::Device` is `Send + Sync` on every native target, so
//! the library is built over a clone of the device's `Arc` and handed back
//! whole; the frame that needs it waits only for what is left. wasm32 has no
//! thread to build on, and there the frame builds it.

use flui_foundation::panic::discard_payload;

use crate::pipeline_set::PipelineLibrary;

/// A pipeline library under construction on its own thread.
///
/// Dropped unfinished (a device rebuild, the context's retirement), it waits
/// for the worker, so no build outlives its owner on a device the owner has
/// let go; what the worker built, or the panic it raised, is discarded.
pub(crate) struct PipelineWarmup {
    worker: Option<std::thread::JoinHandle<PipelineLibrary>>,
}

impl PipelineWarmup {
    /// Start `build` on its own thread. `None` when the OS refuses the
    /// thread: the frame then builds the library itself.
    pub(crate) fn start(build: impl FnOnce() -> PipelineLibrary + Send + 'static) -> Option<Self> {
        std::thread::Builder::new()
            .name("flui-pipeline-warmup".to_owned())
            .spawn(build)
            .ok()
            .map(|worker| Self {
                worker: Some(worker),
            })
    }

    /// The built library, waiting for what is left of the build. A panic of
    /// the build (wgpu's error handler, a tracing subscriber) resumes here,
    /// on the frame that needed the library.
    pub(crate) fn finish(mut self) -> PipelineLibrary {
        let worker = self
            .worker
            .take()
            .expect("BUG: a warm-up's worker is taken only by finish or drop");
        match worker.join() {
            Ok(library) => library,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

impl Drop for PipelineWarmup {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(library) => drop(library),
                Err(payload) => discard_payload(payload),
            }
        }
    }
}
