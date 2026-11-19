//! Standalone AppKit loop ownership and orderly, policy-aware stopping.
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::{NSArray, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSURL};
use std::ptr::NonNull;

use std::time::Duration;

use flui_platform::{Launch, SessionEndAnswer, SessionEndPhase};

use super::owner_lane::{
    exec_after_on_owner, exec_async_on_owner, owner_queue, report_contained_panic,
    run_cleanup_guarded as cleanup_step,
};
use crate::{
    error::PlatformError,
    owner::app_delivery::AppDelivery,
    owner::apple::NativeOwner,
    owner::apple::loop_state::{LoopRequests, LoopState, transition},
    owner::apple::terminate_gate::ANNOUNCEMENT_LIFETIME,
    owner::apple::terminate_gate::DEFERRAL_LIMIT,
    owner::apple::terminate_gate::TerminateGate,
    owner::apple::terminate_gate::Termination,
    owner::panic_boundary::{AfterPanic, ContainedIvars, contain_os_entry, os_entry},
    owner::signal::OwnerTurnSlot,
    owner::wake_deadline::TurnDeadline,
};

/// Whether finishing a session end also answers a waiting
/// `applicationShouldTerminate:`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reply {
    /// The terminate request was answered already (`NSTerminateNow`).
    AlreadyAnswered,
    /// It was answered `NSTerminateLater`: reply to AppKit.
    ToAppKit,
}

/// The macOS loop's control: its quit, reopen and session-end handling.
///
/// The control crosses threads, held weakly: the owner signal's wake and the
/// system notices' observers run where they are called and post to the main
/// queue, where the control is upgraded. Everything it keeps beyond the
/// signal is the main thread's ([`OwnerPart`]), reached with the
/// `MainThreadMarker` that proves the caller is there; no AppKit object
/// crosses the queue.
pub(crate) struct LoopControl {
    pub(crate) owner_signal: Arc<crate::owner::signal::OwnerSignal>,
    windows: super::WindowRegistry,
    app: Arc<AppDelivery>,
    /// The loop's wake pump, which looks at the wake deadline after every
    /// owner turn: a turn can leave one no redraw request starts the pump for
    /// (a deadline a task sleeps until).
    wake_pump: Weak<super::wake_pump::WakePump>,
    owner: NativeOwner<OwnerPart>,
}

/// What the loop control keeps on the main thread.
///
/// Each value is a `Cell` of a `Copy` state, read and stored around a
/// transition that runs no code of anyone else's: the application is told
/// only after the new state is stored, so a callback that re-enters the
/// control reads it, and nothing is ever found busy.
struct OwnerPart {
    /// Where the loop is in its life and the reopens it owes, with the
    /// owner signal and the application around it.
    requests: LoopRequests,
    /// Connects the session-end announcement to the terminate request that
    /// follows it.
    terminate_gate: Cell<TerminateGate>,
    /// The application's wake callback between owner turns.
    turn: Rc<OwnerTurnSlot>,
    /// The application's turn deadline and the generation of the timer
    /// scheduled for it. AppKit's loop has no idle wait of its own to bound,
    /// so each answer schedules a timer on the main queue and supersedes the
    /// last ([`LoopControl::schedule_turn_deadline`]).
    turn_timer: Cell<(TurnDeadline, u64)>,
}

impl LoopControl {
    /// What the control keeps on the main thread.
    fn owner(&self, main: MainThreadMarker) -> &OwnerPart {
        self.owner.get(main)
    }

    /// Run `body` on the loop's state and store the result.
    fn update<R>(&self, main: MainThreadMarker, body: impl FnOnce(&mut LoopState) -> R) -> R {
        self.owner(main).requests.update(body)
    }

    /// Run `body` on the terminate gate and store the result.
    fn gate<R>(&self, main: MainThreadMarker, body: impl FnOnce(&mut TerminateGate) -> R) -> R {
        transition(&self.owner(main).terminate_gate, body)
    }

    /// The slot the application's wake callback waits in between turns.
    pub(crate) fn turn(&self, main: MainThreadMarker) -> Rc<OwnerTurnSlot> {
        Rc::clone(&self.owner(main).turn)
    }

    /// Stops admission and scheduling, and drops the application's wake
    /// callback.
    fn close_signal(&self, main: MainThreadMarker) {
        self.owner_signal.close_in(&self.owner(main).turn);
    }

