//! What a window delivers to the handler it was opened with (ADR-0254 §4).
//!
//! A host keeps one [`WindowDelivery`] per window. It holds the window's
//! [`WindowHandler`], made before the window's first event and kept in an
//! owner-thread slot, so it is reached, called and dropped only on the owner
//! thread. Every delivery goes through one causal FIFO: an event raised while
//! a handler method runs (a command that closes the window from its input, a
//! frame its state change requests) is queued behind the running method and
//! delivered after it returns, in order, whatever its kind. Each method is
//! called through the call point (`call_point::call`), with no lock or borrow
//! of the host held.
//!
//! A panicking method is contained: what was queued behind it is still
//! delivered, each in its own containment, and the first panic resumes
//! afterwards (ADR-0207 §7).

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use parking_lot::Mutex;

use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    CloseDecision, DispatchEventResult, FrameOutcome, FrameRequest, PlatformInput, SurfaceChange,
    WindowHandler,
};

use super::call_point::{Called, Failure, HandlerSlot, call, retire};
use super::frame_timing::{PresentationReports, frame_request_now};
use super::panic_boundary::contain_owner_callback;
use super::signal::WindowWake;
use super::slot::OwnerSlot;

/// Keys remembered as answered "not handled" while queued; a platform sends
/// its follow-up right after the answer, so a few suffice.
const UNANSWERED_KEYS: usize = 8;

/// Whether a window's native retirement has started, as any thread reads it.
///
/// Every host clears its window's delivery where the native window is
/// retired (Win32's window retirement, AppKit's `windowWillClose:`, the Linux hosts'
/// window retirement, the Android loop's exit, UIKit's scene teardown, the web
/// canvas's retirement, the headless close), so this is set before the
/// native window it describes is gone. A window's surface target and redraw
/// requester (ADR-0254 §5) live on other threads and outlive the window: they
/// read it to answer for a window that is no longer there.
///
/// It also carries what becomes of the native window once its handler is let
/// go ([`Self::settle`]): destroyed, as every close does, or hidden and kept
/// when the window's surface target asked for it
/// ([`SurfaceTarget::keep_past_close`](flui_platform::SurfaceTarget::keep_past_close),
/// ADR-0305 §9), because a raster thread that did not answer may still
/// present over a surface made from the window's handles.
#[derive(Clone, Debug, Default)]
pub(crate) struct Retirement(Arc<RetirementState>);

#[derive(Debug, Default)]
struct RetirementState {
    retired: AtomicBool,
    /// One of [`Fate::UNDECIDED`], [`Fate::KEPT`] and [`Fate::DESTROYED`]:
    /// it leaves `UNDECIDED` once, by a keep or by the host's settle, and
    /// never changes again.
    fate: AtomicU8,
}

/// The values of [`RetirementState::fate`].
struct Fate;

impl Fate {
    const UNDECIDED: u8 = 0;
    const KEPT: u8 = 1;
    const DESTROYED: u8 = 2;
}

/// What a host's retirement does with its native window once the handler was
/// let go ([`Retirement::settle`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeFate {
    /// Destroy it.
    Destroy,
    /// Hide it and keep it, and everything its handles name, for the life of
    /// the process: the window stays closed to the application.
    Keep,
}

impl Retirement {
    /// Whether the window's native retirement has started.
    pub(crate) fn is_retired(&self) -> bool {
        self.0.retired.load(Ordering::SeqCst)
    }

    /// Ask that the close hide the native window instead of destroying it.
    /// `true` for the call that kept it; `false` when an earlier call did, or
    /// when the host settled the window's fate already (its native window is
    /// destroyed, or the OS or another client destroyed it), in which case
    /// nothing changes.
    pub(crate) fn keep_past_close(&self) -> bool {
        self.0
            .fate
            .compare_exchange(
                Fate::UNDECIDED,
                Fate::KEPT,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Whether a keep reached the window before its fate was settled.
    pub(crate) fn is_kept(&self) -> bool {
        self.0.fate.load(Ordering::SeqCst) == Fate::KEPT
    }

    /// Settle the native window's fate, where the host's retirement is about
    /// to destroy it: [`NativeFate::Keep`] when a keep came first, and
    /// [`NativeFate::Destroy`] otherwise, after which a keep does nothing. A
    /// host whose native window the OS destroyed settles it too, before it
    /// lets go of the handler, so a keep made while that happens does
    /// nothing. Every later call answers the same.
    pub(crate) fn settle(&self) -> NativeFate {
        match self.0.fate.compare_exchange(
            Fate::UNDECIDED,
            Fate::DESTROYED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Err(Fate::KEPT) => NativeFate::Keep,
            Ok(_) | Err(_) => NativeFate::Destroy,
        }
    }
}

/// A window's handler as an owner-thread slot: a call takes a clone, so the
/// handler stays installed while one of its methods runs and may be called
/// again from inside it.
///
/// The clone goes when the call ends. While anything else still holds the
/// handler (the slot, an outer call, the application) that drops no handler
/// and runs no user code, so it happens here, whether the method returned or
/// panicked. Only a clone that is the last one, because the window let go of
/// its handler meanwhile, goes back to the call point, which drops it outside
/// every lock under containment, after a failed method too (ADR-0281).
struct ResidentHandler<'a>(&'a OwnerSlot<Rc<dyn WindowHandler>>);

impl HandlerSlot for ResidentHandler<'_> {
    type Handler = Rc<dyn WindowHandler>;

    fn take(&self) -> Option<Self::Handler> {
        self.0.cloned()
    }

