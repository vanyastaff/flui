//! What one `NSURLSession` task's delegate events leave for the request's
//! thread: the response's head, the body's chunks and the end.
//!
//! The delegate hears each event on the session's queue and the request's
//! thread waits here for it. The model is pure: the task is reached through
//! the closures a caller hands in (`suspend`, `resume`), and what the delegate
//! reads from Foundation through the closure of its step, so the rules run on
//! every host.
//!
//! A delegate method is an OS entry, and a panic in one must still end the
//! transfer: an entry that only answers the session leaves the request's
//! thread waiting for an event that already came. So each step runs contained
//! here, and a step that panicked ends the transfer with
//! [`HttpError::Host`].

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

use flui_platform::http::HttpError;

use crate::http::blocking::Head;
use crate::owner::panic_boundary::contain_os_entry;

/// Body bytes held between the delegate and the request's thread before the
/// task is suspended.
const SUSPEND_AT: usize = 256 * 1024;

/// Body bytes held below which a suspended task is resumed.
const RESUME_BELOW: usize = 64 * 1024;

/// What the delegate does with the task after one of its events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub(super) enum Disposition {
    /// The task goes on.
    Allow,
    /// The task is cancelled: its response is refused, or the delegate's own
    /// step failed and the transfer has ended.
    Cancel,
}

/// One task's transfer, shared by the session's delegate and the request's
/// thread.
#[derive(Default)]
pub(super) struct Transfer {
    /// The request's URL, named by an `InvalidUrl` the stack answers.
    url: String,
    state: Mutex<TransferState>,
    changed: Condvar,
}

#[derive(Default)]
struct TransferState {
    /// The response's head, until the request's thread takes it.
    head: Option<Result<Head, HttpError>>,
    /// The head was taken: what follows is the body.
    headed: bool,
    chunks: VecDeque<Vec<u8>>,
    /// The bytes in `chunks`.
    buffered: usize,
    /// The task ended: completed (`Ok`), failed or cancelled.
    end: Option<Result<(), HttpError>>,
    /// The delegate suspended the task while the buffer is full.
    suspended: bool,
}

impl Transfer {
    /// The transfer of a request for `url`.
    pub(super) fn new(url: String) -> Self {
        Self {
            url,
            ..Self::default()
        }
    }

    /// The request's URL.
    pub(super) fn url(&self) -> &str {
        &self.url
    }

    fn lock(&self) -> MutexGuard<'_, TransferState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records the end, unless the task ended already, and wakes the
    /// request's thread.
    pub(super) fn end(&self, end: Result<(), HttpError>) {
        {
            let mut state = self.lock();
            if state.end.is_none() {
                state.end = Some(end);
            }
        }
        self.changed.notify_all();
    }