    /// The control of a loop made on the main thread `main` proves.
    pub(super) fn new(
        windows: super::WindowRegistry,
        app: Arc<AppDelivery>,
        wake_pump: Weak<super::wake_pump::WakePump>,
        main: MainThreadMarker,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak: &Weak<Self>| {
            let weak = weak.clone();
            let owner_signal = crate::owner::signal::OwnerSignal::new(Arc::new(move || {
                let weak = weak.clone();
                exec_async_on_owner(owner_queue(), move || {
                    cleanup_step(|| {
                        let main = MainThreadMarker::new()
                            .expect("BUG: the owner queue is the AppKit main queue");
                        if let Some(control) = weak.upgrade() {
                            if control.owner_signal.drive_in(&control.turn(main)) {
                                control.quit(main);
                            } else {
                                control.schedule_turn_deadline(main);
                                // The turn may have left a wake deadline that
                                // no redraw request reaches the pump for.
                                if let Some(pump) = control.wake_pump.upgrade() {
                                    pump.after_owner_turn();
                                }
                            }
                        }
                    });
                });
                Ok(())
            }));
            Self {
                owner_signal,
                windows,
                app,
                wake_pump,
                owner: NativeOwner::new(
                    OwnerPart {
                        requests: LoopRequests::new(),
                        terminate_gate: Cell::new(TerminateGate::new()),
                        turn: Rc::default(),
                        turn_timer: Cell::default(),
                    },
                    main,
                ),
            }
        })
    }

    /// `NSWorkspaceWillPowerOffNotification`: the user's session is about to
    /// end. The application is asked whether it needs the user to wait; the
    /// terminate request that follows carries the answer out. If none follows,
    /// the session end was called off, which AppKit does not announce, and the
    /// application is told so.
    pub(crate) fn session_end_announced(self: &Arc<Self>, main: MainThreadMarker) {
        let blocked = self.ask_session_end(SessionEndPhase::Query) == SessionEndAnswer::Block;
        if let Some(epoch) = self.gate(main, |gate| gate.announced(blocked)) {
            self.run_after(ANNOUNCEMENT_LIFETIME, move |control, main| {
                if control.gate(main, |gate| gate.announcement_lapsed(epoch)) {
                    control.ask_session_end(SessionEndPhase::Cancelled);
                }
            });
        }
    }

    /// The application released its last hold on the session end
    /// (`Platform::release_session_end`): answer the terminate request that
    /// waits, if one does.
    ///
    /// The release happens wherever the last `SessionEndBlock` is dropped,
    /// possibly inside a turn of the runtime, so the answer is a task of its
    /// own: telling the application the session is ending, and then letting the
    /// process go, must not happen under the code that dropped the block.
    pub(super) fn session_end_released(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        exec_async_on_owner(owner_queue(), move || {
            cleanup_step(|| {
                let main =
                    MainThreadMarker::new().expect("BUG: the owner queue is the AppKit main queue");
                if let Some(control) = weak.upgrade()
                    && control.gate(main, TerminateGate::released)
                {
                    control.finish_session_end(main, Reply::ToAppKit);
                }
            });
        });
    }

    /// Whether a session end is announced or its reply owed: a terminate
    /// request whose handling panicked lets it go on. The gate is a `Cell`,
    /// so this reads it whatever the delegate was doing when it panicked.
    fn session_ending(&self, main: MainThreadMarker) -> bool {
        self.owner(main).terminate_gate.get().session_ending()
    }

    /// `applicationShouldTerminate:`. A quit nobody announced is cancelled
    /// here and asked of the application in a task of its own
    /// ([`Self::quit_asked`]); a session end is answered at once, or after
    /// the application releases its holds.
    fn terminate_requested(
        self: &Arc<Self>,
        main: MainThreadMarker,
    ) -> NSApplicationTerminateReply {
        match self.gate(main, TerminateGate::terminate_requested) {
            Termination::Quit => {
                let weak = Arc::downgrade(self);
                exec_async_on_owner(owner_queue(), move || {
                    cleanup_step(|| {
                        let main = MainThreadMarker::new()
                            .expect("BUG: the owner queue is the AppKit main queue");
                        if let Some(control) = weak.upgrade() {
                            control.quit_asked(main);
                        }
                    });
                });
                NSApplicationTerminateReply::TerminateCancel
            }
            Termination::Now => {
                // The session ends whatever telling the application raised:
                // contained apart, so a panic there cannot cancel the logout.
                contain_os_entry("applicationShouldTerminate:", || {
                    self.finish_session_end(main, Reply::AlreadyAnswered);
                });
                NSApplicationTerminateReply::TerminateNow
            }
            Termination::Later { epoch } => {
                self.run_after(DEFERRAL_LIMIT, move |control, main| {
                    if control.gate(main, |gate| gate.deferral_ran_out(epoch)) {
                        tracing::warn!("a session end waited the full limit for the application");
                        control.finish_session_end(main, Reply::ToAppKit);
                    }
                });
                NSApplicationTerminateReply::TerminateLater
            }
        }
    }

    /// Ask the application's turn deadline, with nothing borrowed, and
    /// schedule the owner turn it asks for on the main queue: at that instant
    /// the owner gets a turn whether or not a window is open, and no window is
    /// asked for a frame. Called after each owner turn and once the run's
    /// handler has bootstrapped. An answer that changes the instant armed
    /// supersedes the timer an earlier one scheduled, the same instant keeps
    /// it, and an instant a turn served is not scheduled again
    /// ([`TurnDeadline`]).
    pub(crate) fn schedule_turn_deadline(self: &Arc<Self>, main: MainThreadMarker) {
        let answer = if self.owner_signal.accepting() {
            self.app.turn_deadline().or_resume(None)
        } else {
            None
        };
        let changed = transition(&self.owner(main).turn_timer, |(deadline, _)| {
            let before = deadline.armed();
            let armed = deadline.arm(answer);
            // The same instant keeps the timer scheduled for it.
            (armed != before).then_some(armed)
        });
        if let Some(armed) = changed {
            self.schedule_turn_timer(main, armed);
        }
    }

    /// Schedule the main-queue timer for `armed`, superseding the one
    /// scheduled before; `None` only supersedes it. A timer that fires before
    /// its instant schedules the rest of the wait, so the armed instant always
    /// has a timer.
    fn schedule_turn_timer(
        self: &Arc<Self>,
        main: MainThreadMarker,
        armed: Option<web_time::Instant>,
    ) {
        let generation = transition(&self.owner(main).turn_timer, |(_, generation)| {
            *generation = generation
                .checked_add(1)
                .expect("BUG: a turn-deadline generation per owner turn fits in u64");
            *generation
        });
        let Some(at) = armed else {
            return;
        };
        let delay = at.saturating_duration_since(web_time::Instant::now());
        self.run_after(delay, move |control, main| {
            let (current, due) =
                transition(&control.owner(main).turn_timer, |(deadline, scheduled)| {
                    let current = *scheduled == generation;
                    (
                        current,
                        current && deadline.take_due(web_time::Instant::now()),
                    )
                });
            if due {
                let _ = control.owner_signal.wake();
            } else if current {
                // The queue ran the timer before its instant: schedule the
                // rest of the wait.
                control.schedule_turn_timer(main, Some(at));
            }
        });
    }

    /// Run `body` on the main queue after `delay`, if the control is still
    /// alive.
    fn run_after(
        self: &Arc<Self>,
        delay: Duration,
        body: impl FnOnce(&Arc<Self>, MainThreadMarker) + Send + 'static,
    ) {
        let weak = Arc::downgrade(self);
        exec_after_on_owner(owner_queue(), delay, move || {
            let main =
                MainThreadMarker::new().expect("BUG: the owner queue is the AppKit main queue");
            if let Some(control) = weak.upgrade() {
                body(&control, main);
            }
        });
    }

    /// Ask the application whether the quit AppKit heard (the Dock's Quit, a
    /// quit Apple Event, `terminate:`) goes ahead, outside the delegate's
    /// callback. On [`QuitAnswer::Quit`](crate::QuitAnswer::Quit) the loop stops
    /// in order and closes every window; on
    /// [`QuitAnswer::KeepRunning`](crate::QuitAnswer::KeepRunning) every window
    /// stays as it was, its handler still installed, and the application
    /// quits itself once its windows let it. A loop already stopping asks
    /// nothing.
    fn quit_asked(self: &Arc<Self>, main: MainThreadMarker) {
        if self.owner(main).requests.get().stopping() {
            return;
        }
        let answer = self
            .app
            .quit_asked()
            .or_contain(crate::QuitAnswer::Quit, "the application's quit question");
        if answer == crate::QuitAnswer::Quit {
            self.request(main, true);
        }
    }

    /// The session is ending now: tell the application, then let AppKit go on if
    /// it is waiting. The quit callback is not run: the process ends with the
    /// session, and the application has been told what to save.
    fn finish_session_end(&self, main: MainThreadMarker, reply: Reply) {
        self.ask_session_end(SessionEndPhase::Ending);
        if reply == Reply::ToAppKit {
            NSApplication::sharedApplication(main).replyToApplicationShouldTerminate(true);
        }
    }

    /// Ask the application's handler about a session end through the call
    /// point. A handler that panics lets the session end; a run with no
    /// handler does too.
    fn ask_session_end(&self, phase: SessionEndPhase) -> SessionEndAnswer {
        self.app
            .session_end(phase)
            .or_contain(SessionEndAnswer::Proceed, "the session-end handler")
    }

    /// `application:openURLs:`: the application hears what `urls`, the
    /// absolute strings of AppKit's `NSURL`s, open, as its launch until
    /// `applicationDidFinishLaunching:` and as reaching it running after.
    fn opened_urls(&self, main: MainThreadMarker, urls: Vec<String>) {
        let launch = if self.owner(main).requests.get().launched() {
            Launch::Running
        } else {
            Launch::Cold
        };
        self.app
            .opened(crate::owner::file_uri::opened_by_urls(
                urls,
                crate::owner::file_uri::PathConvention::HOST,
                flui_platform::OpenTarget::Application,
                launch,
            ))
            .or_contain((), "the URLs AppKit opened");
    }

    /// Whether a window may open now: the loop is not quitting.
    pub(crate) fn accepts_windows(&self, main: MainThreadMarker) -> bool {
        self.owner(main).requests.admits_windows(&self.owner_signal)
    }

    fn request_reopen(self: &Arc<Self>, main: MainThreadMarker) {
        if self.owner(main).requests.accept_reopen(&self.owner_signal) {
            self.queue_drain();
        }
    }

    fn schedule_reopens(self: &Arc<Self>, main: MainThreadMarker) {
        if self.owner(main).requests.queue_reopens(&self.owner_signal) {
            self.queue_drain();
        }
    }

    /// Queue a drain of the owed reopens on the main queue.
    fn queue_drain(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        exec_async_on_owner(owner_queue(), move || {
            cleanup_step(|| {
                let main =
                    MainThreadMarker::new().expect("BUG: the owner queue is the AppKit main queue");
                if let Some(control) = weak.upgrade() {
                    control.drain_reopens(main);
                }
            });
        });
    }

    fn drain_reopens(self: &Arc<Self>, main: MainThreadMarker) {
        // What arrived while the drain ran is queued once it, and its
        // callback's captures, are gone.
        self.owner(main)
            .requests
            .drain_reopens(&self.owner_signal, &self.app, || {
                cleanup_step(|| self.schedule_reopens(main));
            });
    }

    /// Always deferred: reentrant requests cannot observe an absent leased
    /// hook. An explicit quit fences the owner signal first.
    pub(crate) fn request(self: &Arc<Self>, main: MainThreadMarker, explicit_quit: bool) {
        if self
            .owner(main)
            .requests
            .request(&self.owner_signal, explicit_quit)
        {
            let weak = Arc::downgrade(self);
            exec_async_on_owner(owner_queue(), move || {
                let main =
                    MainThreadMarker::new().expect("BUG: the owner queue is the AppKit main queue");
                if let Some(control) = weak.upgrade() {
                    control.evaluate(main);
                }
            });
        }
    }

    pub(super) fn quit(&self, main: MainThreadMarker) {
        let Some(running) = self.update(main, LoopState::begin_quit) else {
            return;
        };
        self.close_signal(main);
        if running {
            stop_application(main);
        }
    }

    fn evaluate(self: &Arc<Self>, main: MainThreadMarker) {
        let Some(explicit) = self.update(main, LoopState::begin_evaluation) else {
            return;
        };
        if explicit {
            self.quit(main);
            return;
        }
        if !self.windows.is_empty() {
            return;
        }
        // The handler may open a window, request another turn, or quit: it is
        // asked with nothing borrowed. A run with no handler, or one whose
        // handler panicked (contained by the delivery), allows the exit.
        let allowed = self
            .app
            .exit_policy()
            .or_contain(
                crate::platform::ExitAnswer::HostDecides,
                "the application's exit policy",
            )
            .ends_loop(true);
        if allowed && self.owner(main).requests.get().running() && self.windows.is_empty() {
            self.quit(main);
        }
    }

    fn start_running(self: &Arc<Self>, main: MainThreadMarker) -> bool {
        let Some(requested) = self.update(main, LoopState::start) else {
            return false;
        };
        if requested {
            self.request(main, false);
        }
        self.schedule_reopens(main);
        if let Err(error) = self.owner_signal.start() {
            tracing::error!(%error, "could not start owner signals");
        }
        true
    }

    fn finish(&self, main: MainThreadMarker) {
        if !self.update(main, LoopState::begin_finish) {
            return;
        }
        self.close_signal(main);
        // No queued cleanup: these resources must be released while main is alive.
        let windows = self.windows.values();
        for window in &windows {
            cleanup_step(|| window.delivery().clear());
        }
        for window in &windows {
            cleanup_step(|| {
                // An AppKit window always takes a close; the one failure left
                // is a window already closed, which is what quitting wants.
                if let Err(refusal) =
                    flui_platform::WindowCommand::Close.admit_and_apply(window.as_ref())
                {
                    tracing::debug!(?refusal, "a window did not close at quit");
                }
            });
        }
        let remaining: Vec<_> = self
            .windows
            .keys()
            .into_iter()
            .filter_map(|key| self.windows.remove(&key))
            .collect();
        cleanup_step(|| drop(remaining));
        cleanup_step(|| drop(windows));
        self.app
            .quit_requested()
            .or_contain((), "the application's quit");
        self.app.release();
        self.update(main, LoopState::stopped);
    }
}

