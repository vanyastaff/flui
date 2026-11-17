//! What the owner delivers while a modal loop stands in for its message loop:
//! OLE's `DoDragDrop`, run from the owner window, and the system's loops for a
//! window, moving or sizing it and its menus (the system menu included).
//!
//! A modal loop waits in `GetMessage`, so the platform's own loop, which
//! delivers deadlines, does not run: with the pointer still, caret blink,
//! timers and the reveal fallback stop. [`ModalFrames`] delivers what that
//! loop would, each time the pacing's clock fires: a timer of the window,
//! which `DefWindowProc`'s loops dispatch as `WM_TIMER`, or for the drag loop,
//! which dispatches no timer, a thread that sends the owner window
//! [`MODAL_FRAME`]. [`ModalPacing`] sets that clock by the rule of
//! [`PaceTimer`]: for the next deadline, and again soon after the platform's
//! code ran on the thread ([`ModalLoops::ran`]), which may have moved one.
//! With no deadline and nothing run, the clock is not set.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak as RcWeak};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;
use smallvec::SmallVec;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    KillTimer, MSG, MsgWaitForMultipleObjects, PM_NOREMOVE, PM_QS_SENDMESSAGE, PeekMessageW,
    QS_SENDMESSAGE, SMTO_NORMAL, SendMessageTimeoutW, SetTimer, USER_TIMER_MAXIMUM,
    USER_TIMER_MINIMUM, WM_APP, WM_TIMER,
};

use super::{ClassWindows, FrameCount, WindowsLoop};
use crate::owner::app_delivery::AppDelivery;
use crate::owner::modal_pace::{ASK_AGAIN_MS, LONGEST_WAIT_MS, PaceTimer, TimerChange};
use crate::owner::signal::OwnerSignal;
use crate::owner::wake_deadline::{TurnDeadline, WakeDeadline};

/// The timer that paces a window's modal loops. Distinct from the touchpad's.
pub(crate) const MODAL_DEADLINE_TIMER: usize = 0x4D4C;

/// Sent to the owner window by the drag loop's pacing thread when its clock
/// fires: deliver the frames that are due.
pub(crate) const MODAL_FRAME: u32 = WM_APP + 22;

// The rule's own numbers are Win32's.
const _: () = assert!(ASK_AGAIN_MS == USER_TIMER_MINIMUM);
const _: () = assert!(LONGEST_WAIT_MS == USER_TIMER_MAXIMUM);

/// What one modal loop's frames are delivered from: the application whose
/// wake deadline is asked, the windows painted, the frame count the
/// deliveries are keyed by and the last deadline this loop delivered. The
/// owner window keeps one for the drag loop and each window one for its own,
/// all over the platform's one application delivery and frame count.
pub(crate) struct ModalFrames {
    app: Arc<AppDelivery>,
    windows: Weak<dyn ClassWindows>,
    frames: Rc<FrameCount>,
    /// The owner whose turn a due turn deadline brings, through its window
    /// message, which a modal loop dispatches like any other.
    signal: Weak<OwnerSignal>,
    deadline: RefCell<WakeDeadline>,
    turn_deadline: RefCell<TurnDeadline>,
}

impl ModalFrames {
    pub(crate) fn new(
        app: Arc<AppDelivery>,
        windows: Weak<dyn ClassWindows>,
        frames: Rc<FrameCount>,
        signal: Weak<OwnerSignal>,
    ) -> Self {
        Self {
            app,
            windows,
            frames,
            signal,
            deadline: RefCell::default(),
            turn_deadline: RefCell::default(),
        }
    }

    /// A modal loop starts: the wake deadline an earlier one delivered does
    /// not hold back its first. The turn deadline keeps the time its last turn
    /// served, so an instant served before is not served again.
    pub(crate) fn start(&self) {
        *self.deadline.borrow_mut() = WakeDeadline::default();
    }

