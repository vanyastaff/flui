//! The delivery machinery every presentation-local observation shares: a FIFO of
//! committed events, listeners that are cancelled by dropping their token, and
//! panic containment around user callbacks (ADR-0123).
//!
//! The app lifecycle, the host's system notices and a window's state are three
//! instances of [`Observation`]; they differ in the event type and in whether
//! the latest event is kept as a snapshot.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

use crate::lifecycle::LifecycleClosed;

type Callback<E> = Box<dyn FnMut(&E)>;
pub(crate) type Panic = Box<dyn std::any::Any + Send>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Open,
    Closing,
    Closed,
}

struct Listener<E> {
    active: Cell<bool>,
    callback: RefCell<Option<Callback<E>>>,
}

struct Event<E> {
    value: E,
    listeners: Vec<Weak<Listener<E>>>,
}

struct State<E> {
    current: Option<E>,
    phase: Phase,
    listeners: Vec<Rc<Listener<E>>>,
    pending: VecDeque<Event<E>>,
    draining: bool,
    // Caught drain failures also fence retirement from nested callbacks.
    retirement_failed: bool,
    preserving_close: bool,
    finish_requested: bool,
    // Open close windows: a terminal close or its recovery is in progress.
    closing: usize,
}

impl<E> State<E> {
    /// Whether a rejected or cancelled callback is retained rather than
    /// dropped: after a caught drain failure, or while a preserving close is
    /// still in progress. Once that close returns, stale handles retire their
    /// callbacks normally again (ADR-0123).
    fn retains_retired(&self) -> bool {
        self.retirement_failed || (self.preserving_close && (self.closing > 0 || self.draining))
    }
}

struct Inner<E> {
    state: RefCell<State<E>>,
    /// Whether the latest committed event is kept for [`WeakObservation::snapshot`].
    keeps_latest: bool,
}

/// The owner's end of one observation: commits events, drains them to the
/// listeners and closes the observation.
pub(crate) struct Observation<E: Clone + 'static> {
    inner: Rc<Inner<E>>,
}

/// A consumer's weak end of one observation.
pub(crate) struct WeakObservation<E: Clone + 'static> {
    inner: Weak<Inner<E>>,
}

impl<E: Clone + 'static> Clone for WeakObservation<E> {
    fn clone(&self) -> Self {
        Self {
            inner: Weak::clone(&self.inner),
        }
    }
}

/// Cancels a listener on drop, including a callback not yet started during a
/// walk. Does not retain the observation or the callback.
pub(crate) struct Subscription<E: Clone + 'static> {
    source: Weak<Inner<E>>,
    listener: Weak<Listener<E>>,
}

/// Holds the in-progress window of a terminal close until dropped.
pub(crate) struct CloseWindow<E: Clone + 'static> {
    inner: Rc<Inner<E>>,
}