    fn restore(&self, handler: Self::Handler) -> Option<Self::Handler> {
        (Rc::strong_count(&handler) == 1).then_some(handler)
    }

    fn replace(&self, handler: Self::Handler) -> Option<Self::Handler> {
        self.0.replace(handler).unwrap_or_else(Some)
    }
}

/// Reads a window from its window system again and publishes what it read,
/// announcing nothing: [`WindowDelivery::dispatch_state_changed`] calls
/// [`WindowHandler::state_changed`]. A window's host builds it over a weak
/// reference to the window ([`WindowDelivery::publish_through_owner`]), so the
/// delivery never keeps its window alive.
pub(crate) type StatePublisher = Arc<dyn Fn() -> Publication + Send + Sync>;

/// What a [`StatePublisher`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Publication {
    /// A new revision.
    Changed,
    /// The window reads as published already, or is gone.
    Unchanged,
    /// The window holds its publishing back, and the host publishes and
    /// announces later: the Win32 window while a change of several steps is
    /// half made, the X11 window until its loop's one read of the turn.
    #[cfg_attr(
        not(any(windows, x11_host)),
        expect(
            dead_code,
            reason = "only the Win32 and X11 windows hold their publishing back"
        )
    )]
    Held,
}

/// One delivery the window's FIFO holds.
enum Delivery {
    /// The capability snapshot a handler receives as its first input, unless
    /// an input already carried it.
    Snapshot,
    Input(PlatformInput),
    Frame(FrameRequest),
    StateChanged,
    Closed,
    /// A close request: asked with the FIFO idle (`None`, its answer is
    /// returned), or queued behind a running method or from another thread,
    /// answered `KeepOpen` then and closed by its action if the handler
    /// answers `Close` when asked.
    CloseRequest(Option<DeferredClose>),
    Surface(SurfaceChange),
    /// A deferred [`WindowDelivery::clear`]: queued instead of run inline
    /// when the window's FIFO is already draining (a close issued from inside
    /// one of its handler's methods), so every delivery queued ahead of it,
    /// the close's `closed` included, reaches the handler first.
    Clear(Option<RetirementNotice>),
    /// A diagnostic the window has to say, said in the window's turn
    /// ([`WindowDelivery::diagnose`]); said also after `closed`, as it is no
    /// obligation of the handler.
    Diagnostic(Diagnostic),
}

/// Run by a queued clear once the handler is let go (Win32 orders its
/// window-destroyed notice after the handler's release this way).
type RetirementNotice = Box<dyn FnOnce() + Send>;

/// A diagnostic a window hands its delivery to be said in the window's turn,
/// outside whatever borrows the code that found it held. One that panics (a
/// `tracing` subscriber's panic) is contained, logged and discarded, as on
/// every owner callback path, and the queue goes on.
type Diagnostic = Box<dyn FnOnce() + Send>;

/// Closes a window whose queued close request the handler granted, without
/// asking it again.
type DeferredClose = Box<dyn FnOnce() + Send>;

struct DispatchState {
    pending: VecDeque<Delivery>,
    dispatching: bool,
}

/// Owns one FIFO drain. On normal exhaustion it clears `dispatching` under
/// the queue lock, closing the enqueue-vs-finish race. Dropped while still
/// active (an unwind past the recovery, which delivers the tail first), it
/// clears the flag and drops what is left outside the lock.
struct Drain<'a> {
    state: &'a Mutex<DispatchState>,
    active: bool,
    /// How many deliveries are still queued ahead of the one that began this
    /// drain: other threads raised them first, and they are delivered first.
    /// `None` once that delivery was taken, and for a drain that began with
    /// none of its own.
    ahead_of_own: Option<usize>,
}

/// A delivery taken off the FIFO.
struct Taken {
    delivery: Delivery,
    /// It is the delivery that began the drain, whose caller waits for its
    /// answer.
    began_drain: bool,
}

impl<'a> Drain<'a> {
    /// Take the FIFO for a drain, unless one runs: every drain starts here.
    /// `ahead_of_own` is how many deliveries precede the drain's own, `None`
    /// for a drain that has none.
    fn claim(
        state: &'a Mutex<DispatchState>,
        queue: &mut DispatchState,
        ahead_of_own: Option<usize>,
    ) -> Option<Self> {
        if queue.dispatching {
            return None;
        }
        queue.dispatching = true;
        Some(Self {
            state,
            active: true,
            ahead_of_own,
        })
    }

    /// Queue `delivery` and begin its drain, unless one runs: that one
    /// reaches it in order.
    fn begin(state: &'a Mutex<DispatchState>, delivery: Delivery) -> Option<Self> {
        let mut queue = state.lock();
        let ahead = queue.pending.len();
        queue.pending.push_back(delivery);
        Self::claim(state, &mut queue, Some(ahead))
    }

    /// Queue `idle` and begin its drain when none runs, else queue `behind`
    /// for the one that does. The delivery left unqueued is handed back, for
    /// the caller to drop once the queue's lock is released.
    fn begin_or_queue(
        state: &'a Mutex<DispatchState>,
        idle: Delivery,
        behind: Delivery,
    ) -> (Option<Self>, Delivery) {
        let mut queue = state.lock();
        let ahead = queue.pending.len();
        let drain = Self::claim(state, &mut queue, Some(ahead));
        let (queued, unqueued) = if drain.is_some() {
            (idle, behind)
        } else {
            (behind, idle)
        };
        queue.pending.push_back(queued);
        (drain, unqueued)
    }

