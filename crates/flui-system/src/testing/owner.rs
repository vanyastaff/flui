//! An owner whose turns a provider's test runs, on every host: the
//! completions a backend answers, the runtime-local values it keeps, and
//! the acquisition context it is made in.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use flui_platform::capability::{
    CompletionSource, HostBridge, OwnerJob, OwnerPoster, OwnerScope, PendingCompletions,
    ProviderContext, RuntimeLocals, WindowRef,
};

type Jobs = Arc<Mutex<Vec<OwnerJob>>>;

pub(crate) struct TestOwner {
    jobs: Jobs,
    poster: OwnerPoster,
    pending: PendingCompletions,
    completions: CompletionSource,
    locals: RuntimeLocals,
    bridge: HostBridge,
}

impl TestOwner {
    pub(crate) fn new() -> Self {
        let jobs: Jobs = Arc::default();
        let queue = Arc::clone(&jobs);
        let poster = OwnerPoster::new(move |job| {
            queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(job);
            Ok(())
        });
        let pending = PendingCompletions::new();
        let completions = CompletionSource::new(poster.clone(), pending.clone());
        Self {
            jobs,
            poster,
            pending,
            completions,
            locals: RuntimeLocals::new(),
            bridge: HostBridge::new(),
        }
    }

    /// The owner's completions, which a backend's resolvers answer.
    pub(crate) fn completions(&self) -> &CompletionSource {
        &self.completions
    }

    /// The acquisition context of a widget in `window`, or in none.
    pub(crate) fn ctx(&self, window: Option<WindowRef>) -> ProviderContext<'_> {
        ProviderContext::new(
            &self.poster,
            &self.completions,
            &self.locals,
            &self.bridge,
            window,
        )
    }

    /// Run the jobs queued so far; returns how many ran.
    pub(crate) fn turn(&self) -> usize {
        let jobs = std::mem::take(&mut *self.jobs.lock().unwrap_or_else(PoisonError::into_inner));
        let ran = jobs.len();
        let scope = OwnerScope::new(&self.pending);
        for job in jobs {
            job.run(&scope);
        }
        ran
    }

    /// Run turns until `done`, or panic after ten seconds: what another
    /// thread answers arrives as a posted job.
    pub(crate) fn turn_until(&self, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "the owner waited ten seconds");
            if self.turn() == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
