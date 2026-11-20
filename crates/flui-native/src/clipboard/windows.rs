//! The Win32 clipboard, served by a thread of its own (ADR-0337).
//!
//! Every read and write is a request to the process's clipboard thread,
//! which opens the clipboard with its own message-only window, serves the
//! requests one at a time in the order they were made, and answers each
//! through its resolver. The owner thread only queues: it never waits for
//! another process. The queue is bounded twice, by [`MAX_WAITING`] requests
//! and [`MAX_QUEUED_BYTES`] of write text, and a request past either is
//! refused [`ClipboardError::Unavailable`] at once; a request that waited
//! past `clipboard::patience`'s bound for its turn is refused
//! [`ClipboardError::Timeout`] without an attempt. A request the thread can
//! no longer serve answers [`ClipboardError::Gone`].
//!
//! Two waits are another process's to end, and both happen on the clipboard
//! thread. `OpenClipboard` fails while another window holds the clipboard
//! open, as clipboard managers and remote-desktop clients do right after
//! every change; the request tries again on the schedule of
//! `clipboard::patience`, pumping the thread's messages between
//! attempts, and is refused with [`ClipboardError::Unavailable`] when its
//! patience runs out. `GetClipboardData` of a format its owner renders only
//! when asked sends that owner `WM_RENDERFORMAT` and waits for the answer:
//! for as long as the owner takes, and thirty seconds for an owner that never
//! answers, after which the read fails with `ERROR_TIMEOUT` and answers
//! [`ClipboardError::Timeout`].
//!
//! Everything the thread runs that it does not own is contained: the
//! conversions and `tracing` events of a request (a subscriber is user code)
//! and the delivery of its answer. A request whose work panics is answered
//! [`ClipboardError::Unavailable`], the first failure is reported and the
//! thread goes on to the next request.
//!
//! An expiring write that lands is recorded on the thread with the
//! clipboard's sequence number and owner, read once its session closed
//! (closing a session that changed the clipboard moves the number again),
//! and only while the thread's window still owns it (ADR-0363). A
//! conditional clear opens the clipboard, which shuts every other process out
//! until it closes, and empties it only while the owner is still the thread's
//! window and the sequence number is still the recorded one: the check and
//! the clear are one step, so a copy another process makes can land only
//! before the check, which then sees it, or after the clear.

#![expect(
    unsafe_code,
    reason = "calls the operating system directly, which has no safe wrapper"
)]

use std::mem::ManuallyDrop;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

use flui_foundation::panic::{FirstPanic, discard_payload};
use flui_platform::capability::Resolver;
use flui_platform::{
    ClipboardEntryId, ClipboardError, ClipboardRead, ClipboardWrite, ClipboardWritten, Concealment,
    ExpiryClearing,
};

use windows::{
    Win32::{
        Foundation::{
            ERROR_CLASS_ALREADY_EXISTS, ERROR_TIMEOUT, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM,
            LRESULT, WPARAM,
        },
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner,
                GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
                RegisterClipboardFormatW, SetClipboardData,
            },
            LibraryLoader::GetModuleHandleW,
            Memory::{
                GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
            },
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

use crate::clipboard::patience::{OpenAttempts, Waiting};
use crate::host::{
    ClearDecision, ExpiredClear, ExpiredEntry, ExpiringRecord, NativeClipboard, admit_text,
    host_text, text_fits,
};
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
    /// Queued to the clipboard thread; answered from there. A read whose
    /// readers all went away is still performed: the handle's shared read
    /// keeps waiting for it, so the thread cannot tell it apart.
    fn read_text(&self, answer: Resolver<ClipboardRead>) {
        submit(Task::Read, Answer::Read(answer));
    }

    /// Queued to the clipboard thread behind every request made before it,
    /// so a read made after it answers what it wrote. A write is performed
    /// even when nobody waits for its answer.
    fn write(&self, write: ClipboardWrite, answer: Resolver<ClipboardWritten>) {
        submit(Task::Write(write), Answer::Written(answer));
    }

    fn concealment(&self) -> Concealment {
        Concealment::Honored
    }

    /// The window station's clipboard is cleared by sequence number and
    /// ownership, and the Win32 loop hears `WM_WTSSESSION_CHANGE`.
    fn expiry_clearing(&self) -> ExpiryClearing {
        ExpiryClearing::AtTimeAndLock
    }

    /// Queued to the clipboard thread behind every request made before it,
    /// so a clear asked after a write is checked against what that write
    /// left.
    fn clear_expired(&self, entry: ExpiredEntry, answer: Resolver<ExpiredClear>) {
        submit(Task::ClearExpired(entry), Answer::Cleared(answer));
    }

    /// Queued like any clear and waited for: the clipboard thread ends with
    /// the process, and an entry left on the window station's clipboard
    /// outlives it.
    fn clear_expired_before_exit(&self, within: Duration) -> ExpiredClear {
        let (sent, answered) = mpsc::sync_channel(1);
        submit(Task::ClearExpired(ExpiredEntry::Any), Answer::Exit(sent));
        answered
            .recv_timeout(within)
            .unwrap_or(ExpiredClear::Failed(ClipboardError::Timeout))
    }
}