    /// The response's head, on the request's thread: blocks until the
    /// delegate has it, or the task ends without one.
    pub(super) fn await_head(&self) -> Result<Head, HttpError> {
        let mut state = self.lock();
        loop {
            if let Some(head) = state.head.take() {
                state.headed = true;
                return head;
            }
            if let Some(end) = &state.end {
                // A task that ended without a response: its failure, or one
                // the stack never explained.
                return Err(end.clone().err().unwrap_or_else(|| HttpError::Protocol {
                    detail: "the task ended without a response".to_owned(),
                }));
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The body's next chunk, on the request's thread, `None` at its end:
    /// blocks until the delegate has one, or the task ends. `resume` resumes
    /// the task once the reader has drained a buffer that suspended it.
    pub(super) fn await_chunk(&self, resume: impl FnOnce()) -> Result<Option<Vec<u8>>, HttpError> {
        let mut state = self.lock();
        loop {
            if let Some(chunk) = state.chunks.pop_front() {
                state.buffered -= chunk.len();
                // Under the lock, ordered after the delegate's suspend.
                if state.suspended && state.buffered < RESUME_BELOW {
                    state.suspended = false;
                    resume();
                }
                return Ok(Some(chunk));
            }
            if let Some(end) = &state.end {
                return end.clone().map(|()| None);
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The delegate heard the task's response, whose head `read_head` reads:
    /// the head is left for the request's thread, and a head the contract
    /// refuses cancels the task.
    pub(super) fn responded(
        &self,
        read_head: impl FnOnce() -> Result<Head, HttpError>,
    ) -> Disposition {
        let head = contain_os_entry("a URL session task's response", read_head)
            .unwrap_or_else(|| Err(delegate_failed("response")));
        let disposition = if head.is_ok() {
            Disposition::Allow
        } else {
            Disposition::Cancel
        };
        self.lock().head = Some(head);
        self.changed.notify_all();
        disposition
    }

    /// The delegate heard body bytes, which `read_chunk` copies: they wait
    /// for the request's thread, and a buffer at [`SUSPEND_AT`] suspends the
    /// task through `suspend`. A step that failed has lost bytes of the
    /// body, so the transfer ends there and the task is cancelled.
    pub(super) fn received(
        &self,
        read_chunk: impl FnOnce() -> Vec<u8>,
        suspend: impl FnOnce(),
    ) -> Disposition {
        let buffered = contain_os_entry("a URL session task's data", || {
            let chunk = read_chunk();
            let mut state = self.lock();
            if state.end.is_some() {
                return;
            }
            state.buffered += chunk.len();
            state.chunks.push_back(chunk);
            // Under the lock, so a reader's resume cannot run between the
            // flag and the call; neither calls back.
            if !state.suspended && state.buffered >= SUSPEND_AT {
                state.suspended = true;
                suspend();
            }
        });
        if buffered.is_none() {
            self.end(Err(delegate_failed("data")));
            return Disposition::Cancel;
        }
        self.changed.notify_all();
        Disposition::Allow
    }

    /// The delegate heard the task's end. `read_failure` reads why it
    /// failed, given whether the response's head had arrived; `None` is a
    /// task that completed.
    pub(super) fn completed(&self, read_failure: impl FnOnce(bool) -> Option<HttpError>) {
        let end = contain_os_entry("a URL session task's end", || {
            let headed = {
                let state = self.lock();
                state.headed || state.head.is_some()
            };
            read_failure(headed).map_or(Ok(()), Err)
        })
        .unwrap_or_else(|| Err(delegate_failed("end")));
        self.end(end);
    }
}

/// What a transfer ends with when the delegate's own step for `event`
/// panicked: the failure is the client's, outside the exchange.
fn delegate_failed(event: &str) -> HttpError {
    HttpError::Host {
        detail: format!("the URL session delegate failed handling the task's {event}"),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use flui_platform::http::{Headers, StatusCode};

    use super::*;
    use crate::owner::panic_boundary::{AfterPanic, os_entry};

    fn head() -> Head {
        Head {
            status: StatusCode::try_new(200).expect("200 is a status code"),
            headers: Headers::new(),
        }
    }

    /// A delegate method as the session calls it: its body inside the
    /// boundary, which answers the session nothing after a panic.
    fn delegate_method(body: impl FnOnce()) {
        os_entry("a delegate method", (), AfterPanic::Answer(|()| ()), body);
    }

    /// Whether the request's thread has something to wake for.
    fn settled(transfer: &Transfer) -> bool {
        let state = transfer.lock();
        state.head.is_some() || state.end.is_some()
    }

    fn host_failure(result: Result<impl Sized, HttpError>) -> bool {
        matches!(result, Err(HttpError::Host { .. }))
    }

    /// A delegate step that panics still ends the transfer, with the host's
    /// failure, so the request's thread never waits for an event that already
    /// came; and the task it leaves running is cancelled.
    #[test]
    fn a_delegate_step_that_panics_ends_the_transfer() {
        // What a method learned is read outside it: the boundary would keep
        // an assertion's panic inside.
        let transfer = Transfer::new("https://example.test/".to_owned());
        let mut disposition = None;
        delegate_method(|| {
            disposition = Some(transfer.responded(|| panic!("reading the head failed")));
        });
        assert_eq!(disposition, Some(Disposition::Cancel));
        assert!(settled(&transfer), "the response's panic left no head");
        assert!(host_failure(transfer.await_head()));

        let transfer = Transfer::new("https://example.test/".to_owned());
        let _ = transfer.responded(|| Ok(head()));
        assert!(transfer.await_head().is_ok());
        let _ = transfer.received(|| b"first".to_vec(), || ());
        let mut disposition = None;
        delegate_method(|| {
            disposition = Some(transfer.received(|| panic!("copying the data failed"), || ()));
        });
        assert_eq!(disposition, Some(Disposition::Cancel));
        assert!(settled(&transfer), "the data's panic left the body open");
        assert_eq!(
            transfer.await_chunk(|| ()).ok().flatten().as_deref(),
            Some(&b"first"[..]),
            "the bytes before the lost chunk are the body's"
        );
        assert!(
            host_failure(transfer.await_chunk(|| ())),
            "a body with bytes missing ends in an error, never as complete"
        );

        let transfer = Transfer::new("https://example.test/".to_owned());
        delegate_method(|| transfer.completed(|_| panic!("reading the error failed")));
        assert!(settled(&transfer), "the end's panic left the transfer open");
        assert!(host_failure(transfer.await_head()));
        assert_eq!(transfer.url(), "https://example.test/");
    }

    /// The task's end is what the delegate read for it, told whether the head
    /// had arrived; the first end stands, and the chunks before it are read
    /// first.
    #[test]
    fn a_task_ends_as_its_delegate_read_it() {
        let transfer = Transfer::new(String::new());
        transfer.completed(|headed| {
            assert!(!headed, "no response arrived");
            Some(HttpError::Timeout)
        });
        transfer.end(Err(HttpError::Cancelled));
        assert_eq!(transfer.await_head().err(), Some(HttpError::Timeout));

        let transfer = Transfer::new(String::new());
        assert_eq!(transfer.responded(|| Ok(head())), Disposition::Allow);
        assert_eq!(
            transfer.received(|| b"body".to_vec(), || ()),
            Disposition::Allow
        );
        transfer.completed(|headed| {
            assert!(headed, "the head arrived");
            None
        });
        assert!(transfer.await_head().is_ok());
        assert_eq!(
            transfer.await_chunk(|| ()),
            Ok(Some(b"body".to_vec())),
            "the chunk before the end"
        );
        assert_eq!(transfer.await_chunk(|| ()), Ok(None), "the body's end");

        let transfer = Transfer::new(String::new());
        assert_eq!(
            transfer.responded(|| Err(HttpError::Protocol {
                detail: "the response is not HTTP".to_owned()
            })),
            Disposition::Cancel,
            "a head the contract refuses cancels the task"
        );
        assert!(matches!(
            transfer.await_head(),
            Err(HttpError::Protocol { .. })
        ));
    }

    /// A buffer at its limit suspends the task once, and the reader resumes
    /// it once it has drained the buffer below the lower mark.
    #[test]
    fn a_full_buffer_suspends_the_task_until_the_reader_drains_it() {
        const CHUNK: usize = 32 * 1024;
        let transfer = Transfer::new(String::new());
        let suspends = Cell::new(0_usize);
        let chunks = SUSPEND_AT / CHUNK + 2;
        for _ in 0..chunks {
            let _ = transfer.received(|| vec![0; CHUNK], || suspends.set(suspends.get() + 1));
        }
        assert_eq!(
            suspends.get(),
            1,
            "suspended at the limit, and not again while suspended"
        );

        let resumes = Cell::new(0_usize);
        let mut left_at_resume = None;
        for read in 1..=chunks {
            let chunk = transfer.await_chunk(|| resumes.set(resumes.get() + 1));
            assert_eq!(
                chunk.map(|chunk| chunk.map(|bytes| bytes.len())),
                Ok(Some(CHUNK))
            );
            if resumes.get() == 1 && left_at_resume.is_none() {
                left_at_resume = Some((chunks - read) * CHUNK);
            }
        }
        assert_eq!(resumes.get(), 1, "resumed once");
        assert_eq!(
            left_at_resume,
            Some(RESUME_BELOW - CHUNK),
            "by the first read that left less than the lower mark"
        );
    }
}
