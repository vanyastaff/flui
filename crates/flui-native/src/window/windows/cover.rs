//! Whether other windows cover a window of the thread: the Win32 covering
//! walk, a hook on FLUI's window class.
//!
//! Win32 tells a window nothing when another window covers it: the desktop
//! window manager composes it and stops showing it. The walk asks the desktop
//! instead (Chromium's `NativeWindowOcclusionTrackerWin`): the windows in
//! z-order from the top, each opaque one subtracted from what is left of the
//! window's content on the monitors (`coverage`). A window with nothing left
//! is covered, and its snapshot reads occluded.
//!
//! The walk runs when it can change what the application does, and never on
//! a clock of its own:
//!
//! - when a frame is due for a window (`WindowStep::FrameDue`): a window
//!   that keeps drawing learns that it went under another one, and an idle
//!   window costs nothing;
//! - while a window of the thread is covered, on the desktop's changes that
//!   can uncover it, heard through an out-of-context WinEvent hook: a
//!   foreground change, the end of a move or a resize, a minimize or a
//!   restore, a window shown, hidden or destroyed, a maximize or a restore
//!   (`EVENT_OBJECT_STATECHANGE`), a cloak. A covered window draws nothing,
//!   so no frame of its own would look again. The hook is held only while a
//!   window is covered: its events come from every process.
//!
//! Walks are at least [`WALK_INTERVAL`] apart: a request inside the interval
//! arms the class's one walk timer for the rest of it, so a burst of desktop
//! changes is one walk. A window moved by a program without any of those
//! events (`SetWindowPos` alone) is seen at the next one, or at the covered
//! window's next frame.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, EnumDisplayMonitors, GetWindowRgnBox, HDC, HMONITOR, RGN_ERROR,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_STATECHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MOVESIZEEND,
    EVENT_SYSTEM_MOVESIZESTART, EnumThreadWindows, EnumWindows, GWL_EXSTYLE, GetClientRect,
    GetLayeredWindowAttributes, GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible,
    KillTimer, LAYERED_WINDOW_ATTRIBUTES_FLAGS, LWA_ALPHA, LWA_COLORKEY, OBJID_WINDOW,
    SendMessageW, SetTimer, WINEVENT_OUTOFCONTEXT, WM_APP, WM_NCDESTROY, WM_TIMER, WS_EX_LAYERED,
    WS_EX_TRANSPARENT,
};
use windows::core::BOOL;

use super::coverage::{Coverage, ScreenRect, coverage};
use super::proc::WindowContext;
use crate::owner::panic_boundary::{AfterPanic, os_entry};
use crate::owner::windows::{CreatedWindow, WindowMessages, WindowRef, WindowStep, WindowTable};

/// The least time between two covering walks of a thread.
pub(crate) const WALK_INTERVAL: Duration = Duration::from_millis(250);

/// The class's walk timer, armed on one of its windows for a walk asked for
/// inside [`WALK_INTERVAL`].
pub(crate) const COVER_WALK_TIMER: usize = 0x434F;

/// Sent by the desktop hook to a window of the class on the thread: the
/// desktop changed in a way that can uncover a covered window.
const DESKTOP_CHANGED: u32 = WM_APP + 24;

