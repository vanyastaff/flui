//! The Windows host's lifecycle: what the owner's broadcast receiver hears of
//! the session ending, the machine sleeping and waking, the user's session
//! locking and unlocking and the system running low on memory, and what the
//! host answers.
//!
//! - [`messages`]: what each of those messages means, and what a refusal to
//!   end the session says to the system;
//! - [`memory_watch`]: when the level-triggered low-memory event becomes a
//!   notice;
//! - `host_events`: the receiver's subscriber, which performs the Win32 calls
//!   those decisions ask for, which the loop keeps for the lifecycle position.
//!
//! The two rule modules make no Win32 call, so every host checks them.

#[cfg(windows)]
pub(crate) use host_events::HostEvents;
#[cfg(windows)]
pub(crate) use session::SessionWindows;

#[cfg(windows)]
mod session {
    //! The lifecycle's hook on FLUI's window class: each window hears its
    //! session lock and unlock, decoded by [`messages`](super::messages), and
    //! the hook tells the class's hooks of it as the window's step, which the
    //! window follows (a locked session hides it).

    // Win32 calls: each window's session notification. Each `unsafe` block
    // states its invariant.
    #![expect(unsafe_code)]

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::{Rc, Weak};

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
    };
    use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;

    use super::messages::{HostEvent, HostMessage, WM_WTSSESSION_CHANGE, classify};
    use crate::owner::windows::{
        CreatedWindow, WindowIdentity, WindowMessages, WindowRef, WindowStep, window_step,
    };

    /// The session's part of FLUI's window class: what it keeps of each
    /// window, by the window's identity, from the window's creation to its
    /// destruction (or `WM_NCDESTROY` for a window whose destruction was not
    /// announced).
    #[derive(Default)]
    pub(crate) struct SessionWindows {
        windows: RefCell<HashMap<WindowIdentity, SessionWindow>>,
    }

    /// One window as the session's hook keeps it.
    #[derive(Clone)]
    struct SessionWindow {
        window: WindowRef,
        /// The hooks of its class, which its lock and unlock are told to;
        /// weak, since this hook is one of them.
        hooks: Weak<[Rc<dyn WindowMessages>]>,
        /// Whether the system took its subscription, which its destruction
        /// gives back.
        subscribed: bool,
    }

    impl SessionWindows {
        /// The window `hwnd` names, with its class's hooks.
        fn by_handle(&self, hwnd: HWND) -> Option<SessionWindow> {
            self.windows
                .borrow()
                .values()
                .find(|kept| kept.window.hwnd() == hwnd)
                .cloned()
        }

        /// Forget the window `identity` names and give its subscription back.
        fn forget(&self, identity: WindowIdentity) {
            let removed = self.windows.borrow_mut().remove(&identity);
            if let Some(kept) = removed.filter(|kept| kept.subscribed) {
                // SAFETY: the subscription this hook made on the window, which
                // is still being destroyed; a handle already gone only makes
                // the call fail.
                if let Err(error) = unsafe { WTSUnRegisterSessionNotification(kept.window.hwnd()) }
                {
                    tracing::debug!(%error, "the session notification was already gone");
                }
            }
        }
    }

    impl WindowMessages for SessionWindows {
        /// Subscribe the window to its session's lock and unlock.
        fn created(&self, window: &CreatedWindow) {
            // SAFETY: the live window its class just created on this thread;
            // the flag asks for this session's notifications only.
            let subscribed = match unsafe {
                WTSRegisterSessionNotification(window.hwnd, NOTIFY_FOR_THIS_SESSION)
            } {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "session notifications unavailable: a locked session leaves the window visible"
                    );
                    false
                }
            };
            let replaced = self.windows.borrow_mut().insert(
                window.identity,
                SessionWindow {
                    window: window.window(),
                    hooks: Rc::downgrade(&window.hooks),
                    subscribed,
                },
            );
            drop(replaced);
        }

        fn destroyed(&self, window: WindowRef) {
            self.forget(window.identity());
        }

        fn answers(&self, msg: u32, _wparam: WPARAM) -> bool {
            msg == WM_WTSSESSION_CHANGE
        }

        /// The session locked or unlocked: the window takes the step. A
        /// window's last message forgets what is left of a window whose
        /// destruction was not announced (heard, not answered).
        unsafe fn message(
            &self,
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> Option<LRESULT> {
            if msg == WM_NCDESTROY {
                let identity = self.by_handle(hwnd).map(|kept| kept.window.identity());
                if let Some(identity) = identity {
                    self.forget(identity);
                }
                return None;
            }
            if msg != WM_WTSSESSION_CHANGE {
                return None;
            }
            let locked = match classify(msg, wparam.0, lparam.0) {
                HostMessage::Event(HostEvent::SessionLocked) => Some(true),
                HostMessage::Event(HostEvent::SessionUnlocked) => Some(false),
                _ => None,
            };
            if let Some(locked) = locked
                && let Some(kept) = self.by_handle(hwnd)
                && let Some(hooks) = kept.hooks.upgrade()
            {
                window_step(&hooks, kept.window, WindowStep::SessionLocked(locked));
            }
            Some(LRESULT(0))
        }
    }
}