    /// What the main loop would deliver, while the modal loop stands in for it
    /// and dispatches neither deadlines nor paints: a due wake deadline is
    /// delivered as the main loop delivers it, once per frame generation, and
    /// every window with an invalid region is painted, so every runtime on the
    /// thread keeps getting frames. The deadline is armed and taken by the
    /// main loop's own rule ([`WakeDeadline`]), so an instant no frame has
    /// serviced since its delivery is neither delivered again nor waited for.
    /// Returns the earliest deadline left armed, `None` when nothing is due.
    /// Nothing is delivered once the platform no longer accepts work. The
    /// application's handler is user code, asked with no borrow held and in
    /// the main loop's custody; a caller holds its own clone of `self`, since
    /// that code may close the window that keeps it.
    pub(crate) fn deliver(&self) -> Option<web_time::Instant> {
        let signal = self.signal.upgrade();
        if signal.as_ref().is_some_and(|signal| !signal.accepting()) {
            return None;
        }
        let answer = WindowsLoop::ask_wake_deadline(&self.app);
        let now = web_time::Instant::now();
        let frames = self.frames.get();
        let (armed, due) = {
            let mut deadline = self.deadline.borrow_mut();
            let armed = deadline.arm(answer, frames);
            (armed, deadline.take_due(now, frames))
        };
        if due && let Some(windows) = self.windows.upgrade() {
            windows.redraw_for_deadline();
        }
        // The turn deadline, as the main loop asks it: only while the owner
        // accepts work, with no borrow held, then armed and taken by the one
        // rule ([`TurnDeadline`]).
        let turn_answer = if signal.as_ref().is_some_and(|signal| signal.accepting()) {
            WindowsLoop::ask_turn_deadline(&self.app)
        } else {
            None
        };
        let (turn_armed, turn_due) = {
            let mut turn = self.turn_deadline.borrow_mut();
            let armed = turn.arm(turn_answer);
            let due = turn.take_due(now);
            // A delivered instant is served: nothing is left to wait for.
            (armed.filter(|_| !due), due)
        };
        if turn_due && let Some(signal) = &signal {
            let _ = signal.wake();
        }
        if let Some(windows) = self.windows.upgrade() {
            windows.paint_invalid();
        }
        [armed, turn_armed].into_iter().flatten().min()
    }
}

/// The modal loops running on one owner thread, which every procedure of the
/// thread tells when it ran the platform's code ([`Self::ran`]): a window's
/// message, an owner turn or the owner's other messages, a system broadcast, a
/// cloak change, a drop target's event. Kept by FLUI's window class on the
/// thread, so each owner on it shares them: a modal loop dispatches every
/// window of its thread.
#[derive(Default)]
pub(crate) struct ModalLoops {
    running: RefCell<SmallVec<[RcWeak<ModalPacing>; 2]>>,
}

impl ModalLoops {
    /// `pacing` paces a modal loop until [`Self::exit`].
    fn enter(&self, pacing: RcWeak<ModalPacing>) {
        let mut running = self.running.borrow_mut();
        running.retain(|kept| kept.strong_count() > 0);
        running.push(pacing);
    }

    /// `pacing` paces no modal loop any more.
    fn exit(&self, pacing: &ModalPacing) {
        self.running
            .borrow_mut()
            .retain(|kept| kept.strong_count() > 0 && !std::ptr::eq(kept.as_ptr(), pacing));
    }

    /// The platform's code ran on the owner thread, and may have moved the
    /// application's deadlines: every modal loop running asks again soon
    /// ([`PaceTimer::ask_again`]). With none running this is one check.
    pub(crate) fn ran(&self) {
        let running: SmallVec<[Rc<ModalPacing>; 2]> = {
            let Ok(running) = self.running.try_borrow() else {
                return;
            };
            if running.is_empty() {
                return;
            }
            running.iter().filter_map(RcWeak::upgrade).collect()
        };
        // Released first: a pacing that ends as its clone drops stops its
        // clock, which answers the messages other threads send meanwhile.
        for pacing in running {
            pacing.ask_again();
        }
    }

