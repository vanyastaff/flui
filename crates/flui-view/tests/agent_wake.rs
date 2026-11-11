//! An agent's answer tells a waker when it can be taken (ADR-0288 §3): when the owner sends it,
//! and when the owner goes without sending. The waker is the asker's code: the last one
//! registered is woken once, and one that panics leaves the answer sent and is reported.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Waker;

use flui_foundation::testing::{CountingWaker, panicking_waker};
use flui_protocol::ErrorCode;
use flui_view::__runtime::reply_pair;

#[test]
fn an_answer_wakes_the_last_waker_registered_once_when_it_is_sent() {
    let (reply, mut answer) = reply_pair::<u32>();
    let (first, last) = (CountingWaker::new().shared(), CountingWaker::new().shared());
    answer.register(&Waker::from(Arc::clone(&first)));
    answer.register(&Waker::from(Arc::clone(&last)));
    assert!(reply.send(Ok(7)));
    drop(reply);
    assert_eq!(first.count(), 0, "a waker replaced is not woken");
    assert_eq!(last.count(), 1, "the last waker registered is woken once");
    assert_eq!(answer.try_take().map(Result::ok), Some(Some(7)));
}

#[test]
fn an_answer_wakes_its_waker_when_the_owner_goes_without_sending() {
    let (reply, mut answer) = reply_pair::<u32>();
    let wakes = CountingWaker::new().shared();
    answer.register(&Waker::from(Arc::clone(&wakes)));
    let kept = reply.clone();
    drop(reply);
    assert_eq!(wakes.count(), 0, "a clone still holds the request");
    drop(kept);
    assert_eq!(wakes.count(), 1, "the last clone's drop wakes");
    let gone = answer
        .try_take()
        .expect("the answer can be taken once woken");
    assert_eq!(
        gone.map_err(|error| error.code),
        Err(ErrorCode::ShuttingDown)
    );
}

/// Counts the error events traced while it is the thread's subscriber.
#[derive(Clone, Default)]
struct Errors(Arc<AtomicUsize>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Errors {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[test]
fn a_waker_that_panics_leaves_the_answer_sent_and_is_reported() {
    use tracing_subscriber::layer::SubscriberExt as _;

    let (reply, mut answer) = reply_pair::<u32>();
    answer.register(&panicking_waker("the waker panics"));
    let errors = Errors::default();
    let subscriber = tracing_subscriber::registry().with(errors.clone());
    let sent = tracing::subscriber::with_default(subscriber, || reply.send(Ok(7)));
    assert!(sent, "the send stands, its wake contained");
    assert_eq!(
        errors.0.load(Ordering::SeqCst),
        1,
        "the waker's panic is reported once"
    );
    drop(reply);
    assert_eq!(answer.try_take().map(Result::ok), Some(Some(7)));
}
