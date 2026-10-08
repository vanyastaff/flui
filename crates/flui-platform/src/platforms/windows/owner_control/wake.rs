//! Posted owner turns with a kernel-event fallback when queue admission fails.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows::Win32::{
    Foundation::{HANDLE, HWND, LPARAM, WPARAM},
    System::Threading::{CreateEventW, SetEvent},
    UI::WindowsAndMessaging::PostMessageW,
};

pub(in crate::platforms::windows) struct OwnerWake {
    event: OwnedHandle,
    #[cfg(test)]
    pub(in crate::platforms::windows) fail_posts: std::sync::atomic::AtomicBool,
}

impl OwnerWake {
    pub(in crate::platforms::windows) fn new() -> windows::core::Result<Self> {
        // SAFETY: unnamed, non-inheritable auto-reset event; no caller pointers.
        let event = unsafe { CreateEventW(None, false, false, None)? };
        Ok(Self {
            // SAFETY: successful CreateEventW transfers this unique handle;
            // OwnedHandle closes it exactly once, after all transport owners drop.
            event: unsafe { OwnedHandle::from_raw_handle(event.0) },
            #[cfg(test)]
            fail_posts: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub(in crate::platforms::windows) fn handle(&self) -> HANDLE {
        HANDLE(self.event.as_raw_handle())
    }

    pub(in crate::platforms::windows) fn post(&self, hwnd: HWND) -> windows::core::Result<()> {
        #[cfg(test)]
        let inject_failure = self.fail_posts.load(std::sync::atomic::Ordering::Acquire);
        #[cfg(not(test))]
        let inject_failure = false;
        // SAFETY: the caller retains the owner address under its admission lock;
        // this pointer-free message cannot invoke user code synchronously.
        if !inject_failure
            && unsafe { PostMessageW(Some(hwnd), super::WAKE, WPARAM(0), LPARAM(0)) }.is_ok()
        {
            return Ok(());
        }
        // SAFETY: self owns the live event throughout SetEvent. Native wait holds
        // another Arc to the same owner, so shutdown cannot close a waiting handle.
        unsafe { SetEvent(self.handle()) }
    }
}