fn stop_application(main: MainThreadMarker) {
    let app = NSApplication::sharedApplication(main);
    app.stop(None);
    // Apple stop(_:) checks the flag only after an NSEvent is dispatched; a
    // GCD callback alone does not unblock it. Same shape as winit 0.30.13.
    // SAFETY: application-defined event has no context/window or borrowed data.
    let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(NSEventType::ApplicationDefined, NSPoint::new(0.0, 0.0), NSEventModifierFlags::empty(), 0.0, 0, None, 0, 0, 0);
    if let Some(event) = event {
        app.postEvent_atStart(&event, true);
    }
}

define_class!(
    // SAFETY: NSObject has no additional subclass invariants; ivars are Rust-owned.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ContainedIvars<Weak<LoopControl>>]
    /// Private, loop-scoped delegate routing native quit and reopen requests.
    struct LoopDelegate;
    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for LoopDelegate {}
    // SAFETY: this delegate is retained for its entire installation on main.
    unsafe impl NSApplicationDelegate for LoopDelegate {
        // FLUI owns window policy. Suppress AppKit's default untitled
        // document creation, regardless of its visible-window classification,
        // and after a panic too.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_reopen(&self, _app: &NSApplication, _has_visible_windows: bool) -> bool {
            os_entry(
                "applicationShouldHandleReopen:hasVisibleWindows:",
                (), AfterPanic::Answer(|()| false),
                || {
                    if let Some(control) = self.ivars().upgrade() { control.request_reopen(self.mtm()); }
                    false
                },
            )
        }

        // What AppKit opens: the links and documents that launched the
        // application come before `applicationDidFinishLaunching:`, later
        // ones while it runs. Implementing this, AppKit hands documents here
        // as `file:` URLs and calls no `application:openFiles:`.
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            os_entry("application:openURLs:", (), AfterPanic::Answer(|()| ()), || {
                if let Some(control) = self.ivars().upgrade() {
                    control.opened_urls(self.mtm(), crate::owner::apple::opened_urls::absolute_strings(urls));
                }
            });
        }

        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            os_entry("applicationDidFinishLaunching:", (), AfterPanic::Answer(|()| ()), || {
                if let Some(control) = self.ivars().upgrade() {
                    control.update(self.mtm(), LoopState::finish_launching);
                }
            });
        }

        // An ordinary quit never runs user teardown inside the Objective-C
        // callback: it queues the request and cancels this one. A session
        // end is answered here, since the system waits for the answer. After
        // a panic, a session end goes on and any other quit is cancelled
        // (`reply_after_panic`).
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            os_entry(
                "applicationShouldTerminate:",
                self,
                AfterPanic::Answer(reply_after_panic),
                || {
                    self.ivars().upgrade().map_or(
                        NSApplicationTerminateReply::TerminateCancel,
                        |control| control.terminate_requested(self.mtm()),
                    )
                },
            )
        }
    }
);

