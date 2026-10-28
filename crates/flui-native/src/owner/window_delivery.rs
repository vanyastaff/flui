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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::Mutex;

use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    CloseDecision, DispatchEventResult, FrameOutcome, FrameRequest, PlatformInput, SurfaceChange,
    WindowHandler,
};

use super::call_point::{Called, Failure, HandlerSlot, call, retire};
use super::handlers::frame_request_now;
use super::panic_boundary::contain_owner_callback;
use super::signal::WindowWake;
use super::slot::OwnerSlot;

/// Keys remembered as answered "not handled" while queued; a platform sends
/// its follow-up right after the answer, so a few suffice.
const UNANSWERED_KEYS: usize = 8;

/// Whether a window's native retirement has started, as any thread reads it.
///
/// Every host clears its window's delivery where the native window is
/// retired (Win32's `WM_DESTROY`, AppKit's `windowWillClose:`, winit's close
/// completion, the Android loop's exit, UIKit's scene teardown, the web
/// canvas's retirement, the headless close), so this is set before the
/// native window it describes is gone. A window's surface target and redraw
/// requester (ADR-0254 §5) live on other threads and outlive the window: they
/// read it to answer for a window that is no longer there.
#[derive(Clone, Debug, Default)]
pub(crate) struct Retirement(Arc<AtomicBool>);

impl Retirement {
    /// Whether the window's native retirement has started.
    pub(crate) fn is_retired(&self) -> bool {
        self.0.load(Ordering::SeqCst)
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
/// every lock, or retains it when the method failed.
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
/// reference to the window ([`WindowDelivery::publish_through`]), so the
/// delivery never keeps its window alive.
pub(crate) type StatePublisher = Arc<dyn Fn() -> Publication + Send + Sync>;

/// What a [`StatePublisher`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Publication {
    /// A new revision.
    Changed,
    /// The window reads as published already, or is gone.
    Unchanged,
    /// The window holds its publishing back while a change of several steps
    /// is half made; the host publishes and announces once it ends.
    #[cfg_attr(
        not(windows),
        expect(
            dead_code,
            reason = "only the Win32 window holds its publishing back (a DPI change)"
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
}

/// Run by a queued clear once the handler is let go (Win32 orders its
/// window-destroyed notice after the handler's release this way).
type RetirementNotice = Box<dyn FnOnce() + Send>;

/// Closes a window whose queued close request the handler granted, without
/// asking it again.
type DeferredClose = Box<dyn FnOnce() + Send>;

/// Close `window` the way the application would, by an admitted
/// [`WindowCommand::Close`](flui_platform::WindowCommand::Close), which asks
/// no handler: the deferred close of a request the handler already granted.
#[cfg_attr(
    not(all(
        feature = "winit-backend",
        any(target_os = "windows", target_os = "macos", target_os = "linux")
    )),
    expect(
        dead_code,
        reason = "only the winit loop closes a granted request by command"
    )
)]
pub(crate) fn close_by_command(window: &dyn flui_platform::PlatformWindow) {
    let closed = flui_platform::WindowCommand::Close
        .admit(&window.state().capabilities)
        .map_err(|error| error.to_string())
        .and_then(|command| window.apply(command).map_err(|error| error.to_string()));
    if let Err(error) = closed {
        tracing::warn!(%error, "a granted close request could not close its window");
    }
}

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
}

impl<'a> Drain<'a> {
    fn begin(state: &'a Mutex<DispatchState>, delivery: Delivery) -> Option<Self> {
        let mut guard = state.lock();
        guard.pending.push_back(delivery);
        if guard.dispatching {
            return None;
        }
        guard.dispatching = true;
        Some(Self {
            state,
            active: true,
        })
    }

    /// A drain of what is already queued, unless one runs or nothing is.
    fn resume(state: &'a Mutex<DispatchState>) -> Option<Self> {
        let mut guard = state.lock();
        if guard.dispatching || guard.pending.is_empty() {
            return None;
        }
        guard.dispatching = true;
        Some(Self {
            state,
            active: true,
        })
    }

