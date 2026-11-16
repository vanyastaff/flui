//! The Win32 clipboard against the real system clipboard.
//!
//! Every test that touches the clipboard takes [`ClipboardTest`]: it holds
//! this module's lock, so the tests run one at a time under plain
//! `cargo test`, where they share one process and one clipboard thread (the
//! nextest group `system-clipboard` serialises their processes), and it puts
//! back what the clipboard held when the test began, on every exit.
//!
//! Saved and restored are [`SAVED_FORMATS`] alone: the standard formats
//! whose data is plain global memory with no handle inside, which a copy of
//! the bytes reproduces whole (text, the DIB images, the file list, the
//! locale). Not saved: formats held as GDI handles (`CF_BITMAP`,
//! `CF_ENHMETAFILE`, `CF_METAFILEPICT`, `CF_PALETTE`); registered formats,
//! which may be rendered only on demand by an owner that takes up to thirty
//! seconds each, and include OLE's `DataObject` and `Ole Private Data`, whose
//! bytes name the original owner's objects and must never be set again by
//! anyone else; owner-display formats; and the original owner itself.
//!
//! The guard never panics: a save or a restore the clipboard thread does not
//! answer in time is reported on stderr, and the test's own outcome stands.
//!
//! Another process may hold the clipboard at any moment (this machine's
//! remote-desktop client reads every change): no assertion depends on its
//! absence beyond waiting up to [`ANSWER_WITHIN`] for it to let go.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, WM_RENDERFORMAT};

use super::*;
use crate::clipboard::patience::PATIENCE;
use crate::testing::clipboard::{SettlingOwner, read_within};

/// Longer than any answer the clipboard thread owes a test, another
/// process's hold of the clipboard included.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);

/// How long the guard waits for its save: each saved format may be one an
/// owner renders on demand, which Win32 waits thirty seconds for.
const SAVE_WITHIN: Duration = Duration::from_secs(5 * 31);

/// Serialises this module's tests within one process.
static SERIAL: Mutex<()> = Mutex::new(());

/// The formats the guard saves and restores: plain global memory with no
/// handle inside, reproduced whole by a copy of its bytes.
const SAVED_FORMATS: [u32; 5] = [
    13, // CF_UNICODETEXT
    8,  // CF_DIB
    17, // CF_DIBV5
    15, // CF_HDROP: a DROPFILES header and the paths, inline
    16, // CF_LOCALE: an LCID
];

/// One test's hold on the system clipboard: the module's lock, and what the
/// clipboard held, put back when it drops.
struct ClipboardTest {
    /// `None` when the save was not answered in time: nothing is restored.
    saved: Option<Vec<(u32, Vec<u8>)>>,
    _serial: MutexGuard<'static, ()>,
}

impl ClipboardTest {
    fn begin() -> Self {
        let serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
        // A save refused because another process held the clipboard past the
        // request's patience is asked again, within the test's own patience.
        let started = Instant::now();
        let saved = loop {
            let saved = try_inspect(SAVE_WITHIN, || {
                SAVED_FORMATS
                    .iter()
                    .filter(|format| {
                        // SAFETY: plain FFI call; the session holds the
                        // clipboard open on this thread.
                        unsafe { IsClipboardFormatAvailable(**format) }.is_ok()
                    })
                    .filter_map(|format| copy_global(*format).map(|bytes| (*format, bytes)))
                    .collect::<Vec<_>>()
            });
            if saved.is_some() || started.elapsed() >= ANSWER_WITHIN {
                break saved;
            }
        };
        if saved.is_none() {
            eprintln!("the clipboard was not saved in {SAVE_WITHIN:?}: it will not be restored");
        }
        Self {
            saved,
            _serial: serial,
        }
    }
}

impl Drop for ClipboardTest {
    fn drop(&mut self) {
        let Some(saved) = self.saved.take() else {
            return;
        };
        // Asked again while another process holds the clipboard, within the
        // test's own patience.
        let started = Instant::now();
        let restored = loop {
            let attempt = saved.clone();
            let restored = try_inspect(ANSWER_WITHIN, move || {
                // SAFETY: the session holds the clipboard open on this
                // thread; each format's memory is ours until `hand_over`
                // succeeds.
                unsafe {
                    if EmptyClipboard().is_err() {
                        return false;
                    }
                    attempt.into_iter().all(|(format, bytes)| {
                        Prepared::try_new(format, &bytes).is_ok_and(|prepared| prepared.hand_over())
                    })
                }
            });
            if restored.is_some() || started.elapsed() >= ANSWER_WITHIN {
                break restored;
            }
        };
        if restored != Some(true) {
            eprintln!("the clipboard was not restored: {restored:?}");
        }
    }
}