    /// A message of a FLUI window ran: the platform's code ran, unless it was
    /// a pacing timer's own `WM_TIMER`, which sets its timer itself.
    pub(crate) fn after_window_message(&self, msg: u32, wparam: WPARAM) {
        if msg == WM_TIMER && wparam.0 == MODAL_DEADLINE_TIMER {
            return;
        }
        self.ran();
    }

    /// A guard that tells `loops` the platform's code ran when it drops, a
    /// panic's unwinding included: a procedure holds it across the code it
    /// runs for a message.
    pub(crate) fn ask_again_after(loops: &Rc<Self>) -> AskAgainAfter {
        AskAgainAfter(Rc::clone(loops))
    }
}

/// See [`ModalLoops::ask_again_after`].
#[must_use = "it tells the modal loops as it drops"]
pub(crate) struct AskAgainAfter(Rc<ModalLoops>);

impl Drop for AskAgainAfter {
    fn drop(&mut self) {
        self.0.ran();
    }
}

/// What fires a modal loop's deliveries.
pub(crate) enum PaceClock {
    /// A timer of the window the system's loops run for, which they dispatch
    /// as `WM_TIMER` ([`MODAL_DEADLINE_TIMER`]).
    WindowTimer(HWND),
    /// A thread that sends the owner window [`MODAL_FRAME`]: OLE's drag loop
    /// dispatches no timer and no posted message while the pointer rests, but
    /// it does answer messages other threads send.
    Thread(PacerThread),
}

impl PaceClock {
    fn apply(&self, change: TimerChange) {
        match (self, change) {
            (_, TimerChange::Keep) => {}
            (Self::WindowTimer(hwnd), TimerChange::Set(millis)) => {
                // SAFETY: a plain call on the window's handle with no
                // callback: the timer posts `WM_TIMER`, and setting it again
                // replaces it.
                let _ = unsafe { SetTimer(Some(*hwnd), MODAL_DEADLINE_TIMER, millis, None) };
            }
            (Self::WindowTimer(hwnd), TimerChange::Kill) => kill_window_timer(*hwnd),
            (Self::Thread(thread), change) => thread.apply(change),
        }
    }
}

/// Kill `hwnd`'s pacing timer.
pub(crate) fn kill_window_timer(hwnd: HWND) {
    // SAFETY: a plain call on the window's handle; a timer that is not set,
    // or a handle already gone, fails harmlessly.
    let _ = unsafe { KillTimer(Some(hwnd), MODAL_DEADLINE_TIMER) };
}

/// A window's (or the owner window's) modal-loop pacing: what its loops'
/// frames are delivered from, how many of them run, and the clock its
/// deliveries are fired by, set by the rule of [`PaceTimer`].
pub(crate) struct ModalPacing {
    frames: Rc<ModalFrames>,
    loops: Rc<ModalLoops>,
    /// How many modal loops are running for the window.
    depth: Cell<u32>,
    timer: Cell<PaceTimer>,
    /// The clock, while a modal loop runs.
    clock: RefCell<Option<PaceClock>>,
}

impl ModalPacing {
    /// The pacing of the loops whose frames `frames` delivers, told by `loops`
    /// when the thread's code ran.
    pub(crate) fn new(frames: ModalFrames, loops: Rc<ModalLoops>) -> Rc<Self> {
        Rc::new(Self {
            frames: Rc::new(frames),
            loops,
            depth: Cell::new(0),
            timer: Cell::new(PaceTimer::Stopped),
            clock: RefCell::new(None),
        })
    }

