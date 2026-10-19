//! The system events the Win32 platform's hidden receiver forwards to the
//! host: the session ending, sleep and wake, the user's session locking and
//! unlocking. The receiver is found by its title and sent the real messages the
//! system sends.

#![cfg(target_os = "windows")]
#![expect(
    unsafe_code,
    reason = "native Win32 calls against the real window system"
)]

use std::sync::{Arc, Mutex};

use flui_platform::{
    Platform as _, SessionEndAnswer, SessionEndPhase, SystemNotice, WindowsPlatform,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::Shutdown::ShutdownBlockReasonQuery,
    System::Threading::GetCurrentThreadId,
    UI::WindowsAndMessaging::{
        ENDSESSION_CRITICAL, ENDSESSION_LOGOFF, EnumThreadWindows, GetWindowTextW,
        PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, SendMessageW, WM_ENDSESSION,
        WM_POWERBROADCAST, WM_QUERYENDSESSION, WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK,
        WTS_SESSION_UNLOCK,
    },
};
use windows::core::{BOOL, PWSTR};

/// A platform with a session-end hook that records its phases and answers with
/// `answer`, and a notice hook that records what it hears.
struct Hosted {
    platform: WindowsPlatform,
    receiver: HWND,
    phases: Arc<Mutex<Vec<SessionEndPhase>>>,
    notices: Arc<Mutex<Vec<SystemNotice>>>,
}

fn hosted(answer: impl Fn(SessionEndPhase) -> SessionEndAnswer + Send + 'static) -> Hosted {
    let platform = WindowsPlatform::new().expect("native Windows platform");
    let phases = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&phases);
    platform.on_session_end(Box::new(move |phase| {
        recorded.lock().expect("phase log").push(phase);
        answer(phase)
    }));
    let notices = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&notices);
    platform.on_host_notice(Box::new(move |notice| {
        heard.lock().expect("notice log").push(notice);
    }));
    let receiver = receiver_of_this_thread();
    Hosted {
        platform,
        receiver,
        phases,
        notices,
    }
}

impl Hosted {
    fn send(&self, message: u32, wparam: usize, lparam: isize) -> isize {
        // SAFETY: a pointer-free message to this platform's own receiver, sent
        // from its owner thread.
        unsafe {
            SendMessageW(
                self.receiver,
                message,
                Some(WPARAM(wparam)),
                Some(LPARAM(lparam)),
            )
        }
        .0
    }

    /// The reason the system holds for the receiver, if one is registered.
    fn block_reason(&self) -> Option<String> {
        let mut buffer = [0_u16; 300];
        let mut length = buffer.len() as u32;
        // SAFETY: `buffer` holds `length` units; the receiver is live.
        unsafe {
            ShutdownBlockReasonQuery(
                self.receiver,
                Some(PWSTR(buffer.as_mut_ptr())),
                &raw mut length,
            )
        }
        .ok()?;
        Some(String::from_utf16_lossy(&buffer[..length as usize]))
    }

    fn phases(&self) -> Vec<SessionEndPhase> {
        self.phases.lock().expect("phase log").clone()
    }

    fn notices(&self) -> Vec<SystemNotice> {
        self.notices.lock().expect("notice log").clone()
    }
}

const FALSE: isize = 0;
const TRUE: isize = 1;

fn a_blocked_query_refuses_and_gives_the_system_a_reason() {
    let host = hosted(|_| SessionEndAnswer::Block);
    assert_eq!(
        host.send(WM_QUERYENDSESSION, 0, ENDSESSION_LOGOFF as isize),
        FALSE
    );
    assert_eq!(host.phases(), [SessionEndPhase::Query]);
    let reason = host.block_reason().expect("the system holds a reason");
    assert!(!reason.is_empty(), "a blank reason shows the user nothing");

    // Another application cancelled: the reason goes with the block.
    host.send(WM_ENDSESSION, 0, 0);
    assert_eq!(
        host.phases(),
        [SessionEndPhase::Query, SessionEndPhase::Cancelled]
    );
    assert_eq!(host.block_reason(), None);
}

fn releasing_the_block_withdraws_the_reason() {
    let host = hosted(|_| SessionEndAnswer::Block);
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), FALSE);
    assert!(host.block_reason().is_some());
    host.platform.release_session_end();
    assert_eq!(host.block_reason(), None, "the session may go on");
}

fn a_query_the_host_lets_through_is_allowed() {
    let host = hosted(|_| SessionEndAnswer::Proceed);
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), TRUE);
    assert_eq!(host.block_reason(), None);
}

fn without_a_hook_a_query_is_allowed() {
    let platform = WindowsPlatform::new().expect("native Windows platform");
    let receiver = receiver_of_this_thread();
    // SAFETY: a pointer-free message to this platform's own receiver.
    let result = unsafe { SendMessageW(receiver, WM_QUERYENDSESSION, None, None) };
    assert_eq!(result.0, TRUE);
    drop(platform);
}

