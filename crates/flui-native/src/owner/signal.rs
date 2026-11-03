//! Coalesced, loop-scoped owner turns. No user work crosses the sender boundary.
//!
//! The signal carries admission and scheduling state only; the owner-turn
//! callback waits between turns in a [`TurnSlot`] passed to
//! [`OwnerSignal::register_in`] and `OwnerSignal::drive_in`. A backend
//! that keeps an `OwnerTurnSlot` (Windows only) in owner-only state therefore never has
//! its callback dropped by whichever thread closes or drops the last
//! `Arc<OwnerSignal>`. Backends not yet converted use the signal's own
//! shared slot through [`OwnerSignal::register`] and [`OwnerSignal::drive`].
//!
//! A posted turn is begun before the loop reads anything a wake asked it to
//! read ([`OwnerSignal::begin_turn`]), and driven after: a wake that lands
//! while the loop reads posts the next turn, which reads again.
/// Work a window asks its owner to run in an owner turn of its own (a frame
/// requested from another thread), through [`OwnerSignal::window_wake`].
pub(crate) type WindowWake = Arc<dyn Fn() + Send + Sync>;

use super::panic_boundary::contain_owner_callback;
use crate::{
    OpenWindowError, OpenedWindow, PendingWindow, PlatformError, ProxySendError,
    WakeRegistrationError, WindowRequest, platform::Platform,
};
use flui_foundation::{ClaimSlot, claim_slot};
use parking_lot::Mutex;
use std::collections::VecDeque;
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
    /// Windows whose work waits for an owner turn.
    window_wakes: Vec<WindowWake>,
    /// Window opens other threads asked for, oldest first, waiting for an
    /// owner turn; at most [`OPEN_LANE_CAPACITY`].
    opens: VecDeque<QueuedOpen>,
}

impl State {
    /// Whether anything waits for an owner turn.
    fn has_work(&self) -> bool {
        self.pending || self.quit || !self.window_wakes.is_empty() || !self.opens.is_empty()
    }
}

/// The most window opens from other threads the owner holds at once; one
/// more is refused with [`ProxySendError::Full`].
pub(crate) const OPEN_LANE_CAPACITY: usize = 64;

/// What an open from another thread resolves to.
type OpenWindowResult = Result<OpenedWindow, OpenWindowError>;

/// A window open another thread asked for: the request, and the slot its
/// [`PendingWindow`] waits on. Dropped unresolved, the slot answers
/// [`OpenWindowError::OwnerGone`].
struct QueuedOpen {
    request: WindowRequest,
    slot: ClaimSlot<OpenWindowResult>,
}

