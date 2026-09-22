//! Presentation-local lifecycle observation. Owners commit before notifying.
use flui_scheduler::AppLifecycleState;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

type Callback = Box<dyn FnMut(AppLifecycleState)>;
type Panic = Box<dyn std::any::Any + Send>;

/// The presentation has closed or no longer accepts subscriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the presentation lifecycle is closed")]
pub struct LifecycleClosed;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Open,
    #[cfg(any(test, feature = "runtime-internals"))]
    Closing,
    Closed,
}
struct Listener {
    active: Cell<bool>,
    callback: RefCell<Option<Callback>>,
}
struct Event {
    state: AppLifecycleState,
    listeners: Vec<Weak<Listener>>,
}
struct State {
    current: Option<AppLifecycleState>,
    phase: Phase,
    listeners: Vec<Rc<Listener>>,
    pending: VecDeque<Event>,
    draining: bool,
    finish_requested: bool,
}
struct Inner(RefCell<State>);

/// Weak, owner-local capability for one presentation's lifecycle history.
///
/// Acquire from `BuildContext::lifecycle_handle` in `init_state` or
/// `did_change_dependencies`. This is not the realm scheduler aggregate.
#[derive(Clone)]
pub struct LifecycleHandle {
    inner: Weak<Inner>,
}
impl std::fmt::Debug for LifecycleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleHandle").finish_non_exhaustive()
    }
}
impl LifecycleHandle {
    /// Latest committed observation, or `None` before the first observation.
    /// A queued callback can describe an older event than this snapshot.
    ///
    /// # Errors
    /// Returns [`LifecycleClosed`] after terminal notification or owner drop.
    pub fn snapshot(&self) -> Result<Option<AppLifecycleState>, LifecycleClosed> {
        let inner = self.inner.upgrade().ok_or(LifecycleClosed)?;
        let state = inner.0.borrow();
        if state.phase == Phase::Closed {
            Err(LifecycleClosed)
        } else {
            Ok(state.current)
        }
    }
    /// Register future events and atomically return the current observation.
    /// The callback is never invoked synchronously by this method. Events
    /// committed before registration are not replayed. Retain the returned token.
    ///
    /// Notifications run in FIFO order. Reentrant notifications queue behind the
    /// active walk instead of recursively invoking callbacks. The callback argument
    /// describes its event; [`Self::snapshot`] reads the latest committed state,
    /// which may already be newer after a reentrant commit.
    ///
    /// A callback panic does not skip eligible siblings or queued notifications.
    /// The owner resumes the first panic after delivery and cleanup complete.
    /// Cancellation releases callback captures outside source borrows; an active
    /// callback releases its captures after returning if it was cancelled. A
    /// capture-destructor panic cannot undo cancellation or skip siblings, and
    /// does not replace an earlier callback panic.
    ///
    /// # Errors
    /// Returns [`LifecycleClosed`] once terminal close begins or the owner dies.
    pub fn subscribe(
        &self,
        callback: impl FnMut(AppLifecycleState) + 'static,
    ) -> Result<(Option<AppLifecycleState>, LifecycleSubscription), LifecycleClosed> {
        let inner = self.inner.upgrade().ok_or(LifecycleClosed)?;
        let mut state = inner.0.borrow_mut();
        if state.phase != Phase::Open {
            return Err(LifecycleClosed);
        }
        let listener = Rc::new(Listener {
            active: Cell::new(true),
            callback: RefCell::new(Some(Box::new(callback))),
        });
        let token = LifecycleSubscription {
            source: Rc::downgrade(&inner),
            listener: Rc::downgrade(&listener),
        };
        state.listeners.push(listener);
        Ok((state.current, token))
    }
}

/// Cancels a lifecycle callback on drop, including callbacks not yet started
/// during a notification walk. Does not retain the presentation or callback.
#[must_use = "dropping the subscription cancels lifecycle observation"]
pub struct LifecycleSubscription {
    source: Weak<Inner>,
    listener: Weak<Listener>,
}
impl std::fmt::Debug for LifecycleSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleSubscription")
            .finish_non_exhaustive()
    }
}
impl Drop for LifecycleSubscription {
    fn drop(&mut self) {
        let Some(listener) = self.listener.upgrade() else {
            return;
        };
        listener.active.set(false);
        let removed = self.source.upgrade().and_then(|source| {
            let mut state = source.0.borrow_mut();
            state
                .listeners
                .iter()
                .position(|entry| Rc::ptr_eq(entry, &listener))
                .map(|index| state.listeners.remove(index))
        });
        // A leased callback is dropped by the dispatcher after invocation.
        let callback = listener.callback.borrow_mut().take();
        drop(removed);
        let failure = catch_unwind(AssertUnwindSafe(|| drop(callback))).err();
        if let Some(payload) = failure {
            if std::thread::panicking() {
                std::mem::forget(payload);
            } else {
                resume_unwind(payload);
            }
        }
    }
}

