//! The blocking client over a scripted stack: what a request answers, how
//! its body waits for the reader, and how a dropped request, a dropped body
//! and a passed deadline end the request's thread.

use std::sync::Arc;
use std::sync::atomic::Ordering as AtomicOrdering;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use flui_platform::http::{HeaderName, HeaderValue, Http, HttpError, HttpRequest, StatusCode};

use super::{BlockingClient, CHUNKS_IN_FLIGHT};
use crate::testing::http::{Body, Scripted};

/// Polls `future` to its end on this thread; a future still pending after
/// five seconds fails the test.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let thread = std::thread::current();
    let waker = flui_foundation::testing::CountingWaker::calling(move || thread.unpark())
        .shared()
        .waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
        assert!(Instant::now() < until, "timed out waiting for a future");
        std::thread::park_timeout(Duration::from_millis(50));
    }
}

/// Waits up to five seconds for `done`.
fn eventually(what: &str, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn http(stack: Scripted) -> Http {
    Http::new(Arc::new(BlockingClient::new(stack)))
}

/// A request answers its head and its whole body, and a stack's failure
/// reaches the caller as it is.
#[test]
fn a_request_on_its_own_thread_answers_its_head_and_body() {
    let (stack, counters) = Scripted::new(Body::Chunks(3));
    let http = http(stack);
    let response =
        block_on(http.send(HttpRequest::get("https://example.test/a"))).expect("the stack answers");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(block_on(response.into_body().into_bytes()), Ok(vec![7; 24]));
    eventually("the request's thread leaves", || {
        counters.closed.load(AtomicOrdering::SeqCst) == 1
    });
    assert_eq!(counters.aborted.load(AtomicOrdering::SeqCst), 0);

    assert_eq!(
        block_on(http.send(HttpRequest::get("ftp://example.test/a"))).map(|_| ()),
        Err(HttpError::InvalidUrl {
            url: "ftp://example.test/a".to_owned()
        })
    );

    let (mut declared, _) = Scripted::new(Body::Chunks(1));
    declared.headers.append(
        HeaderName::try_new("content-length").expect("a token"),
        HeaderValue::try_new("1000").expect("a value"),
    );
    assert_eq!(
        block_on(
            Http::new(Arc::new(BlockingClient::new(declared)))
                .send(HttpRequest::get("https://example.test/large").with_body_limit(100))
        )
        .map(|_| ()),
        Err(HttpError::TooLarge { limit: 100 })
    );
}

/// The request's thread reads ahead only as far as the queue holds, and a
/// body dropped while the thread waits for room aborts the connection and
/// ends the thread.
#[test]
fn a_body_left_unread_holds_the_download_until_it_is_dropped() {
    let (stack, counters) = Scripted::new(Body::Endless);
    let http = http(stack);
    let response = block_on(http.send(HttpRequest::get("https://example.test/stream")))
        .expect("the stack answers");
    // The queue fills, and one more chunk waits for room.
    eventually("the queue fills", || {
        counters.read.load(AtomicOrdering::SeqCst) == CHUNKS_IN_FLIGHT + 1
    });
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        counters.read.load(AtomicOrdering::SeqCst),
        CHUNKS_IN_FLIGHT + 1,
        "no chunk is read past the queue"
    );
    let mut body = response.into_body();
    assert_eq!(block_on(body.next_chunk()), Some(Ok(vec![7; 8])));
    eventually("a chunk read makes room for one more", || {
        counters.read.load(AtomicOrdering::SeqCst) == CHUNKS_IN_FLIGHT + 2
    });
    drop(body);
    eventually("the dropped body ends the request's thread", || {
        counters.closed.load(AtomicOrdering::SeqCst) == 1
    });
    assert_eq!(counters.aborted.load(AtomicOrdering::SeqCst), 1);
}

/// A request dropped before its head aborts the connection and ends its
/// thread; a passed deadline does the same and answers `Timeout`, before the
/// head and while the reader holds the body unread.
#[test]
fn a_request_left_or_past_its_deadline_aborts_its_connection() {
    let (mut silent, counters) = Scripted::new(Body::Endless);
    silent.silent = true;
    let http = http(silent);
    let pending = http.send(HttpRequest::get("https://example.test/silent"));
    std::thread::sleep(Duration::from_millis(20));
    drop(pending);
    eventually("the dropped request ends its thread", || {
        counters.closed.load(AtomicOrdering::SeqCst) == 1
    });
    assert_eq!(counters.aborted.load(AtomicOrdering::SeqCst), 1);

    let late = block_on(http.send(
        HttpRequest::get("https://example.test/silent").with_deadline(Duration::from_millis(50)),
    ));
    assert_eq!(late.map(|_| ()), Err(HttpError::Timeout));
    eventually("the deadline ends the request's thread", || {
        counters.closed.load(AtomicOrdering::SeqCst) == 2
    });

    let (stack, counters) = Scripted::new(Body::Endless);
    let held = block_on(Http::new(Arc::new(BlockingClient::new(stack))).send(
        HttpRequest::get("https://example.test/stream").with_deadline(Duration::from_millis(100)),
    ))
    .expect("the head arrives within the deadline");
    eventually(
        "a body held unread past its deadline ends the thread",
        || counters.closed.load(AtomicOrdering::SeqCst) == 1,
    );
    assert_eq!(counters.aborted.load(AtomicOrdering::SeqCst), 1);
    assert_eq!(
        block_on(held.into_body().into_bytes()),
        Err(HttpError::Timeout),
        "the reader learns the deadline passed after the chunks it left"
    );
}