#[cfg(windows)]
mod host_events {
    //! The system events the hidden top-level receiver forwards to the owner: the
    //! session ending, the machine sleeping and waking, the user's session locking
    //! and unlocking, and the system running low on memory.
    //!
    //! Everything here runs on the owner thread, inside the receiver's window
    //! procedure. What each message means is decided by the pure rules in
    //! [`messages`] and [`memory_watch`](super::memory_watch);
    //! this file performs the Win32 calls those decisions ask for.

    // Win32 calls: the shutdown block, the session notification, the low-memory
    // wait and its pool-thread callback. Each `unsafe` block states its invariant.
    #![expect(unsafe_code)]

    use std::{
        cell::{Cell, RefCell},
        ffi::c_void,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Instant,
    };

    use flui_platform::{MemoryPressureLevel, SessionEndAnswer, SessionEndPhase, SystemNotice};
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
                GetCurrentThreadId, RegisterWaitForSingleObject, UnregisterWaitEx,
                WT_EXECUTEONLYONCE,
            },
        },
        UI::WindowsAndMessaging::{
            EnumThreadWindows, IsWindowVisible, KillTimer, PostMessageW, SetTimer, WM_APP, WM_TIMER,
        },
    };
    use windows::core::{BOOL, PCWSTR};

    use super::memory_watch::{HOLD_INTERVAL, MemoryWatch};
    use super::messages::{self, HostEvent, HostMessage, QueryAnswer};
    use crate::owner::call_point::{Field, call};
    use crate::owner::handlers::PlatformHandlers;
    use crate::owner::notices::NoticeChannel;
    use crate::owner::panic_boundary::{AfterPanic, os_entry};
    use crate::owner::windows::broadcast::Subscriber;
    use crate::owner::windows::control::OwnerLifecycle;

    /// Posted by the pool thread that waits on the low-memory event.
    pub(super) const LOW_MEMORY: u32 = WM_APP + 2;

    /// The receiver's timer that ends the hold after a low-memory report.
    const REARM_TIMER: usize = 1;

    /// The host's side of the receiver. `!Send`: it is created, used and dropped
    /// on the owner thread.
    pub(crate) struct HostEvents {
        handlers: Rc<RefCell<PlatformHandlers>>,
        notices: Arc<dyn NoticeChannel>,
        clock_origin: Instant,
        /// The window a shutdown-blocking reason is registered on, while one is.
        blocking: Cell<Option<HWND>>,
        session_notification: Cell<Option<HWND>>,
        memory: RefCell<Option<LowMemoryWait>>,
        /// The timer that ends a low-memory hold could not be set and is retried.
        rearm_timer_owed: Cell<bool>,
    }

    impl std::fmt::Debug for HostEvents {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("HostEvents").finish_non_exhaustive()
        }
    }

    impl HostEvents {
        pub(crate) fn new(
            handlers: Rc<RefCell<PlatformHandlers>>,
            notices: Arc<dyn NoticeChannel>,
        ) -> Self {
            Self {
                handlers,
                notices,
                clock_origin: Instant::now(),
                blocking: Cell::new(None),
                session_notification: Cell::new(None),
                memory: RefCell::new(None),
                rearm_timer_owed: Cell::new(false),
            }
        }

        fn report(&self, event: HostEvent) {
            match event {
                HostEvent::SessionEnded => {
                    // The block stays up while the host saves; the process is about to
                    // go, and the system lets it once this returns.
                    self.ask_session_end(SessionEndPhase::Ending);
                    self.release_block();
                }
                HostEvent::SessionEndCancelled => {
                    // A block this platform answered a query with is itself followed by
                    // this message: the reason stays until the host releases it
                    // (`release_session_end`), since the work that took the block is
                    // not done because the end was cancelled.
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
            match messages::query_answer(critical, block.then_some("")) {
                QueryAnswer::Allow => LRESULT(1),
                QueryAnswer::Block { reason } => {
                    let window = visible_window_of_this_thread().unwrap_or(receiver);
                    // SAFETY: `reason` is NUL-terminated and outlives the call; a
                    // `window` that has gone since it was found only makes the call
                    // fail.
                    match unsafe { ShutdownBlockReasonCreate(window, PCWSTR(reason.as_ptr())) } {
                        Ok(()) => {
                            self.blocking.set(Some(window));
                            LRESULT(0)
                        }
                        Err(error) => {
                            // Without a reason the system would not wait; answer that
                            // the session may end instead of a refusal nobody sees.
                            tracing::warn!(%error, "the session end could not be blocked");
                            LRESULT(1)
                        }
                    }
                }
            }
        }

        /// Let go of a registered shutdown-blocking reason, so the system carries
        /// on. A no-op when none is held.
        fn release_block(&self) {
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
            // Through the call point: no borrow of the handler set is held while
            // the hook runs, and a superseded hook is retired in a boundary of its
            // own, so a capture that panics in `Drop` cannot turn the answer into
            // the default one.
            call(
                &Field::new(&*self.handlers, |handlers| &mut handlers.session_end),
                |hook| hook(phase),
            )
            .or_contain(SessionEndAnswer::Proceed, "the session-end hook")
        }

        fn now(&self) -> std::time::Duration {
            self.clock_origin.elapsed()
        }

        fn low_memory(&self, receiver: HWND) {
            let owed = self
                .memory
                .borrow()
                .as_ref()
                .is_some_and(|wait| wait.shared.watch.lock().take_report(self.now()));
            if !owed {
                return;
            }
            self.arm_rearm_timer(receiver);
            self.notices
                .dispatch(SystemNotice::MemoryPressure(MemoryPressureLevel::Critical));
        }

        /// Start the timer that ends the hold after a report. If the system will
        /// not give one, the obligation stays: every later message to the receiver
        /// tries again, so the wait is not lost for the life of the platform.
        fn arm_rearm_timer(&self, receiver: HWND) {
            // SAFETY: a timer on the live receiver; the id is this file's.
            let timer = unsafe {
                SetTimer(
                    Some(receiver),
                    REARM_TIMER,
                    u32::try_from(HOLD_INTERVAL.as_millis()).unwrap_or(u32::MAX),
                    None,
                )
            };
            let failed = timer == 0;
            if failed && !self.rearm_timer_owed.replace(true) {
                tracing::warn!("the low-memory re-arm timer could not be set; retrying later");
            }
            if !failed {
                self.rearm_timer_owed.set(false);
            }
        }

        /// Whether the timer that ends a hold is still to be set.
        #[cfg(test)]
        pub(super) fn rearm_timer_owed_for_test(&self) -> bool {
            self.rearm_timer_owed.get()
        }

        /// Pretend the system refused the re-arm timer.
        #[cfg(test)]
        pub(super) fn owe_rearm_timer_for_test(&self) {
            self.rearm_timer_owed.set(true);
        }

        /// Pretend the pool thread signaled and its wake could not be posted.
        #[cfg(test)]
        pub(super) fn fail_low_memory_post_for_test(&self) {
            let signaled = self.signal_low_memory_for_test();
            if let Some(wait) = self.memory.borrow().as_ref() {
                wait.shared.post_failed.store(signaled, Ordering::Release);
            }
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
                .is_some_and(|wait| wait.shared.watch.lock().on_signal())
        }

        /// Pretend the hold after a report has run out.
        #[cfg(test)]
        pub(super) fn end_low_memory_hold_for_test(&self, receiver: HWND) {
            self.rearm_low_memory_at(receiver, self.now() + HOLD_INTERVAL);
        }
    }

    /// The lifecycle hears the receiver's session, power, lock and memory
    /// messages, and pays its own debts on any message it gets.
    impl Subscriber for HostEvents {
        /// Start listening on `receiver`, the hidden window that hears the
        /// messages. A source that cannot start is reported once and the others
        /// go on.
        fn attach(&self, receiver: HWND) {
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
        fn detach(&self, receiver: HWND) {
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

        /// A contained panic must not answer a session-end query with a refusal.
        fn answer_after_panic(&self, message: u32) -> Option<LRESULT> {
            (message == messages::WM_QUERYENDSESSION).then_some(LRESULT(1))
        }

        /// Answer `message` if it is one of the system's. `None` leaves it to the
        /// default window procedure.
        fn message(
            &self,
            hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> Option<LRESULT> {
            if self.rearm_timer_owed.get() {
                self.arm_rearm_timer(hwnd);
            }
            let post_failed = self
                .memory
                .borrow()
                .as_ref()
                .is_some_and(|wait| wait.shared.post_failed.swap(false, Ordering::AcqRel));
            if post_failed {
                self.low_memory(hwnd);
            }
            if message == LOW_MEMORY {
                self.low_memory(hwnd);
                return Some(LRESULT(0));
            }
            if message == WM_TIMER && wparam.0 == REARM_TIMER {
                self.rearm_low_memory(hwnd);
                return Some(LRESULT(0));
            }
            match messages::classify(message, wparam.0, lparam.0) {
                HostMessage::Query { critical } => Some(self.query_session_end(hwnd, critical)),
                HostMessage::Event(event) => {
                    self.report(event);
                    // `WM_POWERBROADCAST` grants a query with TRUE; the others
                    // define no result.
                    Some(LRESULT(1))
                }
                HostMessage::Ignored => None,
            }
        }
    }

    /// What the owner keeps of the lifecycle: the receiver's subscriber, and the
    /// block the host releases.
    impl OwnerLifecycle for HostEvents {
        fn release_session_end(&self) {
            self.release_block();
        }
    }

    /// The first visible top-level window of this thread: the one the system lists
    /// as holding the session up. The hidden receiver is the fallback.
    fn visible_window_of_this_thread() -> Option<HWND> {
        unsafe extern "system" fn first_visible(window: HWND, found: LPARAM) -> BOOL {
            // A panic ends the enumeration; the receiver is the fallback.
            os_entry(
                "the thread's window enumeration",
                (),
                AfterPanic::Answer(|()| BOOL::from(false)),
                || {
                    // SAFETY: `found` is the address of the `Option<HWND>` below,
                    // alive for the whole enumeration on this thread.
                    let found = unsafe { &mut *(found.0 as *mut Option<HWND>) };
                    // SAFETY: a plain query of the handle the enumeration hands over.
                    if unsafe { IsWindowVisible(window) }.as_bool() {
                        *found = Some(window);
                        BOOL::from(false)
                    } else {
                        BOOL::from(true)
                    }
                },
            )
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
        /// The wake could not be posted; the report is still owed.
        post_failed: AtomicBool,
    }

    /// A wait registered on the system's low-memory event.
    struct LowMemoryWait {
        event: HANDLE,
        wait: Option<HANDLE>,
        shared: Box<LowMemoryShared>,
    }

    /// The thread pool's callback for the low-memory event; a panic in it
    /// reports nothing for this signal.
    unsafe extern "system" fn low_memory_fired(context: *mut c_void, _timed_out: bool) {
        os_entry(
            "the low-memory wait",
            (),
            AfterPanic::Answer(|()| ()),
            || {
                // SAFETY: `context` is the address of the `LowMemoryShared` its
                // `LowMemoryWait` keeps alive until the wait is unregistered and every
                // running callback has returned.
                let shared = unsafe { &*context.cast::<LowMemoryShared>() };
                if shared.watch.lock().on_signal() {
                    let receiver = HWND(shared.receiver as *mut _);
                    // SAFETY: posting is legal from any thread; a destroyed receiver
                    // only makes it fail.
                    if unsafe { PostMessageW(Some(receiver), LOW_MEMORY, WPARAM(0), LPARAM(0)) }
                        .is_err()
                    {
                        // The report is owed and no message carries it: the owner
                        // collects it with the next message it gets.
                        shared.post_failed.store(true, Ordering::Release);
                    }
                }
            },
        );
    }

    impl LowMemoryWait {
        fn start(receiver: HWND) -> windows::core::Result<Self> {
            // SAFETY: creating the event takes a plain enum value.
            let event = unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification)? };
            let shared = Box::new(LowMemoryShared {
                watch: Mutex::new(MemoryWatch::armed()),
                receiver: receiver.0 as isize,
                post_failed: AtomicBool::new(false),
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
            if !self.shared.watch.lock().try_rearm(now) {
                return false;
            }
            self.unregister();
            if let Err(error) = self.register() {
                // No wait is registered: the hold starts over, and the timer, which
                // keeps firing, tries again.
                tracing::warn!(%error, "low-memory notices could not be re-armed; retrying");
                self.shared.watch.lock().hold_again(now);
                return false;
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
}

#[cfg(any(windows, test))]
#[forbid(unsafe_code)]
pub(crate) mod messages {
    //! What the messages the hidden top-level receiver gets from the system mean:
    //! a session ending or not, the machine sleeping or waking, the user's session
    //! locking or unlocking, and what a refusal to end the session says to the OS.
    //!
    //! The decisions need no Win32 call, so every host runs their tests. The
    //! numbers are the documented message and flag values; a Windows-only test
    //! checks them against the `windows` crate's.

    /// `WM_QUERYENDSESSION`: the system asks whether the session may end.
    pub(crate) const WM_QUERYENDSESSION: u32 = 0x0011;
    /// `WM_ENDSESSION`: the session is ending, or an earlier query was cancelled.
    pub(crate) const WM_ENDSESSION: u32 = 0x0016;
    /// `WM_POWERBROADCAST`: a power-management event.
    pub(crate) const WM_POWERBROADCAST: u32 = 0x0218;
    /// `WM_WTSSESSION_CHANGE`: the state of a terminal-services session changed.
    pub(crate) const WM_WTSSESSION_CHANGE: u32 = 0x02B1;

    /// `PBT_APMSUSPEND`: the system is about to sleep.
    const PBT_APMSUSPEND: usize = 0x0004;
    /// `PBT_APMRESUMEAUTOMATIC`: the system has resumed. Sent on every resume, so
    /// it alone means "awake"; `PBT_APMRESUMESUSPEND` follows it only when the
    /// user triggered the resume, and would deliver a second wake.
    const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

    /// `WTS_SESSION_LOCK`.
    const WTS_SESSION_LOCK: usize = 0x0007;
    /// `WTS_SESSION_UNLOCK`.
    const WTS_SESSION_UNLOCK: usize = 0x0008;

    /// `ENDSESSION_CRITICAL`: the session ends whatever the application answers.
    const ENDSESSION_CRITICAL: u32 = 0x4000_0000;

    /// The most UTF-16 units `ShutdownBlockReasonCreate` accepts, terminator
    /// included (`MAX_STR_BLOCKREASON`).
    const MAX_BLOCK_REASON_UNITS: usize = 256;

    /// What the system shows when the application blocks the end of the session
    /// without saying why.
    const FALLBACK_BLOCK_REASON: &str = "Finishing up before closing";

    /// A system event the receiver reports to the owner.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum HostEvent {
        /// The session ended; the process is about to be terminated.
        SessionEnded,
        /// A query to end the session was cancelled, by this application or by
        /// another.
        SessionEndCancelled,
        /// The machine is about to sleep.
        Sleep,
        /// The machine woke.
        Wake,
        /// The user's session locked.
        SessionLocked,
        /// The user's session unlocked.
        SessionUnlocked,
    }

    /// What a message asks of the receiver.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum HostMessage {
        /// The system asks whether the session may end; the receiver answers with
        /// [`query_answer`].
        Query { critical: bool },
        /// The message reports an event.
        Event(HostEvent),
        /// The message is none of the receiver's business.
        Ignored,
    }

    /// Classify one message by its number and parameters.
    pub(crate) fn classify(message: u32, wparam: usize, lparam: isize) -> HostMessage {
        match message {
            WM_QUERYENDSESSION => HostMessage::Query {
                // The flags are the low 32 bits of `lParam`.
                critical: lparam as u32 & ENDSESSION_CRITICAL != 0,
            },
            WM_ENDSESSION => HostMessage::Event(if wparam != 0 {
                HostEvent::SessionEnded
            } else {
                HostEvent::SessionEndCancelled
            }),
            WM_POWERBROADCAST => match wparam {
                PBT_APMSUSPEND => HostMessage::Event(HostEvent::Sleep),
                PBT_APMRESUMEAUTOMATIC => HostMessage::Event(HostEvent::Wake),
                _ => HostMessage::Ignored,
            },
            WM_WTSSESSION_CHANGE => match wparam {
                WTS_SESSION_LOCK => HostMessage::Event(HostEvent::SessionLocked),
                WTS_SESSION_UNLOCK => HostMessage::Event(HostEvent::SessionUnlocked),
                _ => HostMessage::Ignored,
            },
            _ => HostMessage::Ignored,
        }
    }

    /// How the receiver answers a query to end the session.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum QueryAnswer {
        /// Return `TRUE`: the session may end.
        Allow,
        /// Register `reason` with `ShutdownBlockReasonCreate`, NUL-terminated (the
        /// system shows it to the user), then return `FALSE`.
        Block { reason: Vec<u16> },
    }

    /// The answer to a query when the application asks to block, giving `block` as
    /// its reason (`Some`), or lets the session end (`None`). A blank reason is
    /// replaced by a neutral one.
    ///
    /// A critical session ends whatever the application answers: refusing would
    /// only show the user a reason for something that happens anyway, so it is
    /// always allowed.
    pub(crate) fn query_answer(critical: bool, block: Option<&str>) -> QueryAnswer {
        match block {
            Some(reason) if !critical => QueryAnswer::Block {
                reason: block_reason(reason),
            },
            _ => QueryAnswer::Allow,
        }
    }

    /// `reason` as the NUL-terminated UTF-16 string `ShutdownBlockReasonCreate`
    /// takes: at most 255 units, never ending in half a surrogate pair, and the
    /// fallback text when `reason` says nothing.
    pub(crate) fn block_reason(reason: &str) -> Vec<u16> {
        let reason = match reason.trim() {
            "" => FALLBACK_BLOCK_REASON,
            reason => reason,
        };
        let mut units: Vec<u16> = Vec::with_capacity(MAX_BLOCK_REASON_UNITS);
        for character in reason.chars() {
            let mut buffer = [0u16; 2];
            let encoded = character.encode_utf16(&mut buffer);
            if units.len() + encoded.len() >= MAX_BLOCK_REASON_UNITS {
                break;
            }
            units.extend_from_slice(encoded);
        }
        units.push(0);
        units
    }

    #[cfg(test)]
    mod tests {
        use super::{
            FALLBACK_BLOCK_REASON, HostEvent, HostMessage, MAX_BLOCK_REASON_UNITS,
            PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, QueryAnswer, WM_ENDSESSION, WM_POWERBROADCAST,
            WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
            block_reason, classify, query_answer,
        };

        fn utf16(text: &str) -> Vec<u16> {
            text.encode_utf16().chain([0]).collect()
        }

        fn a_query_is_critical_only_with_the_critical_flag() {
            // The flags are the low 32 bits of `lParam`; on a 32-bit target the top
            // bit makes it negative, which the rule must read the same.
            const LOGOFF: isize = 0x8000_0000_u32 as isize;
            const CLOSEAPP: isize = 0x1;
            const CRITICAL: isize = 0x4000_0000;
            for (flags, critical) in [
                (0, false),
                (LOGOFF, false),
                (CLOSEAPP, false),
                (CRITICAL, true),
                (CRITICAL | LOGOFF, true),
            ] {
                assert_eq!(
                    classify(WM_QUERYENDSESSION, 0, flags),
                    HostMessage::Query { critical },
                    "flags {flags:#x}"
                );
            }
        }

        fn an_end_session_message_ends_or_cancels_by_its_flag() {
            assert_eq!(
                classify(WM_ENDSESSION, 1, 0x8000_0000_u32 as isize),
                HostMessage::Event(HostEvent::SessionEnded)
            );
            assert_eq!(
                classify(WM_ENDSESSION, 0, 0),
                HostMessage::Event(HostEvent::SessionEndCancelled),
                "a false flag means the end was cancelled, not that it ended"
            );
        }

        fn power_broadcasts_map_sleep_and_one_wake() {
            assert_eq!(
                classify(WM_POWERBROADCAST, PBT_APMSUSPEND, 0),
                HostMessage::Event(HostEvent::Sleep)
            );
            assert_eq!(
                classify(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, 0),
                HostMessage::Event(HostEvent::Wake)
            );
            for other in [
                0x0007, // PBT_APMRESUMESUSPEND, which follows the automatic resume
                0x0006, // PBT_APMRESUMECRITICAL
                0x000A, // PBT_APMPOWERSTATUSCHANGE
                0x8013, // PBT_POWERSETTINGCHANGE
            ] {
                assert_eq!(
                    classify(WM_POWERBROADCAST, other, 0),
                    HostMessage::Ignored,
                    "{other:#x} would repeat or invent an event"
                );
            }
        }

        fn only_lock_and_unlock_are_session_events() {
            assert_eq!(
                classify(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, 1),
                HostMessage::Event(HostEvent::SessionLocked)
            );
            assert_eq!(
                classify(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK, 1),
                HostMessage::Event(HostEvent::SessionUnlocked)
            );
            for other in [1, 2, 3, 4, 5, 6, 9] {
                assert_eq!(
                    classify(WM_WTSSESSION_CHANGE, other, 1),
                    HostMessage::Ignored,
                    "WTS code {other}"
                );
            }
        }

        fn other_messages_are_ignored() {
            for message in [0, 0x000F, 0x001A, 0x0112, 0x0219] {
                assert_eq!(
                    classify(message, 0, 0),
                    HostMessage::Ignored,
                    "{message:#x}"
                );
            }
        }

        fn an_ordinary_query_is_blocked_with_a_reason() {
            assert_eq!(
                query_answer(false, Some("Saving the document")),
                QueryAnswer::Block {
                    reason: utf16("Saving the document"),
                }
            );
            assert_eq!(query_answer(false, None), QueryAnswer::Allow);
        }

        fn a_critical_query_is_never_blocked() {
            assert_eq!(
                query_answer(true, Some("Saving the document")),
                QueryAnswer::Allow
            );
        }

        fn a_reason_is_nul_terminated_and_never_empty() {
            assert_eq!(block_reason("Exporting"), utf16("Exporting"));
            for blank in ["", "   ", "\t\n"] {
                assert_eq!(
                    block_reason(blank),
                    utf16(FALLBACK_BLOCK_REASON),
                    "{blank:?} tells the user nothing"
                );
            }
        }

        fn a_long_reason_is_cut_below_the_limit() {
            let long = "x".repeat(1000);
            let units = block_reason(&long);
            assert_eq!(
                units.len(),
                MAX_BLOCK_REASON_UNITS,
                "255 units and the terminator"
            );
            assert_eq!(units.last(), Some(&0));
            assert!(
                units[..units.len() - 1]
                    .iter()
                    .all(|&unit| unit == u16::from(b'x'))
            );
        }

        fn a_cut_never_splits_a_surrogate_pair() {
            // 254 units, then a character that needs two: 256 would not fit with the
            // terminator, so it is left out whole.
            let reason = format!("{}{}", "x".repeat(254), '\u{1F4BE}');
            let units = block_reason(&reason);
            assert_eq!(units.len(), 255, "254 units and the terminator");
            assert_eq!(
                String::from_utf16(&units[..254]).ok().as_deref(),
                Some(&reason[..254])
            );
            // One unit fewer and the pair fits exactly.
            let reason = format!("{}{}", "x".repeat(253), '\u{1F4BE}');
            let units = block_reason(&reason);
            assert_eq!(units.len(), 256);
            assert_eq!(
                String::from_utf16(&units[..255]).ok().as_deref(),
                Some(reason.as_str())
            );
        }

        #[test]
        fn host_message_classification() {
            crate::table_test::run_table(
                "host_message_classification",
                &[
                    (
                        "a_query_is_critical_only_with_the_critical_flag",
                        a_query_is_critical_only_with_the_critical_flag as fn(),
                    ),
                    (
                        "an_end_session_message_ends_or_cancels_by_its_flag",
                        an_end_session_message_ends_or_cancels_by_its_flag as fn(),
                    ),
                    (
                        "power_broadcasts_map_sleep_and_one_wake",
                        power_broadcasts_map_sleep_and_one_wake as fn(),
                    ),
                    (
                        "only_lock_and_unlock_are_session_events",
                        only_lock_and_unlock_are_session_events as fn(),
                    ),
                    (
                        "other_messages_are_ignored",
                        other_messages_are_ignored as fn(),
                    ),
                    (
                        "an_ordinary_query_is_blocked_with_a_reason",
                        an_ordinary_query_is_blocked_with_a_reason as fn(),
                    ),
                    (
                        "a_critical_query_is_never_blocked",
                        a_critical_query_is_never_blocked as fn(),
                    ),
                    (
                        "a_reason_is_nul_terminated_and_never_empty",
                        a_reason_is_nul_terminated_and_never_empty as fn(),
                    ),
                    (
                        "a_long_reason_is_cut_below_the_limit",
                        a_long_reason_is_cut_below_the_limit as fn(),
                    ),
                    (
                        "a_cut_never_splits_a_surrogate_pair",
                        a_cut_never_splits_a_surrogate_pair as fn(),
                    ),
                ],
            );
        }

        #[cfg(windows)]
        #[test]
        fn the_numbers_are_the_documented_win32_values() {
            use windows::Win32::UI::WindowsAndMessaging::{
                ENDSESSION_CRITICAL, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, WM_ENDSESSION,
                WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK,
                WTS_SESSION_UNLOCK,
            };

            assert_eq!(super::WM_QUERYENDSESSION, WM_QUERYENDSESSION);
            assert_eq!(super::WM_ENDSESSION, WM_ENDSESSION);
            assert_eq!(super::WM_POWERBROADCAST, WM_POWERBROADCAST);
            assert_eq!(super::WM_WTSSESSION_CHANGE, WM_WTSSESSION_CHANGE);
            assert_eq!(super::PBT_APMSUSPEND, PBT_APMSUSPEND as usize);
            assert_eq!(
                super::PBT_APMRESUMEAUTOMATIC,
                PBT_APMRESUMEAUTOMATIC as usize
            );
            assert_eq!(super::WTS_SESSION_LOCK, WTS_SESSION_LOCK as usize);
            assert_eq!(super::WTS_SESSION_UNLOCK, WTS_SESSION_UNLOCK as usize);
            assert_eq!(super::ENDSESSION_CRITICAL, ENDSESSION_CRITICAL);
        }
    }
}

#[cfg(any(windows, test))]
#[forbid(unsafe_code)]
pub(crate) mod memory_watch {
    //! When the system's low-memory event becomes a notice for the owner.
    //!
    //! `CreateMemoryResourceNotification` gives an event that stays signaled for as
    //! long as memory is low: it is a level, not an edge. A wait registered on it
    //! without care fires again at once, forever. The watch therefore waits
    //! once, reports once, stays quiet for a hold interval whatever the event does,
    //! and only then waits again, so memory that stays low is reported at most once
    //! per interval. A signal that lands while a report is already owed (the pool
    //! thread had one in flight) joins it.
    //!
    //! The state is a plain value so every host runs the rules; the backend keeps
    //! it behind a lock the pool thread and the owner share.

    use std::time::Duration;

    /// How long after a report the watch stays deaf before it waits again.
    pub(crate) const HOLD_INTERVAL: Duration = Duration::from_secs(30);

    /// Where the watch is in its cycle.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Phase {
        /// A wait is registered on the event.
        Armed,
        /// The event fired; the owner has not yet taken the report.
        Owed,
        /// The owner took the report at `since`; the watch is deaf until the hold
        /// interval ends.
        Holding { since: Duration },
    }

    /// The cycle of one low-memory watch. Times are a monotonic clock's readings.
    #[derive(Debug)]
    pub(crate) struct MemoryWatch {
        phase: Phase,
    }

    impl MemoryWatch {
        /// A watch whose wait is registered.
        pub(crate) const fn armed() -> Self {
            Self {
                phase: Phase::Armed,
            }
        }

        /// The event fired. Returns whether the owner must be woken: `false` when
        /// a report is already owed or the watch is holding, because the signal
        /// joins what is pending or is the same low-memory episode.
        #[must_use]
        pub(crate) fn on_signal(&mut self) -> bool {
            match self.phase {
                Phase::Armed => {
                    self.phase = Phase::Owed;
                    true
                }
                Phase::Owed | Phase::Holding { .. } => false,
            }
        }

        /// The owner takes what is owed at `now`. Returns whether a report is to
        /// be delivered; the hold starts with it.
        #[must_use]
        pub(crate) fn take_report(&mut self, now: Duration) -> bool {
            match self.phase {
                Phase::Owed => {
                    self.phase = Phase::Holding { since: now };
                    true
                }
                Phase::Armed | Phase::Holding { .. } => false,
            }
        }

        /// Whether the hold is over at `now` and the wait should be registered
        /// again. Answers `true` once per hold; a clock that reads earlier than the
        /// start of the hold counts as no time passed.
        #[must_use]
        pub(crate) fn try_rearm(&mut self, now: Duration) -> bool {
            match self.phase {
                Phase::Holding { since } if now.saturating_sub(since) >= HOLD_INTERVAL => {
                    self.phase = Phase::Armed;
                    true
                }
                _ => false,
            }
        }

        /// The wait could not be registered again after [`try_rearm`](Self::try_rearm): hold
        /// for another interval from `now`, then try again.
        pub(crate) fn hold_again(&mut self, now: Duration) {
            if self.phase == Phase::Armed {
                self.phase = Phase::Holding { since: now };
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::time::Duration;

        use super::{HOLD_INTERVAL, MemoryWatch};

        const START: Duration = Duration::from_secs(1000);

        fn a_signal_owes_one_report_that_the_owner_takes() {
            let mut watch = MemoryWatch::armed();
            assert!(watch.on_signal(), "the first signal wakes the owner");
            assert!(watch.take_report(START));
            assert!(!watch.take_report(START), "a report is delivered once");
        }

        fn signals_while_a_report_is_owed_join_it() {
            let mut watch = MemoryWatch::armed();
            assert!(watch.on_signal());
            for _ in 0..5 {
                assert!(!watch.on_signal(), "the owner is already woken");
            }
            assert!(watch.take_report(START));
            assert!(!watch.take_report(START), "five signals, one report");
        }

        fn a_signal_during_the_hold_is_the_same_episode() {
            let mut watch = MemoryWatch::armed();
            assert!(watch.on_signal());
            assert!(watch.take_report(START));
            // The pool thread had a callback in flight when the owner took the report.
            assert!(!watch.on_signal());
            assert!(!watch.take_report(START));
            assert!(
                !watch.try_rearm(START + HOLD_INTERVAL.saturating_sub(Duration::from_millis(1))),
                "armed too early"
            );
        }

        fn the_wait_is_registered_again_only_after_the_hold() {
            let mut watch = MemoryWatch::armed();
            assert!(watch.on_signal());
            assert!(watch.take_report(START));
            assert!(!watch.try_rearm(START));
            assert!(watch.try_rearm(START + HOLD_INTERVAL));
            assert!(!watch.try_rearm(START + HOLD_INTERVAL * 2), "once per hold");
            // Armed again: a new episode is a new report.
            assert!(watch.on_signal());
            assert!(watch.take_report(START + HOLD_INTERVAL * 2));
        }

        fn the_owner_taking_nothing_changes_nothing() {
            let mut watch = MemoryWatch::armed();
            assert!(!watch.take_report(START));
            assert!(!watch.try_rearm(START + HOLD_INTERVAL * 10));
            assert!(watch.on_signal(), "an armed watch is still armed");
        }

        fn a_wait_that_cannot_be_registered_is_tried_again_after_another_hold() {
            let mut watch = MemoryWatch::armed();
            assert!(watch.on_signal());
            assert!(watch.take_report(START));
            let later = START + HOLD_INTERVAL;
            assert!(watch.try_rearm(later));
            watch.hold_again(later);
            assert!(!watch.try_rearm(later), "not at once");
            assert!(watch.try_rearm(later + HOLD_INTERVAL), "after another hold");
            assert!(watch.on_signal(), "armed again");
        }

        fn a_clock_that_reads_earlier_counts_as_no_time_passed() {
            let mut watch = MemoryWatch::armed();
            assert!(watch.on_signal());
            assert!(watch.take_report(START));
            assert!(!watch.try_rearm(Duration::ZERO));
        }

        #[test]
        fn low_memory_reports() {
            crate::table_test::run_table(
                "low_memory_reports",
                &[
                    (
                        "a_signal_owes_one_report_that_the_owner_takes",
                        a_signal_owes_one_report_that_the_owner_takes as fn(),
                    ),
                    (
                        "signals_while_a_report_is_owed_join_it",
                        signals_while_a_report_is_owed_join_it as fn(),
                    ),
                    (
                        "a_signal_during_the_hold_is_the_same_episode",
                        a_signal_during_the_hold_is_the_same_episode as fn(),
                    ),
                    (
                        "the_wait_is_registered_again_only_after_the_hold",
                        the_wait_is_registered_again_only_after_the_hold as fn(),
                    ),
                    (
                        "the_owner_taking_nothing_changes_nothing",
                        the_owner_taking_nothing_changes_nothing as fn(),
                    ),
                    (
                        "a_wait_that_cannot_be_registered_is_tried_again_after_another_hold",
                        a_wait_that_cannot_be_registered_is_tried_again_after_another_hold
                            as fn(),
                    ),
                    (
                        "a_clock_that_reads_earlier_counts_as_no_time_passed",
                        a_clock_that_reads_earlier_counts_as_no_time_passed as fn(),
                    ),
                ],
            );
        }
    }
}

#[cfg(all(test, windows))]
mod tests;
