//! An [`HttpClient`] over a blocking network stack: each request runs on a
//! thread of its own and answers through a channel, so its future and its
//! body need no runtime of the caller's.
//!
//! The body travels through a queue of [`CHUNKS_IN_FLIGHT`] chunks: the
//! request's thread reads the next chunk only when the reader made room, so a
//! reader that stops reading stops the download. Dropping the request's
//! future before its head, or its body before the end, aborts the connection
//! from the dropping thread, and the request's deadline aborts it from the
//! client's watchdog thread with [`HttpError::Timeout`]; either way the
//! request's thread leaves at its next step, a wait for room included.
//!
//! The stack is a [`Stack`]: Android's `HttpURLConnection` on the device, a
//! scripted one in the tests beside this file.

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use flui_platform::http::{
    BodySource, Headers, HttpAuthority, HttpClient, HttpError, HttpFuture, HttpRequest,
    HttpResponse, ResponseBody, StatusCode,
};
use tokio::sync::oneshot;

/// Body chunks a response holds unread before the request's thread waits for
/// the reader.
pub(crate) const CHUNKS_IN_FLIGHT: usize = 4;

/// A response's status and headers.
pub(crate) struct Head {
    pub(crate) status: StatusCode,
    pub(crate) headers: Headers,
}

/// A blocking network stack.
pub(crate) trait Stack: Send + Sync + 'static {
    /// The connection for `request`, not yet sent. Runs on the request's
    /// thread.
    ///
    /// # Errors
    ///
    /// [`HttpError::InvalidUrl`] for a URL the stack cannot request, or the
    /// stack's own failure.
    fn open(&self, request: &HttpRequest) -> Result<Box<dyn Connection>, HttpError>;
}

/// One request's connection, used from the request's thread.
pub(crate) trait Connection: Send {
    /// What aborts this connection from another thread.
    fn aborter(&self) -> Arc<dyn Abort>;

    /// Send the request and read the response's head. Blocks.
    ///
    /// # Errors
    ///
    /// What the stack reports; an aborted connection fails here.
    fn send(&mut self) -> Result<Head, HttpError>;

    /// The body's next chunk, `None` at its end. Blocks.
    ///
    /// # Errors
    ///
    /// What the stack reports; an aborted connection fails here.
    fn read(&mut self) -> Result<Option<Vec<u8>>, HttpError>;
}

/// Aborts a connection from any thread: its blocked call fails at once.
pub(crate) trait Abort: Send + Sync {
    /// Abort the connection; harmless once it ended.
    fn abort(&self);
}

/// An [`HttpClient`] over `S`.
pub(crate) struct BlockingClient<S> {
    stack: Arc<S>,
    watchdog: Arc<Watchdog>,
    /// What [`HttpClient::authority`] names.
    authority: Arc<()>,
}

impl<S: Stack> BlockingClient<S> {
    pub(crate) fn new(stack: S) -> Self {
        Self {
            stack: Arc::new(stack),
            watchdog: Arc::default(),
            authority: Arc::default(),
        }
    }
}

impl<S> Drop for BlockingClient<S> {
    fn drop(&mut self) {
        self.watchdog.close();
    }
}

impl<S: Stack> HttpClient for BlockingClient<S> {
    fn authority(&self) -> HttpAuthority {
        HttpAuthority::of(&self.authority)
    }

    fn send(&self, request: HttpRequest) -> HttpFuture<HttpResponse> {
        let exchange = Arc::new(Exchange::default());
        // `None` past what the clock can name: no deadline to keep.
        let due = Instant::now().checked_add(request.deadline());
        if let Some(due) = due
            && let Err(error) = self.watchdog.watch(due, &exchange)
        {
            return failed(error);
        }
        let (head_tx, head_rx) = oneshot::channel();
        let stack = Arc::clone(&self.stack);
        let thread = Arc::clone(&exchange);
        let spawned = std::thread::Builder::new()
            .name("flui-http".to_owned())
            .spawn(move || run(stack.as_ref(), &request, &thread, head_tx));
        if let Err(error) = spawned {
            return failed(HttpError::Host {
                detail: format!("starting the request's thread: {error}"),
            });
        }
        Box::pin(HeadFuture {
            head: head_rx,
            exchange,
            answered: false,
        })
    }
}

fn failed(error: HttpError) -> HttpFuture<HttpResponse> {
    Box::pin(std::future::ready(Err(error)))
}