fn a_critical_session_end_cannot_be_blocked() {
    let host = hosted(|_| SessionEndAnswer::Block);
    assert_eq!(
        host.send(
            WM_QUERYENDSESSION,
            0,
            (ENDSESSION_CRITICAL | ENDSESSION_LOGOFF) as isize
        ),
        TRUE
    );
    assert_eq!(host.block_reason(), None);
    assert_eq!(host.phases(), [SessionEndPhase::Query], "it is still asked");
}

fn a_session_that_ends_reaches_the_hook() {
    let host = hosted(|_| SessionEndAnswer::Proceed);
    host.send(WM_QUERYENDSESSION, 0, 0);
    host.send(WM_ENDSESSION, 1, ENDSESSION_LOGOFF as isize);
    assert_eq!(
        host.phases(),
        [SessionEndPhase::Query, SessionEndPhase::Ending]
    );
}

fn a_panicking_hook_does_not_hold_the_session_up() {
    let host = hosted(|phase| {
        assert_ne!(phase, SessionEndPhase::Query, "a broken subscriber");
        SessionEndAnswer::Proceed
    });
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), TRUE);
    assert_eq!(host.block_reason(), None);
    host.send(WM_ENDSESSION, 1, 0);
    assert_eq!(
        host.phases(),
        [SessionEndPhase::Query, SessionEndPhase::Ending],
        "the hook is still installed after the panic"
    );
}

fn sleep_and_wake_reach_the_host_once_each() {
    let host = hosted(|_| SessionEndAnswer::Proceed);
    host.send(WM_POWERBROADCAST, PBT_APMSUSPEND as usize, 0);
    host.send(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC as usize, 0);
    // Sent after the automatic resume when the user caused it: not a second wake.
    host.send(WM_POWERBROADCAST, PBT_APMRESUMESUSPEND as usize, 0);
    assert_eq!(host.notices(), [SystemNotice::Sleep, SystemNotice::Wake]);
}

fn locking_and_unlocking_the_session_reach_the_host() {
    let host = hosted(|_| SessionEndAnswer::Proceed);
    host.send(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 1);
    host.send(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK as usize, 1);
    // Remote-session changes are not lock changes.
    host.send(WM_WTSSESSION_CHANGE, 1, 1);
    assert_eq!(
        host.notices(),
        [SystemNotice::SessionLocked, SystemNotice::SessionUnlocked]
    );
}

#[test]
fn native_host_events() {
    let cases: &[(&str, fn())] = &[
        (
            "a_blocked_query_refuses_and_gives_the_system_a_reason",
            a_blocked_query_refuses_and_gives_the_system_a_reason,
        ),
        (
            "releasing_the_block_withdraws_the_reason",
            releasing_the_block_withdraws_the_reason,
        ),
        (
            "a_query_the_host_lets_through_is_allowed",
            a_query_the_host_lets_through_is_allowed,
        ),
        (
            "without_a_hook_a_query_is_allowed",
            without_a_hook_a_query_is_allowed,
        ),
        (
            "a_critical_session_end_cannot_be_blocked",
            a_critical_session_end_cannot_be_blocked,
        ),
        (
            "a_session_that_ends_reaches_the_hook",
            a_session_that_ends_reaches_the_hook,
        ),
        (
            "a_panicking_hook_does_not_hold_the_session_up",
            a_panicking_hook_does_not_hold_the_session_up,
        ),
        (
            "sleep_and_wake_reach_the_host_once_each",
            sleep_and_wake_reach_the_host_once_each,
        ),
        (
            "locking_and_unlocking_the_session_reach_the_host",
            locking_and_unlocking_the_session_reach_the_host,
        ),
    ];
    crate::run_table("native_host_events", cases);
}

/// The platform's hidden receiver: the window of this thread titled so. Found
/// among this thread's windows, not system-wide, so another test process's
/// platform is never mistaken for it.
fn receiver_of_this_thread() -> HWND {
    unsafe extern "system" fn titled(window: HWND, found: LPARAM) -> BOOL {
        let mut title = [0_u16; 64];
        // SAFETY: the buffer is a live stack array; `found` is the address of
        // the `Option<HWND>` below, alive for the whole enumeration.
        let (length, found) = unsafe {
            (
                GetWindowTextW(window, &mut title),
                &mut *(found.0 as *mut Option<HWND>),
            )
        };
        let length = usize::try_from(length).unwrap_or(0);
        if String::from_utf16_lossy(&title[..length]) == "FLUI system preferences" {
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
            Some(titled),
            LPARAM((&raw mut found) as isize),
        );
    }
    found.expect("the platform's hidden receiver")
}
