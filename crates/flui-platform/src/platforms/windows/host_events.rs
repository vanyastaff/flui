//! The system events the hidden top-level receiver forwards to the owner: the
//! session ending, the machine sleeping and waking, the user's session locking
//! and unlocking, and the system running low on memory.
//!
//! Everything here runs on the owner thread, inside the receiver's window
//! procedure. What each message means is decided by the pure rules in
//! [`crate::shared::windows_host_messages`] and
//! [`crate::shared::windows_memory_watch`]; this file performs the Win32 calls
//! those decisions ask for.

use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::Arc,
    time::Instant,
};

use flui_platform_api::{MemoryPressureLevel, SessionEndAnswer, SessionEndPhase, SystemNotice};
use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM},
    System::{
        Memory::{CreateMemoryResourceNotification, LowMemoryResourceNotification},
        RemoteDesktop::{
            NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
            WTSUnRegisterSessionNotification,
        },
        Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy},
        Threading::{
            GetCurrentThreadId, RegisterWaitForSingleObject, UnregisterWaitEx, WT_EXECUTEONLYONCE,
        },
    },
    UI::WindowsAndMessaging::{
        EnumThreadWindows, IsWindowVisible, KillTimer, PostMessageW, SetTimer, WM_APP, WM_TIMER,
    },
};
use windows::core::{BOOL, PCWSTR};

use crate::shared::{
    PlatformHandlers,
    system_notice::SystemNotices,
    windows_host_messages::{self, HostEvent, Received},
    windows_memory_watch::{HOLD_INTERVAL, MemoryWatch},
};

/// Posted by the pool thread that waits on the low-memory event.
pub(super) const LOW_MEMORY: u32 = WM_APP + 1;

/// The receiver's timer that ends the hold after a low-memory report.
const REARM_TIMER: usize = 1;

/// The host's side of the receiver. `!Send`: it is created, used and dropped
/// on the owner thread.
pub(super) struct HostEvents {
    handlers: Rc<RefCell<PlatformHandlers>>,
    notices: Arc<SystemNotices>,
    clock_origin: Instant,
    /// The window a shutdown-blocking reason is registered on, while one is.
    blocking: Cell<Option<HWND>>,
    session_notification: Cell<Option<HWND>>,
    memory: RefCell<Option<LowMemoryWait>>,
}

impl HostEvents {
    pub(super) fn new(
        handlers: Rc<RefCell<PlatformHandlers>>,
        notices: Arc<SystemNotices>,
    ) -> Self {
        Self {
            handlers,
            notices,
            clock_origin: Instant::now(),
            blocking: Cell::new(None),
            session_notification: Cell::new(None),
            memory: RefCell::new(None),
        }
    }

    /// Start listening on `receiver`, the hidden window that hears the
    /// messages. A source that cannot start is reported once and the others
    /// go on.
    pub(super) fn attach(&self, receiver: HWND) {
        // SAFETY: `receiver` is the live window the caller just created on this
        // thread; the flag asks for this session's notifications only.
        match unsafe { WTSRegisterSessionNotification(receiver, NOTIFY_FOR_THIS_SESSION) } {
            Ok(()) => self.session_notification.set(Some(receiver)),
            Err(error) => {
                tracing::warn!(%error, "session lock and unlock notices are unavailable");
            }
        }
        match LowMemoryWait::start(receiver) {
            Ok(wait) => *self.memory.borrow_mut() = Some(wait),
            Err(error) => tracing::warn!(%error, "low-memory notices are unavailable"),
        }
    }

    /// Stop listening and give back what was taken, before `receiver` is
    /// destroyed.
    pub(super) fn detach(&self, receiver: HWND) {
        self.release_block();
        let memory = self.memory.borrow_mut().take();
        drop(memory);
        if self.session_notification.take() == Some(receiver) {
            // SAFETY: registered on this live window by `attach`.
            if let Err(error) = unsafe { WTSUnRegisterSessionNotification(receiver) } {
                tracing::debug!(%error, "session notification was already gone");
            }
        }
    }

    /// Answer `message` if it is one of the system's. `None` leaves it to the
    /// default window procedure.
    pub(super) fn handle(
        &self,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        if message == LOW_MEMORY {
            self.low_memory(hwnd);
            return Some(LRESULT(0));
        }
        if message == WM_TIMER && wparam.0 == REARM_TIMER {
            self.rearm_low_memory(hwnd);
            return Some(LRESULT(0));
        }
        match windows_host_messages::classify(message, wparam.0, lparam.0) {
            Received::Query { critical } => Some(self.query_session_end(hwnd, critical)),
            Received::Event(event) => {
                self.report(event);
                // `WM_POWERBROADCAST` grants a query with TRUE; the others
                // define no result.
                Some(LRESULT(1))
            }
            Received::Ignored => None,
        }
    }