/// The most requests waiting for the clipboard thread at once. The handles
/// of a UI runtime share one read in flight, so only writes and many
/// runtimes reach it; past it a request is refused, not queued.
const MAX_WAITING: usize = 64;

/// What one request asks of the clipboard.
enum Task {
    Read,
    Write(ClipboardWrite),
    /// Empty the clipboard if it still holds this expiring entry.
    ClearExpired(ExpiredEntry),
    /// A test's look at the clipboard while a session holds it open.
    #[cfg(test)]
    Inspect(Box<dyn FnOnce() + Send>),
}

/// Whom a request answers. Kept apart from its [`Task`], so a task that
/// panics leaves the answer to be given.
enum Answer {
    Read(Resolver<ClipboardRead>),
    Written(Resolver<ClipboardWritten>),
    Cleared(Resolver<ExpiredClear>),
    /// The loop's end, waiting on the owner thread for the clear.
    Exit(mpsc::SyncSender<ExpiredClear>),
    /// A test's look, which answers nobody.
    #[cfg(test)]
    Nobody,
}

/// A request's [`Answer`], owed until it is settled. Dropped unsettled (the
/// thread ended with the request in its queue or at its turn) it answers
/// [`ClipboardError::Gone`], so no request is left without a typed answer.
struct Owed(Option<Answer>);

impl Owed {
    /// Settle the answer: see [`Answer::settle`].
    fn settle(mut self, settled: Option<Settlement>, error: ClipboardError) {
        if let Some(answer) = self.0.take() {
            answer.settle(settled, error);
        }
    }
}

impl Drop for Owed {
    fn drop(&mut self) {
        if let Some(answer) = self.0.take() {
            answer.settle(None, ClipboardError::Gone);
        }
    }
}

/// What a task found, for its [`Answer`].
enum Settlement {
    Read(ClipboardRead),
    Written(ClipboardWritten),
    Cleared(ExpiredClear),
    #[cfg(test)]
    Nothing,
}

/// One request waiting for its turn.
struct Request {
    task: Task,
    answer: Owed,
    waiting: Waiting,
    /// The bytes a write holds in the queue, given back as it is answered.
    _reserved: Option<Reservation>,
}

impl Request {
    /// The request's turn came at `now`: its [`Turn`], or its answer back
    /// when it waited past the queue's bound for it.
    fn take_turn(self, now: Instant) -> Result<Turn, Owed> {
        let Self {
            task,
            answer,
            waiting,
            _reserved: reserved,
        } = self;
        match waiting.turn(now) {
            Some(attempts) => Ok(Turn {
                task,
                answer,
                attempts,
                _reserved: reserved,
            }),
            None => Err(answer),
        }
    }
}

/// One request at its turn, trying to open the clipboard.
struct Turn {
    task: Task,
    answer: Owed,
    attempts: OpenAttempts,
    _reserved: Option<Reservation>,
}

/// The most bytes of write text the queue holds at once, besides a write
/// that comes to an empty queue, which is taken whatever its size: the
/// host keeps no size limit of its own on a write. Past it a write is
/// refused [`ClipboardError::Unavailable`], which a retry once the queue
/// drains can fix.
const MAX_QUEUED_BYTES: usize = crate::host::MAX_TEXT;

