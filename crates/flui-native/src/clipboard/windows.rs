//! The Win32 clipboard, served by a thread of its own (ADR-0336).
//!
//! Every read and write is a request to the process's clipboard thread,
//! which opens the clipboard with its own message-only window, serves the
//! requests one at a time in the order they were made, and answers each
//! through its resolver. The owner thread only queues: it never waits for
//! another process.
//!
//! Two waits are another process's to end, and both happen on the clipboard
//! thread. `OpenClipboard` fails while another window holds the clipboard
//! open, as clipboard managers and remote-desktop clients do right after
//! every change; the request tries again on the schedule of
//! `clipboard::patience`, pumping the thread's messages between
//! attempts, and is refused with [`ClipboardError::Unavailable`] when its
//! patience runs out. `GetClipboardData` of a format its owner renders only
//! when asked sends that owner `WM_RENDERFORMAT` and waits for the answer:
//! measured on Windows 11, for as long as the owner takes, and thirty seconds
//! for an owner that never answers, after which the read fails with
//! `ERROR_TIMEOUT` and answers [`ClipboardError::Timeout`].

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{OnceLock, mpsc};
use std::time::Instant;

use flui_foundation::panic::{FirstPanic, discard_payload};
use flui_platform::capability::Resolver;
use flui_platform::{ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment};

use windows::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, ERROR_TIMEOUT, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM,
            LRESULT, WPARAM,
        },
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
                OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
            },
            LibraryLoader::GetModuleHandleW,
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::CF_UNICODETEXT,
            Threading::{CreateEventW, INFINITE, SetEvent},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, HWND_MESSAGE, MSG,
            MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
            RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_QUIT, WNDCLASSW,
        },
    },
    core::w,
};

use crate::clipboard::patience::OpenAttempts;
use crate::host::{NativeClipboard, admit_text, host_text, text_fits};
use crate::owner::panic_boundary::{AfterPanic, os_entry};

/// The Win32 clipboard: a handle to the process's clipboard thread.
///
/// Stateless: the clipboard is a process-wide resource, so every instance,
/// on any thread, queues to the one thread that opens it.
#[derive(Debug)]
#[non_exhaustive]
pub struct WindowsClipboard;

impl WindowsClipboard {
    /// A handle to the process's clipboard; the clipboard thread starts with
    /// the first request.
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::host::Clipboard<crate::owner::windows::WindowsLoop> for WindowsClipboard {
    fn clipboard(_: &crate::owner::windows::WindowsLoop) -> std::rc::Rc<dyn NativeClipboard> {
        std::rc::Rc::new(WindowsClipboard::new())
    }
}

impl NativeClipboard for WindowsClipboard {
    /// Queued to the clipboard thread; answered from there.
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        submit(Job::Read(answer));
    }

    /// Queued to the clipboard thread behind every request made before it,
    /// so a read made after it answers what it wrote.
    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        submit(Job::Write(write, answer));
    }

    fn concealment(&self) -> Concealment {
        Concealment::Honored
    }
}

/// The most requests waiting for the clipboard thread at once. The handles
/// of a UI runtime share one read in flight, so only writes and many
/// runtimes reach it; past it a request is refused, not queued.
const MAX_WAITING: usize = 64;

/// What one request asks of the clipboard.
enum Job {
    Read(Resolver<ClipboardRead>),
    Write(ClipboardWrite, Resolver<ClipboardWritten>),
    /// A test's look at the clipboard while a session holds it open.
    #[cfg(test)]
    Inspect(Box<dyn FnOnce() + Send>),
}

/// One request and when it was accepted, which its patience counts from.
struct Request {
    job: Job,
    accepted: Instant,
}

/// What a job leaves to deliver once the clipboard is closed again.
enum Answer {
    Read(Resolver<ClipboardRead>, ClipboardRead),
    Written(Resolver<ClipboardWritten>, ClipboardWritten),
    /// A test's look, which answers nobody.
    #[cfg(test)]
    None,
}

impl Job {
    /// Run the job in the open `session`, returning its answer undelivered.
    fn run(self, session: &ClipboardSession) -> Answer {
        match self {
            Job::Read(answer) => Answer::Read(answer, session.read_text()),
            Job::Write(write, answer) => Answer::Written(answer, session.write(write)),
            #[cfg(test)]
            Job::Inspect(look) => {
                look();
                Answer::None
            }
        }
    }

    /// The answer of a job the clipboard could not be opened for.
    fn refused(self, error: ClipboardError) -> Answer {
        match self {
            Job::Read(answer) => Answer::Read(answer, Err(error)),
            Job::Write(_, answer) => Answer::Written(answer, Err(error)),
            #[cfg(test)]
            Job::Inspect(look) => {
                drop(look);
                Answer::None
            }
        }
    }
}