/// What a log of the delegate's entries shows: which object it is.
impl std::fmt::Debug for LoopDelegate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoopDelegate").finish_non_exhaustive()
    }
}

/// What `applicationShouldTerminate:` answers when its handling panicked: a
/// session end goes on, since cancelling it holds the user's logout up; any
/// other quit is cancelled, as an ordinary one is.
fn reply_after_panic(delegate: &LoopDelegate) -> NSApplicationTerminateReply {
    if delegate
        .ivars()
        .upgrade()
        .is_some_and(|control| control.session_ending(delegate.mtm()))
    {
        NSApplicationTerminateReply::TerminateNow
    } else {
        NSApplicationTerminateReply::TerminateCancel
    }
}

/// Scope guard retains the non-owning delegate until after cleanup and detachment.
pub(super) struct LoopOwnership {
    app: Retained<NSApplication>,
    delegate: Retained<LoopDelegate>,
    control: Arc<LoopControl>,
    previous_policy: NSApplicationActivationPolicy,
    _key_up: Option<KeyUpRoute>,
    main: MainThreadMarker,
}

/// The route of a key release made while Command is held.
///
/// `NSApplication`'s `sendEvent:` keeps such a `keyUp:` from the key window
/// (it treats Command as a menu chord), so a view would see Command+K go down
/// and never come up, and its next K read as a repeat of a held key. A local
/// monitor sees the release before that dispatch and hands it to the key
/// window, as GLFW does; the event goes on to AppKit, which still drops it.
struct KeyUpRoute(Retained<AnyObject>);

