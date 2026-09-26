//! Shared fixtures for the Win32 callback-affinity tests: a probe callback
//! that records whether it ran and which thread released it, and a hidden
//! window opened on the calling (owner) thread.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::ThreadId,
};

use parking_lot::Mutex;
use windows::Win32::Foundation::HWND;

use super::{WindowsPlatform, WindowsWindow};
use crate::traits::{Platform, PlatformWindow, WindowOptions};

/// What a [`Probe`] observed: how often its callback ran, on which thread,
/// and which thread dropped it.
#[derive(Default)]
pub(super) struct ProbeLog {
    runs: AtomicUsize,
    ran_on: Mutex<Option<ThreadId>>,
    dropped_on: Mutex<Option<ThreadId>>,
}

impl ProbeLog {
    pub(super) fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }

    pub(super) fn ran_on(&self) -> Option<ThreadId> {
        *self.ran_on.lock()
    }

    pub(super) fn dropped_on(&self) -> Option<ThreadId> {
        *self.dropped_on.lock()
    }
}

/// The capture a test moves into a registered callback.
pub(super) struct Probe(Arc<ProbeLog>);

impl Probe {
    pub(super) fn new(log: &Arc<ProbeLog>) -> Self {
        Self(Arc::clone(log))
    }

    /// Records one run on the current thread.
    pub(super) fn hit(&self) {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        *self.0.ran_on.lock() = Some(std::thread::current().id());
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        *self.0.dropped_on.lock() = Some(std::thread::current().id());
    }
}

/// Opens a hidden window on the calling thread, which must be the
/// platform's owner.
pub(super) fn open_hidden(platform: &WindowsPlatform) -> Arc<dyn PlatformWindow> {
    platform
        .open_window(WindowOptions {
            visible: false,
            ..Default::default()
        })
        .expect("open a hidden window on the owner thread")
}

/// The native handle behind a window this backend opened.
pub(super) fn hwnd_of(window: &Arc<dyn PlatformWindow>) -> HWND {
    window
        .as_any()
        .downcast_ref::<WindowsWindow>()
        .expect("a Win32 window")
        .hwnd()
}
