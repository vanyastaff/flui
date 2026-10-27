//! The system events the Win32 platform's hidden receiver forwards to the
//! host: the session ending, sleep and wake, the user's session locking and
//! unlocking. The receiver is found by its title and sent the real messages the
//! system sends.

#![cfg(target_os = "windows")]
#![expect(
    unsafe_code,
    reason = "native Win32 calls against the real window system"
)]

use flui_testing::window_commands::AdmitAndApply as _;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::app::{AppCall, AppLog};
use flui_native::{AppHandler, OwnerPlatform, Platform as _, WindowsPlatform};
use flui_platform::{SessionEndAnswer, SessionEndPhase, SystemNotice};
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

/// A running platform whose application handler answers a session end with
/// `answer` and records what it hears, and its receiver.
struct Hosted {
    owner: OwnerPlatform,
    receiver: HWND,
    log: Rc<AppLog>,
}

/// The case waiting for the owner turn it runs in.
type Case = Box<dyn FnOnce(&Hosted)>;

thread_local! {
    /// The running case's host and the case, handed from the bootstrap to the
    /// owner turn on the same thread.
    static RUNNING: RefCell<Option<(Hosted, Case)>> = const { RefCell::new(None) };
}

/// Run a Win32 platform whose handler answers a session end with `answer`,
/// and run `case` in an owner turn of its loop, where what the receiver
/// reports reaches the handler at once; the loop then quits.
fn hosted(
    answer: impl Fn(SessionEndPhase) -> SessionEndAnswer + 'static,
    case: impl FnOnce(&Hosted) + 'static,
) {
    let log = AppLog::new();
    log.answer_session_end_with(answer);
    let kept = Rc::downgrade(&log);
    log.bootstrap(move |owner| {
        let proxy = owner.proxy();
        let quit = owner.proxy();
        owner.on_wake(Box::new(move || {
            if let Some((host, case)) = RUNNING.with(|running| running.borrow_mut().take()) {
                case(&host);
                drop(host);
                quit.request_quit().expect("the loop takes a quit");
            }
        }))?;
        let host = Hosted {
            owner,
            receiver: receiver_of_this_thread(),
            log: kept.upgrade().expect("the log runs its bootstrap"),
        };
        RUNNING.with(|running| *running.borrow_mut() = Some((host, Box::new(case))));
        proxy.wake().map_err(|error| error.to_string())?;
        Ok(())
    });
    Box::new(WindowsPlatform::new().expect("native Windows platform"))
        .run(log as Rc<dyn AppHandler>)
        .expect("the Win32 loop");
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
        reason_held_for(self.receiver)
    }

    fn phases(&self) -> Vec<SessionEndPhase> {
        self.log
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                AppCall::SessionEnd(phase) => Some(phase),
                _ => None,
            })
            .collect()
    }

    fn notices(&self) -> Vec<SystemNotice> {
        self.log
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                AppCall::Notice(notice) => Some(notice),
                _ => None,
            })
            .collect()
    }
}

const FALSE: isize = 0;
const TRUE: isize = 1;

fn a_blocked_query_refuses_and_gives_the_system_a_reason() {
    hosted(|_| SessionEndAnswer::Block, |host| {
    assert_eq!(
        host.send(WM_QUERYENDSESSION, 0, ENDSESSION_LOGOFF as isize),
        FALSE
    );
    assert_eq!(host.phases(), [SessionEndPhase::Query]);
    let reason = host.block_reason().expect("the system holds a reason");
    assert!(!reason.is_empty(), "a blank reason shows the user nothing");

    // The end was cancelled (the system sends this after a refusal too): the
    // reason stays until the host lets go, because its work is not done.
    host.send(WM_ENDSESSION, 0, 0);
    assert_eq!(
        host.phases(),
        [SessionEndPhase::Query, SessionEndPhase::Cancelled]
    );
    assert!(host.block_reason().is_some());
    host.owner.release_session_end();
    assert_eq!(host.block_reason(), None);
    });
}

fn releasing_the_block_withdraws_the_reason() {
    hosted(|_| SessionEndAnswer::Block, |host| {
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), FALSE);
    assert!(host.block_reason().is_some());
    host.owner.release_session_end();
    assert_eq!(host.block_reason(), None, "the session may go on");
    });
}

fn a_query_the_host_lets_through_is_allowed() {
    hosted(|_| SessionEndAnswer::Proceed, |host| {
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), TRUE);
    assert_eq!(host.block_reason(), None);
    });
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
    hosted(|_| SessionEndAnswer::Block, |host| {
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
    });
}

fn a_session_that_ends_reaches_the_hook() {
    hosted(|_| SessionEndAnswer::Proceed, |host| {
    host.send(WM_QUERYENDSESSION, 0, 0);
    host.send(WM_ENDSESSION, 1, ENDSESSION_LOGOFF as isize);
    assert_eq!(
        host.phases(),
        [SessionEndPhase::Query, SessionEndPhase::Ending]
    );
    });
}

