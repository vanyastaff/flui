//! Test support for the foundation layer (ADR-0243), behind the `testing` feature.
//!
//! A test that hands a future, a channel or a slot a [`Waker`] and checks how
//! often it was woken takes it from here instead of writing its own
//! [`Wake`] implementation.
//!
//! ```
//! use flui_foundation::testing::CountingWaker;
//!
//! let wakes = CountingWaker::new().shared();
//! let waker = wakes.waker();
//! waker.wake_by_ref();
//! waker.wake();
//! assert_eq!(wakes.count(), 2);
//! ```

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Wake, Waker};

/// A [`Wake`] that counts its wakes and can run an action on each one.
///
/// A waker crosses threads, so the counter is atomic and the action is
/// `Send + Sync`: the [`Waker`] contract requires both. The test keeps the
/// [`shared`](Self::shared) handle and reads [`count`](Self::count) or
/// [`was_woken`](Self::was_woken) while the wakers made from it are out.
pub struct CountingWaker {
    wakes: AtomicUsize,
    action: Option<Box<dyn Fn() + Send + Sync>>,
}

impl CountingWaker {
    /// A waker that only counts.
    #[must_use]
    pub fn new() -> Self {
        Self {
            wakes: AtomicUsize::new(0),
            action: None,
        }
    }

    /// A waker that counts, then runs `action` on every wake, inside the wake call.
    #[must_use]
    pub fn calling(action: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            wakes: AtomicUsize::new(0),
            action: Some(Box::new(action)),
        }
    }

    /// The shared handle a test keeps to read the count while wakers are out.
    #[must_use]
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// A [`Waker`] that wakes this counter.
    #[must_use]
    pub fn waker(self: &Arc<Self>) -> Waker {
        Waker::from(Arc::clone(self))
    }

    /// A [`Waker`] with captures of its own: each wake counts into this counter, then runs
    /// `action`, and `captures` are dropped when the last clone of the returned waker is.
    ///
    /// The counter outlives the waker, so a test reads the count after the captures'
    /// destructors ran: the retirement of a waker a future replaced or dropped.
    #[must_use]
    pub fn retiring_waker(
        self: &Arc<Self>,
        action: impl Fn() + Send + Sync + 'static,
        captures: impl Any + Send + Sync,
    ) -> Waker {
        Waker::from(Arc::new(RetiringWaker {
            counter: Arc::clone(self),
            action: Box::new(action),
            _captures: Box::new(captures),
        }))
    }

    /// How many times a [`Waker`] made from this one was woken.
    #[must_use]
    pub fn count(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }

    /// Whether a [`Waker`] made from this one was woken at least once.
    #[must_use]
    pub fn was_woken(&self) -> bool {
        self.count() > 0
    }
}

impl Default for CountingWaker {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CountingWaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CountingWaker")
            .field("wakes", &self.count())
            .field("calls_action", &self.action.is_some())
            .finish()
    }
}

impl Wake for CountingWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        if let Some(action) = &self.action {
            action();
        }
    }
}

/// The waker [`CountingWaker::retiring_waker`] returns.
struct RetiringWaker {
    counter: Arc<CountingWaker>,
    action: Box<dyn Fn() + Send + Sync>,
    _captures: Box<dyn Any + Send + Sync>,
}

impl Wake for RetiringWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.counter.wake_by_ref();
        (self.action)();
    }
}

/// A [`Waker`] whose every wake panics with `message` as a `&'static str` payload: the
/// user code a test needs to fail inside a wake call.
///
/// # Panics
///
/// The returned waker panics on every `wake` and `wake_by_ref`; making it does not.
#[must_use]
pub fn panicking_waker(message: &'static str) -> Waker {
    CountingWaker::calling(move || std::panic::panic_any(message))
        .shared()
        .waker()
}