impl<E: Clone + 'static> Observation<E> {
    /// An observation that keeps its latest event as a snapshot.
    pub(crate) fn keeping_latest() -> Self {
        Self::with(true)
    }

    /// An observation of one-off events: nothing is kept after delivery.
    pub(crate) fn transient() -> Self {
        Self::with(false)
    }

    fn with(keeps_latest: bool) -> Self {
        Self {
            inner: Rc::new(Inner {
                state: RefCell::new(State {
                    current: None,
                    phase: Phase::Open,
                    listeners: Vec::new(),
                    pending: VecDeque::new(),
                    draining: false,
                    retirement_failed: false,
                    preserving_close: false,
                    finish_requested: false,
                    closing: 0,
                }),
                keeps_latest,
            }),
        }
    }

    pub(crate) fn downgrade(&self) -> WeakObservation<E> {
        WeakObservation {
            inner: Rc::downgrade(&self.inner),
        }
    }

    pub(crate) fn latest(&self) -> Option<E> {
        self.inner.state.borrow().current.clone()
    }

    pub(crate) fn close_window(&self) -> CloseWindow<E> {
        self.inner.state.borrow_mut().closing += 1;
        CloseWindow {
            inner: Rc::clone(&self.inner),
        }
    }

    /// Fence new subscriptions and ordinary commits before terminal callbacks.
    pub(crate) fn begin_close(&self) {
        let mut state = self.inner.state.borrow_mut();
        if state.phase == Phase::Open {
            state.phase = Phase::Closing;
        }
    }

    /// Seed terminal recovery: a failure-preserving close drops queued events.
    pub(crate) fn begin_close_preserving(&self, preserving: bool) {
        self.begin_close();
        if preserving {
            let mut state = self.inner.state.borrow_mut();
            state.preserving_close = true;
            state.pending.clear();
        }
    }

    /// Commit an event in `phase` without calling user code, when `accept`
    /// agrees given the latest kept event.
    ///
    /// # Errors
    /// [`LifecycleClosed`] when the observation is not in `phase`.
    pub(crate) fn commit(
        &self,
        value: E,
        phase: Phase,
        accept: impl FnOnce(Option<&E>) -> bool,
    ) -> Result<(), LifecycleClosed> {
        let mut state = self.inner.state.borrow_mut();
        if state.phase != phase {
            return Err(LifecycleClosed);
        }
        if !accept(state.current.as_ref()) {
            return Ok(());
        }
        if self.inner.keeps_latest {
            state.current = Some(value.clone());
        }
        if !state.preserving_close {
            let listeners = state.listeners.iter().map(Rc::downgrade).collect();
            state.pending.push_back(Event { value, listeners });
        }
        Ok(())
    }

    /// Ask the next drain to release every listener once its events are out.
    pub(crate) fn request_finish(&self) {
        self.inner.state.borrow_mut().finish_requested = true;
    }

    /// Drain committed events in FIFO order. Nested drains defer to this walk.
    /// All eligible callbacks run before the first panic is returned.
    pub(crate) fn drain(&self) -> Option<Panic> {
        {
            let mut state = self.inner.state.borrow_mut();
            if state.draining {
                return None;
            }
            state.draining = true;
            state.retirement_failed = false;
        }
        let mut first = None;
        loop {
            let event = self.inner.state.borrow_mut().pending.pop_front();
            let Some(event) = event else {
                break;
            };
            for listener in event.listeners {
                let Some(listener) = listener.upgrade() else {
                    continue;
                };
                if !listener.active.get() {
                    continue;
                }
                let callback = listener.callback.borrow_mut().take();
                let Some(mut callback) = callback else {
                    continue;
                };
                preserve(
                    &mut first,
                    catch_unwind(AssertUnwindSafe(|| callback(&event.value))).err(),
                );
                if first.is_some() {
                    self.inner.state.borrow_mut().retirement_failed = true;
                }
                if listener.active.get() && self.inner.state.borrow().phase != Phase::Closed {
                    let previous = listener.callback.replace(Some(callback));
                    drop(previous);
                } else {
                    retire_callback(Some(callback), &mut first);
                    if first.is_some() {
                        self.inner.state.borrow_mut().retirement_failed = true;
                    }
                }
            }
        }
        if self.inner.state.borrow().finish_requested {
            self.release(&mut first);
        }
        {
            let mut state = self.inner.state.borrow_mut();
            state.draining = false;
            state.retirement_failed = false;
        }
        first
    }

    /// Invalidate the observation and release its callbacks. Cleanup completes
    /// before a callback-capture destructor panic is reported through `first`.
    pub(crate) fn release(&self, first: &mut Option<Panic>) {
        let listeners = {
            let mut state = self.inner.state.borrow_mut();
            state.phase = Phase::Closed;
            state.pending.clear();
            std::mem::take(&mut state.listeners)
        };
        for listener in listeners {
            listener.active.set(false);
            let callback = listener.callback.borrow_mut().take();
            let (failed, preserving) = {
                let state = self.inner.state.borrow();
                (state.retirement_failed, state.preserving_close)
            };
            if failed || preserving {
                std::mem::forget(callback);
            } else {
                retire_callback(callback, first);
            }
            if first.is_some() {
                let mut state = self.inner.state.borrow_mut();
                state.retirement_failed = true;
                state.preserving_close = true;
            }
        }
    }
}