impl Answer {
    /// Hand the answer to its resolver. A retired owner has nobody left to
    /// answer.
    fn deliver(self) {
        match self {
            Answer::Read(answer, read) => drop(answer.resolve(read)),
            Answer::Written(answer, written) => drop(answer.resolve(written)),
            #[cfg(test)]
            Answer::None => {}
        }
    }
}

/// Queue `job` to the clipboard thread, or refuse it at once when there is
/// none or [`MAX_WAITING`] requests wait already. A refusal settles at the
/// caller's next turn, never inside this call.
fn submit(job: Job) {
    let request = Request {
        job,
        accepted: Instant::now(),
    };
    let refused = match ClipboardThread::get() {
        Some(thread) => match thread.requests.try_send(request) {
            Ok(()) => {
                thread.wake();
                return;
            }
            Err(mpsc::TrySendError::Full(request) | mpsc::TrySendError::Disconnected(request)) => {
                request
            }
        },
        None => request,
    };
    refused.job.refused(ClipboardError::Unavailable).deliver();
    tracing::warn!("a clipboard request was refused: the clipboard thread cannot take it");
}

/// The process's clipboard thread: where requests are sent, and the event
/// that wakes it.
struct ClipboardThread {
    requests: mpsc::SyncSender<Request>,
    wake: OwnedHandle,
}

impl ClipboardThread {
    /// The thread, started on first use and never stopped; `None` if it
    /// could not be started, in which case every request is refused.
    fn get() -> Option<&'static Self> {
        static THREAD: OnceLock<Option<ClipboardThread>> = OnceLock::new();
        THREAD.get_or_init(Self::spawn).as_ref()
    }

    /// Wake the thread to take the requests sent to it.
    fn wake(&self) {
        // SAFETY: the event is owned by the process-lifetime thread record,
        // so it is live for the call.
        if let Err(error) = unsafe { SetEvent(HANDLE(self.wake.as_raw_handle())) } {
            tracing::error!(%error, "the clipboard thread could not be woken");
        }
    }

    /// Start the thread and wait until its window exists or failed to.
    fn spawn() -> Option<Self> {
        // SAFETY: an unnamed, non-inheritable auto-reset event; no pointers.
        let event = match unsafe { CreateEventW(None, false, false, None) } {
            // SAFETY: a successful `CreateEventW` hands over this unique
            // handle; `OwnedHandle` closes it once, and the thread record
            // that owns it lives as long as the process.
            Ok(event) => unsafe { OwnedHandle::from_raw_handle(event.0) },
            Err(error) => {
                tracing::error!(%error, "failed to create the clipboard thread's event");
                return None;
            }
        };
        // The raw value crosses to the thread; the handle it names outlives it.
        let wake = event.as_raw_handle() as isize;
        let (requests, received) = mpsc::sync_channel(MAX_WAITING);
        let (created, window) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("flui-clipboard".into())
            .spawn(move || {
                // SAFETY: the window is created on this thread, which pumps
                // its messages from here until the process exits.
                let hwnd = match unsafe { create_owner_window() } {
                    Ok(hwnd) => hwnd,
                    Err(error) => {
                        let _ = created.send(Err(error));
                        return;
                    }
                };
                let _ = created.send(Ok(()));
                serve(hwnd, HANDLE(wake as *mut _), &received);
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "failed to spawn the clipboard thread");
            return None;
        }
        // A closed channel means the thread died before reporting; it cannot
        // have created a window that outlives it.
        match window.recv() {
            Ok(Ok(())) => Some(Self {
                requests,
                wake: event,
            }),
            Ok(Err(error)) => {
                tracing::error!(%error, "failed to create the clipboard owner window");
                None
            }
            Err(_) => {
                tracing::error!("the clipboard thread exited before creating its window");
                None
            }
        }
    }
}