/// A copy of `format`'s global memory, if it holds any.
fn copy_global(format: u32) -> Option<Vec<u8>> {
    // SAFETY: called with the clipboard open on this thread. The handle is
    // checked before it is locked, the lock before it is read, and the copy
    // is bounded by the allocation's own size.
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        if size == 0 {
            return None;
        }
        let ptr = GlobalLock(global);
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(global);
        Some(bytes)
    }
}

/// Run `look` on the clipboard thread inside a session and wait up to
/// `within` for it: `None` when it did not answer in time.
fn try_inspect<T: Send + 'static>(
    within: Duration,
    look: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sent, seen) = mpsc::channel();
    submit(
        Task::Inspect(Box::new(move || {
            let _ = sent.send(look());
        })),
        Answer::Nobody,
    );
    seen.recv_timeout(within).ok()
}

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

/// Only an owner that did not render in time answers `Timeout`.
#[test]
fn only_an_owner_that_did_not_render_in_time_answers_timeout() {
    let timeout = windows::core::Error::from_hresult(ERROR_TIMEOUT.to_hresult());
    assert_eq!(read_failure(&timeout), ClipboardError::Timeout);
    let other = windows::core::Error::from_hresult(
        windows::Win32::Foundation::ERROR_ACCESS_DENIED.to_hresult(),
    );
    assert_eq!(read_failure(&other), ClipboardError::Unavailable);
}

/// Run `look` on the clipboard thread inside a session, and wait for it.
fn inspect<T: Send + 'static>(look: impl FnOnce() -> T + Send + 'static) -> T {
    try_inspect(ANSWER_WITHIN, look).expect("the clipboard thread opens the clipboard for a look")
}

/// Write `text` and wait up to `within` for the write's answer.
fn write_within(clipboard: &WindowsClipboard, text: &str, within: Duration) -> ClipboardWritten {
    let owner = SettlingOwner::new();
    let (mut completion, answer) = owner.pending::<ClipboardWritten>();
    clipboard.write(ClipboardWrite::text(text), answer);
    owner
        .settle_within(&mut completion, within)
        .expect("the write is answered in time")
        .expect("the write is answered, not abandoned")
}

