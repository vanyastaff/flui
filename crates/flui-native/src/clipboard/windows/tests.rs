use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, WM_RENDERFORMAT};

use super::*;
use crate::clipboard::patience::PATIENCE;
use crate::testing::clipboard::{SettlingOwner, read_within};

/// Longer than any answer the clipboard thread owes a test.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);

/// The measure matches what the lossy decode writes, surrogate pairs and
/// an unpaired surrogate included.
#[test]
fn decoded_len_matches_the_lossy_decode() {
    for text in [
        &[0x61, 0x44a, 0x20ac][..],
        &[0xd83d, 0xde00],
        &[0xd800, 0x61],
        &[],
    ] {
        assert_eq!(decoded_len(text), String::from_utf16_lossy(text).len());
    }
}

/// Run `look` on the clipboard thread inside a session, and wait for it.
fn inspect<T: Send + 'static>(look: impl FnOnce() -> T + Send + 'static) -> T {
    let (sent, seen) = mpsc::channel();
    submit(Job::Inspect(Box::new(move || {
        let _ = sent.send(look());
    })));
    seen.recv_timeout(ANSWER_WITHIN)
        .expect("the clipboard thread opens the clipboard for a look")
}

/// Write `text` and wait for the write's answer.
fn write_and_wait(clipboard: &WindowsClipboard, text: &str) -> ClipboardWritten {
    let owner = SettlingOwner::new();
    let (mut completion, answer) = owner.pending::<ClipboardWritten>();
    clipboard.write(ClipboardWrite::text(text), answer);
    owner
        .settle_within(&mut completion, ANSWER_WITHIN)
        .expect("the write is answered in time")
        .expect("the write is answered, not abandoned")
}

/// Read and wait for the read's answer.
fn read_and_wait(clipboard: &WindowsClipboard) -> Result<Option<String>, ClipboardError> {
    let owner = SettlingOwner::new();
    let (mut completion, answer) = owner.pending::<ClipboardRead>();
    clipboard.read_text(answer);
    owned(
        owner
            .settle_within(&mut completion, ANSWER_WITHIN)
            .expect("the read is answered in time")
            .expect("the read is answered, not abandoned"),
    )
}

/// A read's answer with its text copied out, to compare.
fn owned(read: ClipboardRead) -> Result<Option<String>, ClipboardError> {
    read.map(|text| text.as_deref().map(str::to_owned))
}

/// Another window, as a clipboard manager's, holds the clipboard open for
/// `held` from a thread of its own: returns once it holds it, and the
/// thread, which closes it after `held`.
fn hold_clipboard(held: Duration) -> std::thread::JoinHandle<()> {
    let (opened, holding) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        // A `NULL` owner is another owner than the clipboard thread's window,
        // so Win32 refuses the clipboard thread while this holds it.
        let mut tries = 0;
        // SAFETY: plain FFI calls with no pointer arguments; the close runs
        // only after this thread's own open succeeded.
        while unsafe { OpenClipboard(None) }.is_err() {
            tries += 1;
            assert!(tries < 500, "the clipboard stayed held by another process");
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = opened.send(());
        std::thread::sleep(held);
        // SAFETY: as above.
        let _ = unsafe { CloseClipboard() };
    });
    holding
        .recv_timeout(ANSWER_WITHIN)
        .expect("the holder opens the clipboard");
    holder
}