/// The clipboard thread's loop: serve the requests in order, each tried
/// until its clipboard opens or its patience runs out, and between attempts
/// wait for the next attempt, a new request or a message, pumping the
/// messages. The owner window must be pumped while it owns the clipboard:
/// another process's `EmptyClipboard` sends it `WM_DESTROYCLIPBOARD` and
/// waits, and so would keep the clipboard it holds open meanwhile.
fn serve(window: HWND, wake: HANDLE, requests: &mpsc::Receiver<Request>) {
    let mut turn: Option<(Job, OpenAttempts)> = None;
    loop {
        if !pump_messages() {
            return;
        }
        let mut retry_at = None;
        loop {
            let (job, mut attempts) = match turn.take() {
                Some(current) => current,
                None => match requests.try_recv() {
                    Ok(request) => (request.job, OpenAttempts::new(request.accepted)),
                    Err(_) => break,
                },
            };
            let answer = if let Some(session) = ClipboardSession::try_open(window) {
                let answer = contained(|| job.run(&session));
                // The clipboard closes before the answer is delivered.
                drop(session);
                answer
            } else if let Some(at) = attempts.failed(Instant::now()) {
                turn = Some((job, attempts));
                retry_at = Some(at);
                break;
            } else {
                tracing::warn!("another process held the clipboard past this request's patience");
                Some(job.refused(ClipboardError::Unavailable))
            };
            if let Some(answer) = answer {
                contained(|| answer.deliver());
            }
        }
        let timeout = retry_at.map_or(INFINITE, |at| {
            // Rounded up, so a wait shorter than a millisecond cannot spin;
            // `INFINITE` is reserved for an idle wait.
            let left = at.saturating_duration_since(Instant::now());
            let milliseconds = left.as_nanos().saturating_add(999_999) / 1_000_000;
            u32::try_from(milliseconds.min(u128::from(INFINITE - 1)))
                .expect("BUG: a bounded wait fits u32")
        });
        // SAFETY: `wake` is the live event the thread record owns for the
        // process's life; the wait returns for it, a message, or the timeout,
        // each of which the loop handles by trying again.
        let _ = unsafe {
            MsgWaitForMultipleObjectsEx(Some(&[wake]), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
    }
}

/// Run `run` on the clipboard thread with its panic contained: a job or a
/// delivery that panics (a `tracing` subscriber, an owner's wake hook) drops
/// what it held, which abandons its request, and the thread goes on serving
/// the others. Nobody resumes the payload on this thread, so it is reported
/// and discarded.
fn contained<R>(run: impl FnOnce() -> R) -> Option<R> {
    let mut failure = FirstPanic::new();
    let result = failure.contain(run);
    if let Some(payload) = failure.into_payload() {
        discard_payload(payload);
        tracing::error!("a clipboard request panicked on the clipboard thread; it is abandoned");
    }
    result
}

/// Dispatch every message waiting on this thread: `false` once `WM_QUIT`
/// arrives, which nothing posts while the process lives.
fn pump_messages() -> bool {
    let mut message = MSG::default();
    // SAFETY: `message` is a valid, correctly sized `MSG` for both calls.
    unsafe {
        while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
            if message.message == WM_QUIT {
                return false;
            }
            DispatchMessageW(&raw const message);
        }
    }
    true
}

/// How many UTF-8 bytes `wide` decodes to, each unpaired surrogate as
/// U+FFFD, as `String::from_utf16_lossy` writes it.
fn decoded_len(wide: &[u16]) -> usize {
    char::decode_utf16(wide.iter().copied())
        .map(|unit| unit.map_or(char::REPLACEMENT_CHARACTER.len_utf8(), char::len_utf8))
        .sum()
}

/// The clipboard, open on the clipboard thread with the owner window. The
/// only way this module opens it, and only the clipboard thread does, so no
/// two of FLUI's sessions ever overlap.
///
/// Anyone else is shut out by Win32 itself: the session opens with the owner
/// window, and Win32 refuses a concurrent `OpenClipboard` from any other
/// owner, `NULL` included. Dropping the session closes the clipboard.
struct ClipboardSession {
    /// A session is the clipboard thread's: Win32 ties an open clipboard
    /// to the thread that opened it.
    _thread: std::marker::PhantomData<*const ()>,
}

impl ClipboardSession {
    /// Opens the clipboard with `owner`; `None` if another window holds it
    /// open.
    fn try_open(owner: HWND) -> Option<Self> {
        // SAFETY: plain FFI call; `owner` is the owner window, which is never
        // destroyed. The result is checked, and the session is only
        // constructed on success.
        unsafe { OpenClipboard(Some(owner)) }.ok()?;
        Some(Self {
            _thread: std::marker::PhantomData,
        })
    }

    /// The clipboard's text, or why it could not be read.
    #[expect(
        clippy::unused_self,
        reason = "the session is the proof that this thread holds the clipboard open"
    )]
    fn read_text(&self) -> ClipboardRead {
        // SAFETY: `self` holds the clipboard open on this thread, and no
        // other session of FLUI's exists, so no `EmptyClipboard` can free the
        // `CF_UNICODETEXT` handle while it is locked and scanned below.
        // `IsClipboardFormatAvailable`/`GetClipboardData` are plain FFI calls
        // with no pointer arguments of ours; every one of their results is
        // checked before the next step runs, so `handle` is only converted to
        // `HGLOBAL` once known valid. `GlobalLock` returning non-null is
        // checked before `ptr` is dereferenced at all. The clipboard is owned
        // by another, untrusted process: the NUL scan is hard-bounded by
        // `GlobalSize(hglobal)` (the allocation's byte length, halved for
        // `u16` units), so it never reads past `wide_ptr.add(max_len - 1)`,
        // and a missing terminator within that bound is malformed data, not
        // walked past. `GlobalUnlock`'s result is discarded on every path: it
        // cannot tell "already unlocked, success" from failure without
        // `GetLastError`, and nothing is left to do with the lock either way.
        unsafe {
            if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
                return Ok(None);
            }

            // An owner that renders on demand is sent `WM_RENDERFORMAT` here,
            // and Win32 gives up on it after thirty seconds.
            let handle = match GetClipboardData(CF_UNICODETEXT.0 as u32) {
                Ok(handle) => handle,
                Err(error) if error.code() == ERROR_TIMEOUT.to_hresult() => {
                    tracing::warn!("the clipboard's owner did not render its text in time");
                    return Err(ClipboardError::Timeout);
                }
                Err(error) => {
                    tracing::warn!(%error, "failed to get the clipboard's text");
                    return Err(ClipboardError::Unavailable);
                }
            };
            if handle.is_invalid() {
                return Err(ClipboardError::Unavailable);
            }
            let hglobal = HGLOBAL(handle.0);

            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                tracing::warn!("failed to lock the clipboard's text");
                return Err(ClipboardError::Unavailable);
            }

            let byte_size = GlobalSize(hglobal);
            let max_len = byte_size / std::mem::size_of::<u16>();
            let wide_ptr = ptr as *const u16;
            let mut len: usize = 0;
            while len < max_len && *wide_ptr.add(len) != 0 {
                len += 1;
            }
            if len == max_len {
                let _ = GlobalUnlock(hglobal);
                tracing::warn!(
                    byte_size,
                    "the clipboard's CF_UNICODETEXT has no NUL terminator within its allocation"
                );
                return Err(ClipboardError::Malformed);
            }

            let wide_slice = std::slice::from_raw_parts(wide_ptr, len);
            // The text is measured before anything is allocated for it: past
            // the limit nothing is decoded. The decode needs the allocation
            // locked, while the refusal's warning (user code) runs after the
            // unlock, so a panicking subscriber cannot leave it locked.
            let measured = decoded_len(wide_slice);
            let decoded = text_fits(measured).then(|| String::from_utf16_lossy(wide_slice));

            let _ = GlobalUnlock(hglobal);
            admit_text(measured)?;
            // `admit_text` accepted exactly what `text_fits` decoded.
            host_text(decoded.unwrap_or_default())
        }
    }

    /// Replace the clipboard's text with `write`'s, or say why it could not.
    #[expect(
        clippy::unused_self,
        reason = "the session is the proof that this thread holds the clipboard open"
    )]
    fn write(&self, write: ClipboardWrite) -> ClipboardWritten {
        let concealed = write.is_concealed();
        let text = write.into_text();
        // SAFETY: `self` holds the clipboard open on this thread for the
        // whole block, and `EmptyClipboard` succeeds before any `set_data`,
        // which is all `set_data` asks. `RegisterClipboardFormatW` takes a
        // NUL-terminated wide string that `w!` builds as a static. `bytes`
        // views `wide`, a live `Vec<u16>`, as its `len * 2` bytes, which `u8`
        // may alias at any alignment.
        unsafe {
            if let Err(error) = EmptyClipboard() {
                tracing::error!(%error, "failed to empty the clipboard for a write");
                return Err(ClipboardError::Unavailable);
            }

            // The markers go first: a concealed write the clipboard could not
            // mark leaves the clipboard empty rather than publish the secret
            // to the clipboard's history (ADR-0152 §6).
            if concealed {
                for (name, label) in CONCEALMENT_FORMATS {
                    let format = RegisterClipboardFormatW(name);
                    if format == 0 || !set_data(format, &EXCLUDE.to_le_bytes()) {
                        tracing::error!(
                            format = label,
                            "a concealed clipboard write was dropped: its marker could not be set"
                        );
                        return Err(ClipboardError::Unavailable);
                    }
                }
            }

            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = std::slice::from_raw_parts(
                wide.as_ptr().cast::<u8>(),
                wide.len() * std::mem::size_of::<u16>(),
            );
            if set_data(CF_UNICODETEXT.0 as u32, bytes) {
                Ok(())
            } else {
                Err(ClipboardError::Unavailable)
            }
        }
    }
}