    fn report(&self, event: HostEvent) {
        match event {
            HostEvent::SessionEnded => {
                self.release_block();
                self.ask_session_end(SessionEndPhase::Ending);
            }
            HostEvent::SessionEndCancelled => {
                self.release_block();
                self.ask_session_end(SessionEndPhase::Cancelled);
            }
            HostEvent::Sleep => self.notices.dispatch(SystemNotice::Sleep),
            HostEvent::Wake => self.notices.dispatch(SystemNotice::Wake),
            HostEvent::SessionLocked => self.notices.dispatch(SystemNotice::SessionLocked),
            HostEvent::SessionUnlocked => self.notices.dispatch(SystemNotice::SessionUnlocked),
        }
    }

    /// Ask the host whether the session may end, and tell Windows.
    fn query_session_end(&self, receiver: HWND, critical: bool) -> LRESULT {
        // A query that follows another without an end or a cancel in between
        // replaces the earlier block rather than leaving its reason behind.
        self.release_block();
        let block = self.ask_session_end(SessionEndPhase::Query) == SessionEndAnswer::Block;
        // The host's answer carries no reason, so the system is given a fixed,
        // neutral one.
        let answer = windows_host_messages::query_answer(critical, block.then_some(""));
        if let Some(reason) = &answer.reason {
            let window = visible_window_of_this_thread().unwrap_or(receiver);
            // SAFETY: `reason` is NUL-terminated and outlives the call; `window`
            // is a live top-level window of this thread.
            match unsafe { ShutdownBlockReasonCreate(window, PCWSTR(reason.as_ptr())) } {
                Ok(()) => self.blocking.set(Some(window)),
                Err(error) => {
                    // Without a reason the system would not wait; answer that
                    // the session may end instead of a refusal nobody sees.
                    tracing::warn!(%error, "the session end could not be blocked");
                    return LRESULT(1);
                }
            }
        }
        LRESULT(isize::from(answer.allow))
    }

    /// Let go of a registered shutdown-blocking reason, so the system carries
    /// on. A no-op when none is held.
    pub(super) fn release_block(&self) {
        let Some(window) = self.blocking.take() else {
            return;
        };
        // SAFETY: the reason was created on `window`, a top-level window of this
        // thread; a window destroyed since takes its reason with it and the
        // call only fails.
        if let Err(error) = unsafe { ShutdownBlockReasonDestroy(window) } {
            tracing::debug!(%error, "the shutdown reason was already gone");
        }
    }

    /// Run the host's session-end hook. A panic in it answers `Proceed`: the
    /// session must not hang on a broken subscriber.
    fn ask_session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        let hook = self.handlers.borrow_mut().session_end.take();
        let Some(mut hook) = hook else {
            return SessionEndAnswer::Proceed;
        };
        let answer = match catch_unwind(AssertUnwindSafe(|| hook(phase))) {
            Ok(answer) => answer,
            Err(payload) => {
                flui_foundation::panic::retain_opaque_payload(payload);
                SessionEndAnswer::Proceed
            }
        };
        let superseded = {
            let mut handlers = self.handlers.borrow_mut();
            if handlers.session_end.is_none() {
                handlers.session_end = Some(hook);
                None
            } else {
                Some(hook)
            }
        };
        drop(superseded);
        answer
    }

    fn now(&self) -> std::time::Duration {
        self.clock_origin.elapsed()
    }

    fn low_memory(&self, receiver: HWND) {
        let owed = self
            .memory
            .borrow()
            .as_ref()
            .is_some_and(|wait| wait.shared.watch.lock().take(self.now()));
        if !owed {
            return;
        }
        // The hold ends by timer; the wait is registered again then.
        // SAFETY: a timer on the live receiver; the id is this file's.
        let timer = unsafe {
            SetTimer(
                Some(receiver),
                REARM_TIMER,
                u32::try_from(HOLD_INTERVAL.as_millis()).unwrap_or(u32::MAX),
                None,
            )
        };
        if timer == 0 {
            tracing::warn!("low-memory notices cannot be re-armed; none will follow");
        }
        self.notices
            .dispatch(SystemNotice::MemoryPressure(MemoryPressureLevel::Critical));
    }

    fn rearm_low_memory(&self, receiver: HWND) {
        self.rearm_low_memory_at(receiver, self.now());
    }

    fn rearm_low_memory_at(&self, receiver: HWND, now: std::time::Duration) {
        let rearmed = self
            .memory
            .borrow_mut()
            .as_mut()
            .is_some_and(|wait| wait.rearm(now));
        if rearmed {
            // SAFETY: the timer was set on this receiver by `low_memory`.
            let _ = unsafe { KillTimer(Some(receiver), REARM_TIMER) };
        }
    }

    /// Stand in for the pool thread that waits on the system's event: whether
    /// the signal woke the owner (and so is followed by [`LOW_MEMORY`]).
    #[cfg(test)]
    pub(super) fn signal_low_memory_for_test(&self) -> bool {
        self.memory
            .borrow()
            .as_ref()
            .is_some_and(|wait| wait.shared.watch.lock().signaled())
    }

    /// Pretend the hold after a report has run out.
    #[cfg(test)]
    pub(super) fn end_low_memory_hold_for_test(&self, receiver: HWND) {
        self.rearm_low_memory_at(receiver, self.now() + HOLD_INTERVAL);
    }
}

