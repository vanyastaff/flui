//! Coalesced, loop-scoped owner turns. No user work crosses the sender boundary.
//!
//! The signal carries admission and scheduling state only; the owner-turn
//! callback waits between turns in a [`TurnSlot`] passed to
//! [`OwnerSignal::register_in`] and [`OwnerSignal::drive_in`]. A backend
//! that keeps an `OwnerTurnSlot` (Windows only) in owner-only state therefore never has
//! its callback dropped by whichever thread closes or drops the last
//! `Arc<OwnerSignal>`. Backends not yet converted use the signal's own
//! shared slot through [`OwnerSignal::register`] and [`OwnerSignal::drive`].
use super::handlers::{InputWake, WindowCallbacks};
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
#[cfg(target_os = "windows")]
#[derive(Default)]
pub(crate) struct OwnerTurnSlot(std::cell::RefCell<Option<Callback>>);

#[cfg(target_os = "windows")]
impl OwnerTurnSlot {
    /// Drops the registered callback, if any, on the calling (owner) thread.
    pub(crate) fn clear(&self) {
        let callback = self.0.borrow_mut().take();
        contain_owner_callback(|| drop(callback));
    }
}

#[cfg(target_os = "windows")]
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
    /// Windows whose held input waits for an owner turn (ADR-0213 §3).
    input_resumes: Vec<InputWake>,
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
                input_resumes: Vec::new(),
            }),
            notify,
            owner: Mutex::new(std::thread::current().id()),
            shared: SharedTurnSlot::default(),
        })
    }
    #[cfg(any(
        target_os = "macos",
        all(
            feature = "winit-backend",
            any(target_os = "windows", target_os = "linux")
        )
    ))]
    pub(crate) fn fence(&self) {
        let mut state = self.state.lock();
        state.accepting = false;
        state.pending = false;
    }
    #[cfg(all(
        feature = "winit-backend",
        any(target_os = "windows", target_os = "macos", target_os = "linux")
    ))]
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
        if !accepted {
            return Err(WakeRegistrationError::OwnerGone);
        }
        // A wake that found no callback is owed to this one.
        if let Err(error) = self.schedule() {
            contain_owner_callback(
                || tracing::error!(%error, "owed owner turn could not be posted"),
            );
        }
        Ok(())
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
        let work = state.pending || state.quit || !state.input_resumes.is_empty();
        if state.running && work && !state.queued && !state.active {
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
    /// Ask for an owner turn that runs `resume`, a window's held input
    /// delivery; one queued run per window.
    pub(crate) fn resume_input(&self, resume: InputWake) {
        {
            let mut state = self.state.lock();
            if !state.accepting {
                return;
            }
            if !state
                .input_resumes
                .iter()
                .any(|queued| Arc::ptr_eq(queued, &resume))
            {
                state.input_resumes.push(resume);
            }
        }
        if let Err(error) = self.schedule() {
            contain_owner_callback(|| tracing::error!(%error, "input resume could not be posted"));
        }
    }

    /// The wake a window installs with
    /// [`WindowCallbacks::set_input_wake`](WindowCallbacks::set_input_wake).
    /// `resume` calls the window's
    /// [`WindowCallbacks::resume_input_delivery`](WindowCallbacks::resume_input_delivery)
    /// through a weak reference, so a queued turn never keeps a window alive.
    pub(crate) fn input_wake(
        signal: &Arc<Self>,
        resume: impl Fn() + Send + Sync + 'static,
    ) -> InputWake {
        let signal = Arc::downgrade(signal);
        let resume: InputWake = Arc::new(resume);
        Arc::new(move || {
            if let Some(signal) = signal.upgrade() {
                signal.resume_input(Arc::clone(&resume));
            }
        })
    }

    /// The wake for a window whose callbacks are shared.
    pub(crate) fn callbacks_wake(
        signal: &Arc<Self>,
        callbacks: &Arc<WindowCallbacks>,
    ) -> InputWake {
        let callbacks = Arc::downgrade(callbacks);
        Self::input_wake(signal, move || {
            if let Some(callbacks) = callbacks.upgrade() {
                callbacks.resume_input_delivery();
            }
        })
    }

    /// Resume every window that asked, outside the state lock.
    fn run_input_resumes(&self) {
        let resumes = std::mem::take(&mut self.state.lock().input_resumes);
        for resume in resumes {
            contain_owner_callback(|| resume());
            contain_owner_callback(|| drop(resume));
        }
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
            if state.accepting && state.running && !state.input_resumes.is_empty() {
                // Held window input resumes in its own turn, before the
                // application's turn; resumes it asks for get a later turn.
                state.active = true;
                drop(state);
                self.run_input_resumes();
                self.state.lock().active = false;
                if let Err(error) = self.schedule() {
                    contain_owner_callback(|| {
                        tracing::error!(%error, "owner turn could not be rearmed");
                    });
                }
                return false;
            }
            if !state.accepting || !state.running || !state.pending {
                return false;
            }
            // With no callback registered the wake stays owed: registering
            // one schedules the turn that runs it.
            let Some(callback) = slot.take() else {
                return false;
            };
            state.pending = false;
            state.active = true;
            Some(callback)
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
impl super::ProxyTransport for SignalTransport {
    fn open_window(
        &self,
        options: flui_platform::WindowOptions,
    ) -> Result<crate::PendingWindow, ProxySendError<flui_platform::WindowOptions>> {
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