/// Write `text` and wait for the write's answer.
fn write_and_wait(clipboard: &WindowsClipboard, text: &str) -> ClipboardWritten {
    write_within(clipboard, text, ANSWER_WITHIN)
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
/// thread, which closes it after `held`. Another process holding the
/// clipboard first is waited for, up to [`ANSWER_WITHIN`].
fn hold_clipboard(held: Duration) -> std::thread::JoinHandle<()> {
    let (opened, holding) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        let deadline = Instant::now() + ANSWER_WITHIN;
        // A `NULL` owner is another owner than the clipboard thread's window,
        // so Win32 refuses the clipboard thread while this holds it.
        // SAFETY: plain FFI calls with no pointer arguments; the close runs
        // only after this thread's own open succeeded.
        while unsafe { OpenClipboard(None) }.is_err() {
            assert!(
                Instant::now() < deadline,
                "another process held the clipboard for {ANSWER_WITHIN:?}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let _ = opened.send(());
        std::thread::sleep(held);
        // SAFETY: as above.
        let _ = unsafe { CloseClipboard() };
    });
    holding
        .recv_timeout(ANSWER_WITHIN + Duration::from_secs(1))
        .expect("the holder opens the clipboard");
    holder
}

/// The scenario every host's clipboard runs: a concealed write pastes back,
/// with the formats that keep it out of the history and the cloud
/// clipboard (ADR-0152 §6).
#[test]
fn a_concealed_write_stays_out_of_the_windows_history() {
    let _test = ClipboardTest::begin();
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
    let _test = ClipboardTest::begin();
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
    let _test = ClipboardTest::begin();
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
    let _test = ClipboardTest::begin();
    let clipboard = WindowsClipboard::new();
    assert_eq!(write_and_wait(&clipboard, "before the hold"), Ok(()));
    let held = PATIENCE + Duration::from_millis(400);
    let holder = hold_clipboard(held);
    let started = Instant::now();
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

/// A write whose memory cannot be made is refused before the clipboard is
/// emptied, so the clipboard keeps what it held.
#[test]
fn a_write_that_cannot_be_made_leaves_the_clipboard_as_it_was() {
    let _test = ClipboardTest::begin();
    let clipboard = WindowsClipboard::new();
    assert_eq!(write_and_wait(&clipboard, "before"), Ok(()));
    FAIL_NEXT_PREPARE.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        write_and_wait(&clipboard, "never made"),
        Err(ClipboardError::Unavailable)
    );
    assert_eq!(read_and_wait(&clipboard), Ok(Some("before".to_owned())));
}

thread_local! {
    /// A subscriber that panics at every event, installed on the clipboard
    /// thread for one test.
    static PANICKING: RefCell<Option<tracing::dispatcher::DefaultGuard>> =
        const { RefCell::new(None) };
}

/// A `tracing` subscriber that panics at every event, as an application's
/// faulty one does.
struct PanickingSubscriber;

impl tracing::Subscriber for PanickingSubscriber {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, _: &tracing::Event<'_>) {
        panic!("a subscriber's own failure");
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A subscriber that panics at the warning of a refused write leaves the
/// clipboard thread serving: the write answers its typed refusal, and the
/// next write and read, once the clipboard is free, land.
#[test]
fn a_panicking_subscriber_leaves_the_clipboard_thread_serving() {
    let _test = ClipboardTest::begin();
    let clipboard = WindowsClipboard::new();
    inspect(|| {
        let guard = tracing::dispatcher::set_default(&tracing::Dispatch::new(PanickingSubscriber));
        PANICKING.with(|slot| *slot.borrow_mut() = Some(guard));
    });
    let holder = hold_clipboard(PATIENCE + Duration::from_millis(400));
    let refused = write_and_wait(&clipboard, "refused");
    holder.join().expect("the holder thread");
    let taken = write_and_wait(&clipboard, "after the panic");
    let read = read_and_wait(&clipboard);
    inspect(|| PANICKING.with(|slot| drop(slot.borrow_mut().take())));
    assert_eq!(refused, Err(ClipboardError::Unavailable));
    assert_eq!(taken, Ok(()));
    assert_eq!(read, Ok(Some("after the panic".to_owned())));
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
                let bytes: Vec<u8> = wide.iter().flat_map(|unit| unit.to_le_bytes()).collect();
                if let Ok(prepared) = Prepared::try_new(CF_UNICODETEXT.0 as u32, &bytes) {
                    // SAFETY: Win32 holds the clipboard open for the renderer
                    // while it answers `WM_RENDERFORMAT`, which is what
                    // `hand_over` asks.
                    let _ = unsafe { prepared.hand_over() };
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
            let deadline = Instant::now() + ANSWER_WITHIN;
            while OpenClipboard(Some(hwnd)).is_err() {
                assert!(
                    Instant::now() < deadline,
                    "another process held the clipboard for {ANSWER_WITHIN:?}"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            EmptyClipboard().expect("the renderer empties the clipboard");
            // A NULL handle offers the format, rendered on demand; the
            // wrapper reports the NULL it returns as an error.
            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, None);
            CloseClipboard().expect("the renderer closes the clipboard");
            let _ = owned.send(());
            let mut message = MSG::default();
            while GetMessageW(&raw mut message, None, 0, 0).0 > 0 {
                DispatchMessageW(&raw const message);
            }
        }
    });
    owning
        .recv_timeout(ANSWER_WITHIN + Duration::from_secs(1))
        .expect("the renderer owns the clipboard");
}

/// A read of text its owner renders late returns to the caller at once and
/// is answered when the owner renders, while the caller's thread turns; a
/// write made meanwhile waits behind it and lands after.
#[test]
fn a_read_of_text_rendered_late_never_holds_the_caller() {
    let _test = ClipboardTest::begin();
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

/// How long Win32 waits for an owner's `WM_RENDERFORMAT` answer before
/// `GetClipboardData` fails with `ERROR_TIMEOUT`, as measured on Windows 11.
const WIN32_RENDER_WAIT: Duration = Duration::from_secs(30);

/// A read of text whose owner never renders in time answers `Timeout` after
/// Win32 gives up, without ever holding the caller, and the write behind it
/// is answered once the owner wakes. Takes Win32's thirty seconds.
#[test]
fn a_read_of_text_never_rendered_in_time_answers_timeout() {
    let _test = ClipboardTest::begin();
    let hung_for = WIN32_RENDER_WAIT + Duration::from_secs(3);
    own_with_late_text(hung_for);
    let clipboard = WindowsClipboard::new();
    let owner = SettlingOwner::new();
    let (mut read, answer) = owner.pending::<ClipboardRead>();

    let started = Instant::now();
    clipboard.read_text(answer);
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "the read returned to its caller at once"
    );
    let (mut written, answer) = owner.pending::<ClipboardWritten>();
    clipboard.write(ClipboardWrite::text("after the timeout"), answer);

    let within = hung_for + ANSWER_WITHIN;
    let text = owner
        .settle_within(&mut read, within)
        .expect("the read is answered once Win32 gives up")
        .expect("the read is answered, not abandoned");
    assert_eq!(owned(text), Err(ClipboardError::Timeout));
    assert_eq!(
        owner
            .settle_within(&mut written, within)
            .expect("the write behind the read is answered")
            .expect("the write is answered, not abandoned"),
        Ok(())
    );
}

/// Every request a burst makes past what the thread holds waiting is
/// answered: taken or refused, none dropped.
#[test]
fn a_burst_of_clipboard_writes_is_answered_in_full() {
    let _test = ClipboardTest::begin();
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