/// The desktop's changes that can uncover a window, as the WinEvent ranges
/// the hook subscribes to.
const UNCOVERING_EVENTS: [(u32, u32); 6] = [
    (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
    (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
    (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
    (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
    (EVENT_OBJECT_STATECHANGE, EVENT_OBJECT_STATECHANGE),
    (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
];

/// The thread's covering walk: whether each window of the class reads
/// covered, when the last walk ran, the timer armed for the next and the
/// desktop hook held while a window is covered.
#[derive(Default)]
pub(crate) struct CoverWatch {
    /// Each window of the class on the thread, with whether the last walk
    /// found it covered.
    windows: WindowTable<bool>,
    /// When the last walk ran.
    walked: Cell<Option<Instant>>,
    /// The window the walk timer is armed on, if it is.
    timer: Cell<Option<HWND>>,
    /// The desktop hook's subscriptions, held while a window is covered.
    subscriptions: RefCell<Vec<HWINEVENTHOOK>>,
}

impl CoverWatch {
    /// A walk is asked for at `now`: run it, or, inside [`WALK_INTERVAL`] of
    /// the last, arm the walk timer for the rest of the interval on `hwnd`,
    /// a window of the class, unless it is armed.
    fn request_walk(&self, hwnd: HWND, now: Instant) {
        let wait = self.walked.get().map_or(Duration::ZERO, |walked| {
            WALK_INTERVAL.saturating_sub(now.saturating_duration_since(walked))
        });
        if wait.is_zero() {
            self.walk(now);
            return;
        }
        if self.timer.get().is_none() {
            self.arm_timer(hwnd, wait);
        }
    }

    /// Arm the walk timer on `hwnd`, a window of the class, to fire after
    /// `wait`.
    fn arm_timer(&self, hwnd: HWND, wait: Duration) {
        // Whole milliseconds, rounded up, so the timer never fires inside the
        // interval and asks again.
        let millis = u32::try_from(wait.as_micros().div_ceil(1000)).unwrap_or(u32::MAX);
        // SAFETY: a window of the class on this thread, its owner; the timer
        // has no callback, so its `WM_TIMER` comes to the class's procedure,
        // which hands it to this hook.
        if unsafe { SetTimer(Some(hwnd), COVER_WALK_TIMER, millis, None) } != 0 {
            self.timer.set(Some(hwnd));
        }
    }

    /// Walk the desktop at `now`: find which windows of the class the windows
    /// above them cover, hold the desktop hook while any is, and tell each
    /// window whose answer changed. The windows are told last, with nothing
    /// of the watch borrowed: a window publishes its snapshot, and the
    /// application's handler may open or close windows.
    fn walk(&self, now: Instant) {
        self.walked.set(Some(now));
        if let Some(armed) = self.timer.take() {
            // SAFETY: the timer this watch armed on a window of this thread;
            // a window destroyed since fails the call harmlessly.
            let _ = unsafe { KillTimer(Some(armed), COVER_WALK_TIMER) };
        }
        let windows = self.windows.entries();
        if windows.is_empty() {
            return;
        }
        let screens = monitors();
        let desktop = z_order();
        let mut changed = Vec::new();
        let mut any_covered = false;
        for (window, was_covered) in windows {
            let covered = is_covered(window.hwnd(), &screens, &desktop);
            any_covered |= covered;
            if covered != was_covered {
                let _ = self.windows.update(window, |kept| *kept = covered);
                changed.push((window, covered));
            }
        }
        self.hear_the_desktop(any_covered);
        for (window, covered) in changed {
            let _ = window.with_context("covering walk", |context: &WindowContext| {
                context.set_covered(covered);
            });
        }
    }

    /// Hold the desktop hook while `listen`, release it otherwise. A
    /// subscription that fails leaves a covered window to be uncovered by its
    /// next frame or the next event of another range.
    fn hear_the_desktop(&self, listen: bool) {
        let mut subscriptions = self.subscriptions.borrow_mut();
        if !listen {
            release(subscriptions.drain(..));
            return;
        }
        if !subscriptions.is_empty() {
            return;
        }
        for (first, last) in UNCOVERING_EVENTS {
            // SAFETY: `desktop_changed` has the `WINEVENTPROC` signature and
            // stays valid for the process; an out-of-context hook runs it on
            // the thread that installed it, this one, while it takes
            // messages.
            let hook = unsafe {
                SetWinEventHook(
                    first,
                    last,
                    None,
                    Some(desktop_changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_invalid() {
                tracing::warn!(
                    first,
                    last,
                    "desktop changes unavailable: a covered window is uncovered by its next frame"
                );
            } else {
                subscriptions.push(hook);
            }
        }
    }

    /// Forget `window`: a walk timer armed on it went with it, so the walk it
    /// owed is armed again on a window left, a whole interval on; once no
    /// window left is covered the desktop hook is released.
    fn forget(&self, window: WindowRef) {
        let left = self.windows.entries();
        if self.timer.get() == Some(window.hwnd()) {
            self.timer.set(None);
            if let Some((other, _)) = left.first() {
                self.arm_timer(other.hwnd(), WALK_INTERVAL);
            }
        }
        let any_covered = left.into_iter().any(|(_, covered)| covered);
        if !any_covered {
            self.hear_the_desktop(false);
        }
    }
}

/// Release the hook's `subscriptions`, installed on this thread.
fn release(subscriptions: impl IntoIterator<Item = HWINEVENTHOOK>) {
    for hook in subscriptions {
        // SAFETY: a hook the watch installed on this thread, released once:
        // the watch no longer holds it.
        let _ = unsafe { UnhookWinEvent(hook) };
    }
}

impl Drop for CoverWatch {
    fn drop(&mut self) {
        release(self.subscriptions.get_mut().drain(..));
    }
}

impl WindowMessages for CoverWatch {
    fn created(&self, window: &CreatedWindow) {
        let _ = self.windows.put(window.window(), false);
    }

    fn step(&self, window: WindowRef, step: WindowStep) {
        if matches!(step, WindowStep::FrameDue) {
            self.request_walk(window.hwnd(), Instant::now());
        }
    }

    fn destroyed(&self, window: WindowRef) {
        let _ = self.windows.take(window.identity());
        self.forget(window);
    }

    /// The walk timer, the desktop hook's notice and a window's last message.
    fn answers(&self, msg: u32, wparam: WPARAM) -> bool {
        match msg {
            WM_TIMER => wparam.0 == COVER_WALK_TIMER,
            DESKTOP_CHANGED => true,
            _ => false,
        }
    }

    /// A window's last message forgets a window whose destruction was not
    /// announced (heard, not answered).
    unsafe fn message(
        &self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        match msg {
            WM_TIMER if wparam.0 == COVER_WALK_TIMER => {
                self.walk(Instant::now());
                Some(LRESULT(0))
            }
            DESKTOP_CHANGED => {
                self.request_walk(hwnd, Instant::now());
                Some(LRESULT(0))
            }
            WM_NCDESTROY => {
                if let Some((window, _)) = self.windows.take_handle(hwnd) {
                    self.forget(window);
                }
                None
            }
            _ => None,
        }
    }
}

/// Whether the windows above `hwnd` in `desktop` cover its content on
/// `screens`. A window that is hidden, minimized or not in the z-order is
/// not covered: those are facts of their own.
fn is_covered(hwnd: HWND, screens: &[ScreenRect], desktop: &[(HWND, Option<ScreenRect>)]) -> bool {
    // SAFETY: plain queries of a window of this thread.
    let shown = unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() };
    let Some(position) = desktop.iter().position(|(window, _)| *window == hwnd) else {
        return false;
    };
    let Some(target) = shown.then(|| client_on_screen(hwnd)).flatten() else {
        return false;
    };
    let above = desktop[..position].iter().filter_map(|(_, rect)| *rect);
    coverage(screens, above, target) == Coverage::Covered
}

/// `hwnd`'s client area on the virtual screen, in device pixels (the process
/// is per-monitor aware, so these are the desktop's own coordinates).
fn client_on_screen(hwnd: HWND) -> Option<ScreenRect> {
    let mut client = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: both out-parameters are live locals of the sizes the calls
    // write.
    unsafe {
        GetClientRect(hwnd, &raw mut client).ok()?;
        ClientToScreen(hwnd, &raw mut origin).ok().ok()?;
    }
    Some(ScreenRect {
        left: origin.x,
        top: origin.y,
        right: origin.x.saturating_add(client.right - client.left),
        bottom: origin.y.saturating_add(client.bottom - client.top),
    })
}

/// The monitors' rectangles on the virtual screen.
fn monitors() -> Vec<ScreenRect> {
    unsafe extern "system" fn each(_: HMONITOR, _: HDC, rect: *mut RECT, list: LPARAM) -> BOOL {
        // A panic ends the enumeration with the monitors read so far.
        os_entry(
            "the covering walk's monitor enumeration",
            (),
            AfterPanic::Answer(|()| BOOL::from(false)),
            || {
                // SAFETY: `list` is the address of the vector below, alive and
                // borrowed by nothing else for the whole enumeration, which
                // runs on this thread; `rect` is the monitor's rectangle the
                // system hands the callback, valid for this call.
                let (list, rect) = unsafe {
                    (
                        &mut *std::ptr::with_exposed_provenance_mut::<Vec<ScreenRect>>(
                            list.0 as usize,
                        ),
                        *rect,
                    )
                };
                list.push(screen_rect(rect));
                BOOL::from(true)
            },
        )
    }

    let mut list: Vec<ScreenRect> = Vec::new();
    // SAFETY: the callback writes through the pointer only during this call.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(each),
            LPARAM((&raw mut list).expose_provenance() as isize),
        );
    }
    list
}

/// The desktop's top-level windows from the top of the z-order, each with
/// the rectangle it covers, if it is one that covers what is under it.
fn z_order() -> Vec<(HWND, Option<ScreenRect>)> {
    unsafe extern "system" fn each(window: HWND, list: LPARAM) -> BOOL {
        // A panic ends the enumeration with the windows read so far: the
        // windows under it read uncovered.
        os_entry(
            "the covering walk's window enumeration",
            (),
            AfterPanic::Answer(|()| BOOL::from(false)),
            || {
                // SAFETY: `list` is the address of the vector below, alive and
                // borrowed by nothing else for the whole enumeration, which
                // runs on this thread.
                let list = unsafe {
                    &mut *std::ptr::with_exposed_provenance_mut::<Vec<(HWND, Option<ScreenRect>)>>(
                        list.0 as usize,
                    )
                };
                list.push((window, covering_rect(window)));
                BOOL::from(true)
            },
        )
    }

    let mut list: Vec<(HWND, Option<ScreenRect>)> = Vec::new();
    // SAFETY: the callback writes through the pointer only during this call.
    unsafe {
        let _ = EnumWindows(
            Some(each),
            LPARAM((&raw mut list).expose_provenance() as isize),
        );
    }
    list
}

/// The rectangle `window` hides what is under it in, or `None` for a window
/// that hides nothing for certain: one not shown, minimized, cloaked (on
/// another virtual desktop), see-through (layered with a colour key, partly
/// transparent or drawn per pixel, or transparent to input, as overlays
/// are), or shaped by a window region. The rectangle is the visible frame
/// (`DWMWA_EXTENDED_FRAME_BOUNDS`), without the invisible resize borders.
fn covering_rect(window: HWND) -> Option<ScreenRect> {
    // SAFETY: plain queries of a top-level window the enumeration handed
    // over; one destroyed since answers as a hidden window.
    let hides = unsafe { IsWindowVisible(window).as_bool() && !IsIconic(window).as_bool() };
    if !hides || is_cloaked(window) || is_see_through(window) {
        return None;
    }
    let mut region = RECT::default();
    // SAFETY: `region` is a live local of the size the call writes.
    if unsafe { GetWindowRgnBox(window, &raw mut region) } != RGN_ERROR {
        return None;
    }
    let mut frame = RECT::default();
    // SAFETY: `frame` is a live `RECT`, the size `DWMWA_EXTENDED_FRAME_BOUNDS`
    // writes, passed with that size.
    let extended = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&raw mut frame).cast(),
            size_of::<RECT>() as u32,
        )
    };
    if extended.is_err() {
        // SAFETY: as above.
        unsafe { GetWindowRect(window, &raw mut frame) }.ok()?;
    }
    Some(screen_rect(frame))
}

/// Whether the desktop window manager cloaks `window`.
fn is_cloaked(window: HWND) -> bool {
    let mut cloaked = 0_u32;
    // SAFETY: `cloaked` is a live `u32`, the size `DWMWA_CLOAKED` writes,
    // passed with that size.
    let read = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&raw mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
    };
    read.is_ok() && cloaked != 0
}

/// Whether what is under `window` may show through it.
fn is_see_through(window: HWND) -> bool {
    // SAFETY: a plain read of the window's extended style.
    let style = unsafe { GetWindowLongW(window, GWL_EXSTYLE) } as u32;
    if style & WS_EX_TRANSPARENT.0 != 0 {
        return true;
    }
    if style & WS_EX_LAYERED.0 == 0 {
        return false;
    }
    let mut alpha = 0_u8;
    let mut flags = LAYERED_WINDOW_ATTRIBUTES_FLAGS::default();
    // SAFETY: both out-parameters are live locals; a window drawn per pixel
    // (`UpdateLayeredWindow`) fails the call.
    let read = unsafe {
        GetLayeredWindowAttributes(window, None, Some(&raw mut alpha), Some(&raw mut flags))
    };
    read.is_err()
        || flags & LWA_COLORKEY == LWA_COLORKEY
        || (flags & LWA_ALPHA == LWA_ALPHA && alpha < u8::MAX)
}

fn screen_rect(rect: RECT) -> ScreenRect {
    ScreenRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

/// A desktop change some window of some process made. The hook keeps the
/// top-level windows' own changes and tells a window of the class on this
/// thread, whose class hands the notice to the walk.
unsafe extern "system" fn desktop_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    // The walk publishes snapshots, and the application's handler is user
    // code: as in `window_proc`, the state behind a panic mid-dispatch may be
    // torn, so it is logged and the process ends.
    os_entry("the desktop-change hook", hwnd, AfterPanic::Abort, || {
        if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 {
            return;
        }
        if let Some(window) = first_window_of_the_class() {
            // SAFETY: a window of the class on this thread: the message is a
            // call of its procedure, which hands it to the walk.
            let _ = unsafe { SendMessageW(window, DESKTOP_CHANGED, None, None) };
        }
    });
}

/// A window of FLUI's class on this thread, if one is left.
fn first_window_of_the_class() -> Option<HWND> {
    unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
        os_entry(
            "the covering walk's thread window enumeration",
            (),
            AfterPanic::Answer(|()| BOOL::from(false)),
            || {
                // A window of the class whose procedure holds a context, not
                // the message-only receiver.
                if super::proc::with_window_context(window, "desktop change", |_| ()).is_none() {
                    return BOOL::from(true);
                }
                // SAFETY: `found` is the address of the `Option<HWND>` below,
                // alive for the whole enumeration on this thread.
                unsafe {
                    *std::ptr::with_exposed_provenance_mut::<Option<HWND>>(found.0 as usize) =
                        Some(window);
                };
                BOOL::from(false)
            },
        )
    }

    let mut found: Option<HWND> = None;
    // SAFETY: the callback writes through the pointer only during this call.
    unsafe {
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(each),
            LPARAM((&raw mut found).expose_provenance() as isize),
        );
    }
    found
}