/// The scenario every host's clipboard runs: a concealed write pastes back,
/// with the formats that keep it out of the history and the cloud
/// clipboard (ADR-0152 §6).
#[test]
fn a_concealed_write_stays_out_of_the_windows_history() {
    let clipboard = WindowsClipboard::new();
    assert_eq!(
        crate::testing::clipboard::a_concealed_write_stays_out_of_history(
            &clipboard,
            read_within(ANSWER_WITHIN),
        ),
        Ok(())
    );
    let missing = inspect(|| {
        CONCEALMENT_FORMATS
            .iter()
            .filter(|(name, _)| {
                // SAFETY: plain FFI calls; `name` is a static NUL-terminated
                // wide string, and the session holds the clipboard open.
                let available = unsafe {
                    let format = RegisterClipboardFormatW(*name);
                    IsClipboardFormatAvailable(format).is_ok()
                };
                !available
            })
            .map(|(_, label)| *label)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        missing,
        Vec::<&str>::new(),
        "every marker is on the clipboard"
    );
}

#[test]
fn a_null_owner_open_on_another_thread_fails_while_a_session_is_open() {
    let probe_opened = inspect(|| {
        std::thread::spawn(|| {
            // SAFETY: plain FFI calls with no pointer arguments. The close
            // runs only if the probe's own open succeeded.
            unsafe {
                let opened = OpenClipboard(None).is_ok();
                if opened {
                    let _ = CloseClipboard();
                }
                opened
            }
        })
        .join()
        .expect("probe thread panicked")
    });
    assert!(
        !probe_opened,
        "a NULL-owner OpenClipboard on another thread succeeded while a FLUI session held the clipboard"
    );
}

/// A write and a read made while another window holds the clipboard open
/// for a moment, as a clipboard manager does after every change, land once
/// it lets go, rather than being dropped and answering `Unavailable`.
#[test]
fn a_write_and_a_read_while_the_clipboard_is_held_for_a_moment_land_once_it_is_let_go() {
    let clipboard = WindowsClipboard::new();
    let holder = hold_clipboard(Duration::from_millis(100));
    assert_eq!(write_and_wait(&clipboard, "written while held"), Ok(()));
    holder.join().expect("the holder thread");

    let holder = hold_clipboard(Duration::from_millis(100));
    assert_eq!(
        read_and_wait(&clipboard),
        Ok(Some("written while held".to_owned()))
    );
    holder.join().expect("the holder thread");
}

/// A clipboard held past the request's patience refuses the write with
/// `Unavailable`, before the holder lets go, and leaves the clipboard as it
/// was.
#[test]
fn a_clipboard_held_past_the_patience_refuses_the_write() {
    let clipboard = WindowsClipboard::new();
    assert_eq!(write_and_wait(&clipboard, "before the hold"), Ok(()));
    let held = PATIENCE + Duration::from_millis(400);
    let started = Instant::now();
    let holder = hold_clipboard(held);
    assert_eq!(
        write_and_wait(&clipboard, "refused"),
        Err(ClipboardError::Unavailable)
    );
    assert!(
        started.elapsed() < held,
        "the refusal came while the clipboard was still held"
    );
    holder.join().expect("the holder thread");
    assert_eq!(
        read_and_wait(&clipboard),
        Ok(Some("before the hold".to_owned()))
    );
}

/// How long the renderer below takes to answer `WM_RENDERFORMAT`.
static RENDER_DELAY_MS: AtomicU64 = AtomicU64::new(0);

/// The text the renderer below renders.
const RENDERED: &str = "rendered late";

unsafe extern "system" fn late_renderer(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    os_entry(
        "a test's late clipboard renderer",
        (),
        AfterPanic::Answer(|()| LRESULT(0)),
        || {
            if message == WM_RENDERFORMAT {
                if !asked_by_this_process() {
                    // Another process's monitor (WSLg's `msrdc.exe` reads
                    // every change, whatever its markers): left unrendered,
                    // so the format is still rendered on demand when this
                    // process asks.
                    return LRESULT(0);
                }
                std::thread::sleep(Duration::from_millis(
                    RENDER_DELAY_MS.load(Ordering::SeqCst),
                ));
                let wide: Vec<u16> = RENDERED.encode_utf16().chain(std::iter::once(0)).collect();
                // SAFETY: `wide` is a live `Vec<u16>`, viewed as its bytes;
                // Win32 holds the clipboard open for the renderer while it
                // answers `WM_RENDERFORMAT`, which is what `set_data` asks.
                unsafe {
                    let bytes = std::slice::from_raw_parts(
                        wide.as_ptr().cast::<u8>(),
                        wide.len() * std::mem::size_of::<u16>(),
                    );
                    let _ = set_data(CF_UNICODETEXT.0 as u32, bytes);
                }
                return LRESULT(0);
            }
            // SAFETY: Win32's own arguments, forwarded unchanged.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        },
    )
}

/// Whether the window that holds the clipboard open, the one whose
/// `GetClipboardData` sent `WM_RENDERFORMAT`, is this process's.
fn asked_by_this_process() -> bool {
    // SAFETY: plain FFI calls; `pid` outlives the call that writes it.
    unsafe {
        let Ok(open) = windows::Win32::System::DataExchange::GetOpenClipboardWindow() else {
            return false;
        };
        let mut pid = 0_u32;
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(open, Some(&raw mut pid));
        pid == std::process::id()
    }
}

/// Another window, on a thread of its own, takes the clipboard with text it
/// renders only when asked, and answers `WM_RENDERFORMAT` after `delay`.
fn own_with_late_text(delay: Duration) {
    RENDER_DELAY_MS.store(
        u64::try_from(delay.as_millis()).expect("a test's delay fits"),
        Ordering::SeqCst,
    );
    let (owned, owning) = mpsc::channel();
    std::thread::spawn(move || {
        // SAFETY: the window is created on this thread, which pumps its
        // messages for the rest of the test process; `class` and its name
        // outlive the calls; the clipboard is opened with the window and
        // closed again on this thread.
        unsafe {
            let instance = GetModuleHandleW(None).expect("the module");
            let class = WNDCLASSW {
                lpfnWndProc: Some(late_renderer),
                hInstance: instance.into(),
                lpszClassName: w!("FLUITestLateRenderer"),
                ..Default::default()
            };
            RegisterClassW(&raw const class);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("FLUITestLateRenderer"),
                w!("late renderer"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .expect("the renderer's window");
            // The renderer has pumped before it owns the clipboard, as any
            // live application has: Win32 treats a thread that never took a
            // message as hung and fails the read at once.
            assert!(pump_messages());
            let mut tries = 0;
            while OpenClipboard(Some(hwnd)).is_err() {
                tries += 1;
                assert!(tries < 500, "the clipboard stayed held by another process");
                std::thread::sleep(Duration::from_millis(2));
            }
            EmptyClipboard().expect("the renderer empties the clipboard");
            // A NULL handle offers the format, rendered on demand; the
            // wrapper reports the NULL it returns as an error.
            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, None);
            // Kept from clipboard history and clipboard managers, which
            // would otherwise ask for the text first and hold the clipboard
            // open while it renders, past the read's patience.
            for (name, _) in CONCEALMENT_FORMATS {
                let format = RegisterClipboardFormatW(name);
                assert!(
                    set_data(format, &EXCLUDE.to_le_bytes()),
                    "the marker is set"
                );
            }
            CloseClipboard().expect("the renderer closes the clipboard");
            let _ = owned.send(());
            let mut message = MSG::default();
            while GetMessageW(&raw mut message, None, 0, 0).0 > 0 {
                DispatchMessageW(&raw const message);
            }
        }
    });
    owning
        .recv_timeout(ANSWER_WITHIN)
        .expect("the renderer owns the clipboard");
}

/// A read of text its owner renders late returns to the caller at once and
/// is answered when the owner renders, while the caller's thread turns; a
/// write made meanwhile waits behind it and lands after.
#[test]
fn a_read_of_text_rendered_late_never_holds_the_caller() {
    let delay = Duration::from_millis(1500);
    own_with_late_text(delay);
    let clipboard = WindowsClipboard::new();
    let owner = SettlingOwner::new();
    let (mut read, answer) = owner.pending::<ClipboardRead>();

    let started = Instant::now();
    clipboard.read_text(answer);
    let returned = started.elapsed();
    assert!(
        returned < Duration::from_millis(250),
        "the read returned to its caller after {returned:?}, not at once"
    );

    let (mut written, answer) = owner.pending::<ClipboardWritten>();
    clipboard.write(ClipboardWrite::text("after the render"), answer);

    let text = owner
        .settle_within(&mut read, ANSWER_WITHIN)
        .expect("the read is answered once the owner renders")
        .expect("the read is answered, not abandoned");
    assert_eq!(owned(text), Ok(Some(RENDERED.to_owned())));
    assert!(
        started.elapsed() >= delay,
        "the answer waited for the render"
    );
    assert_eq!(
        owner
            .settle_within(&mut written, ANSWER_WITHIN)
            .expect("the write is answered after the read")
            .expect("the write is answered, not abandoned"),
        Ok(())
    );
    assert_eq!(
        read_and_wait(&clipboard),
        Ok(Some("after the render".to_owned()))
    );
}

/// Every request a burst makes past what the thread holds waiting is
/// answered: taken or refused, none dropped.
#[test]
fn a_burst_of_clipboard_writes_is_answered_in_full() {
    let clipboard = WindowsClipboard::new();
    let owner = SettlingOwner::new();
    let holder = hold_clipboard(Duration::from_millis(50));
    let mut writes: Vec<_> = (0..MAX_WAITING * 2)
        .map(|index| {
            let (completion, answer) = owner.pending::<ClipboardWritten>();
            clipboard.write(ClipboardWrite::text(format!("burst {index}")), answer);
            completion
        })
        .collect();
    let refused = writes
        .iter_mut()
        .map(|write| {
            owner
                .settle_within(write, ANSWER_WITHIN)
                .expect("every write is answered")
                .expect("every write is answered, not abandoned")
        })
        .filter(|written| *written == Err(ClipboardError::Unavailable))
        .count();
    holder.join().expect("the holder thread");
    assert!(refused >= MAX_WAITING - 1, "{refused} refused");
}