    fn next(&mut self) -> Option<Delivery> {
        let mut state = self.state.lock();
        if let Some(delivery) = state.pending.pop_front() {
            return Some(delivery);
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
    /// Incremented each time `frame_requested` produces a frame (or panics),
    /// nested requests delivered later included; a declined request, or one
    /// with no handler to run, leaves it unchanged. Shareable so a backend can count frames across every window
    /// it opens ([`Self::counting_frames`]).
    frames_run: Arc<AtomicU64>,
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
    /// input, AppKit's tracking area, winit's cursor events), a hover reaches
    /// the window through [`Self::report_hover`] alone, and the window's
    /// snapshot reads it here ([`Self::hovered`]).
    hovered: AtomicBool,
    /// `closed` was delivered: nothing is delivered after it.
    closed: AtomicBool,
    /// The window's native retirement has started: an immediate delivery
    /// cannot overtake a clear queued in the FIFO.
    retiring: Retirement,
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
            close_answer: Mutex::new(None),
            close_granted: AtomicBool::new(false),
            state_publisher: Mutex::new(None),
            hovered: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            retiring: Retirement::default(),
        }
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

    /// [`Self::publish_through`] for a window that lives on the owner thread
    /// (ADR-0254 §5): the window is reached, and read, there alone; a
    /// publication raised elsewhere publishes nothing, and the owner's next
    /// read publishes what changed.
    #[cfg_attr(
        all(windows, not(feature = "winit-backend")),
        expect(
            dead_code,
            reason = "the Win32 window publishes through its window procedure"
        )
    )]
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
    /// or web input, AppKit's tracking area, winit's cursor events), with no
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
    /// is retired: winit's `complete_window_close` and the quit route that
    /// releases every window still tracked (`owner/winit.rs`), the headless
    /// `complete_close`, Win32's `WM_DESTROY`, AppKit's `windowWillClose:`
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
    pub(crate) fn clear(&self) {
        self.clear_with(None);
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
    /// (a renderer and its surface).
    #[cfg(any(windows, all(test, not(target_arch = "wasm32"))))]
    pub(crate) fn clear_then(&self, on_retired: impl FnOnce() + Send + 'static) {
        self.clear_with(Some(Box::new(on_retired)));
    }

    fn clear_with(&self, retired: Option<RetirementNotice>) {
        self.retiring.0.store(true, Ordering::SeqCst);
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
    /// Returns the result of the first input the drain delivered.
    fn drain(&self, delivery: Delivery) -> Option<DispatchEventResult> {
        if !self.handler.is_owner_thread() {
            self.queue_for_owner(delivery);
            return None;
        }
        let drain = Drain::begin(&self.dispatch, delivery)?;
        self.run(drain)
    }

    /// Keep a delivery raised on another thread (a command applied from a
    /// worker publishes the window's new state there) in the FIFO, and ask
    /// the owner for a turn that delivers it ([`Self::deliver_queued`]).
    fn queue_for_owner(&self, delivery: Delivery) {
        self.dispatch.lock().pending.push_back(delivery);
        let wake = self.owner_turn.lock().clone();
        if let Some(wake) = wake {
            wake();
        } else {
            // Still queued: the owner's next delivery to this window drains it.
            tracing::warn!(
                "a window delivery raised off its owner thread waits for the owner's next one"
            );
        }
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

    fn run(&self, mut drain: Drain<'_>) -> Option<DispatchEventResult> {
        let mut input_result = None;
        let failure = loop {
            let Some(delivery) = drain.next() else {
                return input_result;
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.deliver(delivery, &mut input_result);
            }));
            if let Err(failure) = outcome {
                break failure;
            }
        };
        // Every delivery the platform queued was accepted: the tail is still
        // delivered, each in its own containment, and the first panic resumes
        // afterwards. A later panic does not replace it.
        while let Some(delivery) = drain.next() {
            contain_owner_callback(|| self.deliver(delivery, &mut None));
        }
        std::panic::resume_unwind(failure)
    }

    /// One delivery of the window's FIFO; an input records its result in
    /// `input_result`.
    fn deliver(&self, delivery: Delivery, input_result: &mut Option<DispatchEventResult>) {
        if let Delivery::Clear(retired) = delivery {
            self.clear_now();
            if let Some(retired) = retired {
                contain_owner_callback(retired);
            }
            return;
        }
        if self.closed.load(Ordering::SeqCst) {
            // `closed` is the last obligation.
            return;
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
                    let result = called.or_resume(DispatchEventResult::default());
                    input_result.get_or_insert(result);
                    return;
                };
                // The snapshot's panic came first: the input still had its
                // delivery, and a panic of its own does not replace that one.
                match called {
                    Called::Returned(result) => {
                        input_result.get_or_insert(result);
                    }
                    Called::Panicked(second) => second.contain("a window handler's input"),
                    Called::Vacant => {}
                }
                first.resume();
            }
            Delivery::Frame(request) => {
                let called = call(&self.handler(), |handler| handler.frame_requested(request));
                // A frame that unwinds counts as one that ran: it had its
                // chance at what was due. A declined one serviced nothing.
                if matches!(
                    called,
                    Called::Returned(FrameOutcome::Produced) | Called::Panicked(_)
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
                call(&self.handler(), |handler| {
                    handler.closed();
                })
                .or_resume(());
            }
            Delivery::CloseRequest(later) => {
                if self.close_granted.load(Ordering::SeqCst) {
                    // An earlier request was granted and the window is
                    // closing: there is nothing left to ask.
                    if later.is_none() {
                        *self.close_answer.lock() = Some(CloseDecision::KeepOpen);
                    }
                    return;
                }
                let decision = call(&self.handler(), |handler| handler.close_requested())
                    .or_resume(CloseDecision::Close);
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
            Delivery::Clear(_) => unreachable!("BUG: a clear was delivered above"),
        }
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
    /// The outer drain returns the handler's result for its own event. A
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
    /// is the event clock's reading now (ADR-0257).
    pub(crate) fn dispatch_request_frame(&self) {
        self.dispatch_frame(frame_request_now());
    }

    /// Deliver a frame request with what the platform reported for the frame.
    pub(crate) fn dispatch_frame(&self, request: FrameRequest) {
        self.drain(Delivery::Frame(request));
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
        let drain = {
            let mut state = self.dispatch.lock();
            if state.dispatching {
                state
                    .pending
                    .push_back(Delivery::CloseRequest(Some(Box::new(close_later))));
                return CloseDecision::KeepOpen;
            }
            state.dispatching = true;
            state.pending.push_back(Delivery::CloseRequest(None));
            Drain {
                state: &self.dispatch,
                active: true,
            }
        };
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