    /// A modal loop starts. The outermost starts the pacing: its frames start
    /// over, the clock `clock` makes fires its first delivery at once, and the
    /// thread's code that runs from now on asks again. Without a clock
    /// (the drag loop's thread did not start) no frame is delivered.
    pub(crate) fn enter(self: &Rc<Self>, clock: impl FnOnce() -> Option<PaceClock>) {
        let depth = self.depth.get();
        self.depth.set(depth.saturating_add(1));
        if depth > 0 {
            return;
        }
        self.frames.start();
        let clock = clock();
        *self.clock.borrow_mut() = clock;
        self.loops.enter(Rc::downgrade(self));
        self.ask_again();
    }

    /// A modal loop ends. With none left the clock stops.
    pub(crate) fn exit(&self) {
        let depth = self.depth.get().saturating_sub(1);
        self.depth.set(depth);
        if depth == 0 {
            self.end();
        }
    }

    /// The window the loops run for is destroyed: the pacing ends whatever
    /// loops have not said they ended.
    pub(crate) fn abandon(&self) {
        if self.depth.replace(0) > 0 {
            self.end();
        }
    }

    fn end(&self) {
        let mut timer = self.timer.get();
        let change = timer.stop();
        self.timer.set(timer);
        self.loops.exit(self);
        // Taken out before it stops: a thread clock answers the messages
        // other threads send until it has gone, which runs code that asks
        // this pacing again (it is no longer running).
        let clock = self.clock.borrow_mut().take();
        if let Some(clock) = clock {
            clock.apply(change);
        }
    }

    /// The modal loops running on the thread, which a window's code outside
    /// its class's procedure (a cloak change) tells when it ran.
    pub(crate) fn loops(&self) -> &ModalLoops {
        &self.loops
    }

    /// Whether a modal loop runs for the window.
    pub(crate) fn running(&self) -> bool {
        self.depth.get() > 0
    }

    /// The thread's code ran: the clock fires again soon.
    fn ask_again(&self) {
        if !self.running() {
            return;
        }
        let mut timer = self.timer.get();
        let change = timer.ask_again(web_time::Instant::now());
        self.timer.set(timer);
        self.apply(change);
    }

    /// The clock fired: deliver what is due and set the clock for the next
    /// delivery. `false`, delivering nothing, when no modal loop runs (a
    /// timer that fired as its loop ended): the caller stops that timer. The
    /// delivery runs user code, which may end the loop, start another, or
    /// close the window that keeps this pacing: the caller holds no borrow.
    pub(crate) fn tick(&self) -> bool {
        if !self.running() {
            return false;
        }
        let mut timer = self.timer.get();
        timer.deliver();
        self.timer.set(timer);
        let mut unwinding = DeliveryUnwind {
            pacing: self,
            returned: false,
        };
        let frames = Rc::clone(&self.frames);
        let deadline = frames.deliver();
        unwinding.returned = true;
        drop(unwinding);
        // User code ended the loop, or ended it and started another, whose
        // own first delivery stands: only this delivery's timer is set.
        let mut timer = self.timer.get();
        if self.running() && matches!(timer, PaceTimer::Delivering { .. }) {
            let change = timer.delivered(deadline, web_time::Instant::now());
            self.timer.set(timer);
            self.apply(change);
        }
        true
    }

    fn apply(&self, change: TimerChange) {
        if let Some(clock) = self.clock.borrow().as_ref() {
            clock.apply(change);
        }
    }

    /// The pacing's timer, read by a test.
    #[cfg(test)]
    pub(crate) fn timer(&self) -> PaceTimer {
        self.timer.get()
    }
}

/// A delivery that unwinds (the handler's panic, which the owner window's
/// procedure contains) leaves no timer stuck delivering, which would hold
/// every later request back: the pacing asks again, as after code that ran.
struct DeliveryUnwind<'a> {
    pacing: &'a ModalPacing,
    returned: bool,
}

impl Drop for DeliveryUnwind<'_> {
    fn drop(&mut self) {
        if self.returned {
            return;
        }
        if matches!(self.pacing.timer.get(), PaceTimer::Delivering { .. }) {
            self.pacing.timer.set(PaceTimer::Stopped);
        }
        self.pacing.ask_again();
    }
}

