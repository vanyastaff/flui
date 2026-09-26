//! Coalesced, loop-scoped owner turns. No user work crosses the sender boundary.
//!
//! The signal carries admission and scheduling state only; the owner-turn
//! callback waits between turns in a [`TurnSlot`] passed to
//! [`OwnerSignal::register_in`] and [`OwnerSignal::drive_in`]. A backend
//! that keeps an `OwnerTurnSlot` (Windows only) in owner-only state therefore never has
//! its callback dropped by whichever thread closes or drops the last
//! `Arc<OwnerSignal>`. Backends not yet converted use the signal's own
//! shared slot through [`OwnerSignal::register`] and [`OwnerSignal::drive`].
use super::panic_boundary::contain_owner_callback;
use crate::{PlatformError, ProxySendError, WakeRegistrationError};
use parking_lot::Mutex;
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

type Notify = Arc<dyn Fn() -> Result<(), PlatformError> + Send + Sync>;
type Callback = Box<dyn FnMut() + Send>;

/// Where the owner-turn callback waits between turns. Every method is
/// called with the signal's state lock held and runs no user code.
pub(crate) trait TurnSlot {
    /// Installs `callback`, returning the one it replaces.
    fn replace(&self, callback: Callback) -> Option<Callback>;
    /// Leases the callback out for one turn.
    fn take(&self) -> Option<Callback>;
    /// Returns a leased callback unless a replacement was registered while
    /// it was out; hands it back when it was superseded.
    fn restore(&self, callback: Callback) -> Option<Callback>;
}

/// An owner-turn slot for owner-only state. `!Send` and `!Sync`, so the
/// callback in it is dropped wherever the owner drops the slot, never by the
/// thread that closes or drops the signal.
#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
pub(crate) struct OwnerTurnSlot(std::cell::RefCell<Option<Callback>>);

#[cfg(any(target_os = "windows", test))]
impl OwnerTurnSlot {
    /// Drops the registered callback, if any, on the calling (owner) thread.
    pub(crate) fn clear(&self) {
        let callback = self.0.borrow_mut().take();
        contain_owner_callback(|| drop(callback));
    }
}

#[cfg(any(target_os = "windows", test))]
impl TurnSlot for OwnerTurnSlot {
    fn replace(&self, callback: Callback) -> Option<Callback> {
        self.0.borrow_mut().replace(callback)
    }
    fn take(&self) -> Option<Callback> {
        self.0.borrow_mut().take()
    }
    fn restore(&self, callback: Callback) -> Option<Callback> {
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(callback);
            None
        } else {
            Some(callback)
        }
    }
}

/// The slot inside the signal itself, for backends whose owner-turn
/// callback has not moved into owner-only state: whichever thread closes or
/// drops the signal drops the callback.
#[derive(Default)]
struct SharedTurnSlot(Mutex<Option<Callback>>);

impl TurnSlot for SharedTurnSlot {
    fn replace(&self, callback: Callback) -> Option<Callback> {
        self.0.lock().replace(callback)
    }
    fn take(&self) -> Option<Callback> {
        self.0.lock().take()
    }
    fn restore(&self, callback: Callback) -> Option<Callback> {
        let mut slot = self.0.lock();
        if slot.is_none() {
            *slot = Some(callback);
            None
        } else {
            Some(callback)
        }
    }
}

struct State {
    accepting: bool,
    running: bool,
    pending: bool,
    queued: bool,
    active: bool,
    quit: bool,
}

pub(crate) struct OwnerSignal {
    state: Mutex<State>,
    notify: Notify,
    owner: Mutex<ThreadId>,
    shared: SharedTurnSlot,
}