/// The first visible top-level window of this thread: the one the system lists
/// as holding the session up. The hidden receiver is the fallback.
fn visible_window_of_this_thread() -> Option<HWND> {
    unsafe extern "system" fn first_visible(window: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the address of the `Option<HWND>` below, alive for
        // the whole enumeration on this thread.
        let found = unsafe { &mut *(found.0 as *mut Option<HWND>) };
        // SAFETY: a plain query of the handle the enumeration hands over.
        if unsafe { IsWindowVisible(window) }.as_bool() {
            *found = Some(window);
            BOOL::from(false)
        } else {
            BOOL::from(true)
        }
    }

    let mut found: Option<HWND> = None;
    // SAFETY: the callback only writes through the pointer, during this call.
    unsafe {
        let _ = EnumThreadWindows(
            GetCurrentThreadId(),
            Some(first_visible),
            LPARAM((&raw mut found) as isize),
        );
    }
    found
}

/// What the pool thread that waits on the low-memory event shares with the
/// owner.
struct LowMemoryShared {
    watch: Mutex<MemoryWatch>,
    receiver: isize,
}

/// A wait registered on the system's low-memory event.
struct LowMemoryWait {
    event: HANDLE,
    wait: Option<HANDLE>,
    shared: Box<LowMemoryShared>,
}

unsafe extern "system" fn low_memory_fired(context: *mut c_void, _timed_out: bool) {
    // SAFETY: `context` is the address of the `LowMemoryShared` its
    // `LowMemoryWait` keeps alive until the wait is unregistered and every
    // running callback has returned.
    let shared = unsafe { &*context.cast::<LowMemoryShared>() };
    if shared.watch.lock().signaled() {
        let receiver = HWND(shared.receiver as *mut _);
        // SAFETY: posting is legal from any thread; a destroyed receiver only
        // makes it fail.
        let _ = unsafe { PostMessageW(Some(receiver), LOW_MEMORY, WPARAM(0), LPARAM(0)) };
    }
}

impl LowMemoryWait {
    fn start(receiver: HWND) -> windows::core::Result<Self> {
        // SAFETY: creating the event takes a plain enum value.
        let event = unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification)? };
        let shared = Box::new(LowMemoryShared {
            watch: Mutex::new(MemoryWatch::armed()),
            receiver: receiver.0 as isize,
        });
        let mut wait = Self {
            event,
            wait: None,
            shared,
        };
        wait.register()?;
        Ok(wait)
    }

    fn register(&mut self) -> windows::core::Result<()> {
        let mut handle = HANDLE::default();
        // SAFETY: the event is open, and the context outlives the wait: `drop`
        // unregisters it and waits for a running callback before the `Arc`
        // goes. Fires once; the hold decides when it is registered again.
        unsafe {
            RegisterWaitForSingleObject(
                &raw mut handle,
                self.event,
                Some(low_memory_fired),
                Some(std::ptr::from_ref::<LowMemoryShared>(&self.shared).cast()),
                windows::Win32::System::Threading::INFINITE,
                WT_EXECUTEONLYONCE,
            )?;
        }
        self.wait = Some(handle);
        Ok(())
    }

    /// Whether the hold is over and the wait was registered again.
    fn rearm(&mut self, now: std::time::Duration) -> bool {
        if !self.shared.watch.lock().rearm(now) {
            return false;
        }
        self.unregister();
        if let Err(error) = self.register() {
            tracing::warn!(%error, "low-memory notices could not be re-armed");
        }
        true
    }

    fn unregister(&mut self) {
        if let Some(wait) = self.wait.take() {
            // SAFETY: `wait` came from `register`. Blocks until a running
            // callback returns; the callback never waits on this thread.
            if let Err(error) = unsafe { UnregisterWaitEx(wait, Some(INVALID_HANDLE_VALUE)) } {
                tracing::debug!(%error, "low-memory wait was already unregistered");
            }
        }
    }
}

impl Drop for LowMemoryWait {
    fn drop(&mut self) {
        self.unregister();
        // SAFETY: the event was created by `start` and nothing waits on it now.
        let _ = unsafe { CloseHandle(self.event) };
    }
}