pub(crate) struct OwnerSignal {
    state: Mutex<State>,
    notify: Notify,
    owner: Mutex<ThreadId>,
    shared: SharedTurnSlot,
    /// The platform that opens the windows other threads ask for; `None`
    /// where the owner opens none for them.
    opener: Mutex<Option<Weak<dyn Platform>>>,
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
                window_wakes: Vec::new(),
                opens: VecDeque::new(),
            }),
            notify,
            owner: Mutex::new(std::thread::current().id()),
            shared: SharedTurnSlot::default(),
            opener: Mutex::new(None),
        })
    }
    /// Let the owner open the windows other threads ask for
    /// ([`PlatformProxy::open_window`](crate::PlatformProxy::open_window))
    /// through `platform`, whose loop keeps each until it closes, as it keeps
    /// the windows the owner opens itself.
    pub(crate) fn open_windows_through(&self, platform: &Arc<dyn Platform>) {
        *self.opener.lock() = Some(Arc::downgrade(platform));
    }
    /// Queue a window open for the owner's next turn.
    #[expect(
        clippy::result_large_err,
        reason = "the rejection hands the caller's request back for a retry; a window open is not a hot path"
    )]
    fn enqueue_open(
        &self,
        request: WindowRequest,
    ) -> Result<PendingWindow, ProxySendError<WindowRequest>> {
        if self.opener.lock().is_none() {
            return Err(ProxySendError::Unsupported { rejected: request });
        }
        // No owner parks on an abandoned request: the drain finds it.
        let (slot, handle) = claim_slot::<OpenWindowResult>(Arc::new(|| {}));
        let refused = {
            let mut state = self.state.lock();
            if !state.accepting {
                Err(ProxySendError::OwnerGone { rejected: request })
            } else if state.opens.len() >= OPEN_LANE_CAPACITY {
                Err(ProxySendError::Full {
                    capacity: OPEN_LANE_CAPACITY,
                    rejected: request,
                })
            } else {
                state.opens.push_back(QueuedOpen { request, slot });
                match self.post_locked(&mut state) {
                    Ok(()) => Ok(()),
                    Err(source) => {
                        // The request goes back to its sender: nothing of it
                        // stays queued.
                        let QueuedOpen { request, slot } = state
                            .opens
                            .pop_back()
                            .expect("BUG: the open queued under this lock is the last one");
                        drop(state);
                        drop(slot);
                        Err(ProxySendError::WakeFailed {
                            rejected: request,
                            source,
                        })
                    }
                }
            }
        };
        refused.map(|()| PendingWindow::new(handle, self.owner()))
    }
    /// Open the windows other threads asked for, outside the state lock: a
    /// handler's factory and its window's first delivery are application
    /// code. Each request is answered: with its window, with why it did not
    /// open, or, when the owner stops meanwhile, with `OwnerGone`.
    fn run_opens(&self) {
        let opens = std::mem::take(&mut self.state.lock().opens);
        let opener = self.opener.lock().clone();
        for QueuedOpen { request, slot } in opens {
            // Each request is answered in its own containment: a requester's
            // waker that panics on delivery leaves the rest answered and the
            // turn able to end.
            contain_owner_callback(|| {
                let platform = opener
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .filter(|_| self.accepting());
                match platform {
                    Some(platform) if !slot.is_abandoned() => {
                        let result = open_requested(&*platform, request);
                        if let Err(unclaimed) = slot.deliver(result) {
                            // Its requester let go meanwhile; an opened window
                            // stays with the loop until it closes.
                            drop(unclaimed);
                        }
                    }
                    // Nobody waits for it, or the owner stops: the request goes
                    // unopened, and a waiting requester hears `OwnerGone`.
                    _ => drop((request, slot)),
                }
            });
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn fence(&self) {
        let mut state = self.state.lock();
        state.accepting = false;
        state.pending = false;
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
        self.post_locked(&mut state)
            .map_err(|source| ProxySendError::WakeFailed {
                rejected: (),
                source,
            })
    }
    /// Post the owner turn the work in `state` asks for, under its lock.
    fn post_locked(&self, state: &mut State) -> Result<(), PlatformError> {
        if state.running && state.has_work() && !state.queued && !state.active {
            // Backend-only nonblocking post, never a user callback. Serialize
            // posting with admission so another sender cannot acknowledge a
            // wake that the first sender has not successfully posted yet.
            (self.notify)()?;
            state.queued = true;
        }
        Ok(())
    }
    /// Ask for an owner turn that runs `resume`, a window's work for its
    /// owner; one queued run per window.
    pub(crate) fn run_for_window(&self, resume: WindowWake) {
        {
            let mut state = self.state.lock();
            if !state.accepting {
                return;
            }
            if !state
                .window_wakes
                .iter()
                .any(|queued| Arc::ptr_eq(queued, &resume))
            {
                state.window_wakes.push(resume);
            }
        }
        if let Err(error) = self.schedule() {
            contain_owner_callback(
                || tracing::error!(%error, "a window's owner turn could not be posted"),
            );
        }
    }

    /// The wake for a window's work in an owner turn of its own: `resume`
    /// runs there. It holds the signal weakly, so a queued turn never keeps
    /// the loop alive; `resume` holds its window weakly for the same reason.
    pub(crate) fn window_wake(
        signal: &Arc<Self>,
        resume: impl Fn() + Send + Sync + 'static,
    ) -> WindowWake {
        let signal = Arc::downgrade(signal);
        let resume: WindowWake = Arc::new(resume);
        Arc::new(move || {
            if let Some(signal) = signal.upgrade() {
                signal.run_for_window(Arc::clone(&resume));
            }
        })
    }

    /// Resume every window that asked, outside the state lock.
    fn run_window_wakes(&self) {
        let resumes = std::mem::take(&mut self.state.lock().window_wakes);
        for resume in resumes {
            contain_owner_callback(|| resume());
            contain_owner_callback(|| drop(resume));
        }
    }

    /// Begin the turn the loop was woken for: from here on a wake posts a
    /// turn of its own instead of joining this one.
    ///
    /// A loop that reads what wakes ask of it before the application's turn
    /// (a source a background thread fills: an X11 window's properties, host
    /// notices, the portal's appearance) begins the turn, reads, then drives
    /// it. A wake that lands while it reads finds this turn already reading,
    /// so it posts the next one, which reads again; were it folded into this
    /// turn, the drive would settle it and what it asked for would wait for
    /// some later, unrelated wake.
    pub(crate) fn begin_turn(&self) -> OwnerTurn<'_> {
        debug_assert_eq!(self.owner(), std::thread::current().id());
        self.state.lock().queued = false;
        OwnerTurn {
            signal: self,
            driven: false,
        }
    }
    /// One finite turn over the signal's own shared slot, begun and driven
    /// at once: for a loop that reads nothing of its own first.
    pub(crate) fn drive(&self) -> bool {
        self.begin_turn().drive()
    }
    /// [`drive`](Self::drive) over `slot`. `true` asks the native owner to
    /// perform its quit path.
    #[cfg(target_os = "windows")]
    pub(crate) fn drive_in(&self, slot: &dyn TurnSlot) -> bool {
        self.begin_turn().drive_in(slot)
    }
    /// Post the turn that work still owed asks for; a failure is logged, the
    /// work stays owed.
    fn rearm(&self) {
        if let Err(error) = self.schedule() {
            contain_owner_callback(|| tracing::error!(%error, "owner turn could not be rearmed"));
        }
    }
    /// Stops admission and scheduling, and drops the callback in the
    /// signal's own shared slot. A slot held by owner-side state is the
    /// owner's to clear.
    ///
    /// The window opens still queued are refused: each waiting requester
    /// hears `OwnerGone`.
    pub(crate) fn close(&self) {
        let (callback, opens) = {
            let mut state = self.state.lock();
            state.accepting = false;
            state.running = false;
            state.pending = false;
            state.queued = false;
            state.quit = false;
            (self.shared.take(), std::mem::take(&mut state.opens))
        };
        contain_owner_callback(|| drop(callback));
        for open in opens {
            contain_owner_callback(|| drop(open));
        }
    }
}
/// Open the window `request` asks for on the owner thread. A factory that
/// panics is contained and the open fails with what it said. So does a handler
/// that panics on its window's first delivery, but that window, which the loop
/// already holds, stays open as it would for an open on the owner itself.
fn open_requested(platform: &dyn Platform, request: WindowRequest) -> OpenWindowResult {
    let (options, handler) = request.build_on_owner()?;
    let opened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        platform.open_window(options, handler)
    }));
    let window = match opened {
        Ok(opened) => opened?,
        Err(payload) => {
            let message = format!(
                "opening the window panicked: {}",
                flui_foundation::panic::payload_text(&*payload)
                    .unwrap_or("non-string panic payload")
            );
            contain_owner_callback(|| tracing::error!("{message}"));
            flui_foundation::panic::discard_payload(payload);
            return Err(OpenWindowError::Backend { message });
        }
    };
    let id = OpenedWindow::new(&window);
    contain_owner_callback(|| drop(window));
    Ok(id)
}