/// The bytes of one queued write, counted in the queue's total until it
/// drops.
struct Reservation {
    queued: &'static AtomicUsize,
    bytes: usize,
}

impl Reservation {
    /// Count `bytes` more in `queued`, or `None` past [`MAX_QUEUED_BYTES`].
    fn try_new(queued: &'static AtomicUsize, bytes: usize) -> Option<Self> {
        queued
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                if held == 0 {
                    return Some(bytes);
                }
                held.checked_add(bytes)
                    .filter(|total| *total <= MAX_QUEUED_BYTES)
            })
            .ok()?;
        Some(Self { queued, bytes })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.queued.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

impl Task {
    /// Run the task in the open `session`, against the thread's record of
    /// the last expiring entry that landed. A write that landed returns its
    /// [`Landed`], which is stamped once the clipboard is closed.
    fn run(
        self,
        session: &ClipboardSession,
        expiring: &mut ExpiringRecord<Stamp>,
    ) -> (Settlement, Option<Landed>) {
        match self {
            Task::Read => (Settlement::Read(session.read_text()), None),
            Task::Write(write) => {
                let entry = write.entry();
                let written = session.write(write);
                // A write that failed before `EmptyClipboard` left the
                // clipboard as it was; one that failed after moved the
                // sequence number, which the recorded entry then misses.
                let landed = written.is_ok().then_some(Landed(entry));
                (Settlement::Written(written), landed)
            }
            Task::ClearExpired(entry) => (
                Settlement::Cleared(session.clear_expired(entry, expiring)),
                None,
            ),
            #[cfg(test)]
            Task::Inspect(look) => {
                look();
                (Settlement::Nothing, None)
            }
        }
    }
}

impl Answer {
    /// Hand the request its answer: what its task found, or `error` when
    /// the task found nothing (it panicked, or never ran). A retired owner,
    /// or a loop's end that stopped waiting, has nobody left to answer.
    fn settle(self, settled: Option<Settlement>, error: ClipboardError) {
        let _ = match (self, settled) {
            (Answer::Read(answer), Some(Settlement::Read(read))) => answer.resolve(read),
            (Answer::Read(answer), _) => answer.resolve(Err(error)),
            (Answer::Written(answer), Some(Settlement::Written(written))) => {
                answer.resolve(written)
            }
            (Answer::Written(answer), _) => answer.resolve(Err(error)),
            (Answer::Cleared(answer), Some(Settlement::Cleared(cleared))) => {
                answer.resolve(cleared)
            }
            (Answer::Cleared(answer), _) => answer.resolve(ExpiredClear::Failed(error)),
            (Answer::Exit(answer), settled) => {
                let cleared = match settled {
                    Some(Settlement::Cleared(cleared)) => cleared,
                    _ => ExpiredClear::Failed(error),
                };
                let _ = answer.try_send(cleared);
                Ok(())
            }
            #[cfg(test)]
            (Answer::Nobody, _) => Ok(()),
        };
    }
}

/// Queue a request to the clipboard thread, or refuse it at once when there
/// is none, [`MAX_WAITING`] requests wait already or a write's text would
/// take the queue past [`MAX_QUEUED_BYTES`]. A refusal settles at the
/// caller's next turn, never inside this call.
fn submit(task: Task, answer: Answer) {
    submit_accepted_at(task, answer, Instant::now());
}

/// [`submit`], the request accepted at `accepted`, which its wait for its
/// turn counts from.
fn submit_accepted_at(task: Task, answer: Answer, accepted: Instant) {
    let answer = Owed(Some(answer));
    // A full queue is worth a retry; a thread that never started or ended is
    // gone for the rest of the run.
    let Some(thread) = ClipboardThread::get() else {
        return refuse(answer, ClipboardError::Gone);
    };
    let reserved = match &task {
        Task::Write(write) => {
            let Some(reserved) = Reservation::try_new(&thread.queued_bytes, write.as_text().len())
            else {
                return refuse(answer, ClipboardError::Unavailable);
            };
            Some(reserved)
        }
        Task::Read | Task::ClearExpired(_) => None,
        #[cfg(test)]
        Task::Inspect(_) => None,
    };
    let request = Request {
        task,
        answer,
        waiting: Waiting::since(accepted),
        _reserved: reserved,
    };
    match thread.requests.try_send(request) {
        Ok(()) => thread.wake(),
        Err(mpsc::TrySendError::Full(request)) => {
            refuse(request.answer, ClipboardError::Unavailable);
        }
        Err(mpsc::TrySendError::Disconnected(request)) => {
            refuse(request.answer, ClipboardError::Gone);
        }
    }
}