/// Runs `request` on its own thread: answers its head on `head_tx`, then
/// queues its body for the reader.
fn run(
    stack: &dyn Stack,
    request: &HttpRequest,
    exchange: &Arc<Exchange>,
    head_tx: oneshot::Sender<Result<HttpResponse, HttpError>>,
) {
    let limit = request.body_limit();
    let mut connection = match stack.open(request) {
        Ok(connection) => connection,
        Err(error) => return refuse(exchange, head_tx, error),
    };
    if !exchange.arm(connection.aborter()) {
        return refuse(exchange, head_tx, HttpError::Cancelled);
    }
    let head = match connection.send() {
        Ok(head) => head,
        Err(error) => return refuse(exchange, head_tx, error),
    };
    let declared = head
        .headers
        .get("content-length")
        .and_then(|length| length.to_str())
        .and_then(|length| length.trim().parse::<u64>().ok());
    if declared.is_some_and(|length| length > limit) {
        return refuse(exchange, head_tx, HttpError::TooLarge { limit });
    }
    let body = Chunks {
        exchange: Arc::clone(exchange),
    };
    let response = HttpResponse::new(head.status, head.headers, ResponseBody::new(body, limit));
    if head_tx.send(Ok(response)).is_err() {
        // The response, and with it its body, dropped: the exchange stopped.
        return;
    }
    loop {
        match connection.read() {
            Ok(Some(chunk)) => {
                if !exchange.push(chunk) {
                    return;
                }
            }
            Ok(None) => return exchange.finish(None),
            Err(error) => return exchange.finish(Some(error)),
        }
    }
}

/// Answer the head with `error`, unless the exchange stopped: a passed
/// deadline overrides what the aborted stack reported, and a request nobody
/// waits for answers nothing.
fn refuse(
    exchange: &Exchange,
    head_tx: oneshot::Sender<Result<HttpResponse, HttpError>>,
    error: HttpError,
) {
    let answer = match exchange.stopped() {
        Some(Stop::Left) => return,
        Some(Stop::Timeout) => HttpError::Timeout,
        None => error,
    };
    let _ = head_tx.send(Err(answer));
}

/// Why an exchange stopped before its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// Nobody waits for it: its future or its body dropped.
    Left,
    /// Its deadline passed.
    Timeout,
}

/// What the request's thread and its reader share.
#[derive(Default)]
struct Exchange {
    state: Mutex<State>,
    /// Signalled when the reader makes room, and when the exchange stops.
    room: Condvar,
}

#[derive(Default)]
struct State {
    chunks: VecDeque<Vec<u8>>,
    /// Set once the request's thread reported the body's end (`Ok`) or its
    /// failure.
    ended: Option<Result<(), HttpError>>,
    stop: Option<Stop>,
    /// The reader waiting for a chunk.
    reader: Option<Waker>,
    /// What aborts the connection, once it is open.
    abort: Option<Arc<dyn Abort>>,
}

impl Exchange {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Nothing panics while the lock is held; a poisoned one is still
        // whole.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stopped(&self) -> Option<Stop> {
        self.lock().stop
    }

    /// Stop the exchange for `why`, once: abort its connection and wake its
    /// reader and its thread. The first reason stays.
    fn stop(&self, why: Stop) {
        let (abort, reader) = {
            let mut state = self.lock();
            if state.stop.is_some() || state.ended.is_some() {
                return;
            }
            state.stop = Some(why);
            (state.abort.take(), state.reader.take())
        };
        self.room.notify_all();
        // Outside the lock: the abort calls into the stack, the waker into
        // whoever reads.
        if let Some(abort) = abort {
            abort.abort();
        }
        if let Some(reader) = reader {
            reader.wake();
        }
    }

    /// Hand over what aborts the open connection; `false`, with the
    /// connection aborted, when the exchange already stopped.
    fn arm(&self, abort: Arc<dyn Abort>) -> bool {
        {
            let mut state = self.lock();
            if state.stop.is_none() {
                state.abort = Some(abort);
                return true;
            }
        }
        abort.abort();
        false
    }

    /// Queue `chunk` once there is room; `false` when the exchange stopped.
    fn push(&self, chunk: Vec<u8>) -> bool {
        let reader = {
            let mut state = self.lock();
            while state.stop.is_none() && state.chunks.len() >= CHUNKS_IN_FLIGHT {
                state = self
                    .room
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if state.stop.is_some() {
                return false;
            }
            state.chunks.push_back(chunk);
            state.reader.take()
        };
        if let Some(reader) = reader {
            reader.wake();
        }
        true
    }

    /// The body ended (`None`) or failed.
    fn finish(&self, failure: Option<HttpError>) {
        let reader = {
            let mut state = self.lock();
            if state.stop.is_some() {
                // The reader learns why from the stop itself.
                return;
            }
            state.ended = Some(failure.map_or(Ok(()), Err));
            state.abort = None;
            state.reader.take()
        };
        if let Some(reader) = reader {
            reader.wake();
        }
    }
}

/// The head of a request in flight; dropping it before the head arrives
/// aborts the request.
struct HeadFuture {
    head: oneshot::Receiver<Result<HttpResponse, HttpError>>,
    exchange: Arc<Exchange>,
    answered: bool,
}

