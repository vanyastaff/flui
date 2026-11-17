//! Waiting on the owner thread for a future, for a bounded time.
//!
//! The GPU context's pipeline cache is read from the application's storage
//! before the first window attaches, and that read completes on the
//! application's IO pool. An application may supply a pool the owner thread
//! itself drives (`DeterministicExecutors`, an embedder's loop), and then a
//! wait without a bound would never end.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;
use std::time::{Duration, Instant};

/// How long the first window waits for its pipeline cache to be read. A
/// read of the cache file takes a few milliseconds on a local disk; past
/// this the context starts without a cache.
pub(super) const PIPELINE_CACHE_LOAD_BOUND: Duration = Duration::from_millis(250);

/// Wakes the waiting thread.
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Poll `future` on this thread until it completes or `bound` has passed;
/// `None` past the bound, with the future dropped.
pub(super) fn wait_within<F: Future>(future: F, bound: Duration) -> Option<F::Output> {
    let deadline = Instant::now().checked_add(bound);
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut ctx = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut ctx) {
            return Some(output);
        }
        match deadline.map(|deadline| deadline.saturating_duration_since(Instant::now())) {
            Some(Duration::ZERO) => return None,
            Some(left) => std::thread::park_timeout(left),
            None => std::thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll};
    use std::time::Duration;

    use super::wait_within;

    /// Ready once another thread, woken by the first poll, has set `done`
    /// and woken the waiter.
    struct DoneElsewhere {
        done: Arc<AtomicBool>,
        started: bool,
    }

    impl Future for DoneElsewhere {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<()> {
            if self.done.load(Ordering::Acquire) {
                return Poll::Ready(());
            }
            if !self.started {
                self.started = true;
                let done = Arc::clone(&self.done);
                let waker = ctx.waker().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(10));
                    done.store(true, Ordering::Release);
                    waker.wake();
                });
            }
            Poll::Pending
        }
    }

    #[test]
    fn a_future_is_answered_when_ready_or_woken_and_dropped_at_the_bound() {
        assert_eq!(wait_within(async { 7 }, Duration::ZERO), Some(7));
        let woken = DoneElsewhere {
            done: Arc::new(AtomicBool::new(false)),
            started: false,
        };
        assert_eq!(wait_within(woken, Duration::from_secs(10)), Some(()));
        assert_eq!(
            wait_within(std::future::pending::<()>(), Duration::from_millis(20)),
            None
        );
    }
}
