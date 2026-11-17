//! An owner for the providers' own tests: its poster queues each job, and
//! a turn runs what was queued, as a host's owner turn runs posted work.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use flui_platform::capability::{
    CompletionSource, HostBridge, OwnerJob, OwnerPoster, OwnerScope, PendingCompletions,
    ProviderContext, RuntimeLocals, WindowCloser, WindowRef,
};
use flui_platform::{SurfaceTarget, SwapChainStatistics, WindowIdMint};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
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

/// A window with no native handle: what a provider sees of a window it
/// cannot own a dialog with.
struct Handleless;

impl HasWindowHandle for Handleless {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for Handleless {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for Handleless {
    fn pre_present_notify(
        &self,
        _: Option<flui_sdk::foundation::FrameStamp>,
        _: SwapChainStatistics,
    ) {
    }

    fn keep_past_close(&self) {}
}

/// A window with no native handle, open until its closer closes it.
pub(crate) fn window() -> (WindowRef, WindowCloser) {
    open(Arc::new(Handleless))
}

/// A window whose handle names the Win32 window `hwnd`, which a fake
/// system never reaches: what a provider owns its calls with.
pub(crate) fn win32_window(hwnd: isize) -> (WindowRef, WindowCloser) {
    open(Arc::new(Named(
        std::num::NonZeroIsize::new(hwnd).expect("the test names a window"),
    )))
}

fn open(target: Arc<dyn SurfaceTarget>) -> (WindowRef, WindowCloser) {
    let (closer, lifetime) = WindowCloser::new();
    let id = WindowIdMint::new()
        .next()
        .expect("BUG: a fresh mint issues its first id");
    (WindowRef::new(id, 1.0, target, lifetime), closer)
}

/// A window known only by its handle's value.
struct Named(std::num::NonZeroIsize);

impl HasWindowHandle for Named {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = raw_window_handle::RawWindowHandle::Win32(
            raw_window_handle::Win32WindowHandle::new(self.0),
        );
        // SAFETY: the handle carries a value no code under test hands to
        // the system: the fakes it reaches only compare it.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Named {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl SurfaceTarget for Named {
    fn pre_present_notify(
        &self,
        _: Option<flui_sdk::foundation::FrameStamp>,
        _: SwapChainStatistics,
    ) {
    }

    fn keep_past_close(&self) {}
}
