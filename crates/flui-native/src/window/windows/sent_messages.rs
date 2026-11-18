//! The owner thread's wait for a raster thread, serving the messages sent
//! to its windows meanwhile (ADR-0305 §9).
//!
//! A DXGI present, resize or release on a raster thread can send a message
//! to the thread the window was made on and block until it is handled, so
//! an owner that waits on the raster thread with a plain wait deadlocks with
//! it (Microsoft's "DXGI: multithread considerations"). This wait serves
//! sent messages only: their window procedures run, with whatever handler of
//! the application they reach, while a posted message, input or a timer
//! stays queued for the loop.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::time::Duration;

use windows::Win32::{
    Foundation::HANDLE,
    System::Threading::{CreateEventW, INFINITE, SetEvent},
    UI::WindowsAndMessaging::{
        MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_NOREMOVE, PM_QS_SENDMESSAGE,
        PeekMessageW, QS_SENDMESSAGE,
    },
};

/// A wait on an auto-reset event that the other thread sets, ended early by
/// a message sent to the waiting thread, which it handles before it returns.
pub(crate) struct SentMessageWait {
    /// Set by [`notify`](flui_platform::OwnerWait::notify); auto-reset, so a
    /// notification before the wait ends that wait and no later one.
    event: OwnedHandle,
}

impl SentMessageWait {
    /// An unnamed, unsignalled auto-reset event to wait on; `None`, logged,
    /// when the system refused one.
    pub(crate) fn try_new() -> Option<Self> {
        // SAFETY: an unnamed, non-inheritable auto-reset event; no pointers.
        match unsafe { CreateEventW(None, false, false, None) } {
            Ok(event) => Some(Self {
                // SAFETY: a successful `CreateEventW` hands over this unique
                // handle, which `OwnedHandle` closes once.
                event: unsafe { OwnedHandle::from_raw_handle(event.0) },
            }),
            Err(error) => {
                tracing::error!(%error, "failed to create the owner's raster wait event");
                None
            }
        }
    }
}

impl flui_platform::OwnerWait for SentMessageWait {
    fn notify(&self) {
        // SAFETY: `self.event` owns the event, live for the call.
        if let Err(error) = unsafe { SetEvent(HANDLE(self.event.as_raw_handle())) } {
            tracing::error!(%error, "the owner's raster wait could not be ended");
        }
    }

    fn wait(&self, timeout: Duration) {
        // Rounded up, so a wait shorter than a millisecond does not spin;
        // `INFINITE` is never asked for.
        let milliseconds = timeout.as_nanos().saturating_add(999_999) / 1_000_000;
        let milliseconds = u32::try_from(milliseconds.min(u128::from(INFINITE - 1)))
            .expect("BUG: a bounded wait fits u32");
        // SAFETY: `self.event` is live for the call. The wait returns for
        // the event, a sent message (already waiting ones included, by
        // `MWMO_INPUTAVAILABLE`) or the timeout.
        let _ = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[HANDLE(self.event.as_raw_handle())]),
                milliseconds,
                QS_SENDMESSAGE,
                MWMO_INPUTAVAILABLE,
            )
        };
        // Whatever ended the wait, the messages sent meanwhile are handled
        // here: `PeekMessageW` calls their window procedures for every sent
        // message waiting and removes nothing else from the queue.
        let mut message = MSG::default();
        // SAFETY: `message` is a valid, correctly sized `MSG`.
        let _ = unsafe {
            PeekMessageW(
                &raw mut message,
                None,
                0,
                0,
                PM_NOREMOVE | PM_QS_SENDMESSAGE,
            )
        };
    }
}