/// When the drag loop's pacing thread sends next, and whether it stops.
#[derive(Default)]
struct PacerPace {
    due: Option<web_time::Instant>,
    stopped: bool,
}

/// The drag loop's clock: a thread that sends the owner window
/// [`MODAL_FRAME`] when the pacing's timer is due, and otherwise sleeps,
/// with no timeout while nothing is due.
pub(crate) struct PacerThread {
    pace: Arc<Mutex<PacerPace>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PacerThread {
    /// The clock of a loop run from `owner_window`; `None`, with a warning,
    /// when the thread does not start.
    pub(crate) fn start(owner_window: HWND) -> Option<Self> {
        let address = owner_window.0 as isize;
        let pace = Arc::new(Mutex::new(PacerPace::default()));
        let paced = Arc::clone(&pace);
        let spawned = std::thread::Builder::new()
            .name("flui-drag-frames".into())
            .spawn(move || {
                loop {
                    let due = {
                        let pace = paced.lock();
                        if pace.stopped {
                            break;
                        }
                        pace.due
                    };
                    let Some(due) = due else {
                        std::thread::park();
                        continue;
                    };
                    let now = web_time::Instant::now();
                    if due > now {
                        std::thread::park_timeout(due - now);
                        continue;
                    }
                    // Taken: the owner sets the next one as it delivers.
                    {
                        let mut pace = paced.lock();
                        if pace.stopped {
                            break;
                        }
                        if pace.due == Some(due) {
                            pace.due = None;
                        }
                    }
                    // SAFETY: a message with no pointer payload, sent to the
                    // owner window, which outlives the session this thread
                    // paces. The timeout bounds the wait if the owner thread
                    // is busy.
                    let sent = unsafe {
                        SendMessageTimeoutW(
                            HWND(address as *mut _),
                            MODAL_FRAME,
                            WPARAM(0),
                            LPARAM(0),
                            SMTO_NORMAL,
                            100,
                            None,
                        )
                    };
                    if sent.0 == 0 {
                        // The owner was busy: the delivery is owed, soon.
                        let mut pace = paced.lock();
                        if pace.due.is_none() {
                            pace.due = Some(
                                web_time::Instant::now()
                                    + Duration::from_millis(u64::from(ASK_AGAIN_MS)),
                            );
                        }
                    }
                }
            });
        match spawned {
            Ok(thread) => Some(Self {
                pace,
                thread: Some(thread),
            }),
            Err(error) => {
                tracing::warn!(%error, "no frames while the drag rests: the pacing thread did not start");
                None
            }
        }
    }

    fn apply(&self, change: TimerChange) {
        let due = match change {
            TimerChange::Keep => return,
            TimerChange::Set(millis) => {
                Some(web_time::Instant::now() + Duration::from_millis(u64::from(millis)))
            }
            TimerChange::Kill => None,
        };
        self.pace.lock().due = due;
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

impl Drop for PacerThread {
    fn drop(&mut self) {
        self.pace.lock().stopped = true;
        let Some(thread) = self.thread.take() else {
            return;
        };
        thread.thread().unpark();
        // A delivery request already being sent waits for this thread: keep
        // answering sent messages until the pacer has gone.
        while !thread.is_finished() {
            let mut msg = MSG::default();
            // SAFETY: `PM_QS_SENDMESSAGE` with `PM_NOREMOVE` only dispatches
            // messages other threads sent this one; `msg` is a live local.
            // The wait takes no handles and lasts at most a millisecond.
            unsafe {
                let _ = PeekMessageW(&raw mut msg, None, 0, 0, PM_NOREMOVE | PM_QS_SENDMESSAGE);
                MsgWaitForMultipleObjects(None, false, 1, QS_SENDMESSAGE);
            }
        }
        let _ = thread.join();
    }
}
