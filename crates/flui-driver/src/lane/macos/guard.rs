//! The macOS lane's user-activity guard: a listen-only event tap at the session level, on a thread
//! of its own while armed, each event classified by [`activity::foreign`].
//!
//! A tap's callback receives the pointer it was created with, which points at state the watching
//! thread owns for as long as the tap lives: no process-global cell holds it (ADR-0097). The
//! callback only classifies the event and reports the first foreign one over a channel; it
//! returns the event untouched, which a listen-only tap ignores anyway.
//!
//! A guard that cannot watch never reads as one that saw nothing: a tap the system turned off
//! (it reports `kCGEventTapDisabledByTimeout` or `ByUserInput` to the callback, and
//! `CGEventTapIsEnabled` turns false) and a watcher gone while armed are reported as foreign input
//! ([`STOPPED_WATCHING`]). The thread runs its run loop in short turns and looks at its stop flag
//! between them, so disarming never has to reach a run loop on another thread.
#![expect(
    unsafe_code,
    reason = "an event tap's C callback and its creation; the callback reads state its thread owns"
)]

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, CFRunLoopSource, kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use crate::error::Result;
use crate::lane::macos::activity::{self, Motion, Observed, STOPPED_WATCHING, TapEvent, Watcher};
use crate::lane::macos::quartz::{granted, lane_error};
use crate::lane::macos::safety::{self, Permission, Refusal};
use crate::lane::{ActivityGuard, ForeignInput};

/// How long arming waits for the tap to be installed.
const ARM_WITHIN: Duration = Duration::from_secs(5);

/// How long one turn of the watching thread's run loop lasts, between two looks at its stop flag.
const TURN: Duration = Duration::from_millis(50);

/// The macOS lane's user-activity guard: while armed, a thread of its own listens on a session
/// event tap and reports the first input the lane did not send, or that it stopped listening.
#[derive(Debug, Default)]
pub struct MacosGuard {
    watching: Option<Watch>,
    reported: bool,
}

#[derive(Debug)]
struct Watch {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
    seen: Receiver<&'static str>,
}

/// What the watching thread tells `arm`.
enum Start {
    /// The tap is installed and enabled.
    Watching,
    /// Why it could not install it.
    Failed(String),
}

impl MacosGuard {
    /// A guard that is not watching yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ActivityGuard for MacosGuard {
    fn arm(&mut self) -> Result<()> {
        self.disarm();
        self.reported = false;
        safety::permitted(
            Permission::InputMonitoring,
            granted(Permission::InputMonitoring),
        )
        .map_err(Refusal::into_error)?;
        let (ready, armed) = mpsc::channel::<Start>();
        let (report, seen) = mpsc::channel::<&'static str>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("flui-driver-macos-guard".into())
            .spawn(move || watch(&ready, report, &stopped))?;
        let failure = match armed.recv_timeout(ARM_WITHIN) {
            Ok(Start::Watching) => {
                self.watching = Some(Watch { stop, thread, seen });
                return Ok(());
            }
            Ok(Start::Failed(why)) => format!("the user-activity guard could not watch: {why}"),
            Err(RecvTimeoutError::Timeout) => {
                "the user-activity guard did not start watching in time".to_owned()
            }
            Err(RecvTimeoutError::Disconnected) => {
                "the user-activity guard's watcher ended before it watched".to_owned()
            }
        };
        // Nobody waits for the watcher any more: it is told to stop, and one that installs its
        // tap after this finds its `Watching` undelivered and removes it.
        drop(armed);
        stop.store(true, Ordering::Release);
        let _ = thread.join();
        Err(lane_error(failure))
    }

    fn foreign_input(&mut self) -> Option<ForeignInput> {
        if self.reported {
            return None;
        }
        let watcher = match self.watching.as_ref()?.seen.try_recv() {
            Ok(what) => Watcher::Reported(what),
            Err(TryRecvError::Empty) => Watcher::Quiet,
            Err(TryRecvError::Disconnected) => Watcher::Gone,
        };
        let what = activity::guard_report(watcher)?;
        self.reported = true;
        Some(ForeignInput::new(what))
    }

    fn disarm(&mut self) {
        if let Some(watch) = self.watching.take() {
            watch.stop.store(true, Ordering::Release);
            let _ = watch.thread.join();
        }
    }
}

impl Drop for MacosGuard {
    fn drop(&mut self) {
        self.disarm();
    }
}

/// What the tap's callback reads and writes: owned by the watching thread, which outlives the
/// tap.
struct TapState {
    report: Sender<&'static str>,
    own_pid: u32,
    /// Whether the first foreign event was reported; only the first is.
    sent: Cell<bool>,
}

impl TapState {
    fn report(&self, what: &'static str) {
        if !self.sent.get() {
            self.sent.set(self.report.send(what).is_ok());
        }
    }
}

/// The events the tap listens for, as a `CGEventMask`.
fn interest() -> u64 {
    [
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDown,
        CGEventType::LeftMouseUp,
        CGEventType::RightMouseDown,
        CGEventType::RightMouseUp,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
        CGEventType::OtherMouseDown,
        CGEventType::OtherMouseUp,
        CGEventType::OtherMouseDragged,
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
        CGEventType::ScrollWheel,
        CGEventType::TabletPointer,
        CGEventType::TabletProximity,
    ]
    .iter()
    .fold(0, |mask, event_type| mask | (1 << event_type.0))
}