impl KeyUpRoute {
    fn install() -> Option<Self> {
        let handler = RcBlock::new(|event: NonNull<NSEvent>| {
            os_entry(
                "the Command key-release monitor",
                event,
                AfterPanic::Answer(NonNull::as_ptr),
                || {
                    // SAFETY: AppKit hands the monitor a live event for the
                    // duration of the call.
                    let key_up = unsafe { event.as_ref() };
                    if key_up
                        .modifierFlags()
                        .contains(NSEventModifierFlags::Command)
                        && let Some(window) = key_up.window(
                            MainThreadMarker::new()
                                .expect("BUG: AppKit runs local event monitors on main"),
                        )
                    {
                        window.sendEvent(key_up);
                    }
                    event.as_ptr()
                },
            )
        });
        // SAFETY: the handler returns the event it was given, a valid pointer.
        unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyUp, &handler)
        }
        .map(Self)
    }
}

impl Drop for KeyUpRoute {
    fn drop(&mut self) {
        // SAFETY: the token is the monitor `addLocalMonitor…` returned.
        unsafe { NSEvent::removeMonitor(&self.0) };
    }
}

impl LoopOwnership {
    pub(super) fn acquire(
        app: &NSApplication,
        control: Arc<LoopControl>,
        main: MainThreadMarker,
    ) -> Result<Self, PlatformError> {
        if app.isRunning() || app.delegate().is_some() {
            return Err(PlatformError::EventLoop { message: "FLUI requires a standalone NSApplication with no running loop or existing delegate".into() });
        }
        let allocated =
            LoopDelegate::alloc(main).set_ivars(ContainedIvars::new(Arc::downgrade(&control)));
        // SAFETY: NSObject initialization of our newly allocated subclass.
        let delegate: Retained<LoopDelegate> = unsafe { msg_send![super(allocated), init] };
        let previous_policy = app.activationPolicy();
        control.update(main, LoopState::install);
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        Ok(Self {
            app: app.retain(),
            delegate,
            control,
            previous_policy,
            _key_up: KeyUpRoute::install(),
            main,
        })
    }

    pub(super) fn run(&self) {
        // quit during bootstrap must not be lost by starting a fresh run loop.
        if self.control.start_running(self.main) {
            self.app.activateIgnoringOtherApps(true);
            self.app.run();
        }
    }
}

impl Drop for LoopOwnership {
    fn drop(&mut self) {
        // A callback destructor can panic as well as a callback body. Keep
        // detachment outside the shield so even bootstrap unwinding cannot
        // leave AppKit pointing at the delegate that this guard will drop.
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: loop detachment finishes and the phase is set even if finish panics"
        )]
        let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.control.finish(self.main);
        }));
        self.control.update(self.main, LoopState::stopped);
        if let Err(payload) = cleanup {
            report_contained_panic(payload);
        }
        let ours = ProtocolObject::<dyn NSApplicationDelegate>::from_ref(&*self.delegate);
        if self
            .app
            .delegate()
            .as_deref()
            .is_some_and(|current| std::ptr::eq(current, ours))
        {
            if self.app.activationPolicy() == NSApplicationActivationPolicy::Regular {
                self.app.setActivationPolicy(self.previous_policy);
            }
            self.app.setDelegate(None);
        }
    }
}