impl OwnerSignal {
    pub(crate) fn new(notify: Notify) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                accepting: true,
                running: false,
                pending: false,
                queued: false,
                active: false,
                quit: false,
            }),
            notify,
            owner: Mutex::new(std::thread::current().id()),
            shared: SharedTurnSlot::default(),
        })
    }
    #[cfg(any(target_os = "macos", feature = "winit-backend"))]
    pub(crate) fn fence(&self) {
        let mut state = self.state.lock();
        state.accepting = false;
        state.pending = false;
    }
    #[cfg(feature = "winit-backend")]
    pub(crate) fn quitting(&self) -> bool {
        self.state.lock().quit
    }
    pub(crate) fn accepting(&self) -> bool {
        self.state.lock().accepting
    }
    pub(crate) fn bind_owner(&self) {
        let mut owner = self.owner.lock();
        *owner = std::thread::current().id();
    }
    pub(crate) fn owner(&self) -> ThreadId {
        *self.owner.lock()
    }
    /// Registers into the signal's own shared slot.
    pub(crate) fn register(&self, callback: Callback) -> Result<(), WakeRegistrationError> {
        self.register_in(&self.shared, callback)
    }
    /// Registers into `slot`, which the caller's owner-side state holds.
    pub(crate) fn register_in(
        &self,
        slot: &dyn TurnSlot,
        callback: Callback,
    ) -> Result<(), WakeRegistrationError> {
        debug_assert_eq!(self.owner(), std::thread::current().id());
        let mut incoming = Some(callback);
        let (previous, accepted) = {
            let state = self.state.lock();
            if state.accepting {
                (
                    incoming.take().and_then(|callback| slot.replace(callback)),
                    true,
                )
            } else {
                (None, false)
            }
        };
        contain_owner_callback(|| drop(previous));
        contain_owner_callback(|| drop(incoming));
        if accepted {
            Ok(())
        } else {
            Err(WakeRegistrationError::OwnerGone)
        }
    }
    pub(crate) fn start(&self) -> Result<(), ProxySendError<()>> {
        self.state.lock().running = true;
        self.schedule()
    }
    pub(crate) fn wake(&self) -> Result<(), ProxySendError<()>> {
        {
            let mut state = self.state.lock();
            if !state.accepting {
                return Err(ProxySendError::OwnerGone { rejected: () });
            }
            state.pending = true;
        }
        self.schedule()
    }
    pub(crate) fn request_quit(&self) -> Result<(), ProxySendError<()>> {
        {
            let mut state = self.state.lock();
            if !state.accepting && !state.quit {
                return Err(ProxySendError::OwnerGone { rejected: () });
            }
            state.accepting = false;
            state.quit = true;
            state.pending = false;
        }
        self.schedule()
    }
    fn schedule(&self) -> Result<(), ProxySendError<()>> {
        let mut state = self.state.lock();
        if state.running && (state.pending || state.quit) && !state.queued && !state.active {
            // Backend-only nonblocking post, never a user callback. Serialize
            // posting with admission so another sender cannot acknowledge a
            // wake that the first sender has not successfully posted yet.
            (self.notify)().map_err(|source| ProxySendError::WakeFailed {
                rejected: (),
                source,
            })?;
            state.queued = true;
        }
        Ok(())
    }
    /// One finite turn over the signal's own shared slot.
    pub(crate) fn drive(&self) -> bool {
        self.drive_in(&self.shared)
    }
    /// One finite turn over `slot`. `true` asks the native owner to perform its quit path.
    pub(crate) fn drive_in(&self, slot: &dyn TurnSlot) -> bool {
        debug_assert_eq!(self.owner(), std::thread::current().id());
        let mut callback = {
            let mut state = self.state.lock();
            state.queued = false;
            if state.active {
                return false;
            }
            if state.quit {
                state.quit = false;
                return true;
            }
            if !state.accepting || !state.running || !state.pending {
                return false;
            }
            state.pending = false;
            state.active = true;
            slot.take()
        };
        contain_owner_callback(|| {
            if let Some(callback) = callback.as_mut() {
                callback();
            }
        });
        {
            let state = self.state.lock();
            if state.accepting {
                callback = callback.and_then(|callback| slot.restore(callback));
            }
        }
        // Nested native loops during Drop must observe active and leave pending work alone.
        contain_owner_callback(|| drop(callback));
        self.state.lock().active = false;
        if let Err(error) = self.schedule() {
            contain_owner_callback(|| tracing::error!(%error, "owner turn could not be rearmed"));
        }
        false
    }
    /// Stops admission and scheduling, and drops the callback in the
    /// signal's own shared slot. A slot held by owner-side state is the
    /// owner's to clear.
    pub(crate) fn close(&self) {
        let callback = {
            let mut state = self.state.lock();
            state.accepting = false;
            state.running = false;
            state.pending = false;
            state.queued = false;
            state.quit = false;
            self.shared.take()
        };
        contain_owner_callback(|| drop(callback));
    }
}
impl Drop for OwnerSignal {
    fn drop(&mut self) {
        self.close();
    }
}

