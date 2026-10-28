//! The Win32 window's activation against another application, in a second
//! real process: Windows' foreground rules decide, the window never takes the
//! focus from the application the user works with, and asks for the user's
//! attention instead; once that application hands this process the right
//! (`AllowSetForegroundWindow`, as a second launch does before it hands its
//! arguments to the running instance), the activation brings it forward.
//!
//! The other application is this test binary started again as
//! [`foreground_peer`]: a plain Win32 window that takes the foreground the
//! way a user's click gives it, then answers the lines its starter writes.
//! Its windows appear on the desktop for the length of the case.

#![cfg(target_os = "windows")]
#![expect(
    unsafe_code,
    reason = "native Win32 calls against the real window system, in two processes"
)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use flui_native::{HostWindow, Platform as _, WindowsPlatform};
use flui_platform::{WindowCommand, WindowOptions};
use flui_testing::window_commands::AdmitAndApply as _;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, CreateWindowExW, DestroyWindow, DispatchMessageW,
    GetForegroundWindow, GetWindowThreadProcessId, HSHELL_HIGHBIT, HSHELL_REDRAW, MSG, PM_REMOVE,
    PeekMessageW, RegisterShellHookWindow, RegisterWindowMessageW, SW_SHOWNORMAL,
    SetForegroundWindow, ShowWindow, TranslateMessage, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows::core::w;

/// Set in the environment of the process [`foreground_peer`] runs in.
const PEER: &str = "FLUI_FOREGROUND_PEER";

/// The shell hook's code for a window that flashes: `HSHELL_FLASH`.
const HSHELL_FLASH: usize = (HSHELL_REDRAW | HSHELL_HIGHBIT) as usize;