impl Drop for ClipboardSession {
    fn drop(&mut self) {
        // SAFETY: `CloseClipboard` takes no arguments; a session exists only
        // after a successful `OpenClipboard` on this thread, so this always
        // closes a clipboard this thread holds open. The result is discarded:
        // `Drop` cannot return it and nothing would recover; an unbalanced
        // close would surface as the next `OpenClipboard` failing, not as
        // memory unsafety here.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// The registered formats that keep an entry out of Windows' clipboard
/// history and cloud clipboard, each with the DWORD [`EXCLUDE`]: the
/// monitor-processing exclusion clipboard managers honour, and the two
/// formats Windows' own history reads.
const CONCEALMENT_FORMATS: [(windows::core::PCWSTR, &str); 3] = [
    (
        w!("ExcludeClipboardContentFromMonitorProcessing"),
        "ExcludeClipboardContentFromMonitorProcessing",
    ),
    (
        w!("CanIncludeInClipboardHistory"),
        "CanIncludeInClipboardHistory",
    ),
    (w!("CanUploadToCloudClipboard"), "CanUploadToCloudClipboard"),
];

/// The DWORD each of [`CONCEALMENT_FORMATS`] holds: zero, "not allowed".
const EXCLUDE: u32 = 0;

/// Puts a copy of `bytes` on the clipboard as `format`; `false` if it could
/// not, after logging why.
///
/// # Safety
///
/// The calling thread holds the clipboard open through a
/// [`ClipboardSession`] whose `EmptyClipboard` succeeded.
unsafe fn set_data(format: u32, bytes: &[u8]) -> bool {
    // SAFETY: `GlobalAlloc` is asked for `bytes.len()` bytes and checked, so
    // the matching `GlobalLock`'s non-null `ptr` is valid for writes of that
    // many bytes; `copy_nonoverlapping` copies between `bytes` and the new
    // allocation, two distinct regions. `GlobalUnlock`'s result is
    // discarded: it cannot tell "unlocked, success" from failure without
    // `GetLastError`, and nothing is left to do with the lock either way.
    // Until `SetClipboardData` succeeds `global` is ours alone, so the failure
    // paths free it (`HGLOBAL` has no `Drop`); after, the clipboard owns it.
    // The `GlobalFree` results are discarded because the wrapper maps its
    // NULL success return to `Err`. The caller holds the clipboard open and
    // emptied, as `SetClipboardData` requires.
    unsafe {
        let global = match GlobalAlloc(GMEM_MOVEABLE, bytes.len()) {
            Ok(global) => global,
            Err(error) => {
                tracing::error!(?error, "Failed to allocate global memory");
                return false;
            }
        };
        let ptr = GlobalLock(global);
        if ptr.is_null() {
            tracing::error!("Failed to lock global memory");
            let _ = GlobalFree(Some(global));
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(global);
        if SetClipboardData(format, Some(HANDLE(global.0))).is_err() {
            tracing::error!(format, "Failed to set clipboard data");
            let _ = GlobalFree(Some(global));
            return false;
        }
        true
    }
}

/// Registers the owner window's class and creates the window on this thread.
///
/// # Safety
///
/// The calling thread must pump messages for as long as the window lives.
unsafe fn create_owner_window() -> windows::core::Result<HWND> {
    let class_name = w!("FLUIClipboardOwner");
    // SAFETY: `class` and the class name it points to outlive the call, and
    // `owner_procedure` implements the window-procedure ABI. This function
    // runs once per process; `ERROR_CLASS_ALREADY_EXISTS` means another copy
    // of this crate in the process registered an equivalent class, which
    // serves just as well.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(owner_procedure),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&raw const class) == 0 {
            let error = windows::core::Error::from_thread();
            if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
                return Err(error);
            }
        }
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("FLUI clipboard owner"),
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
    }
}

/// The owner window's procedure: every message, `WM_DESTROYCLIPBOARD`
/// included, gets the default handling — FLUI keeps no clipboard state to
/// release and never renders formats on demand.
unsafe extern "system" fn owner_procedure(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    os_entry(
        "the clipboard owner's procedure",
        (),
        AfterPanic::Answer(|()| LRESULT(0)),
        // SAFETY: forwards Win32's own arguments unchanged, on the thread
        // Win32 delivers them on.
        || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    )
}

#[cfg(test)]
mod tests;