/// Refuse a request at once with `error`, then say so.
fn refuse(answer: Owed, error: ClipboardError) {
    answer.settle(None, error);
    tracing::warn!(
        ?error,
        "a clipboard request was refused: the clipboard thread cannot take it"
    );
}

/// The process's clipboard thread: where requests are sent, and the event
/// that wakes it.
struct ClipboardThread {
    requests: mpsc::SyncSender<Request>,
    wake: Arc<OwnedHandle>,
    /// The bytes of write text queued and not yet answered.
    queued_bytes: AtomicUsize,
}

impl ClipboardThread {
    /// The thread, started on first use and never stopped; `None` if it
    /// could not be started, in which case every request answers
    /// [`ClipboardError::Gone`].
    fn get() -> Option<&'static Self> {
        static THREAD: OnceLock<Option<ClipboardThread>> = OnceLock::new();
        THREAD.get_or_init(Self::spawn).as_ref()
    }

    /// Wake the thread to take the requests sent to it.
    fn wake(&self) {
        // SAFETY: `self.wake` owns the event, live for the call.
        if let Err(error) = unsafe { SetEvent(HANDLE(self.wake.as_raw_handle())) } {
            tracing::error!(%error, "the clipboard thread could not be woken");
        }
    }

    /// Start the thread and wait until its window exists or failed to.
    fn spawn() -> Option<Self> {
        // SAFETY: an unnamed, non-inheritable auto-reset event; no pointers.
        let wake = match unsafe { CreateEventW(None, false, false, None) } {
            // SAFETY: a successful `CreateEventW` hands over this unique
            // handle, which `OwnedHandle` closes once, after both the thread
            // record and the thread have let go of it.
            Ok(event) => Arc::new(unsafe { OwnedHandle::from_raw_handle(event.0) }),
            Err(error) => {
                tracing::error!(%error, "failed to create the clipboard thread's event");
                return None;
            }
        };
        let thread_wake = Arc::clone(&wake);
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
                serve(hwnd, &thread_wake, &received);
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
                wake,
                queued_bytes: AtomicUsize::new(0),
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

/// The clipboard thread's body. Serve the requests in order, each tried
/// until its clipboard opens or its patience runs out, and between attempts
/// wait for the next attempt, a new request or a message, pumping the
/// messages. The owner window must be pumped while it owns the clipboard:
/// another process's `EmptyClipboard` sends it `WM_DESTROYCLIPBOARD` and
/// waits, and so would keep the clipboard it holds open meanwhile. Returns at
/// `WM_QUIT`, which nothing posts while the process lives.
///
/// Every request runs contained, and nothing else here runs code the thread
/// does not own (the window's messages go through `os_entry`), so the loop
/// has no failure to restart from. Were it to end all the same, the request
/// at its turn and those in the queue drop with it and answer
/// [`ClipboardError::Gone`] ([`Owed`]), and so does every later request,
/// which finds the channel closed.
///
/// A request in the queue is [`Waiting`]; its turn makes it a [`Turn`],
/// whose attempts keep their patience whatever it waited for its turn: the
/// queue's bound is measured once, as the turn comes.
fn serve(window: HWND, wake: &OwnedHandle, requests: &mpsc::Receiver<Request>) {
    let mut turn: Option<Turn> = None;
    let mut expiring = ExpiringRecord::<Stamp>::default();
    loop {
        if !pump_messages() {
            return;
        }
        let mut retry_at = None;
        loop {
            let current = if let Some(current) = turn.take() {
                current
            } else {
                let Ok(request) = requests.try_recv() else {
                    break;
                };
                match request.take_turn(Instant::now()) {
                    Ok(current) => current,
                    Err(answer) => {
                        let mut failure = FirstPanic::new();
                        failure.contain(|| answer.settle(None, ClipboardError::Timeout));
                        failure.contain(|| {
                            tracing::warn!(
                                "a clipboard request waited its whole bound for its turn"
                            );
                        });
                        report(failure);
                        continue;
                    }
                }
            };
            let Some(session) = ClipboardSession::try_open(window) else {
                let mut current = current;
                if let Some(at) = current.attempts.failed(Instant::now()) {
                    turn = Some(current);
                    retry_at = Some(at);
                    break;
                }
                let mut failure = FirstPanic::new();
                failure.contain(|| current.answer.settle(None, ClipboardError::Unavailable));
                failure.contain(|| {
                    tracing::warn!(
                        "another process held the clipboard past this request's patience"
                    );
                });
                report(failure);
                continue;
            };
            let Turn { task, answer, .. } = current;
            let mut failure = FirstPanic::new();
            let ran = failure.contain(|| task.run(&session, &mut expiring));
            let (settled, landed) =
                ran.map_or((None, None), |(settled, landed)| (Some(settled), landed));
            // The clipboard closes before the answer is delivered, and before
            // a write that landed is stamped: closing it moves the sequence
            // number once more.
            drop(session);
            if let Some(landed) = landed {
                landed.stamp(window, &mut expiring);
            }
            failure.contain(|| answer.settle(settled, ClipboardError::Unavailable));
            report(failure);
        }
        let timeout = retry_at.map_or(INFINITE, |at| {
            // Rounded up, so a wait shorter than a millisecond cannot spin;
            // `INFINITE` is reserved for an idle wait.
            let left = at.saturating_duration_since(Instant::now());
            let milliseconds = left.as_nanos().saturating_add(999_999) / 1_000_000;
            u32::try_from(milliseconds.min(u128::from(INFINITE - 1)))
                .expect("BUG: a bounded wait fits u32")
        });
        // SAFETY: `wake` is the live event the thread holds a share of; the
        // wait returns for it, a message, or the timeout, each of which the
        // loop handles by trying again.
        let _ = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[HANDLE(wake.as_raw_handle())]),
                timeout,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
    }
}