impl Future for HeadFuture {
    type Output = Result<HttpResponse, HttpError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let answer = std::task::ready!(Pin::new(&mut self.head).poll(cx));
        self.answered = true;
        // A thread that left unanswered is one that died.
        Poll::Ready(answer.unwrap_or(Err(HttpError::Cancelled)))
    }
}

impl Drop for HeadFuture {
    fn drop(&mut self) {
        if !self.answered {
            self.exchange.stop(Stop::Left);
        }
    }
}

/// A response's body, read from the exchange's queue.
struct Chunks {
    exchange: Arc<Exchange>,
}

impl BodySource for Chunks {
    fn poll_chunk(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Vec<u8>, HttpError>>> {
        // Cloned before the lock: a waker's clone is the caller's code.
        let waker = cx.waker().clone();
        let (answer, replaced) = {
            let mut state = self.exchange.lock();
            if let Some(chunk) = state.chunks.pop_front() {
                (Poll::Ready(Some(Ok(chunk))), None)
            } else if let Some(ended) = state.ended.take() {
                state.ended = Some(Ok(()));
                (Poll::Ready(ended.err().map(Err)), None)
            } else if state.stop == Some(Stop::Timeout) {
                (Poll::Ready(Some(Err(HttpError::Timeout))), None)
            } else {
                (Poll::Pending, state.reader.replace(waker))
            }
        };
        if answer.is_ready() {
            self.exchange.room.notify_all();
        }
        // The waker it replaced drops outside the lock.
        drop(replaced);
        answer
    }
}

impl Drop for Chunks {
    fn drop(&mut self) {
        self.exchange.stop(Stop::Left);
    }
}

/// A client's deadlines, kept by one thread that starts with the first
/// request and leaves once the client is gone and no deadline is left.
#[derive(Default)]
struct Watchdog {
    state: Mutex<Watch>,
    changed: Condvar,
}

#[derive(Default)]
struct Watch {
    due: BinaryHeap<Reverse<Due>>,
    /// Orders deadlines that fall on one instant.
    next: u64,
    running: bool,
    closed: bool,
}

/// One exchange's deadline.
struct Due {
    at: Instant,
    order: u64,
    exchange: Weak<Exchange>,
}

impl PartialEq for Due {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.order) == (other.at, other.order)
    }
}

impl Eq for Due {}

impl PartialOrd for Due {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Due {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, self.order).cmp(&(other.at, other.order))
    }
}

impl Watchdog {
    fn lock(&self) -> MutexGuard<'_, Watch> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stop `exchange` with [`Stop::Timeout`] at `at`.
    ///
    /// # Errors
    ///
    /// [`HttpError::Host`] when the watchdog's thread cannot start.
    fn watch(self: &Arc<Self>, at: Instant, exchange: &Arc<Exchange>) -> Result<(), HttpError> {
        let start = {
            let mut watch = self.lock();
            let order = watch.next;
            watch.next = watch.next.wrapping_add(1);
            watch.due.push(Reverse(Due {
                at,
                order,
                exchange: Arc::downgrade(exchange),
            }));
            !std::mem::replace(&mut watch.running, true)
        };
        self.changed.notify_all();
        if start {
            let watchdog = Arc::clone(self);
            let spawned = std::thread::Builder::new()
                .name("flui-http-deadlines".to_owned())
                .spawn(move || watchdog.keep());
            if let Err(error) = spawned {
                let mut watch = self.lock();
                watch.running = false;
                watch.due.clear();
                return Err(HttpError::Host {
                    detail: format!("starting the deadline thread: {error}"),
                });
            }
        }
        Ok(())
    }

    fn close(&self) {
        self.lock().closed = true;
        self.changed.notify_all();
    }

    /// The watchdog's thread: stops each exchange whose deadline passed.
    fn keep(&self) {
        let mut watch = self.lock();
        loop {
            let now = Instant::now();
            let mut passed = Vec::new();
            while watch.due.peek().is_some_and(|Reverse(due)| due.at <= now) {
                if let Some(Reverse(due)) = watch.due.pop() {
                    passed.push(due.exchange);
                }
            }
            if !passed.is_empty() {
                drop(watch);
                // Outside the lock: a stop aborts into the stack.
                for exchange in passed {
                    if let Some(exchange) = exchange.upgrade() {
                        exchange.stop(Stop::Timeout);
                    }
                }
                watch = self.lock();
                continue;
            }
            let next = watch.due.peek().map(|Reverse(due)| due.at);
            watch = match next {
                Some(at) => {
                    self.changed
                        .wait_timeout(watch, at.saturating_duration_since(now))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None if watch.closed => {
                    watch.running = false;
                    return;
                }
                None => self
                    .changed
                    .wait(watch)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }
}

#[cfg(test)]
mod tests;