fn a_panicking_hook_does_not_hold_the_session_up() {
    hosted(|phase| {
        assert_ne!(phase, SessionEndPhase::Query, "a broken subscriber");
        SessionEndAnswer::Proceed
    }, |host| {
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), TRUE);
    assert_eq!(host.block_reason(), None);
    host.send(WM_ENDSESSION, 1, 0);
    assert_eq!(
        host.phases(),
        [SessionEndPhase::Query, SessionEndPhase::Ending],
        "the hook is still installed after the panic"
    );
    });
}

fn sleep_and_wake_reach_the_host_once_each() {
    hosted(|_| SessionEndAnswer::Proceed, |host| {
    host.send(WM_POWERBROADCAST, PBT_APMSUSPEND as usize, 0);
    host.send(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC as usize, 0);
    // Sent after the automatic resume when the user caused it: not a second wake.
    host.send(WM_POWERBROADCAST, PBT_APMRESUMESUSPEND as usize, 0);
    assert_eq!(host.notices(), [SystemNotice::Sleep, SystemNotice::Wake]);
    });
}

fn locking_and_unlocking_the_session_reach_the_host() {
    hosted(|_| SessionEndAnswer::Proceed, |host| {
    host.send(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK as usize, 1);
    host.send(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK as usize, 1);
    // Remote-session changes are not lock changes.
    host.send(WM_WTSSESSION_CHANGE, 1, 1);
    assert_eq!(
        host.notices(),
        [SystemNotice::SessionLocked, SystemNotice::SessionUnlocked]
    );
    });
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
            "a_block_is_registered_on_the_visible_window",
            a_block_is_registered_on_the_visible_window,
        ),
        (
            "a_second_query_replaces_the_first_block",
            a_second_query_replaces_the_first_block,
        ),
        (
            "the_block_is_held_while_the_host_saves_for_an_ending_session",
            the_block_is_held_while_the_host_saves_for_an_ending_session,
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
        if String::from_utf16_lossy(&title[..length]) == "FLUI system broadcasts" {
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

/// The reason the system holds for `window`, if one is registered.
fn reason_held_for(window: HWND) -> Option<String> {
    let mut buffer = [0_u16; 300];
    let mut length = buffer.len() as u32;
    // SAFETY: `buffer` holds `length` units; the window is live.
    unsafe { ShutdownBlockReasonQuery(window, Some(PWSTR(buffer.as_mut_ptr())), &raw mut length) }
        .ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

fn a_block_is_registered_on_the_visible_window() {
    hosted(|_| SessionEndAnswer::Block, |host| {
    let window = host
        .owner
        .open_window(
            flui_platform::WindowOptions::default().with_visible(true),
            flui_testing::window_handler::RecordingWindowHandler::new(),
        )
        .expect("a window")
        .try_ready()
        .expect("the window opens at once");
    let window_hwnd = crate::win32_handle(window.as_ref());
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), FALSE);
    assert!(
        reason_held_for(window_hwnd).is_some(),
        "the system lists the window the user can see"
    );
    assert_eq!(host.block_reason(), None, "not the hidden receiver");
    host.owner.release_session_end();
    assert_eq!(reason_held_for(window_hwnd), None);
    // The window goes with the case: the next one must not find it visible.
    window.close_now();
    });
}

fn a_second_query_replaces_the_first_block() {
    hosted(|_| SessionEndAnswer::Block, |host| {
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), FALSE);
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), FALSE);
    host.owner.release_session_end();
    assert_eq!(host.block_reason(), None, "no reason is left behind");
    });
}

fn the_block_is_held_while_the_host_saves_for_an_ending_session() {
    use std::sync::atomic::{AtomicIsize, Ordering};

    let receiver = Arc::new(AtomicIsize::new(0));
    let held_at_ending = Arc::new(Mutex::new(None));
    let (seen_receiver, seen_held) = (Arc::clone(&receiver), Arc::clone(&held_at_ending));
    hosted(move |phase| {
        if phase == SessionEndPhase::Ending {
            let window = HWND(seen_receiver.load(Ordering::SeqCst) as *mut _);
            *seen_held.lock().expect("log") = Some(reason_held_for(window).is_some());
        }
        SessionEndAnswer::Block
    }, move |host| {
    receiver.store(host.receiver.0 as isize, Ordering::SeqCst);
    assert_eq!(host.send(WM_QUERYENDSESSION, 0, 0), FALSE);
    host.send(WM_ENDSESSION, 1, 0);
    assert_eq!(
        *held_at_ending.lock().expect("log"),
        Some(true),
        "the system waits for the hook; the reason goes after it"
    );
    assert_eq!(host.block_reason(), None);
    });
}
