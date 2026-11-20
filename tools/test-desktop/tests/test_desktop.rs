//! A process `test-desktop` starts runs on the run's own desktop: a window it shows is listed
//! there and not on the desktop the user sees (ADR-0355).
//!
//! The test starts itself through the built `test-desktop` as a peer that shows a window and
//! waits; meanwhile it lists the windows of both desktops from this process.

#![cfg(windows)]
#![expect(
    unsafe_code,
    reason = "Win32 FFI; each call takes plain values or pointers to locals"
)]

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, DESKTOP_ENUMERATE,
    DESKTOP_READOBJECTS, EnumDesktopWindows, HDESK, OpenDesktopW, OpenInputDesktop,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetWindowTextW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
    WS_VISIBLE,
};
use windows::core::{BOOL, HSTRING, w};

/// Set on the peer: the title of the window it shows.
const PEER_TITLE: &str = "FLUI_TEST_DESKTOP_PEER_TITLE";
/// The line the peer prints once its window exists.
const READY: &str = "test-desktop peer ready";

/// The peer: shows a window with the title it was given, says so, and keeps it until its
/// standard input closes.
fn show_window_until_stdin_closes(title: &str) {
    // SAFETY: a window of the system's STATIC class with a NUL-terminated title, no parent.
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            &HSTRING::from(title),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            0,
            0,
            320,
            200,
            None,
            None,
            None,
            None,
        )
    }
    .expect("the peer's window opens");
    println!("{READY}");
    let mut rest = Vec::new();
    let _read = std::io::stdin().read_to_end(&mut rest);
    // SAFETY: the window was created on this thread above.
    unsafe { DestroyWindow(window) }.expect("the peer's window closes");
}

/// The titles of the top-level windows of `desktop`.
fn window_titles(desktop: HDESK) -> Vec<String> {
    unsafe extern "system" fn collect(window: HWND, titles: LPARAM) -> BOOL {
        // SAFETY: `titles` is the `Vec` `window_titles` passed, alive for the enumeration.
        let titles = unsafe { &mut *(titles.0 as *mut Vec<String>) };
        let mut text = [0u16; 256];
        // SAFETY: `text` is a writable buffer of its own length.
        let len = unsafe { GetWindowTextW(window, &mut text) };
        let len = usize::try_from(len).unwrap_or(0);
        titles.push(String::from_utf16_lossy(&text[..len]));
        BOOL::from(true)
    }
    let mut titles: Vec<String> = Vec::new();
    // SAFETY: the callback reads `titles` only during the call.
    let _listed = unsafe {
        EnumDesktopWindows(
            Some(desktop),
            Some(collect),
            LPARAM(&raw mut titles as isize),
        )
    };
    titles
}

#[test]
fn a_window_of_a_test_is_on_the_test_desktop_and_not_on_the_users() {
    if let Ok(title) = std::env::var(PEER_TITLE) {
        show_window_until_stdin_closes(&title);
        return;
    }
    let unique = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("BUG: the clock is past 1970")
            .as_nanos()
    );
    let title = format!("flui test-desktop peer {unique}");
    let run = format!("self-check-{unique}");
    let mut peer = Command::new(env!("CARGO_BIN_EXE_test-desktop"))
        .arg(std::env::current_exe().expect("this test's executable"))
        .args([
            "--exact",
            "a_window_of_a_test_is_on_the_test_desktop_and_not_on_the_users",
            "--nocapture",
        ])
        .env(PEER_TITLE, &title)
        .env("NEXTEST_RUN_ID", &run)
        .env_remove("FLUI_TEST_VISIBLE_DESKTOP")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("test-desktop starts the peer");
    let stdout = peer.stdout.take().expect("BUG: stdout is piped");
    let mut lines = BufReader::new(stdout).lines().map_while(Result::ok);
    let ready = lines.any(|line| line == READY);
    assert!(ready, "the peer never showed its window: {:?}", peer.wait());

    let desktop_name = HSTRING::from(format!("flui-test-{run}"));
    // SAFETY: a NUL-terminated name; the handle is closed below.
    let test_desktop = unsafe {
        OpenDesktopW(
            &desktop_name,
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_READOBJECTS.0 | DESKTOP_ENUMERATE.0,
        )
    }
    .expect("the run's desktop exists while its test runs");
    let on_the_test_desktop = window_titles(test_desktop);
    // SAFETY: opened above, closed once.
    let _closed = unsafe { CloseDesktop(test_desktop) };

    // SAFETY: the handle is closed below. A session without an input desktop (a service's)
    // shows no window to anyone.
    let on_the_users_desktop = unsafe {
        OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0 | DESKTOP_ENUMERATE.0),
        )
    }
    .map_or_else(
        |_| Vec::new(),
        |input| {
            let titles = window_titles(input);
            // SAFETY: opened above, closed once.
            let _closed = unsafe { CloseDesktop(input) };
            titles
        },
    );

    drop(peer.stdin.take());
    // The peer's harness reports its test after the window closes.
    lines.for_each(drop);
    let status = peer.wait().expect("the peer ends");
    assert!(status.success(), "the peer failed: {status}");
    assert!(
        on_the_test_desktop.contains(&title),
        "the peer's window is not on the run's desktop"
    );
    assert!(
        !on_the_users_desktop.contains(&title),
        "the peer's window is on the desktop the user sees"
    );
}
