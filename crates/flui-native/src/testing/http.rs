//! A scripted network stack for the request-per-thread HTTP client
//! (`http::blocking`), whose connections answer a test's body and block
//! until aborted where the test asks.

use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex};

use flui_platform::http::{Headers, HttpError, HttpRequest, StatusCode};

use crate::http::blocking::{Abort, Connection, Head, Stack};

/// What a scripted connection answers.
#[derive(Clone, Copy)]
pub(crate) enum Body {
    /// This many 8-byte chunks, then the end.
    Chunks(usize),
    /// Chunks without end.
    Endless,
}

#[derive(Default)]
pub(crate) struct Counters {
    /// Chunks the stack read.
    pub(crate) read: AtomicUsize,
    /// Aborts the stack received.
    pub(crate) aborted: AtomicUsize,
    /// Connections dropped: requests whose thread left.
    pub(crate) closed: AtomicUsize,
}

/// A stack whose connections answer `200` with `body`, after a head the
/// test may hold until the connection is aborted.
pub(crate) struct Scripted {
    body: Body,
    /// The head waits for an abort.
    pub(crate) silent: bool,
    pub(crate) headers: Headers,
    counters: Arc<Counters>,
}

impl Scripted {
    pub(crate) fn new(body: Body) -> (Self, Arc<Counters>) {
        let counters = Arc::new(Counters::default());
        let stack = Self {
            body,
            silent: false,
            headers: Headers::new(),
            counters: Arc::clone(&counters),
        };
        (stack, counters)
    }
}

/// An abort the connection's blocked calls wait on.
#[derive(Default)]
struct Line {
    aborted: Mutex<bool>,
    changed: Condvar,
}

struct LineAbort {
    line: Arc<Line>,
    counters: Arc<Counters>,
}

impl Abort for LineAbort {
    fn abort(&self) {
        self.counters.aborted.fetch_add(1, AtomicOrdering::SeqCst);
        *self.line.aborted.lock().expect("BUG: the line's lock") = true;
        self.line.changed.notify_all();
    }
}

struct ScriptedConnection {
    body: Body,
    silent: bool,
    headers: Headers,
    served: usize,
    line: Arc<Line>,
    counters: Arc<Counters>,
}

impl ScriptedConnection {
    fn aborted(&self) -> bool {
        *self.line.aborted.lock().expect("BUG: the line's lock")
    }
}

impl Drop for ScriptedConnection {
    fn drop(&mut self) {
        self.counters.closed.fetch_add(1, AtomicOrdering::SeqCst);
    }
}

impl Stack for Scripted {
    fn open(&self, request: &HttpRequest) -> Result<Box<dyn Connection>, HttpError> {
        if !request.url.starts_with("https://") {
            return Err(HttpError::InvalidUrl {
                url: request.url.clone(),
            });
        }
        Ok(Box::new(ScriptedConnection {
            body: self.body,
            silent: self.silent,
            headers: self.headers.clone(),
            served: 0,
            line: Arc::default(),
            counters: Arc::clone(&self.counters),
        }))
    }
}

impl Connection for ScriptedConnection {
    fn aborter(&self) -> Arc<dyn Abort> {
        Arc::new(LineAbort {
            line: Arc::clone(&self.line),
            counters: Arc::clone(&self.counters),
        })
    }

    fn send(&mut self) -> Result<Head, HttpError> {
        if self.silent {
            let mut aborted = self.line.aborted.lock().expect("BUG: the line's lock");
            while !*aborted {
                aborted = self
                    .line
                    .changed
                    .wait(aborted)
                    .expect("BUG: the line's lock");
            }
        }
        if self.aborted() {
            return Err(HttpError::Connect {
                detail: "aborted".to_owned(),
            });
        }
        Ok(Head {
            status: StatusCode::OK,
            headers: self.headers.clone(),
        })
    }

    fn read(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        if self.aborted() {
            return Err(HttpError::Protocol {
                detail: "aborted".to_owned(),
            });
        }
        if let Body::Chunks(total) = self.body
            && self.served == total
        {
            return Ok(None);
        }
        self.served += 1;
        self.counters.read.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(Some(vec![7; 8]))
    }
}
