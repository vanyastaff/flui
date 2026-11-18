//! A lost device's replacement, asked for off the executor's thread
//! (ADR-0305 §8).
//!
//! The driver may take hundreds of milliseconds to answer a device request
//! after a reset, and an inline executor runs on the owner thread, where no
//! window is served meanwhile. So the executor asks its backend to start a
//! request, carries on turning, and installs the device on the turn after
//! the request answered. Each request carries the [`DeviceTicket`] the
//! executor minted for it: an answer is installed only while the executor
//! still waits for that ticket, so a request overtaken by a recovery, an
//! adoption or the context's retirement installs nothing.

use std::{num::NonZeroU64, sync::Arc};

/// The identity of one device request an executor started.
///
/// Minted by the executor, once per request, and never reissued: a backend
/// installs the answer to the ticket the executor names and drops any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceTicket(NonZeroU64);

impl DeviceTicket {
    pub(crate) const fn new(raw: NonZeroU64) -> Self {
        Self(raw)
    }
}

/// What a backend calls, from whichever thread answers a device request,
/// once the answer can be installed: it schedules the executor's next turn
/// (the owner's frame inline, the raster thread's doorbell threaded).
///
/// It only schedules, so the caller holds nothing it needs back. A wake that
/// panics is contained, its text logged on `flui.gpu` and its payload
/// discarded, since the thread that answered has nobody to hand it to.
#[derive(Clone)]
pub struct DeviceAnswered(Arc<dyn Fn() + Send + Sync>);

impl DeviceAnswered {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self(wake)
    }

    /// Schedule the executor's turn that installs the answer.
    pub fn notify(&self) {
        let mut caught = flui_foundation::panic::FirstPanic::new();
        let _woken = caught.contain(|| (self.0)());
        if let Some(payload) = caught.into_payload() {
            // Nobody on this thread can take it: its text is logged, so the
            // wake that never came is told.
            tracing::error!(
                target: "flui.gpu",
                panic = flui_foundation::panic::payload_text(&*payload).unwrap_or("non-text payload"),
                "the wake of an answered device request panicked",
            );
            flui_foundation::panic::discard_payload(payload);
        }
    }
}

impl std::fmt::Debug for DeviceAnswered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceAnswered").finish_non_exhaustive()
    }
}