pub(crate) fn preserve(first: &mut Option<Panic>, next: Option<Panic>) {
    if let Some(payload) = next {
        if first.is_none() {
            *first = Some(payload);
        } else {
            std::mem::forget(payload);
        }
    }
}

/// Composition-root owner for a presentation lifecycle source.
///
/// Keep outside element/binding locks. Commit all local states before running
/// scheduler or lifecycle callbacks, then drain notifications. Build owners
/// receive only [`Self::handle`], never ownership of this source.
#[doc(hidden)]
pub struct LifecycleSource {
    inner: Rc<Inner>,
}
impl std::fmt::Debug for LifecycleSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleSource").finish_non_exhaustive()
    }
}
impl Default for LifecycleSource {
    fn default() -> Self {
        Self::new()
    }
}
impl LifecycleSource {
    /// Create an unobserved presentation source.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner(RefCell::new(State {
                current: None,
                phase: Phase::Open,
                listeners: Vec::new(),
                pending: VecDeque::new(),
                draining: false,
                finish_requested: false,
            }))),
        }
    }
    /// Weak owner-local consumer capability.
    #[must_use]
    pub fn handle(&self) -> LifecycleHandle {
        LifecycleHandle {
            inner: Rc::downgrade(&self.inner),
        }
    }
    /// Last committed local observation, including during terminal cleanup.
    #[must_use]
    #[cfg(any(test, feature = "runtime-internals"))]
    pub fn current(&self) -> Option<AppLifecycleState> {
        self.inner.0.borrow().current
    }
    /// Commit an ordinary observation without calling user code.
    /// # Errors
    /// Returns [`LifecycleClosed`] after terminal close begins.
    pub fn commit(&self, state: AppLifecycleState) -> Result<(), LifecycleClosed> {
        self.commit_for_phase(state, Phase::Open)
    }
    /// Fence new subscriptions and ordinary commits before terminal callbacks.
    #[cfg(any(test, feature = "runtime-internals"))]
    pub fn begin_close(&self) {
        let mut state = self.inner.0.borrow_mut();
        if state.phase == Phase::Open {
            state.phase = Phase::Closing;
        }
    }
    /// Commit an authorized terminal ladder step after `begin_close`.
    /// # Errors
    /// Returns [`LifecycleClosed`] outside the terminal notification phase.
    #[cfg(any(test, feature = "runtime-internals"))]
    pub fn commit_terminal(&self, state: AppLifecycleState) -> Result<(), LifecycleClosed> {
        self.commit_for_phase(state, Phase::Closing)
    }
    fn commit_for_phase(
        &self,
        current: AppLifecycleState,
        phase: Phase,
    ) -> Result<(), LifecycleClosed> {
        let mut state = self.inner.0.borrow_mut();
        if state.phase != phase {
            return Err(LifecycleClosed);
        }
        if state.current != Some(current) {
            state.current = Some(current);
            let listeners = state.listeners.iter().map(Rc::downgrade).collect();
            state.pending.push_back(Event {
                state: current,
                listeners,
            });
        }
        Ok(())
    }
    /// Drain committed events in FIFO order. Nested drains defer to this walk.
    /// All eligible callbacks run before the first panic resumes.
    pub fn drain(&self) {
        {
            let mut state = self.inner.0.borrow_mut();
            if state.draining {
                return;
            }
            state.draining = true;
        }
        let mut first = None;
        loop {
            let event = self.inner.0.borrow_mut().pending.pop_front();
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
                    catch_unwind(AssertUnwindSafe(|| callback(event.state))).err(),
                );
                if listener.active.get() && self.inner.0.borrow().phase != Phase::Closed {
                    let previous = listener.callback.replace(Some(callback));
                    drop(previous);
                } else {
                    preserve(
                        &mut first,
                        catch_unwind(AssertUnwindSafe(|| drop(callback))).err(),
                    );
                }
            }
        }
        self.inner.0.borrow_mut().draining = false;
        if self.inner.0.borrow().finish_requested {
            preserve(
                &mut first,
                catch_unwind(AssertUnwindSafe(|| self.release())).err(),
            );
        }
        if let Some(payload) = first {
            resume_unwind(payload);
        }
    }
    /// Invalidate capabilities and release callbacks before widget disposal.
    /// Cleanup completes before any callback-capture destructor panic resumes.
    #[cfg(any(test, feature = "runtime-internals"))]
    pub fn finish_close(&self) {
        self.begin_close();
        self.inner.0.borrow_mut().finish_requested = true;
        // A reentrant close is completed by the active FIFO walk after its
        // terminal event. It must not erase an event still awaiting delivery.
        self.drain();
    }
    fn release(&self) {
        let listeners = {
            let mut state = self.inner.0.borrow_mut();
            state.phase = Phase::Closed;
            state.pending.clear();
            std::mem::take(&mut state.listeners)
        };
        let mut first = None;
        for listener in listeners {
            listener.active.set(false);
            let callback = listener.callback.borrow_mut().take();
            preserve(
                &mut first,
                catch_unwind(AssertUnwindSafe(|| drop(callback))).err(),
            );
        }
        if let Some(payload) = first {
            resume_unwind(payload);
        }
    }
}
impl Drop for LifecycleSource {
    fn drop(&mut self) {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| self.release())) {
            std::mem::forget(payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AppLifecycleState::{Detached, Hidden, Inactive, Resumed};
    static_assertions::assert_not_impl_any!(LifecycleHandle: Send, Sync);
    static_assertions::assert_not_impl_any!(LifecycleSubscription: Send, Sync);

    #[test]
    fn lifecycle_subscription_snapshot_is_atomic_without_replay() {
        let source = LifecycleSource::new();
        let handle = source.handle();
        assert_eq!(handle.snapshot(), Ok(None));
        source.commit(Detached).expect("open");
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let (initial, _token) = handle
            .subscribe(move |state| log.borrow_mut().push(state))
            .expect("subscribe");
        assert_eq!(initial, Some(Detached));
        source.drain();
        assert!(seen.borrow().is_empty());
        source
            .commit(Resumed)
            .expect("observed Detached is reversible");
        source.drain();
        assert_eq!(*seen.borrow(), [Resumed]);
    }

    #[test]
    fn lifecycle_subscription_cancellation_suppresses_not_started_callbacks() {
        let source = LifecycleSource::new();
        let second = Rc::new(RefCell::new(None));
        let cancel = Rc::clone(&second);
        let (_, _first) = source
            .handle()
            .subscribe(move |_| {
                let removed = cancel.borrow_mut().take();
                drop(removed);
            })
            .expect("first");
        let (_, token) = source
            .handle()
            .subscribe(|_| panic!("cancelled callback ran"))
            .expect("second");
        second.replace(Some(token));
        source.commit(Hidden).expect("commit");
        source.drain();
    }

    #[test]
    fn lifecycle_subscription_reentrant_events_are_fifo_and_new_listeners_do_not_replay() {
        let source = Rc::new(LifecycleSource::new());
        let weak = Rc::downgrade(&source);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let new_tokens = Rc::new(RefCell::new(Vec::new()));
        let keep = Rc::clone(&new_tokens);
        let (_, _first) = source
            .handle()
            .subscribe(move |state| {
                log.borrow_mut().push((1, state));
                if state == Inactive {
                    let owner = weak.upgrade().expect("owner");
                    owner.commit(Hidden).expect("nested commit");
                    let (_, token) = owner
                        .handle()
                        .subscribe(|_| panic!("new subscriber got queued replay"))
                        .expect("late subscribe");
                    keep.borrow_mut().push(token);
                    owner.drain();
                }
            })
            .expect("first");
        let log = Rc::clone(&seen);
        let (_, _second) = source
            .handle()
            .subscribe(move |state| log.borrow_mut().push((2, state)))
            .expect("second");
        source.commit(Inactive).expect("commit");
        source.drain();
        assert_eq!(
            *seen.borrow(),
            [(1, Inactive), (2, Inactive), (1, Hidden), (2, Hidden)]
        );
    }

    #[test]
    fn lifecycle_subscription_self_cancel_destructor_panic_does_not_skip_siblings() {
        struct Bomb;
        impl Drop for Bomb {
            fn drop(&mut self) {
                panic!("capture destructor");
            }
        }
        for callback_panics in [false, true] {
            let source = LifecycleSource::new();
            let own = Rc::new(RefCell::new(None));
            let cancel = Rc::clone(&own);
            let bomb = Bomb;
            let (_, token) = source
                .handle()
                .subscribe(move |_| {
                    let _keep = &bomb;
                    let token = cancel.borrow_mut().take();
                    drop(token);
                    assert!(!callback_panics, "first callback");
                })
                .expect("first");
            own.replace(Some(token));
            let calls = Rc::new(Cell::new(0));
            let count = Rc::clone(&calls);
            let (_, _sibling) = source
                .handle()
                .subscribe(move |_| count.set(count.get() + 1))
                .expect("sibling");
            source.commit(Inactive).expect("commit");
            let payload =
                catch_unwind(AssertUnwindSafe(|| source.drain())).expect_err("first panic");
            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&if callback_panics {
                    "first callback"
                } else {
                    "capture destructor"
                })
            );
            assert_eq!(calls.get(), 1);
            source.commit(Hidden).expect("commit");
            source.drain();
            assert_eq!(calls.get(), 2);
        }
    }

    #[test]
    fn lifecycle_subscription_terminal_reentry_drains_before_invalidating() {
        let source = Rc::new(LifecycleSource::new());
        let handle = source.handle();
        let weak = Rc::downgrade(&source);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&seen);
        let (_, _first) = handle
            .subscribe(move |state| {
                log.borrow_mut().push((1, state));
                if state == Inactive {
                    let owner = weak.upgrade().expect("owner");
                    owner.begin_close();
                    assert!(owner.handle().subscribe(|_| {}).is_err());
                    assert_eq!(owner.commit(Resumed), Err(LifecycleClosed));
                    owner.commit_terminal(Detached).expect("terminal");
                    owner.finish_close();
                    assert_eq!(owner.handle().snapshot(), Ok(Some(Detached)));
                    panic!("earlier queued callback");
                }
            })
            .expect("first");
        let log = Rc::clone(&seen);
        let (_, _second) = handle
            .subscribe(move |state| log.borrow_mut().push((2, state)))
            .expect("second");
        source.commit(Inactive).expect("commit");
        let failure =
            catch_unwind(AssertUnwindSafe(|| source.drain())).expect_err("panic retained");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"earlier queued callback")
        );
        assert_eq!(
            *seen.borrow(),
            [(1, Inactive), (2, Inactive), (1, Detached), (2, Detached)]
        );
        assert_eq!(handle.snapshot(), Err(LifecycleClosed));
    }

    #[test]
    fn lifecycle_subscription_owner_drop_closes_retained_handle_and_build_owner() {
        let source = LifecycleSource::new();
        let mut owner = crate::BuildOwner::new();
        assert!(
            owner.lifecycle_handle().is_none(),
            "bare owners have no presentation"
        );
        owner.set_lifecycle_handle(source.handle());
        let handle = owner.lifecycle_handle().expect("installed");
        let (_, token) = handle.subscribe(|_| {}).expect("subscribe");
        drop(source);
        assert_eq!(handle.snapshot(), Err(LifecycleClosed));
        assert!(handle.subscribe(|_| {}).is_err());
        drop(token);
    }

    #[test]
    fn lifecycle_subscription_legacy_panic_still_delivers_scoped_terminal() {
        struct Legacy;
        impl crate::WidgetsBindingObserver for Legacy {
            fn did_change_app_lifecycle_state(&self, _: AppLifecycleState) {
                panic!("legacy first");
            }
        }
        let binding = crate::WidgetsBinding::new();
        binding.add_observer(std::sync::Arc::new(Legacy));
        let handle = binding.lifecycle_source().handle();
        let seen = Rc::new(Cell::new(None));
        let log = Rc::clone(&seen);
        let (_, _token) = handle
            .subscribe(move |state| log.set(Some(state)))
            .expect("scoped");
        binding.lifecycle_source().begin_close();
        binding
            .lifecycle_source()
            .commit_terminal(Detached)
            .expect("terminal commit");
        let failure = catch_unwind(AssertUnwindSafe(|| {
            binding.notify_committed_lifecycle(Detached);
        }))
        .expect_err("legacy panic");
        binding.lifecycle_source().finish_close();
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"legacy first"));
        assert_eq!(seen.get(), Some(Detached));
        assert_eq!(handle.snapshot(), Err(LifecycleClosed));
    }
}