/// Report the first panic `failure` holds and discard it: nobody on the
/// clipboard thread resumes it. The report is a `tracing` event, user code
/// too, so a panic it raises is discarded with its payload.
fn report(failure: FirstPanic) {
    let Some(payload) = failure.into_payload() else {
        return;
    };
    discard_payload(payload);
    let mut reporting = FirstPanic::new();
    reporting.contain(|| {
        tracing::error!("a clipboard request panicked on the clipboard thread");
    });
    if let Some(payload) = reporting.into_payload() {
        discard_payload(payload);
    }
}

/// Dispatch every message waiting on this thread: `false` once `WM_QUIT`
/// arrives.
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

/// What a failed `GetClipboardData` answers: `Timeout` for the owner that
/// did not render its data in time (`ERROR_TIMEOUT`, after Win32's thirty
/// seconds), `Unavailable` for anything else.
fn read_failure(error: &windows::core::Error) -> ClipboardError {
    if error.code() == ERROR_TIMEOUT.to_hresult() {
        ClipboardError::Timeout
    } else {
        ClipboardError::Unavailable
    }
}

/// The clipboard, open on the clipboard thread with the owner window. The
/// only way this module opens it, and only the clipboard thread does, so no
/// two of FLUI's sessions ever overlap.
///
/// Anyone else is shut out by Win32 itself: the session opens with the owner
/// window, and Win32 refuses a concurrent `OpenClipboard` from any other
/// owner, `NULL` included. Dropping the session closes the clipboard.
struct ClipboardSession {
    /// The window the clipboard is open with, which an `EmptyClipboard`
    /// makes its owner.
    owner: HWND,
    /// A session is the clipboard thread's: Win32 ties an open clipboard
    /// to the thread that opened it.
    _thread: std::marker::PhantomData<*const ()>,
}

/// What Win32 says of the clipboard's entry: its sequence number, which
/// every change, every emptying and the close of a session that changed it
/// moves, and whether the thread's window owns it. Two stamps are equal only
/// while no other write landed between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    sequence: u32,
    owned: bool,
}

