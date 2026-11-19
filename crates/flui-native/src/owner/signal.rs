//! Coalesced, loop-scoped owner turns. No user work crosses the sender boundary.
//!
//! The signal carries admission and scheduling state only; the owner-turn
//! callback waits between turns in the [`OwnerTurnSlot`] the host's loop keeps,
//! passed to [`OwnerSignal::register_in`] and `OwnerTurn::drive_in`. The slot
//! is owner-only state, so the callback is never `Send` and is dropped on the
//! owner thread, never by whichever thread closes or drops the last
//! `Arc<OwnerSignal>` (ADR-0327 §5).
//!
//! A posted turn is begun before the loop reads anything a wake asked it to
//! read ([`OwnerSignal::begin_turn`]), and driven after: a wake that lands
//! while the loop reads posts the next turn, which reads again.
/// Work a window asks its owner to run in an owner turn of its own (a frame
/// requested from another thread), through [`OwnerSignal::window_wake`].
pub(crate) type WindowWake = Arc<dyn Fn() + Send + Sync>;

use super::call_point::{Called, HandlerSlot, call, retire};
use super::panic_boundary::contain_owner_callback;
use super::slot::OwnerSlot;
use crate::{
    OpenWindowError, OpenedWindow, PendingWindow, PlatformError, ProxySendError,
    WakeRegistrationError, WindowRequest, platform::Platform,
};
use flui_foundation::{ClaimSlot, claim_slot, panic::FirstPanic};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::thread::ThreadId;

type Notify = Arc<dyn Fn() -> Result<(), PlatformError> + Send + Sync>;
pub(crate) type Callback = Box<dyn FnMut()>;
/// The owner thread's slot of the platform the signal asks through.
type PlatformSlot = OwnerSlot<std::rc::Weak<dyn Platform>>;

/// Where the owner-turn callback waits between turns: owner-only state the
/// host's loop keeps, `!Send` and `!Sync`, so the callback in it is dropped on
/// the owner thread, never by the thread that closes or drops the signal.
/// The signal reaches it only with its state lock held, and runs no user code
/// then.
#[derive(Default)]
pub(crate) struct OwnerTurnSlot(RefCell<Option<Callback>>);

impl OwnerTurnSlot {
    /// Drops the registered callback, if any, on the calling (owner) thread.
    pub(crate) fn clear(&self) {
        let callback = self.0.borrow_mut().take();
        retire(callback);
    }

    /// Whether a callback waits here for the next turn.
    fn is_registered(&self) -> bool {
        self.0.borrow().is_some()
    }
}

/// The owner-turn callback's slot, reached through the call point like every
/// other handler: a turn calls it with no borrow held, and a callback
/// registered while it ran replaces it.
impl HandlerSlot for OwnerTurnSlot {
    type Handler = Callback;

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

struct State {
    accepting: bool,
    running: bool,
    pending: bool,
    queued: bool,
    active: bool,
    quit: bool,
    /// Windows whose work waits for an owner turn.
    window_wakes: Vec<WindowWake>,
    /// Work that runs code the owner does not own (a message the queue
    /// refused, sent to its window), run once the turn released its state
    /// (`OwnerSignal::run_outside_turn`, which only the Windows host builds).
    outside_turn: Vec<WindowWake>,
    /// Window opens other threads asked for, oldest first, waiting for an
    /// owner turn; at most [`OPEN_LANE_CAPACITY`].
    opens: VecDeque<QueuedOpen>,
    /// Another thread asked the owner to ask the exit policy again;
    /// coalesced, settled in the owner's next turn.
    reevaluate_exit: bool,
    /// Whether the owner opens the windows other threads ask for.
    opens_windows: bool,
    /// The application's turn (the owner-turn callback) runs.
    application_turn: bool,
    /// A source handed the running application turn a change its caller
    /// has not confirmed delivering; the turn's end wakes the owner again.
    delivery_owed: bool,
    /// The first failure to post the turn a window asked for, said in the
    /// next owner turn: the window that asked may hold borrows a `tracing`
    /// subscriber takes, so nothing is said where it asks. The work stays
    /// owed either way.
    unreported_window_post: Option<String>,
}

impl State {
    /// Whether anything waits for an owner turn.
    fn has_work(&self) -> bool {
        self.pending || self.quit || self.has_owner_work()
    }