/// Run the thread's window messages for `limit` or until `done` holds.
fn messages_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        let mut message = MSG::default();
        // SAFETY: `message` is a stack local out-parameter; the messages are
        // this thread's own.
        unsafe {
            while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

fn foreground() -> HWND {
    // SAFETY: the call reads no memory of the caller's and returns an opaque
    // handle, compared here and never dereferenced.
    unsafe { GetForegroundWindow() }
}

/// The other application, in a process of its own.
struct Peer {
    child: Child,
    input: ChildStdin,
    /// The peer's output, line by line, read on a thread of its own: this
    /// thread runs its window messages while it waits, as the peer's
    /// foreground calls wait on this thread's window when it is in front.
    lines: Receiver<String>,
    hwnd: HWND,
}

impl Peer {
    /// Start this test binary again as [`foreground_peer`], with its window
    /// shown.
    fn start() -> Self {
        let mut child = Command::new(std::env::current_exe().expect("the test binary's path"))
            .args([
                "--exact",
                "windows_foreground::foreground_peer",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(PEER, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the peer process starts");
        let input = child.stdin.take().expect("the peer's input");
        let output: ChildStdout = child.stdout.take().expect("the peer's output");
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { return };
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        let mut peer = Self {
            child,
            input,
            lines,
            hwnd: HWND::default(),
        };
        let ready = peer.answer("ready");
        let raw: isize = ready.parse().expect("the peer names its window");
        peer.hwnd = HWND(raw as *mut _);
        peer
    }

    /// What follows `@word` on the next line that has it, running this
    /// thread's window messages meanwhile; the harness's own lines are
    /// passed over.
    fn answer(&mut self, word: &str) -> String {
        let mut answer = None;
        let came = messages_until(Duration::from_secs(10), || {
            while answer.is_none() {
                match self.lines.try_recv() {
                    Ok(line) => match line.split_once(&format!("@{word}")) {
                        Some((_, rest)) => answer = Some(rest.trim().to_owned()),
                        None => eprintln!("peer: {line}"),
                    },
                    Err(TryRecvError::Empty) => return false,
                    Err(TryRecvError::Disconnected) => {
                        panic!("the peer ended before it answered {word}")
                    }
                }
            }
            true
        });
        assert!(came, "the peer did not answer {word}");
        answer.expect("BUG: an answer came")
    }

    /// Write `line` and read the answer that starts with its first word.
    fn ask(&mut self, line: &str) -> String {
        writeln!(self.input, "{line}").expect("the peer's input");
        self.input.flush().expect("the peer's input");
        let word = line.split_whitespace().next().expect("a command");
        self.answer(word)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "quit");
        let _ = self.input.flush();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
    }
}

/// An activation while another application has the foreground leaves the
/// foreground there and asks for attention (the window's taskbar button
/// flashes, which the other application hears through the shell hook); once
/// that application hands this process the right, the same command brings
/// the window forward with the focus.
#[test]
fn an_activation_takes_the_foreground_only_when_it_is_handed_over() {
    let platform = WindowsPlatform::new().expect("native Windows platform");
    let opened = crate::Opened::on(
        &platform,
        WindowOptions::default()
            .with_title("activated by a hand-over")
            .with_visible(true),
    )
    .expect("a Win32 window opens");
    let window: Rc<dyn HostWindow> = Rc::clone(&opened.window);
    let hwnd = crate::win32_handle(window.as_ref());
    let _ = messages_until(Duration::from_millis(150), || false);

    let mut peer = Peer::start();
    // Where this process is in front, it lets the other application take the
    // foreground as a click would: Windows then tells this one it lost it.
    // SAFETY: the call reads no memory of the caller's; it changes only which
    // process may set the foreground next.
    let _ = unsafe { AllowSetForegroundWindow(peer.child.id()) };
    assert_eq!(
        peer.ask("front"),
        "true",
        "the other application takes the foreground"
    );
    assert_eq!(foreground(), peer.hwnd, "the other application is in front");

    // Bringing "the application" forward touches no window of another one.
    platform.activate(true);
    let _ = messages_until(Duration::from_millis(150), || false);
    assert_eq!(
        foreground(),
        peer.hwnd,
        "Platform::activate took the foreground"
    );

    window
        .admit_and_apply(WindowCommand::Activate)
        .unwrap_or_else(|refusal| panic!("Activate is supported: {refusal:?}"));
    let _ = messages_until(Duration::from_millis(300), || false);
    assert_eq!(
        foreground(),
        peer.hwnd,
        "the activation took the foreground from the application in front"
    );
    let flashed = peer.ask("flashes");
    assert!(
        flashed
            .split(',')
            .any(|raw| raw.parse::<isize>().ok() == Some(hwnd.0 as isize)),
        "the window asked for attention: its taskbar button flashed (flashes: {flashed:?})"
    );

    // SAFETY: the call reads no memory of the caller's and cannot fail.
    let pid = unsafe { GetCurrentProcessId() };
    assert_eq!(
        peer.ask(&format!("allow {pid}")),
        "true",
        "the application in front hands this process the right"
    );
    window
        .admit_and_apply(WindowCommand::Activate)
        .unwrap_or_else(|refusal| panic!("Activate is supported: {refusal:?}"));
    assert!(
        messages_until(Duration::from_secs(2), || foreground() == hwnd),
        "the activation handed the right brought the window to the foreground"
    );
    drop(peer);
    window.close_for_cleanup();
}

/// The other application of the case above, run only in the process that
/// case starts ([`PEER`] set): a plain Win32 window, registered for the
/// shell's notices, which answers each line its starter writes.
///
/// - `front`: take the foreground the way a user's click gives it, then
///   press and release Shift, so this process received the last input and
///   the foreground lock holds against every other process. Answers whether
///   its window is in front.
/// - `flashes`: the windows whose flash the shell announced so far.
/// - `allow <pid>`: hand process `pid` the right to the foreground.
/// - `quit`.
#[test]
#[ignore = "the other application of the foreground case, which starts it"]
fn foreground_peer() {
    if std::env::var_os(PEER).is_none() {
        return;
    }
    let (lines, commands) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { return };
            if lines.send(line).is_err() {
                return;
            }
        }
    });
    // SAFETY: a top-level window of the system's static class, owned by this
    // thread, which pumps its messages below and destroys it at the end.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            w!("STATIC"),
            w!("flui foreground peer"),
            WS_OVERLAPPEDWINDOW,
            200,
            200,
            320,
            200,
            None,
            None,
            None,
            None,
        )
    }
    .expect("the peer's window");
    // SAFETY: `hwnd` is the live top-level window this thread made, so the
    // shell may post to it; the message name is a static NUL-terminated
    // string.
    let shell_hook = unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        assert!(RegisterShellHookWindow(hwnd).as_bool(), "the shell hook");
        RegisterWindowMessageW(w!("SHELLHOOK"))
    };
    // Each answer starts with `@`, as the harness may have begun the line.
    let say = |line: String| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "@{line}");
        let _ = out.flush();
    };
    say(format!("ready {}", hwnd.0 as isize));
    let mut flashes: Vec<isize> = Vec::new();
    loop {
        let mut message = MSG::default();
        // SAFETY: `message` is a stack local out-parameter; the messages are
        // this thread's own.
        unsafe {
            while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                if message.message == shell_hook && message.wParam.0 == HSHELL_FLASH {
                    flashes.push(message.lParam.0);
                }
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        let Ok(line) = commands.recv_timeout(Duration::from_millis(5)) else {
            continue;
        };
        let mut words = line.split_whitespace();
        match words.next() {
            Some("front") => say(format!("front {}", take_foreground(hwnd))),
            Some("flashes") => say(format!(
                "flashes {}",
                flashes
                    .iter()
                    .map(isize::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )),
            Some("allow") => {
                let pid: u32 = words
                    .next()
                    .and_then(|pid| pid.parse().ok())
                    .expect("allow names a process");
                // SAFETY: the call reads no memory of the caller's; it changes
                // only which process may set the foreground next.
                let allowed = unsafe { AllowSetForegroundWindow(pid) }.is_ok();
                say(format!("allow {allowed}"));
            }
            _ => break,
        }
    }
    // SAFETY: `hwnd` is the window this thread made and still owns; it is
    // destroyed once, on its own thread, and not used after.
    let _ = unsafe { DestroyWindow(hwnd) };
}

/// Bring `hwnd` to the foreground as a user's click would, from a process
/// the foreground rules do not favour: join the input of the thread in front
/// for the call, then press and release Shift, which reaches this window and
/// makes this process the one that received the last input. Whether `hwnd`
/// is in front.
fn take_foreground(hwnd: HWND) -> bool {
    // SAFETY: plain calls on this thread's own window and the thread in
    // front, whose input is joined only for the one call and let go after;
    // the input array is a stack local of the size named.
    unsafe {
        // Handed the right by the process in front (the starter, when the
        // test's window is in front), the call is the click's own. Without
        // it, joining the input of the thread in front lets it through; a
        // joined queue tells the thread left behind nothing of losing the
        // foreground, so this is only the fallback.
        if !SetForegroundWindow(hwnd).as_bool() {
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(GetForegroundWindow(), None);
            let joined =
                theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
            let _ = SetForegroundWindow(hwnd);
            if joined {
                let _ = AttachThreadInput(ours, theirs, false);
            }
        }
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_SHIFT,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let tap = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        let _ = SendInput(&tap, std::mem::size_of::<INPUT>() as i32);
        let _ = messages_until(Duration::from_millis(100), || false);
        GetForegroundWindow() == hwnd
    }
}