pub(crate) struct SignalTransport {
    signal: Weak<OwnerSignal>,
    owner: ThreadId,
}
impl SignalTransport {
    pub(crate) fn new(signal: &Arc<OwnerSignal>) -> Self {
        Self {
            signal: Arc::downgrade(signal),
            owner: signal.owner(),
        }
    }
}
impl crate::traits::owner::ProxyTransport for SignalTransport {
    fn open_window(
        &self,
        options: crate::WindowOptions,
    ) -> Result<crate::PendingWindow, ProxySendError<crate::WindowOptions>> {
        Err(ProxySendError::Unsupported { rejected: options })
    }
    fn wake(&self) -> Result<(), ProxySendError<()>> {
        self.signal
            .upgrade()
            .ok_or(ProxySendError::OwnerGone { rejected: () })?
            .wake()
    }
    fn request_quit(&self) -> Result<(), ProxySendError<()>> {
        self.signal
            .upgrade()
            .ok_or(ProxySendError::OwnerGone { rejected: () })?
            .request_quit()
    }
    fn owner_thread(&self) -> ThreadId {
        self.owner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn worker_burst_coalesces_and_reentrant_wake_gets_a_later_finite_turn() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let calls = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&calls);
        let weak = Arc::downgrade(&signal);
        signal
            .register(Box::new(move || {
                if recorded.fetch_add(1, Ordering::SeqCst) == 0 {
                    let signal = weak.upgrade().expect("live");
                    signal.wake().expect("reentrant wake");
                    assert!(!signal.drive());
                }
            }))
            .expect("register");
        signal.start().expect("start");
        let worker = Arc::clone(&signal);
        std::thread::spawn(move || {
            for _ in 0..100 {
                worker.wake().expect("wake");
            }
        })
        .join()
        .expect("worker");
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        assert!(!signal.drive());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        signal.close();
    }

    #[test]
    fn failed_quit_post_is_retryable_without_reopening_admission() {
        let posts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&posts);
        let signal = OwnerSignal::new(Arc::new(move || {
            if recorded.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(PlatformError::EventLoop {
                    message: "injected post failure".into(),
                })
            } else {
                Ok(())
            }
        }));
        signal.start().expect("start");
        assert!(matches!(
            signal.request_quit(),
            Err(ProxySendError::WakeFailed { .. })
        ));
        assert!(!signal.accepting());
        assert!(matches!(
            signal.wake(),
            Err(ProxySendError::OwnerGone { .. })
        ));
        signal.request_quit().expect("retry the still-latched quit");
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        assert!(signal.drive());
        signal.close();
        assert!(matches!(
            signal.request_quit(),
            Err(ProxySendError::OwnerGone { .. })
        ));
    }

    #[test]
    fn concurrent_sender_does_not_acknowledge_another_senders_failed_post() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let attempts = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&attempts);
        let signal = OwnerSignal::new(Arc::new(move || {
            if recorded.fetch_add(1, Ordering::SeqCst) == 0 {
                entered_tx.send(()).expect("notify first post");
                release_rx.lock().recv().expect("release first post");
                Err(PlatformError::EventLoop {
                    message: "injected first failure".into(),
                })
            } else {
                Ok(())
            }
        }));
        signal.start().expect("start");
        let first_signal = Arc::clone(&signal);
        let first = std::thread::spawn(move || first_signal.wake());
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("posting began");
        let second_signal = Arc::clone(&signal);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let second = std::thread::spawn(move || {
            done_tx.send(second_signal.wake()).expect("report second");
        });
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(20))
                .is_err()
        );
        release_tx.send(()).expect("release");
        assert!(matches!(
            first.join().expect("first"),
            Err(ProxySendError::WakeFailed { .. })
        ));
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("second returns")
            .expect("second post succeeds");
        second.join().expect("second");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        signal.close();
    }
    #[test]
    fn replacement_capture_drop_reentry_and_panic_keep_the_lease_active() {
        struct Capture {
            signal: Weak<OwnerSignal>,
            drops: Arc<AtomicUsize>,
            next_calls: Arc<AtomicUsize>,
            nested_calls: Arc<AtomicUsize>,
        }
        impl Drop for Capture {
            fn drop(&mut self) {
                self.drops.fetch_add(1, Ordering::SeqCst);
                let signal = self.signal.upgrade().expect("signal");
                signal.wake().expect("drop wake");
                signal.drive();
                self.nested_calls
                    .store(self.next_calls.load(Ordering::SeqCst), Ordering::SeqCst);
                panic!("injected capture destructor panic");
            }
        }
        let signal = OwnerSignal::new(Arc::new(|| Ok(())));
        let drops = Arc::new(AtomicUsize::new(0));
        let next_calls = Arc::new(AtomicUsize::new(0));
        let nested_calls = Arc::new(AtomicUsize::new(0));
        let capture = Capture {
            signal: Arc::downgrade(&signal),
            drops: Arc::clone(&drops),
            next_calls: Arc::clone(&next_calls),
            nested_calls: Arc::clone(&nested_calls),
        };
        let weak = Arc::downgrade(&signal);
        let next = Arc::clone(&next_calls);
        signal
            .register(Box::new(move || {
                let _ = &capture;
                let next = Arc::clone(&next);
                weak.upgrade()
                    .expect("signal")
                    .register(Box::new(move || {
                        next.fetch_add(1, Ordering::SeqCst);
                    }))
                    .expect("replacement");
            }))
            .expect("register");
        signal.start().expect("start");
        signal.wake().expect("wake");
        signal.drive();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(nested_calls.load(Ordering::SeqCst), 0);
        assert_eq!(next_calls.load(Ordering::SeqCst), 0);
        signal.drive();
        assert_eq!(next_calls.load(Ordering::SeqCst), 1);
        signal.close();
    }

    #[test]
    fn close_off_owner_does_not_drop_the_turn_callback_there() {
        struct Capture(Arc<Mutex<Option<ThreadId>>>);
        impl Drop for Capture {
            fn drop(&mut self) {
                *self.0.lock() = Some(std::thread::current().id());
            }
        }
        let dropped_on = Arc::new(Mutex::new(None));
        let signal = OwnerSignal::new(Arc::new(|| Ok(())));
        let slot = OwnerTurnSlot::default();
        let capture = Capture(Arc::clone(&dropped_on));
        signal
            .register_in(
                &slot,
                Box::new(move || {
                    let _ = &capture;
                }),
            )
            .expect("register");
        std::thread::spawn(move || {
            signal.close();
            drop(signal);
        })
        .join()
        .expect("worker");
        assert_eq!(
            *dropped_on.lock(),
            None,
            "closing and dropping the signal elsewhere leaves the owner's slot alone"
        );
        slot.clear();
        assert_eq!(*dropped_on.lock(), Some(std::thread::current().id()));
    }

    #[test]
    fn rejected_registration_releases_capture_and_stale_transport_keeps_owner_identity() {
        struct Capture(Arc<AtomicUsize>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let signal = OwnerSignal::new(Arc::new(|| Ok(())));
        let transport = SignalTransport::new(&signal);
        let original_owner = signal.owner();
        signal.close();
        let drops = Arc::new(AtomicUsize::new(0));
        let capture = Capture(Arc::clone(&drops));
        assert!(matches!(
            signal.register(Box::new(move || {
                let _ = &capture;
            })),
            Err(WakeRegistrationError::OwnerGone)
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(signal);
        std::thread::spawn(move || {
            use crate::traits::owner::ProxyTransport;
            assert_eq!(transport.owner_thread(), original_owner);
            assert!(matches!(
                transport.wake(),
                Err(ProxySendError::OwnerGone { .. })
            ));
        })
        .join()
        .expect("worker");
    }
}