impl Drop for OwnerSignal {
    fn drop(&mut self) {
        self.close();
    }
}

/// A turn the loop has begun ([`OwnerSignal::begin_turn`]) and has yet to
/// drive. Dropped without being driven (a read that unwinds, a loop that
/// returns early), it posts the next turn for whatever work is owed, so that
/// work never waits for an unrelated wake.
#[must_use = "a begun turn is driven once the loop has read what wakes asked of it"]
pub(crate) struct OwnerTurn<'signal> {
    signal: &'signal OwnerSignal,
    /// Set once the application's turn has run; an undriven turn rearms when
    /// dropped.
    driven: bool,
}

impl OwnerTurn<'_> {
    /// The application's turn over the signal's own shared slot. `true` asks
    /// the native owner to perform its quit path.
    pub(crate) fn drive(self) -> bool {
        let shared = &self.signal.shared;
        self.drive_in(shared)
    }
    /// The application's turn over `slot`, which the caller's owner-side
    /// state holds. `true` asks the native owner to perform its quit path.
    pub(crate) fn drive_in(mut self, slot: &dyn TurnSlot) -> bool {
        self.driven = true;
        let signal = self.signal;
        let mut callback = {
            let mut state = signal.state.lock();
            if state.active {
                return false;
            }
            if state.quit {
                state.quit = false;
                return true;
            }
            if state.accepting
                && state.running
                && (!state.window_wakes.is_empty() || !state.opens.is_empty())
            {
                // Windows other threads asked for open, and a window's work
                // runs, in a turn of their own, before the application's
                // turn; work they ask for gets a later turn.
                state.active = true;
                drop(state);
                signal.run_opens();
                signal.run_window_wakes();
                signal.state.lock().active = false;
                signal.rearm();
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
            let state = signal.state.lock();
            if state.accepting {
                callback = callback.and_then(|callback| slot.restore(callback));
            }
        }
        // Nested native loops during Drop must observe active and leave pending work alone.
        contain_owner_callback(|| drop(callback));
        signal.state.lock().active = false;
        signal.rearm();
        false
    }
}

impl Drop for OwnerTurn<'_> {
    fn drop(&mut self) {
        if !self.driven {
            self.signal.rearm();
        }
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
        request: WindowRequest,
    ) -> Result<PendingWindow, ProxySendError<WindowRequest>> {
        match self.signal.upgrade() {
            Some(signal) => signal.enqueue_open(request),
            None => Err(ProxySendError::OwnerGone { rejected: request }),
        }
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