/// What an event of `event_type` did; `None` for one the guard does not judge.
fn motion(event_type: CGEventType) -> Option<Motion> {
    Some(match event_type {
        CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => Motion::Move,
        CGEventType::LeftMouseDown
        | CGEventType::LeftMouseUp
        | CGEventType::RightMouseDown
        | CGEventType::RightMouseUp
        | CGEventType::OtherMouseDown
        | CGEventType::OtherMouseUp => Motion::Button,
        CGEventType::KeyDown | CGEventType::KeyUp | CGEventType::FlagsChanged => Motion::Key,
        CGEventType::ScrollWheel => Motion::Scroll,
        CGEventType::TabletPointer | CGEventType::TabletProximity => Motion::Tablet,
        _ => return None,
    })
}

/// The tap's callback: report what the event was when it is not the lane's, and pass it on. Its
/// judgement runs contained (`activity::tap_report`), so a panic never unwinds into the window
/// server's frame and stops the guard instead.
unsafe extern "C-unwind" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `user_info` is the `TapState` the watching thread created the tap with; the thread
    // owns it until after it removed the tap, and the callback runs on that thread's run loop.
    let state = unsafe { &*user_info.cast::<TapState>() };
    let read = || {
        if matches!(
            event_type,
            CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
        ) {
            return TapEvent::Disabled;
        }
        let Some(motion) = motion(event_type) else {
            return TapEvent::Other;
        };
        // SAFETY: the event the tap passes is valid for the call.
        let event_ref = unsafe { event.as_ref() };
        let field = |which| CGEvent::integer_value_field(Some(event_ref), which);
        TapEvent::Event(Observed::new(
            motion,
            field(CGEventField::EventSourceStateID),
            field(CGEventField::EventSourceUserData),
            field(CGEventField::EventSourceUnixProcessID),
        ))
    };
    if let Some(what) = activity::tap_report(read, state.own_pid) {
        state.report(what);
    }
    event.as_ptr()
}

/// The watching thread: install the tap, say it watches, then run the run loop in turns until told
/// to stop. A watcher nobody waits for any more removes its tap and ends.
fn watch(ready: &Sender<Start>, report: Sender<&'static str>, stop: &AtomicBool) {
    let state = TapState {
        report,
        own_pid: std::process::id(),
        sent: Cell::new(false),
    };
    let user_info = (&raw const state).cast_mut().cast::<c_void>();
    let callback: unsafe extern "C-unwind" fn(
        CGEventTapProxy,
        CGEventType,
        NonNull<CGEvent>,
        *mut c_void,
    ) -> *mut CGEvent = tap_callback;
    // SAFETY: a listen-only tap whose callback reads `state` through `user_info`; `state` lives
    // on this frame until after the tap is invalidated below.
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            interest(),
            Some(callback),
            user_info,
        )
    };
    let Some(tap) = tap else {
        let _ = ready.send(Start::Failed(
            "the system made no event tap: the process may lack Input Monitoring".to_owned(),
        ));
        return;
    };
    let installed = Installed::new(&tap);
    let Some(installed) = installed else {
        let _ = ready.send(Start::Failed(
            "the event tap's run-loop source could not be added".to_owned(),
        ));
        tap.invalidate();
        return;
    };
    CGEvent::tap_enable(&tap, true);
    if ready.send(Start::Watching).is_err() {
        // `arm` gave up waiting: remove the tap, and end.
        drop(installed);
        return;
    }
    let turn = TURN.as_secs_f64();
    while !stop.load(Ordering::Acquire) {
        // SAFETY: the default mode, a constant Core Foundation exports.
        let mode = unsafe { kCFRunLoopDefaultMode };
        let _ = CFRunLoop::run_in_mode(mode, turn, true);
        if !CGEvent::tap_is_enabled(&tap) {
            state.report(STOPPED_WATCHING);
            break;
        }
    }
    drop(installed);
}

/// The tap's run-loop source on the watching thread's run loop, and the tap: removed and
/// invalidated when the thread ends, before the state the callback reads goes.
struct Installed<'a> {
    tap: &'a CFMachPort,
    source: CFRetained<CFRunLoopSource>,
    run_loop: CFRetained<CFRunLoop>,
}

impl<'a> Installed<'a> {
    fn new(tap: &'a CFMachPort) -> Option<Self> {
        let source = CFMachPort::new_run_loop_source(None, Some(tap), 0)?;
        let run_loop = CFRunLoop::current()?;
        // SAFETY: the default mode, a constant Core Foundation exports.
        let mode = unsafe { kCFRunLoopDefaultMode };
        run_loop.add_source(Some(&*source), mode);
        Some(Self {
            tap,
            source,
            run_loop,
        })
    }
}

impl Drop for Installed<'_> {
    fn drop(&mut self) {
        CGEvent::tap_enable(self.tap, false);
        // SAFETY: the default mode, a constant Core Foundation exports.
        let mode = unsafe { kCFRunLoopDefaultMode };
        self.run_loop.remove_source(Some(&*self.source), mode);
        self.tap.invalidate();
    }
}