impl<E: Clone + 'static> WeakObservation<E> {
    /// The latest kept event.
    ///
    /// # Errors
    /// [`LifecycleClosed`] after terminal notification or owner drop.
    pub(crate) fn snapshot(&self) -> Result<Option<E>, LifecycleClosed> {
        let inner = self.inner.upgrade().ok_or(LifecycleClosed)?;
        let state = inner.state.borrow();
        if state.phase == Phase::Closed {
            Err(LifecycleClosed)
        } else {
            Ok(state.current.clone())
        }
    }

    /// Register future events and atomically return the latest kept one. The
    /// callback is never invoked synchronously by this method.
    ///
    /// # Errors
    /// [`LifecycleClosed`] once terminal close begins or the owner dies; the
    /// rejected callback is retired by the same rule as a cancelled one.
    pub(crate) fn subscribe(
        &self,
        callback: Callback<E>,
    ) -> Result<(Option<E>, Subscription<E>), LifecycleClosed> {
        let Some(inner) = self.inner.upgrade() else {
            retire_rejected_callback(callback, false);
            return Err(LifecycleClosed);
        };
        let mut state = inner.state.borrow_mut();
        if state.phase != Phase::Open {
            let prior_failure = state.retains_retired();
            drop(state);
            retire_rejected_callback(callback, prior_failure);
            return Err(LifecycleClosed);
        }
        let listener = Rc::new(Listener {
            active: Cell::new(true),
            callback: RefCell::new(Some(callback)),
        });
        let token = Subscription {
            source: Rc::downgrade(&inner),
            listener: Rc::downgrade(&listener),
        };
        state.listeners.push(listener);
        Ok((state.current.clone(), token))
    }
}

impl<E: Clone + 'static> Drop for Subscription<E> {
    fn drop(&mut self) {
        let Some(listener) = self.listener.upgrade() else {
            return;
        };
        listener.active.set(false);
        let (removed, prior_failure) = self.source.upgrade().map_or((None, false), |source| {
            let mut state = source.state.borrow_mut();
            let prior_failure = state.retains_retired();
            let removed = state
                .listeners
                .iter()
                .position(|entry| Rc::ptr_eq(entry, &listener))
                .map(|index| state.listeners.remove(index));
            (removed, prior_failure)
        });
        // A leased callback is dropped by the dispatcher after invocation.
        let callback = listener.callback.borrow_mut().take();
        drop(removed);
        let mut first = None;
        if prior_failure {
            std::mem::forget(callback);
        } else {
            retire_callback(callback, &mut first);
        }
        if let Some(payload) = first {
            resume_unwind(payload);
        }
    }
}

impl<E: Clone + 'static> Drop for CloseWindow<E> {
    fn drop(&mut self) {
        let mut state = self.inner.state.borrow_mut();
        state.closing = state.closing.saturating_sub(1);
    }
}

pub(crate) fn preserve(first: &mut Option<Panic>, next: Option<Panic>) {
    flui_foundation::panic::FirstPanic::fold(first, next);
}

// Rejected admission owns the generic callback too. It must release source
// borrows before ordinary retirement, and cannot compete with an earlier failure.
fn retire_rejected_callback<E>(callback: Callback<E>, prior_failure: bool) {
    if prior_failure || std::thread::panicking() {
        std::mem::forget(callback);
    } else {
        drop(callback);
    }
}

/// Retire one envelope only while no failure already owns the unwind.
fn retire_callback<E>(callback: Option<Callback<E>>, first: &mut Option<Panic>) {
    if first.is_some() || std::thread::panicking() {
        std::mem::forget(callback);
    } else {
        preserve(
            first,
            catch_unwind(AssertUnwindSafe(|| drop(callback))).err(),
        );
    }
}