    /// Whether work the owner runs before the application's turn waits: a
    /// window's work, a window open, an exit-policy request.
    fn has_owner_work(&self) -> bool {
        !self.window_wakes.is_empty()
            || !self.outside_turn.is_empty()
            || !self.opens.is_empty()
            || self.reevaluate_exit
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
    /// The platform the owner opens the windows other threads ask for
    /// through, and asks the exit policy again through; `None` where the
    /// owner opens none for them. The platform itself is the owner thread's:
    /// the slot holds it in that thread's table, and no other thread reaches
    /// it (ADR-0327 §2).
    platform: Mutex<Option<Arc<PlatformSlot>>>,
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
                outside_turn: Vec::new(),
                opens: VecDeque::new(),
                reevaluate_exit: false,
                opens_windows: false,
                application_turn: false,
                delivery_owed: false,
                unreported_window_post: None,
            }),
            notify,
            owner: Mutex::new(std::thread::current().id()),
            platform: Mutex::new(None),
        })
    }
    /// Let the owner ask `platform`'s exit policy again when another thread
    /// asks
    /// ([`PlatformProxy::request_exit_policy_reevaluation`](crate::PlatformProxy::request_exit_policy_reevaluation)).
    /// Called on the owner thread, whose table keeps the platform.
    pub(crate) fn serve_platform(&self, platform: &Rc<dyn Platform>) {
        let slot = OwnerSlot::new();
        if slot.replace(Rc::downgrade(platform)).is_err() {
            tracing::error!("the owner's platform has no slot: owner slots are exhausted");
            return;
        }
        // The slot it replaces drops its value on this, its owner, thread.
        let replaced = self.platform.lock().replace(Arc::new(slot));
        drop(replaced);
    }
    /// [`serve_platform`](Self::serve_platform), and let the owner open the
    /// windows other threads ask for
    /// ([`PlatformProxy::open_window`](crate::PlatformProxy::open_window))
    /// through `platform`, whose loop keeps each until it closes, as it keeps
    /// the windows the owner opens itself.
    pub(crate) fn open_windows_through(&self, platform: &Rc<dyn Platform>) {
        self.serve_platform(platform);
        self.state.lock().opens_windows = true;
    }
    /// The owner's platform, on the owner thread while it lives.
    fn owner_platform(&self) -> Option<Rc<dyn Platform>> {
        // The slot is read outside the lock: reading it first drops what
        // slots dropped on other threads left with this one, and a
        // destructor there may ask this signal again.
        let slot = self.platform.lock().clone()?;
        slot.cloned().and_then(|platform| platform.upgrade())
    }
    /// Ask the owner to ask the exit policy again: on the owner, whose table
    /// holds the platform, at once; from another thread, in the owner's next
    /// turn, coalesced.
    ///
    /// # Errors
    ///
    /// `OwnerGone` once the owner stopped accepting work, `Unsupported` where
    /// it has no platform to ask through, `WakeFailed` when the turn could not
    /// be posted (the request stays owed).
    pub(crate) fn request_exit_policy_reevaluation(&self) -> Result<(), ProxySendError<()>> {
        if self.platform.lock().is_none() {
            return Err(ProxySendError::Unsupported { rejected: () });
        }
        if !self.accepting() {
            return Err(ProxySendError::OwnerGone { rejected: () });
        }
        if let Some(platform) = self.owner_platform() {
            // The owner asks its own platform: no turn is spent on it.
            platform.request_exit_policy_reevaluation();
            return Ok(());
        }
        {
            let mut state = self.state.lock();
            if !state.accepting {
                return Err(ProxySendError::OwnerGone { rejected: () });
            }
            state.reevaluate_exit = true;
        }
        self.schedule()
    }
    /// Ask the platform's exit policy again, when another thread asked.
    fn run_exit_reevaluation(&self) {
        if !std::mem::take(&mut self.state.lock().reevaluate_exit) {
            return;
        }
        if let Some(platform) = self.owner_platform().filter(|_| self.accepting()) {
            contain_owner_callback(|| platform.request_exit_policy_reevaluation());
        }
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
        if !self.state.lock().opens_windows {
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
        let platform = self.owner_platform();
        for QueuedOpen { request, slot } in opens {
            // Each request is answered in its own containment: a requester's
            // waker that panics on delivery leaves the rest answered and the
            // turn able to end.
            contain_owner_callback(|| {
                let platform = platform.as_ref().filter(|_| self.accepting());
                match platform {
                    Some(platform) if !slot.is_abandoned() => {
                        let result = open_requested(&**platform, request);
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
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn fence(&self) {
        let mut state = self.state.lock();
        state.accepting = false;
        state.pending = false;
    }
    pub(crate) fn accepting(&self) -> bool {
        self.state.lock().accepting
    }
    /// Whether the application's turn runs now: the owner-turn callback,
    /// which reads what the sources report and delivers it. The owner-work
    /// part of a turn (window opens, a window's work) is not one. Owner
    /// thread.
    #[cfg(any(target_os = "android", target_os = "macos", test))]
    pub(crate) fn in_application_turn(&self) -> bool {
        self.state.lock().application_turn
    }
    /// A source handed the application's running turn a change its caller
    /// has yet to deliver: unless [`confirm_delivery`](Self::confirm_delivery)
    /// follows within the turn, the turn's end wakes the owner once more.
    /// Outside the application's turn it does nothing; a source wakes the
    /// owner there.
    #[cfg(any(target_os = "android", target_os = "macos", test))]
    pub(crate) fn owe_delivery(&self) {
        let mut state = self.state.lock();
        if state.application_turn {
            state.delivery_owed = true;
        }
    }
    /// The caller delivered what the running turn was owed.
    #[cfg(any(target_os = "android", target_os = "macos", test))]
    pub(crate) fn confirm_delivery(&self) {
        self.state.lock().delivery_owed = false;
    }
    pub(crate) fn bind_owner(&self) {
        let mut owner = self.owner.lock();
        *owner = std::thread::current().id();
    }
    pub(crate) fn owner(&self) -> ThreadId {
        *self.owner.lock()
    }
    /// Registers into `slot`, which the host's loop keeps.
    pub(crate) fn register_in(
        &self,
        slot: &OwnerTurnSlot,
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
            self.state
                .lock()
                .unreported_window_post
                .get_or_insert_with(|| error.to_string());
        }
    }

    /// Ask for an owner turn that runs `work` once the turn has released its
    /// state, as a message taken from the queue would run: `work` reaches code
    /// the owner does not own (a window procedure), so a modal loop it opens
    /// still gets owner turns. A caller coalesces its own requests; each one
    /// runs once.
    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn run_outside_turn(&self, work: WindowWake) {
        {
            let mut state = self.state.lock();
            if !state.accepting {
                return;
            }
            state.outside_turn.push(work);
        }
        if let Err(error) = self.schedule() {
            self.state
                .lock()
                .unreported_window_post
                .get_or_insert_with(|| error.to_string());
        }
    }

    /// Run what waits for the turn's state to be released, each item apart;
    /// once one of them stops the owner, the rest are dropped unrun.
    fn run_outside_turn_work(&self) {
        let work = std::mem::take(&mut self.state.lock().outside_turn);
        for item in work {
            if self.accepting() {
                contain_owner_callback(|| item());
            }
            contain_owner_callback(|| drop(item));
        }
    }

    /// How many items wait to run outside a turn.
    #[cfg(all(test, target_os = "windows"))]
    pub(crate) fn outside_turn_len_for_test(&self) -> usize {
        self.state.lock().outside_turn.len()
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
        let (resumes, unreported) = {
            let mut state = self.state.lock();
            (
                std::mem::take(&mut state.window_wakes),
                state.unreported_window_post.take(),
            )
        };
        if let Some(error) = unreported {
            contain_owner_callback(
                || tracing::error!(%error, "a window's owner turn could not be posted"),
            );
        }
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
    /// One finite turn over `slot`, begun and driven at once: for a loop that
    /// reads nothing of its own first. `true` asks the native owner to perform
    /// its quit path.
    #[cfg_attr(
        not(feature = "scripted"),
        allow(
            dead_code,
            reason = "the Windows display source and the headless host's test controls drive a \
                      turn at once: another target built without the controls never does"
        )
    )]
    pub(crate) fn drive_in(&self, slot: &OwnerTurnSlot) -> bool {
        self.begin_turn().drive_in(slot)
    }
    /// Post the turn that work still owed asks for; a failure is logged, the
    /// work stays owed.
    fn rearm(&self) {
        if let Err(error) = self.schedule() {
            contain_owner_callback(|| tracing::error!(%error, "owner turn could not be rearmed"));
        }
    }
    /// Stops admission and scheduling. The callback in the host's turn slot is
    /// the owner's to drop: [`close_in`](Self::close_in) does both.
    ///
    /// The window opens still queued are refused: each waiting requester
    /// hears `OwnerGone`.
    pub(crate) fn close(&self) {
        let opens = {
            let mut state = self.state.lock();
            state.accepting = false;
            state.running = false;
            state.pending = false;
            state.queued = false;
            state.quit = false;
            state.reevaluate_exit = false;
            std::mem::take(&mut state.opens)
        };
        for open in opens {
            contain_owner_callback(|| drop(open));
        }
    }
    /// [`close`](Self::close), then drops the callback in `slot` on this, the
    /// owner, thread.
    pub(crate) fn close_in(&self, slot: &OwnerTurnSlot) {
        self.close();
        slot.clear();
    }
}
/// Open the window `request` asks for on the owner thread. A factory that
/// panics is contained and the open fails with what it said. So does a handler
/// that panics on its window's first delivery, but that window, which the loop
/// already holds, stays open as it would for an open on the owner itself.
fn open_requested(platform: &dyn Platform, request: WindowRequest) -> OpenWindowResult {
    let (options, handler) = request.build_on_owner()?;
    let mut caught = FirstPanic::new();
    let Some(opened) = caught.contain(|| platform.open_window(options, handler)) else {
        let payload = caught.into_payload();
        let message = format!(
            "opening the window panicked: {}",
            payload
                .as_deref()
                .and_then(flui_foundation::panic::payload_text)
                .unwrap_or("non-string panic payload")
        );
        if let Some(payload) = payload {
            flui_foundation::panic::discard_payload(payload);
        }
        contain_owner_callback(|| tracing::error!("{message}"));
        return Err(OpenWindowError::Backend { message });
    };
    let window = opened?;
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
    /// The application's turn over `slot`, which the host's loop keeps.
    /// `true` asks the native owner to perform its quit path.
    pub(crate) fn drive_in(mut self, slot: &OwnerTurnSlot) -> bool {
        self.driven = true;
        let signal = self.signal;
        {
            let mut state = signal.state.lock();
            if state.active {
                return false;
            }
            if state.quit {
                state.quit = false;
                return true;
            }
            if state.accepting && state.running && state.has_owner_work() {
                // Windows other threads asked for open, an exit-policy request
                // from another thread and a window's work run in a turn of
                // their own, before the application's turn; work they ask for
                // gets a later turn.
                state.active = true;
                drop(state);
                signal.run_opens();
                signal.run_exit_reevaluation();
                signal.run_window_wakes();
                signal.state.lock().active = false;
                // A turn a message sent here asks for nests and runs.
                signal.run_outside_turn_work();
                signal.rearm();
                return false;
            }
            if !state.accepting || !state.running || !state.pending {
                return false;
            }
            // With no callback registered the wake stays owed: registering
            // one schedules the turn that runs it.
            if !slot.is_registered() {
                return false;
            }
            state.pending = false;
            state.active = true;
            state.application_turn = true;
        }
        // The call point drops a callback the turn displaced, or one the
        // closed signal no longer takes back, before `active` clears: nested
        // native loops during its `Drop` observe the turn and leave pending
        // work alone. The callback is the runtime's own owner work; its panic
        // is contained and logged here.
        if let Called::Panicked(failure) = call(&LeasedTurn { signal, slot }, |callback| callback())
        {
            failure.contain("the owner-turn callback");
        }
        {
            let mut state = signal.state.lock();
            // A change the turn was handed and did not deliver (its host
            // went, its delivery failed, the callback panicked) is read again
            // in the next turn.
            let undelivered = std::mem::take(&mut state.delivery_owed)
                && std::mem::take(&mut state.application_turn);
            state.application_turn = false;
            state.active = false;
            if undelivered && state.accepting {
                state.pending = true;
            }
        }
        signal.rearm();
        false
    }
}

/// The turn slot as one turn of a signal leases it: the callback goes back
/// only while the signal still accepts work, so one that ran as the loop
/// closed is dropped, on the owner thread.
struct LeasedTurn<'a> {
    signal: &'a OwnerSignal,
    slot: &'a OwnerTurnSlot,
}

impl HandlerSlot for LeasedTurn<'_> {
    type Handler = Callback;

    fn take(&self) -> Option<Callback> {
        self.slot.take()
    }

    fn restore(&self, callback: Callback) -> Option<Callback> {
        if self.signal.state.lock().accepting {
            self.slot.restore(callback)
        } else {
            Some(callback)
        }
    }

    fn replace(&self, callback: Callback) -> Option<Callback> {
        self.slot.replace(callback)
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
    fn request_exit_policy_reevaluation(&self) -> Result<(), ProxySendError<()>> {
        self.signal
            .upgrade()
            .ok_or(ProxySendError::OwnerGone { rejected: () })?
            .request_exit_policy_reevaluation()
    }
    fn owner_thread(&self) -> ThreadId {
        self.owner
    }
}

#[cfg(test)]
mod tests;