    /// A drain of what is already queued, unless one runs or nothing is.
    fn resume(state: &'a Mutex<DispatchState>) -> Option<Self> {
        let mut queue = state.lock();
        if queue.pending.is_empty() {
            return None;
        }
        Self::claim(state, &mut queue, None)
    }

    fn next(&mut self) -> Option<Taken> {
        let mut state = self.state.lock();
        if let Some(delivery) = state.pending.pop_front() {
            let began_drain = self.ahead_of_own == Some(0);
            self.ahead_of_own = self.ahead_of_own.and_then(|ahead| ahead.checked_sub(1));
            return Some(Taken {
                delivery,
                began_drain,
            });
        }
        state.dispatching = false;
        self.active = false;
        None
    }
}

impl Drop for Drain<'_> {
    fn drop(&mut self) {
        if self.active {
            let pending = {
                let mut state = self.state.lock();
                state.dispatching = false;
                std::mem::take(&mut state.pending)
            };
            // Payloads are user values: dropped outside the lock.
            for delivery in pending {
                contain_owner_callback(|| drop(delivery));
            }
        }
    }
}

/// The capability snapshot a window owes its handler, and the capabilities
/// in effect.
struct InputFront {
    capabilities: InputCapabilities,
    /// The handler has not received its first input, the snapshot.
    snapshot_due: bool,
}

/// What a window delivers to its [`WindowHandler`]: one per window, made as
/// the window opens, on its owner thread.
///
/// `Send + Sync` so a window that is still shared across threads can hold
/// it; the handler itself lives in an owner-thread slot. A delivery raised
/// on any other thread is queued in order and delivered in an owner turn
/// ([`Self::resume_through`]).
pub struct WindowDelivery {
    handler: OwnerSlot<Rc<dyn WindowHandler>>,
    /// Asks the owner for a turn that delivers what other threads queued.
    owner_turn: Mutex<Option<WindowWake>>,
    input: Mutex<InputFront>,
    /// Keys answered "not handled" while queued behind a running method,
    /// newest last, whose platform follow-up must not be delivered again.
    unanswered_keys: Mutex<VecDeque<flui_platform::keyboard::KeyEvent>>,
    dispatch: Mutex<DispatchState>,
    /// Incremented each time `frame_requested` produces a frame (or fails, or panics),
    /// nested requests delivered later included; a declined request, or one
    /// with no handler to run, leaves it unchanged. Shareable so a backend can count frames across every window
    /// it opens ([`Self::counting_frames`]).
    frames_run: Arc<AtomicU64>,
    /// Called on the owner thread, the only one it is reached from, at the
    /// end of each run of the FIFO, one that unwinds included
    /// (`after_each_run`). Runs no user code.
    run_hook: OwnerSlot<Rc<dyn Fn()>>,
    /// Whether `run_hook` holds a hook: a run of a window without one does
    /// not reach the slot for nothing.
    run_hook_installed: bool,
    /// The answer of the close request asked with the FIFO idle
    /// ([`Self::dispatch_close_request`]), left by its delivery.
    close_answer: Mutex<Option<CloseDecision>>,
    /// A close request was granted: the window is closing, and a request
    /// queued behind it is not asked.
    close_granted: AtomicBool,
    /// Publishes the window's snapshot when the window system reports a fact;
    /// see [`Self::set_state_publisher`].
    state_publisher: Mutex<Option<StatePublisher>>,
    /// What the last hover report said: whatever reports it (Win32 or web
    /// input, AppKit's tracking area, the Linux seats' pointer events), a hover reaches
    /// the window through [`Self::report_hover`] alone, and the window's
    /// snapshot reads it here ([`Self::hovered`]).
    hovered: AtomicBool,
    /// `closed` was delivered: nothing is delivered after it.
    closed: AtomicBool,
    /// The window's native retirement has started: an immediate delivery
    /// cannot overtake a clear queued in the FIFO.
    retiring: Retirement,
    /// What the window's platform reported about frames that reached the
    /// screen, which the window's next frame request carries.
    presented: PresentationReports,
    /// The native capture this delivery reports to, in builds that have one.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    journal: crate::journal::capture::Journal,
    /// The owner thread's text store journal of the window, made by the first
    /// journaled host handed out for it and shared by every later one, so
    /// they agree on the focused store and issue each store key once.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    store_journal: OwnerSlot<Rc<crate::journal::store_capture::StoreJournal>>,
}

impl WindowDelivery {
    /// The delivery of a window opened with `handler`, whose input devices
    /// give it `capabilities`: the handler's first input is that snapshot.
    /// Called on the owner thread, which is the only one the handler is
    /// reached from.
    pub(crate) fn new(handler: Rc<dyn WindowHandler>, capabilities: InputCapabilities) -> Self {
        Self::counting_frames(handler, capabilities, Arc::default())
    }

    /// [`Self::new`], incrementing `frames_run` each time the handler's
    /// `frame_requested` produces a frame. A declined request, or one with no
    /// handler to run (the window already let go of it), does not count, so a backend sharing one counter
    /// across its windows can tell a request a frame serviced from one that
    /// reached nothing.
    pub(crate) fn counting_frames(
        handler: Rc<dyn WindowHandler>,
        capabilities: InputCapabilities,
        frames_run: Arc<AtomicU64>,
    ) -> Self {
        let slot = OwnerSlot::new();
        if let Err(refused) = slot.replace(handler) {
            // Only possible once the process issued every slot key: the
            // window delivers nothing, and says so.
            tracing::error!("a window could not keep its handler: owner slots are exhausted");
            retire(refused);
        }
        Self {
            handler: slot,
            owner_turn: Mutex::new(None),
            input: Mutex::new(InputFront {
                capabilities,
                snapshot_due: true,
            }),
            unanswered_keys: Mutex::new(VecDeque::new()),
            dispatch: Mutex::new(DispatchState {
                pending: VecDeque::new(),
                dispatching: false,
            }),
            frames_run,
            run_hook: OwnerSlot::new(),
            run_hook_installed: false,
            close_answer: Mutex::new(None),
            close_granted: AtomicBool::new(false),
            state_publisher: Mutex::new(None),
            hovered: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            retiring: Retirement::default(),
            presented: PresentationReports::default(),
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            journal: crate::journal::capture::Journal::open_stream(
                flui_protocol::native_recording::Stream::Window,
            ),
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            store_journal: OwnerSlot::new(),
        }
    }