impl Stamp {
    /// The clipboard's stamp now, for the thread's window `owner`. With no
    /// session open another process may change the clipboard between the two
    /// reads: the owner is read first, so a change after it moves the
    /// sequence number past what any later stamp, which that process's
    /// ownership marks as not the window's, can equal.
    fn read(owner: HWND) -> Self {
        // SAFETY: a plain FFI call with no arguments. It failing means the
        // clipboard has no owner, which is not the thread's window.
        let current = unsafe { GetClipboardOwner() };
        // SAFETY: a plain FFI call with no arguments, which reads the window
        // station's counter.
        let sequence = unsafe { GetClipboardSequenceNumber() };
        Self {
            sequence,
            owned: current.is_ok_and(|current| current == owner),
        }
    }
}

/// A write that landed and the entry it makes when it expires, stamped once
/// its session closed.
struct Landed(Option<ClipboardEntryId>);

impl Landed {
    /// Record the write's entry in `expiring` with the clipboard's stamp
    /// after the close. A clipboard the window no longer owns lost the entry
    /// to another process already, and nothing is recorded.
    fn stamp(self, owner: HWND, expiring: &mut ExpiringRecord<Stamp>) {
        let stamp = Stamp::read(owner);
        expiring.landed(self.0.filter(|_| stamp.owned), stamp);
    }
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
            owner,
            _thread: std::marker::PhantomData,
        })
    }

    /// Empty the clipboard if it still holds the expiring entry `entry`
    /// names, as `expiring` recorded it: the stamp is read and the clipboard
    /// emptied in this one session, which no other process can change.
    fn clear_expired(
        &self,
        entry: ExpiredEntry,
        expiring: &mut ExpiringRecord<Stamp>,
    ) -> ExpiredClear {
        let now = Stamp::read(self.owner);
        match expiring.decide(entry, &now) {
            ClearDecision::NotHeld => ExpiredClear::NotHeld,
            ClearDecision::Clear => {
                // SAFETY: `self` holds the clipboard open on this thread.
                if let Err(error) = unsafe { EmptyClipboard() } {
                    tracing::warn!(%error, "an expired clipboard entry could not be cleared");
                    return ExpiredClear::Failed(ClipboardError::Unavailable);
                }
                expiring.landed(None, now);
                ExpiredClear::Cleared
            }
        }
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
                Err(error) => {
                    let failure = read_failure(&error);
                    tracing::warn!(%error, ?failure, "failed to get the clipboard's text");
                    return Err(failure);
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
    ///
    /// Every format's memory is made before the clipboard is emptied, so a
    /// write that cannot be made leaves the clipboard as it was. Once it is
    /// emptied, only Win32 itself can fail the write (`SetClipboardData`
    /// refusing a handle): the clipboard then holds no text, and the markers
    /// of a concealed write, which go first, may stand alone.
    #[expect(
        clippy::unused_self,
        reason = "the session is the proof that this thread holds the clipboard open"
    )]
    fn write(&self, write: ClipboardWrite) -> ClipboardWritten {
        let concealed = write.is_concealed();
        let text = write.into_text();
        // The markers go first: a concealed write the clipboard could not
        // mark leaves no text behind rather than publish the secret to the
        // clipboard's history (ADR-0152 §6).
        let mut formats = Vec::with_capacity(CONCEALMENT_FORMATS.len() + 1);
        if concealed {
            for (name, label) in CONCEALMENT_FORMATS {
                // SAFETY: `name` is a static NUL-terminated wide string.
                let format = unsafe { RegisterClipboardFormatW(name) };
                if format == 0 {
                    tracing::error!(
                        format = label,
                        "a concealed clipboard write was refused: its marker has no format"
                    );
                    return Err(ClipboardError::Unavailable);
                }
                formats.push(Prepared::try_new(format, &EXCLUDE.to_le_bytes())?);
            }
        }
        // Encoded straight into the clipboard's memory: the request's text
        // is the one other copy, and it goes before the clipboard is emptied.
        formats.push(Prepared::utf16(CF_UNICODETEXT.0 as u32, &text)?);
        drop(text);

        // SAFETY: `self` holds the clipboard open on this thread.
        if let Err(error) = unsafe { EmptyClipboard() } {
            tracing::error!(%error, "failed to empty the clipboard for a write");
            return Err(ClipboardError::Unavailable);
        }
        for prepared in formats {
            let format = prepared.format;
            // SAFETY: the clipboard is open on this thread and emptied above.
            if !unsafe { prepared.hand_over() } {
                tracing::error!(format, "Win32 refused a clipboard format after emptying it");
                return Err(ClipboardError::Unavailable);
            }
        }
        Ok(())
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

/// A test's switch that makes the next [`Prepared::try_new`] fail, as an
/// exhausted global heap does.
#[cfg(test)]
static FAIL_NEXT_PREPARE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// One format's bytes in global memory, ours until the clipboard takes it:
/// dropped unhanded, the memory is freed.
struct Prepared {
    format: u32,
    global: HGLOBAL,
}

impl Prepared {
    /// Copy `bytes` into new global memory for `format`.
    fn try_new(format: u32, bytes: &[u8]) -> Result<Self, ClipboardError> {
        Self::filled(format, bytes.len(), |memory| memory.copy_from_slice(bytes))
    }

    /// `text` as NUL-terminated little-endian UTF-16 in new global memory
    /// for `format`, encoded straight into it.
    fn utf16(format: u32, text: &str) -> Result<Self, ClipboardError> {
        let units = text.encode_utf16().count().saturating_add(1);
        let len = units
            .checked_mul(std::mem::size_of::<u16>())
            .ok_or(ClipboardError::Unavailable)?;
        Self::filled(format, len, |memory| {
            let wide = text.encode_utf16().chain(std::iter::once(0));
            for (slot, unit) in memory.as_chunks_mut::<2>().0.iter_mut().zip(wide) {
                slot.copy_from_slice(&unit.to_le_bytes());
            }
        })
    }

    /// New global memory of `len` bytes for `format`, written by `fill`
    /// while it is locked.
    fn filled(
        format: u32,
        len: usize,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<Self, ClipboardError> {
        #[cfg(test)]
        if FAIL_NEXT_PREPARE.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(ClipboardError::Unavailable);
        }
        // SAFETY: `GlobalAlloc` is asked for `len` zeroed bytes and checked,
        // so the matching `GlobalLock`'s non-null `ptr` is valid for reads
        // and writes of `len` initialized bytes (a `u8` has no alignment to
        // keep), and nothing else reaches the new allocation while the slice
        // made from it lives. `GlobalUnlock`'s
        // result is discarded: it cannot tell "unlocked, success" from
        // failure without `GetLastError`, and nothing is left to do with the
        // lock either way. A failure frees the allocation through `Drop`.
        unsafe {
            let global = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, len).map_err(|error| {
                tracing::error!(%error, "failed to allocate a clipboard format's memory");
                ClipboardError::Unavailable
            })?;
            let prepared = Self { format, global };
            let ptr = GlobalLock(global);
            if ptr.is_null() {
                tracing::error!("failed to lock a clipboard format's memory");
                return Err(ClipboardError::Unavailable);
            }
            fill(std::slice::from_raw_parts_mut(ptr.cast::<u8>(), len));
            let _ = GlobalUnlock(global);
            Ok(prepared)
        }
    }

    /// Put the memory on the clipboard; `false` if Win32 refused it, which
    /// frees it.
    ///
    /// # Safety
    ///
    /// The calling thread holds the clipboard open through a
    /// [`ClipboardSession`] whose `EmptyClipboard` succeeded.
    unsafe fn hand_over(self) -> bool {
        let this = ManuallyDrop::new(self);
        // SAFETY: the caller holds the clipboard open and emptied, as
        // `SetClipboardData` requires; on success the clipboard owns the
        // memory, which is why `this` is not dropped. On failure it is still
        // ours, and freed here once.
        unsafe {
            if SetClipboardData(this.format, Some(HANDLE(this.global.0))).is_ok() {
                return true;
            }
            let _ = GlobalFree(Some(this.global));
        }
        false
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        // SAFETY: the memory was never handed to the clipboard, so it is ours
        // and freed once. The result is discarded because the wrapper maps
        // its NULL success return to `Err`.
        unsafe {
            let _ = GlobalFree(Some(self.global));
        }
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
