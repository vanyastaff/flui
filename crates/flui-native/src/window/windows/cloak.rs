//! The owner thread's one subscription to the desktop window manager's cloak
//! changes, a hook on FLUI's window class.
//!
//! A cloak hides a shown window without hiding it for Win32: the shell
//! cloaks the windows of a virtual desktop it switches away from. The change
//! is announced only through an out-of-context WinEvent hook, which hears
//! every process's windows and runs on the thread that installed it. One hook
//! per thread is enough to hear every window of the thread, and a hook per
//! window would run every event once per window: a desktop switch that cloaks
//! twenty windows would be heard four hundred times. So the class's hook holds
//! one subscription while a window of the class lives on the thread, and the
//! callback finds the window by its handle.

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED, OBJID_WINDOW,
    WINEVENT_OUTOFCONTEXT, WM_NCDESTROY,
};

use super::proc::with_window_context;
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::windows::{CreatedWindow, WindowMessages, WindowRef, WindowTable};

/// The thread's subscription to cloak changes: installed when the first
/// window of FLUI's class is announced on the thread, released when the last
/// is destroyed. A subscription that fails is tried again by the next window
/// announced; until one holds, a cloaked window stays visible in its snapshot.
#[derive(Default)]
pub(crate) struct CloakWatch {
    windows: WindowTable<()>,
    subscription: Cell<Option<HWINEVENTHOOK>>,
}

impl CloakWatch {
    /// Subscribe on the calling thread, the windows' owner, unless subscribed.
    fn subscribe(&self) {
        if self.subscription.get().is_some() {
            return;
        }
        // Every source, not this thread's: the shell cloaks the windows of a
        // virtual desktop it switches away from, so the event comes from its
        // thread. `cloak_changed` keeps only this thread's FLUI windows.
        //
        // SAFETY: `cloak_changed` has the `WINEVENTPROC` signature and stays
        // valid for the process; an out-of-context hook runs it on the thread
        // that installed it, this one, while it takes messages.
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_CLOAKED,
                EVENT_OBJECT_UNCLOAKED,
                None,
                Some(cloak_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            tracing::warn!("cloak changes unavailable: a cloaked window stays visible");
            return;
        }
        self.subscription.set(Some(hook));
    }

    /// Release the subscription once no window of the class is left.
    fn release_if_unwatched(&self) {
        if !self.windows.is_empty() {
            return;
        }
        release(self.subscription.take());
    }

    /// The subscription the thread holds, read by a test.
    #[cfg(test)]
    pub(crate) fn subscription(&self) -> Option<HWINEVENTHOOK> {
        self.subscription.get()
    }
}

/// Release `subscription`, installed on this thread.
fn release(subscription: Option<HWINEVENTHOOK>) {
    if let Some(hook) = subscription {
        // SAFETY: a hook this watch installed on this thread, released once:
        // the watch no longer holds it.
        let _ = unsafe { UnhookWinEvent(hook) };
    }
}

impl Drop for CloakWatch {
    fn drop(&mut self) {
        release(self.subscription.take());
    }
}

impl WindowMessages for CloakWatch {
    fn created(&self, window: &CreatedWindow) {
        let _ = self.windows.put(window.window(), ());
        self.subscribe();
    }

    fn destroyed(&self, window: WindowRef) {
        let _ = self.windows.take(window.identity());
        self.release_if_unwatched();
    }

    /// A window's last message forgets a window whose destruction was not
    /// announced (heard, not answered).
    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        if msg == WM_NCDESTROY && self.windows.take_handle(hwnd).is_some() {
            self.release_if_unwatched();
        }
        None
    }
}

/// A cloak changed for some window of some process. The context gate keeps
/// this thread's FLUI windows, each of which reads its cloak again.
unsafe extern "system" fn cloak_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    // The visibility callback is user code. As in `window_proc`, the state
    // behind a panic mid-dispatch may be torn: it is logged, then the process
    // aborts.
    os_entry("the cloak-change hook", hwnd, AfterPanic::Abort, || {
        if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
            return;
        }
        let _ = with_window_context(hwnd, "cloak change", |ctx| {
            ctx.refresh_cloaked(hwnd);
            // A modal loop running on the thread asks the deadlines again
            // after the platform's code ran.
            ctx.modal_pacing.loops().ran();
        });
    });
}

#[cfg(test)]
mod tests {
    use super::CloakWatch;
    use crate::owner::windows::WindowMessages as _;
    use crate::owner::windows::testing::created_window;

    /// However many windows the thread opens, it holds one subscription to
    /// cloak changes, from the first window announced to the last destroyed.
    /// Red-check: subscribe in every `created` (a hook per window), and the
    /// second window's subscription differs from the first's.
    #[test]
    fn one_cloak_subscription_serves_every_window_of_the_thread() {
        let watch = CloakWatch::default();
        assert_eq!(watch.subscription(), None, "no window, no subscription");

        let (first, second) = (created_window(), created_window());
        watch.created(&first);
        let held = watch.subscription().expect("the first window subscribes");
        watch.created(&second);
        assert_eq!(
            watch.subscription(),
            Some(held),
            "the second window shares it"
        );

        watch.destroyed(first.window());
        assert_eq!(watch.subscription(), Some(held), "a window is left");
        watch.destroyed(second.window());
        assert_eq!(watch.subscription(), None, "the last window released it");

        watch.created(&created_window());
        assert!(
            watch.subscription().is_some(),
            "a window opened later subscribes again"
        );
    }
}