    /// `host`, journaling into this window's stream what its input method
    /// does to the stores it serves; `host` itself when no capture was
    /// installed as the window opened, or off the window's owner thread.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    pub(crate) fn journaled_store_host(
        &self,
        host: Rc<dyn flui_platform::text_store::TextStoreHost>,
    ) -> Rc<dyn flui_platform::text_store::TextStoreHost> {
        if !self.journal.is_reporting() || !self.store_journal.is_owner_thread() {
            return host;
        }
        if let Some(state) = self.store_journal.cloned() {
            return crate::journal::store_capture::JournaledHost::wrap(host, &self.journal, state);
        }
        let state = Rc::new(crate::journal::store_capture::StoreJournal::default());
        if self.store_journal.replace(Rc::clone(&state)).is_err() {
            // A journal only this host held would issue keys another host
            // issues again: the window's stream says it ends here, and its
            // stores go unjournaled.
            self.journal_close(crate::journal::capture::Captured::Cut(
                flui_protocol::native_recording::CutReason::StoreJournal,
            ));
            return host;
        }
        crate::journal::store_capture::JournaledHost::wrap(host, &self.journal, state)
    }

    /// Journal a close request's answer, the close or the stream's cut, in the window's turn and
    /// before the delivery acts on it. A sink that panics ends its capture and
    /// does not stop the close.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    fn journal_close(&self, call: crate::journal::capture::Captured<'_>) {
        contain_owner_callback(|| self.journal.record(call));
    }

    /// Call `hook` on the owner thread after each run of the window's FIFO,
    /// a run that unwinds included. A loop that no owner turn follows a
    /// native delivery on asks for its wake deadline again there, since any
    /// run may have served it or left a new one: a frame left due again, a
    /// state change that armed a deadline, input that armed a gesture
    /// deadline with no frame asked. UIKit's loop (a display-link frame, a
    /// touch batch) and AppKit's wake pump (a display pass, a window
    /// callback) both do. `hook` runs no user code: it may run while a panic
    /// unwinds.
    #[cfg(any(
        target_os = "ios",
        target_os = "macos",
        all(test, not(target_arch = "wasm32"))
    ))]
    #[must_use]
    pub(crate) fn after_each_run(mut self, hook: impl Fn() + 'static) -> Self {
        let hook: Rc<dyn Fn()> = Rc::new(hook);
        if self.run_hook.replace(hook).is_err() {
            tracing::error!(
                "a window's run hook was installed off its owner thread or with owner slots \
                 exhausted; the runs end in no hook"
            );
            return self;
        }
        self.run_hook_installed = true;
        self
    }

    /// The window's handler, for one call.
    fn handler(&self) -> ResidentHandler<'_> {
        ResidentHandler(&self.handler)
    }

    /// Deliver the capability snapshot the handler is owed as its first
    /// input, unless an input already carried it. A host calls it once the
    /// window is open and none of its locks is held, so a handler hears its
    /// window's capabilities before any later event whether input arrives or
    /// not.
    pub(crate) fn deliver_capability_snapshot(&self) {
        self.drain(Delivery::Snapshot);
    }

    /// Set how this window publishes its snapshot (ADR-0254 §1). The host
    /// sets it once, when the window opens; [`Self::publish_state`] runs it.
    pub(crate) fn set_state_publisher(&self, publisher: StatePublisher) {
        *self.state_publisher.lock() = Some(publisher);
    }

    /// [`Self::set_state_publisher`] over a weak reference to the window:
    /// `publish` reads the window again and says what it published, and a
    /// window that is gone publishes nothing.
    #[cfg(target_os = "android")]
    pub(crate) fn publish_through<W: Send + Sync + 'static>(
        &self,
        window: std::sync::Weak<W>,
        publish: fn(&W) -> Publication,
    ) {
        self.set_state_publisher(Arc::new(move || {
            window
                .upgrade()
                .map_or(Publication::Unchanged, |window| publish(&window))
        }));
    }

    /// [`Self::set_state_publisher`] for a window that lives on the owner
    /// thread (ADR-0254 §5): the window is reached, and read, there alone; a
    /// publication raised elsewhere publishes nothing, and the owner's next
    /// read publishes what changed.
    ///
    /// The headless, X11, Wayland, AppKit, UIKit and web windows publish this
    /// way; the Win32 window publishes through its window procedure, and the
    /// Android window, which its looper shares, through `publish_through`.
    pub(crate) fn publish_through_owner<W: 'static>(
        &self,
        window: &Rc<W>,
        publish: fn(&W) -> Publication,
    ) {
        let window = super::slot::OwnerWeak::new(window);
        self.set_state_publisher(Arc::new(move || {
            window
                .upgrade()
                .map_or(Publication::Unchanged, |window| publish(&window))
        }));
    }

    /// Whether the pointer is over the window, as the last hover report said;
    /// `false` before the first.
    pub(crate) fn hovered(&self) -> bool {
        self.hovered.load(Ordering::Acquire)
    }

    /// Run the publisher, if the host set one.
    fn publish(&self) -> Publication {
        let publisher = self.state_publisher.lock().clone();
        publisher.map_or(Publication::Unchanged, |publisher| publisher())
    }

    /// Publish the window's snapshot now and announce a new revision
    /// (`state_changed`). A host that publishes by itself (the end of a held
    /// change) announces its new revisions with
    /// [`Self::dispatch_state_changed`].
    pub(crate) fn publish_state(&self) {
        if self.publish() == Publication::Changed {
            self.dispatch_state_changed();
        }
    }

    /// The pointer entered (`true`) or left (`false`) the window. A hover
    /// reaches the snapshot through here whichever feature reports it (Win32
    /// or web input, AppKit's tracking area, the Linux seats' pointer events), with no
    /// edge from the input to the window.
    pub(crate) fn report_hover(&self, hovered: bool) {
        self.hovered.store(hovered, Ordering::Release);
        self.publish_state();
    }

    /// Let go of the handler, after every delivery queued ahead of this call.
    ///
    /// The handler is the platform's only owning reference to what the
    /// application keeps for the window (in `flui-app`, its renderer, whose
    /// surface is built from the window and must go while the native window
    /// still exists, ADR-0063), so a host calls this where the native window
    /// is retired: the Linux hosts' window retirement, the headless
    /// `complete_close`, Win32's window retirement, AppKit's `windowWillClose:`
    /// and the Android loop's exit.
    ///
    /// **Drain-ordered, not immediate, while this window's FIFO is draining.**
    /// A close issued from inside one of the handler's methods (a widget that
    /// closes its window from its input) reaches one of those places while
    /// that method is still on the stack: `dispatch_closed` only queues
    /// `closed` behind it, and letting go of the handler at once would drop
    /// that `closed`. So the clear is queued too, behind it, and the running
    /// drain reaches it in order. A clone of the handler out for a method
    /// still running is dropped when that method returns.
    ///
    /// Once the handler let go, the native window's fate is settled
    /// ([`Retirement::settle`]): a keep recorded before then, the handler's
    /// own release included, is the one that stands, and a later keep does
    /// nothing. A host whose native window outlives the release uses
    /// [`Self::clear_then`] to act on that fate.
    pub(crate) fn clear(&self) {
        let retirement = self.retirement();
        self.clear_then(move || {
            let _fate = retirement.settle();
        });
    }

    /// What tells the window's surface target and redraw requester, on any
    /// thread, that this window was retired: set by [`Self::clear`].
    pub(crate) fn retirement(&self) -> Retirement {
        self.retiring.clone()
    }

    /// [`Self::clear`], then `on_retired` once the handler was let go: when
    /// this window's FIFO is draining, that is after the running method
    /// returns and the queued clear runs, not when this call returns. A backend
    /// uses it to order a notice after the release of what the handler owns
    /// (a renderer and its surface), and to destroy its native window after
    /// that release, or keep it when a keep was recorded before it; the
    /// completion settles the fate itself ([`Retirement::settle`]).
    pub(crate) fn clear_then(&self, on_retired: impl FnOnce() + Send + 'static) {
        self.clear_with(Some(Box::new(on_retired)));
    }

    fn clear_with(&self, retired: Option<RetirementNotice>) {
        self.retiring.0.retired.store(true, Ordering::SeqCst);
        self.drain(Delivery::Clear(retired));
    }

    /// Let go of the handler now. Off the owner thread this reaches nothing:
    /// the handler stays in its owner's table until that thread lets go.
    fn clear_now(&self) {
        let handler = self.handler.take();
        let publisher = self.state_publisher.lock().take();
        contain_owner_callback(|| drop(handler));
        contain_owner_callback(|| drop(publisher));
    }

    /// Queue `delivery` and, unless a drain already runs, drain the FIFO.
    fn drain(&self, delivery: Delivery) {
        if !self.handler.is_owner_thread() {
            self.queue_for_owner(delivery);
            return;
        }
        if let Some(drain) = Drain::begin(&self.dispatch, delivery) {
            let _ = self.run(drain);
        }
    }

    /// Keep a delivery raised on another thread (a command applied from a
    /// worker publishes the window's new state there) in the FIFO, and ask
    /// the owner for a turn that delivers it ([`Self::deliver_queued`]).
    fn queue_for_owner(&self, delivery: Delivery) {
        self.dispatch.lock().pending.push_back(delivery);
        if !self.ask_for_owner_turn() {
            // Still queued: the owner's next delivery to this window drains it.
            tracing::warn!(
                "a window delivery raised off its owner thread waits for the owner's next one"
            );
        }
    }

    /// Ask the owner for a turn that delivers what is queued; `false` where
    /// the window's loop gave it no way to ask ([`Self::resume_through`]).
    fn ask_for_owner_turn(&self) -> bool {
        let wake = self.owner_turn.lock().clone();
        wake.map(|wake| wake()).is_some()
    }

    /// A read of the window (`PlatformWindow::state`) found the window system
    /// ahead of the published snapshot and published a new revision: the
    /// `state_changed` it is owed is queued, never called from the read.
    ///
    /// Whoever reads a window may hold a borrow the handler takes again (an
    /// application's window queues read under theirs), and the handler is
    /// user code, which runs at the owner's turn and outside any borrow
    /// (ADR-0254 §4). So the read answers its caller with the new snapshot,
    /// and the announcement waits in the window's FIFO: a drain already
    /// running delivers it when the method it runs returns, the owner's next
    /// turn delivers it where the loop gives the window one
    /// ([`Self::resume_through`]), and elsewhere it goes ahead of the window's
    /// next delivery, so a frame never runs before the state change it follows.
    ///
    /// A host announces a change its window system reported with
    /// [`Self::publish_state`] or [`Self::dispatch_state_changed`]: that is
    /// the host's own event, with no caller above it.
    pub(crate) fn state_changed_after_read(&self) {
        self.dispatch
            .lock()
            .pending
            .push_back(Delivery::StateChanged);
        let _asked = self.ask_for_owner_turn();
    }

    /// Say a diagnostic of the window (its state cell's), in the window's turn and
    /// never here: whoever reads the window may hold borrows a `tracing`
    /// subscriber takes again. It waits in the FIFO as an announcement after
    /// a read does ([`Self::state_changed_after_read`]).
    pub(crate) fn diagnose(&self, diagnostic: impl FnOnce() + Send + 'static) {
        self.dispatch
            .lock()
            .pending
            .push_back(Delivery::Diagnostic(Box::new(diagnostic)));
        let _asked = self.ask_for_owner_turn();
    }

    /// Ask for the owner turns that deliver what other threads raise; `wake`
    /// runs [`Self::deliver_queued`] on the owner thread.
    pub(crate) fn resume_through(&self, wake: WindowWake) {
        *self.owner_turn.lock() = Some(wake);
    }

    /// [`Self::resume_through`] an owner turn of `signal`, the loop's own
    /// wake: the turn delivers what other threads queued, and holds this
    /// delivery weakly, so a queued turn keeps no window alive.
    pub(crate) fn resume_through_signal(
        self: &Arc<Self>,
        signal: &Arc<super::signal::OwnerSignal>,
    ) {
        let queued = Arc::downgrade(self);
        self.resume_through(super::signal::OwnerSignal::window_wake(signal, move || {
            if let Some(delivery) = queued.upgrade() {
                delivery.deliver_queued();
            }
        }));
    }

    /// Deliver, on the owner thread, what other threads queued; nothing when
    /// a drain is already running there, which reaches it in order.
    pub(crate) fn deliver_queued(&self) {
        if !self.handler.is_owner_thread() {
            return;
        }
        if let Some(drain) = Drain::resume(&self.dispatch) {
            // A panic resumes here, in the owner turn's own containment.
            let _ = self.run(drain);
        }
    }

    /// Run `drain` to its end. Returns the handler's answer to the delivery
    /// that began it, when that was an input: what other threads queued ahead
    /// is delivered first, and an input among it was answered where it was
    /// raised, so its answer is not this caller's.
    fn run(&self, mut drain: Drain<'_>) -> Option<DispatchEventResult> {
        let _after = AfterRun(
            self.run_hook_installed
                .then(|| self.run_hook.cloned())
                .flatten(),
        );
        let mut own_answer = None;
        let failure = loop {
            let Some(taken) = drain.next() else {
                return own_answer;
            };
            #[expect(
                clippy::disallowed_methods,
                reason = "ADR-0287: a handler's panic stops the drain but the queued tail is delivered"
            )]
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let answer = self.deliver(taken.delivery);
                if taken.began_drain {
                    own_answer = answer;
                }
            }));
            if let Err(failure) = outcome {
                break failure;
            }
        };
        // Every delivery the platform queued was accepted: the tail is still
        // delivered, each in its own containment, and the first panic resumes
        // afterwards. A later panic does not replace it.
        while let Some(taken) = drain.next() {
            contain_owner_callback(|| {
                let _ = self.deliver(taken.delivery);
            });
        }
        std::panic::resume_unwind(failure)
    }

    /// One delivery of the window's FIFO. An input the handler was asked
    /// returns its answer, the default when the window let go of its handler.
    fn deliver(&self, delivery: Delivery) -> Option<DispatchEventResult> {
        if let Delivery::Clear(retired) = delivery {
            self.clear_now();
            if let Some(retired) = retired {
                contain_owner_callback(retired);
            }
            return None;
        }
        if let Delivery::Diagnostic(diagnostic) = delivery {
            contain_owner_callback(diagnostic);
            return None;
        }
        if self.closed.load(Ordering::SeqCst) {
            // `closed` is the last obligation.
            return None;
        }
        match delivery {
            Delivery::Snapshot => {
                if let Some(failure) = self.deliver_snapshot_if_due() {
                    failure.resume();
                }
            }
            Delivery::Input(event) => {
                let snapshot_failure = self.deliver_snapshot_if_due();
                let called = call(&self.handler(), |handler| handler.input(event));
                let Some(first) = snapshot_failure else {
                    return Some(called.or_resume(DispatchEventResult::default()));
                };
                // The snapshot's panic came first: the input still had its
                // delivery, and a panic of its own does not replace that one.
                if let Called::Panicked(second) = called {
                    second.contain("a window handler's input");
                }
                first.resume();
            }
            Delivery::Frame(request) => {
                let called = call(&self.handler(), |handler| handler.frame_requested(request));
                // A frame that unwinds or fails counts as one that ran: it
                // had its chance at what was due, and a wake deadline answered
                // before it is asked for again. A declined one serviced
                // nothing. A failure the handler contained does not unwind:
                // at an OS entry that would be the entry's answer or an abort
                // (ADR-0258 §4).
                if matches!(
                    called,
                    Called::Returned(FrameOutcome::Produced | FrameOutcome::Failed)
                        | Called::Panicked(_)
                ) {
                    self.frames_run.fetch_add(1, Ordering::Relaxed);
                }
                called.or_resume(FrameOutcome::Declined);
            }
            Delivery::StateChanged => call(&self.handler(), |handler| {
                handler.state_changed();
            })
            .or_resume(()),
            Delivery::Closed => {
                self.closed.store(true, Ordering::SeqCst);
                #[cfg(all(debug_assertions, feature = "native-capture"))]
                {
                    use flui_protocol::native_recording::CloseCause;
                    self.journal_close(crate::journal::capture::Captured::Closed(
                        if self.close_granted.load(Ordering::SeqCst) {
                            CloseCause::GrantedRequest
                        } else {
                            CloseCause::NoRequest
                        },
                    ));
                }
                call(&self.handler(), |handler| {
                    handler.closed();
                })
                .or_resume(());
            }
            Delivery::CloseRequest(later) => {
                if self.close_granted.load(Ordering::SeqCst) {
                    // An earlier request was granted and the window is
                    // closing: there is nothing left to ask.
                    #[cfg(all(debug_assertions, feature = "native-capture"))]
                    self.journal_close(crate::journal::capture::Captured::CloseRequest {
                        answer: flui_protocol::native_recording::CloseAnswer::Closing,
                        queued: later.is_some(),
                    });
                    if later.is_none() {
                        *self.close_answer.lock() = Some(CloseDecision::KeepOpen);
                    }
                    return None;
                }
                let called = call(&self.handler(), |handler| handler.close_requested());
                #[cfg(all(debug_assertions, feature = "native-capture"))]
                {
                    use flui_protocol::native_recording::CloseAnswer;
                    self.journal_close(crate::journal::capture::Captured::CloseRequest {
                        answer: match &called {
                            Called::Vacant => CloseAnswer::NoHandler,
                            Called::Returned(CloseDecision::Close) => CloseAnswer::Close,
                            Called::Returned(CloseDecision::KeepOpen) => CloseAnswer::KeepOpen,
                            Called::Panicked(_) => CloseAnswer::Panicked,
                        },
                        queued: later.is_some(),
                    });
                }
                let decision = called.or_resume(CloseDecision::Close);
                if decision == CloseDecision::Close {
                    self.close_granted.store(true, Ordering::SeqCst);
                }
                match later {
                    None => *self.close_answer.lock() = Some(decision),
                    Some(close) if decision == CloseDecision::Close => {
                        contain_owner_callback(close);
                    }
                    Some(_) => {}
                }
            }
            Delivery::Surface(change) => call(&self.handler(), |handler| {
                handler.surface_changed(change);
            })
            .or_resume(()),
            Delivery::Clear(_) | Delivery::Diagnostic(_) => {
                unreachable!("BUG: a clear or a diagnostic was delivered above")
            }
        }
        None
    }

    /// Deliver the capability snapshot when the handler has not had it. Its
    /// panic is handed back, not resumed: the input it precedes is still
    /// delivered, and the caller resumes it afterwards.
    #[must_use = "the snapshot's panic is the first failure: resume it"]
    fn deliver_snapshot_if_due(&self) -> Option<Failure> {
        let capabilities = {
            let mut front = self.input.lock();
            if !front.snapshot_due {
                return None;
            }
            front.snapshot_due = false;
            front.capabilities
        };
        match call(&self.handler(), |handler| {
            handler.input(PlatformInput::Capabilities(capabilities));
        }) {
            Called::Panicked(failure) => Some(failure),
            Called::Vacant | Called::Returned(()) => None,
        }
    }

    /// Deliver an input event.
    ///
    /// The outer drain returns the handler's result for its own event,
    /// whatever other threads queued ahead of it. A
    /// nested one is queued, never dropped, and answered at once because its
    /// result is not known until the running method returns (ADR-0207 §7): a
    /// key gets [`DispatchEventResult::DEFERRED_UNHANDLED`], so its platform
    /// default runs now, and the platform's follow-up for it (AppKit resends
    /// an unhandled key equivalent as `keyDown:`) is not delivered again; any
    /// other input gets [`DispatchEventResult::DEFERRED`], which suppresses a
    /// default (Win32 mouse promotion) its queued delivery replaces. A
    /// [`PlatformInput::Capabilities`] is a capability change
    /// ([`Self::set_input_capabilities`]).
    pub(crate) fn dispatch_input(&self, event: PlatformInput) -> DispatchEventResult {
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        if let PlatformInput::Ime(ime) = &event {
            self.journal
                .record(crate::journal::capture::Captured::Ime(ime));
        }
        if let PlatformInput::Capabilities(capabilities) = event {
            self.set_input_capabilities(capabilities);
            return DispatchEventResult::DEFERRED;
        }
        if !self.handler.is_owner_thread() {
            // Its answer comes in an owner turn, too late for the caller.
            self.queue_for_owner(Delivery::Input(event));
            return DispatchEventResult::DEFERRED;
        }
        let key = match &event {
            PlatformInput::Keyboard(key) => Some(key.clone()),
            _ => None,
        };
        if let Some(key) = &key {
            let mut unanswered = self.unanswered_keys.lock();
            if let Some(index) = unanswered.iter().position(|marker| marker == key) {
                unanswered.remove(index);
                return DispatchEventResult::default();
            }
        }
        let Some(drain) = Drain::begin(&self.dispatch, Delivery::Input(event)) else {
            // Queued behind the running method. A key cannot wait for its
            // answer: it is answered "not handled" so the platform default
            // runs now, and its follow-up is recognized above.
            let Some(key) = key else {
                return DispatchEventResult::DEFERRED;
            };
            let mut unanswered = self.unanswered_keys.lock();
            if unanswered.len() == UNANSWERED_KEYS {
                unanswered.pop_front();
            }
            unanswered.push_back(key);
            return DispatchEventResult::DEFERRED_UNHANDLED;
        };
        self.run(drain).unwrap_or_default()
    }

    /// Record the window's input capabilities. Before the handler has its
    /// snapshot they become the snapshot; after it, a change delivered in
    /// order with the input around it (ADR-0213). A backend gives the
    /// capabilities of the devices already attached when it builds the
    /// window.
    pub(crate) fn set_input_capabilities(&self, capabilities: InputCapabilities) {
        {
            let mut front = self.input.lock();
            front.capabilities = capabilities;
            if front.snapshot_due {
                return;
            }
        }
        self.drain(Delivery::Input(PlatformInput::Capabilities(capabilities)));
    }

    /// The window's current input capabilities.
    #[must_use]
    pub(crate) fn input_capabilities(&self) -> InputCapabilities {
        self.input.lock().capabilities
    }

    /// Deliver a frame request that carries no platform timestamp: its vsync
    /// is the event clock's reading now (ADR-0347).
    pub(crate) fn dispatch_request_frame(&self) {
        self.dispatch_frame(frame_request_now());
    }

    /// Deliver a frame request with what the platform reported for the frame,
    /// carrying the newest presentation the platform reported since the
    /// window's last request.
    pub(crate) fn dispatch_frame(&self, request: FrameRequest) {
        let request = self.presented.carried_by(request);
        self.drain(Delivery::Frame(request));
    }

    /// Where the window's platform reports that its frames reached the
    /// screen, from any thread: the next frame request carries the newest.
    pub(crate) fn presentation_reports(&self) -> PresentationReports {
        self.presented.clone()
    }

    /// Deliver a state change: a new snapshot was published.
    pub(crate) fn dispatch_state_changed(&self) {
        self.drain(Delivery::StateChanged);
    }

    /// Deliver `closed`, once: later deliveries reach nothing.
    pub(crate) fn dispatch_close(&self) {
        self.drain(Delivery::Closed);
    }

    /// Ask the handler whether the window closes, in the window's FIFO like
    /// every other delivery.
    ///
    /// With the FIFO idle the question is asked now and its answer returned;
    /// what the handler raises while it answers is queued behind it.
    /// `Close` once the window let go of its handler. Raised while a method
    /// of the handler runs (a `WM_CLOSE` sent from inside `input`), or off the
    /// owner thread, the answer cannot wait: the window is kept for now
    /// (`KeepOpen`), the question is queued behind the running method, and
    /// when the handler then answers `Close`, `close_later` closes the window
    /// without asking again.
    pub(crate) fn dispatch_close_request(
        &self,
        close_later: impl FnOnce() + Send + 'static,
    ) -> CloseDecision {
        if !self.handler.is_owner_thread() {
            self.queue_for_owner(Delivery::CloseRequest(Some(Box::new(close_later))));
            return CloseDecision::KeepOpen;
        }
        // With the FIFO idle the deferred close is not needed: it is dropped
        // when this call returns, outside the queue's lock.
        let (drain, _unqueued) = Drain::begin_or_queue(
            &self.dispatch,
            Delivery::CloseRequest(None),
            Delivery::CloseRequest(Some(Box::new(close_later))),
        );
        let Some(drain) = drain else {
            return CloseDecision::KeepOpen;
        };
        // The request's answer is left in `close_answer`.
        let _ = self.run(drain);
        self.close_answer
            .lock()
            .take()
            .unwrap_or(CloseDecision::Close)
    }

    /// Deliver an iOS surface change in its own transaction, even when an
    /// input or frame method holds the general FIFO: a release must free the
    /// surface before UIKit takes the layer away. Not after the window's
    /// retirement started.
    ///
    /// The handler's panic is handed back: the caller finishes what the
    /// UIKit callback owes (a close's `closed` and release) and resumes it
    /// afterwards, so the first failure is neither lost nor allowed to drop
    /// that work.
    #[cfg(target_os = "ios")]
    #[must_use = "the handler's panic is the first failure: resume it"]
    pub(crate) fn dispatch_surface_immediate(&self, change: SurfaceChange) -> Option<Failure> {
        if self.retiring.is_retired() || self.closed.load(Ordering::SeqCst) {
            return None;
        }
        match call(&self.handler(), |handler| handler.surface_changed(change)) {
            Called::Panicked(failure) => Some(failure),
            Called::Vacant | Called::Returned(()) => None,
        }
    }

    /// Deliver a change of the surface behind the window. A
    /// [`SurfaceChange::Release`] runs while the platform still has the
    /// native window in hand and is about to lose it, so the handler must
    /// drop its surface before returning, not schedule the drop.
    pub(crate) fn dispatch_surface_change(&self, change: SurfaceChange) {
        self.drain(Delivery::Surface(change));
    }
}

impl std::fmt::Debug for WindowDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowDelivery")
            .field("handler", &self.handler.is_occupied())
            .field("closed", &self.closed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

/// Calls a window's run hook (`WindowDelivery::after_each_run`), if it has
/// one, when a run of its FIFO ends, by return or by unwinding.
struct AfterRun(Option<Rc<dyn Fn()>>);

impl Drop for AfterRun {
    fn drop(&mut self) {
        if let Some(hook) = self.0.as_ref() {
            hook();
        }
    }
}
