//! What the runtime is told of a Win32 window's visibility.
//!
//! A window is visible when its surface is (shown and not minimized, decided
//! by the `WM_SIZE` and `WM_SHOWWINDOW` arms through `shared::visibility`)
//! and nothing hides it from the user anyway: the desktop window manager
//! cloaking it (it is on another virtual desktop, or an app cloaked it) or
//! the session being locked. One method combines them and reports edges only.

use std::cell::Cell;

use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED, OBJID_WINDOW,
            WINEVENT_OUTOFCONTEXT,
        },
    },
};

use super::platform::WindowContext;

/// What hides a window whose surface is shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Occlusion {
    /// The desktop window manager cloaks the window.
    pub(super) cloaked: bool,
    /// The user's session is locked.
    pub(super) session_locked: bool,
}

/// The parts of a window's visibility, and what the runtime was last told.
pub(super) struct Visibility {
    /// What the surface rules last said: shown and not minimized.
    surface: Cell<bool>,
    occlusion: Cell<Occlusion>,
    /// Last value dispatched through
    /// `WindowCallbacks::dispatch_visibility_status_change`.
    reported: Cell<bool>,
}

impl Visibility {
    /// A window the runtime starts out being told is `visible`.
    pub(super) fn new(visible: bool) -> Self {
        Self {
            surface: Cell::new(visible),
            occlusion: Cell::new(Occlusion::default()),
            reported: Cell::new(visible),
        }
    }

    fn visible(&self) -> bool {
        let occlusion = self.occlusion.get();
        self.surface.get() && !occlusion.cloaked && !occlusion.session_locked
    }
}

impl WindowContext {
    /// The surface rules say the window's surface is (`true`) or is not
    /// shown; the runtime hears of a change in its visibility.
    pub(super) fn report_surface_visibility(&self, surface: bool) {
        self.visibility.surface.set(surface);
        self.settle_visibility();
    }

    /// `WM_WTSSESSION_CHANGE`: the session may have locked or unlocked.
    pub(super) fn session_changed(&self, wparam: usize, lparam: isize) {
        use crate::shared::windows_host_messages::{
            HostEvent, HostMessage, WM_WTSSESSION_CHANGE, classify,
        };
        match classify(WM_WTSSESSION_CHANGE, wparam, lparam) {
            HostMessage::Event(HostEvent::SessionLocked) => self.set_session_locked(true),
            HostMessage::Event(HostEvent::SessionUnlocked) => self.set_session_locked(false),
            _ => {}
        }
    }

    /// The session locked (`true`) or unlocked.
    fn set_session_locked(&self, locked: bool) {
        let occlusion = self.visibility.occlusion.get();
        self.visibility.occlusion.set(Occlusion {
            session_locked: locked,
            ..occlusion
        });
        self.settle_visibility();
    }

    /// Read whether the desktop window manager cloaks the window.
    pub(super) fn refresh_cloaked(&self, hwnd: HWND) {
        let mut cloaked = 0_u32;
        // SAFETY: `cloaked` is a live `u32`, the size `DWMWA_CLOAKED` writes,
        // passed with that size.
        let read = unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast(),
                size_of::<u32>() as u32,
            )
        };
        if read.is_err() {
            return;
        }
        let occlusion = self.visibility.occlusion.get();
        self.visibility.occlusion.set(Occlusion {
            cloaked: cloaked != 0,
            ..occlusion
        });
        self.settle_visibility();
    }

    fn settle_visibility(&self) {
        let visible = self.visibility.visible();
        if self.visibility.reported.replace(visible) != visible {
            tracing::debug!(visible, "window visibility changed");
            self.callbacks.dispatch_visibility_status_change(visible);
        }
    }
}

/// The window's subscription to the cloak changes of its thread's windows;
/// unsubscribed when dropped, which happens on the owner thread with the
/// window's context.
pub(super) struct CloakWatch(Option<HWINEVENTHOOK>);

impl CloakWatch {
    /// Subscribe on the calling thread, the window's owner. Without a
    /// subscription the window is never seen as cloaked.
    pub(super) fn start() -> Self {
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
            return Self(None);
        }
        Self(Some(hook))
    }

    fn is(&self, hook: HWINEVENTHOOK) -> bool {
        self.0 == Some(hook)
    }
}

impl Drop for CloakWatch {
    fn drop(&mut self) {
        if let Some(hook) = self.0.take() {
            // SAFETY: the hook this watch installed on this thread, released
            // once.
            let _ = unsafe { UnhookWinEvent(hook) };
        }
    }
}

/// Every window of the thread has its own hook, and each event reaches all of
/// them: a window takes only the events of its own hook about itself. The
/// context gate refuses another thread's or process's window.
unsafe extern "system" fn cloak_changed(
    hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
        return;
    }
    // The visibility callback is user code. As in `window_proc`, a panic must
    // not unwind across the `extern "system"` boundary: it is logged, then the
    // process aborts. `AssertUnwindSafe` is sound because nothing observes
    // the state the unwind tore.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = super::platform::with_window_context(hwnd, "cloak change", |ctx| {
            if ctx.cloak_watch.is(hook) {
                ctx.refresh_cloaked(hwnd);
            }
        });
    }));
    if let Err(payload) = outcome {
        tracing::error!(
            hwnd = ?hwnd,
            panic = crate::shared::panic_boundary::panic_payload_message(&*payload),
            "panic in a cloak-change callback; aborting — it must not unwind across \
             the extern \"system\" boundary"
        );
        std::process::abort();
    }
}
