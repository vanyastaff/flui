//! Win32 haptics: the pen in range of the window (ADR-0210 §5, §7, §8).
//!
//! The window's own actuator, `InputHapticsManager`, is in no published
//! `windows` crate, so a window plays on a pen: the request's source while
//! that pen is in range of the window (touching or hovering), otherwise the
//! pen read most recently, through the `SimpleHapticsController` that
//! `PenDevice::GetFromPointerId` hands out for its pointer. With no pen in
//! range, or a pen without a controller, both tiers are
//! [`HapticOutcome::Unsupported`]: `Unavailable` while no pen is in range,
//! `NotOnThisDevice` for a pen without the waveform.
//!
//! What plays is the degradation table's (`haptics::degradation`); the
//! controller is read on each request, since the pen behind a pointer is the
//! OS's to change. Its calls go to one worker thread per window, which makes
//! each at its time and stops a continuous waveform the controller cannot
//! be given a length for; a newer request replaces what is left of an older
//! one. The worker's mailbox holds one request, so a request the worker has
//! not taken yet is replaced unplayed rather than queued behind, and a
//! waveform reached more than two frames after its time is skipped: a stalled
//! controller call never leaves a burst of late feedback behind it. No user
//! code runs on that thread.

// This module calls the operating system directly, which has no safe wrapper:
// the workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows::Devices::Haptics::{
    KnownSimpleHapticsControllerWaveforms, SimpleHapticsController, SimpleHapticsControllerFeedback,
};
use windows::Devices::Input::PenDevice;
use windows::Foundation::TimeSpan;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use super::degradation::{
    PenCommand, PenController, PenWaveform, actuator_for, pen_feedback_step, pen_timeline,
    windows_capabilities, windows_feedback, windows_pattern,
};
use crate::owner::windows::WindowRef;
use flui_platform::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform::{DeviceId, HapticFeedback, PlatformHaptics, Unsupported, UnsupportedReason};

/// Nothing plays now: no pen is in range, or the worker cannot take the
/// request; a later request may play.
const NOT_NOW: HapticOutcome =
    HapticOutcome::Unsupported(Unsupported::new(UnsupportedReason::Unavailable));

/// Nothing plays on this pen: its controller has no waveform for the request.
const NOT_ON_THIS_PEN: HapticOutcome =
    HapticOutcome::Unsupported(Unsupported::new(UnsupportedReason::NotOnThisDevice));

/// A pen's controller with what the table needs to know about it, and the
/// feedback object of each waveform it lists.
struct Pen {
    controller: SimpleHapticsController,
    facts: PenController,
    feedback: Vec<(PenWaveform, SimpleHapticsControllerFeedback)>,
}

impl Pen {
    /// The controller of the pen behind `pointer`, or `None` when the
    /// pointer is not a pen with one.
    fn of_pointer(pointer: u32) -> Option<Self> {
        let controller = PenDevice::GetFromPointerId(pointer)
            .and_then(|device| device.SimpleHapticsController())
            .ok()?;
        let known: Vec<(u16, PenWaveform)> = [
            (
                KnownSimpleHapticsControllerWaveforms::Click(),
                PenWaveform::Click,
            ),
            (
                KnownSimpleHapticsControllerWaveforms::Press(),
                PenWaveform::Press,
            ),
            (
                KnownSimpleHapticsControllerWaveforms::Release(),
                PenWaveform::Release,
            ),
            (
                KnownSimpleHapticsControllerWaveforms::BuzzContinuous(),
                PenWaveform::BuzzContinuous,
            ),
            (
                KnownSimpleHapticsControllerWaveforms::Success(),
                PenWaveform::Success,
            ),
            (
                KnownSimpleHapticsControllerWaveforms::Error(),
                PenWaveform::Error,
            ),
        ]
        .into_iter()
        .filter_map(|(id, waveform)| Some((id.ok()?, waveform)))
        .collect();
        let feedback: Vec<_> = controller
            .SupportedFeedback()
            .ok()?
            .into_iter()
            .filter_map(|feedback| {
                let id = feedback.Waveform().ok()?;
                let (_, waveform) = known.iter().find(|(known, _)| *known == id)?;
                Some((*waveform, feedback))
            })
            .collect();
        let facts = PenController::new(
            feedback.iter().map(|(waveform, _)| *waveform).collect(),
            controller.IsIntensitySupported().unwrap_or(false),
        )
        .with_play_duration(controller.IsPlayDurationSupported().unwrap_or(false));
        Some(Self {
            controller,
            facts,
            feedback,
        })
    }

    /// `commands` as a playback of this pen, or `None` when one names a
    /// waveform the pen did not list.
    fn playback(&self, commands: Vec<PenCommand>) -> Option<Playback<SimpleHapticsController>> {
        let calls = commands
            .into_iter()
            .map(|command| {
                Some(match command {
                    PenCommand::Play {
                        at,
                        waveform,
                        intensity,
                        length,
                    } => (
                        at,
                        Call::Play {
                            feedback: self
                                .feedback
                                .iter()
                                .find(|(listed, _)| *listed == waveform)?
                                .1
                                .clone(),
                            intensity,
                            length,
                        },
                    ),
                    PenCommand::Stop { at } => (at, Call::Stop),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Playback {
            sink: self.controller.clone(),
            started: Instant::now(),
            calls,
            next: 0,
        })
    }
}

/// One controller call.
enum Call<F> {
    Play {
        feedback: F,
        intensity: Option<f32>,
        length: Option<Duration>,
    },
    Stop,
}

/// What a playback's calls are made on: a pen's controller.
trait PenSink: Send + 'static {
    /// A waveform the controller lists.
    type Feedback: Send;

    fn call(&self, call: &Call<Self::Feedback>) -> windows_core::Result<()>;
}

impl PenSink for SimpleHapticsController {
    type Feedback = SimpleHapticsControllerFeedback;

    fn call(&self, call: &Call<Self::Feedback>) -> windows_core::Result<()> {
        match call {
            Call::Play {
                feedback,
                intensity,
                length: Some(length),
            } => self.SendHapticFeedbackForDuration(
                feedback,
                intensity.map_or(1.0, f64::from),
                time_span(*length),
            ),
            Call::Play {
                feedback,
                intensity: Some(intensity),
                length: None,
            } => self.SendHapticFeedbackWithIntensity(feedback, f64::from(*intensity)),
            Call::Play {
                feedback,
                intensity: None,
                length: None,
            } => self.SendHapticFeedback(feedback),
            Call::Stop => self.StopFeedback(),
        }
    }
}

/// How late a call may still start: two 60 Hz frames, the default frame
/// deadline. A waveform later than that is skipped, because feedback detached
/// from its visible change misleads (ADR-0210 §4); a stop is always made.
const LATEST_CALL: Duration = Duration::from_millis(34);

/// A request's calls on one controller, each at its time after `started`.
struct Playback<S: PenSink> {
    sink: S,
    started: Instant,
    calls: Vec<(Duration, Call<S::Feedback>)>,
    next: usize,
}

impl<S: PenSink> Playback<S> {
    /// When the next call is due, if one is left.
    fn due(&self) -> Option<Instant> {
        let (at, _) = self.calls.get(self.next)?;
        Some(self.started.checked_add(*at).unwrap_or(self.started))
    }

    /// Make every call due by `now`, skipping a waveform past its latest
    /// start.
    fn run_due(&mut self, now: Instant) {
        while let Some(due) = self.due()
            && due <= now
        {
            let (_, call) = &self.calls[self.next];
            self.next += 1;
            if matches!(call, Call::Play { .. }) && now.saturating_duration_since(due) > LATEST_CALL
            {
                continue;
            }
            if let Err(error) = self.sink.call(call) {
                tracing::debug!(
                    target: "flui_native::windows",
                    %error,
                    "a pen haptic call failed; the pen may have left range"
                );
            }
        }
    }

    /// End what is still playing: a replaced or abandoned playback never
    /// outlasts its request.
    fn stop(&self) {
        if self.due().is_some() {
            let _ = self.sink.call(&Call::Stop);
        }
    }
}

/// The worker's one pending playback. A newer request replaces one the
/// worker has not taken yet, so requests the owner thread made while the
/// worker was busy never queue up to play late in a burst: only the newest
/// is left (ADR-0210 §8).
struct Mailbox<P> {
    slot: parking_lot::Mutex<Slot<P>>,
    ready: parking_lot::Condvar,
}

struct Slot<P> {
    pending: Option<P>,
    closed: bool,
}

/// What the worker found in its mailbox.
enum Taken<P> {
    /// The newest playback.
    Newer(P),
    /// The wait ran out with nothing new.
    Timeout,
    /// The window's haptics are gone.
    Closed,
}

impl<P> Mailbox<P> {
    fn new() -> Self {
        Self {
            slot: parking_lot::Mutex::new(Slot {
                pending: None,
                closed: false,
            }),
            ready: parking_lot::Condvar::new(),
        }
    }

    /// Leave `playback` for the worker. `Ok` with the one it replaced, which
    /// never played; `Err` hands it back once the mailbox is closed.
    fn post(&self, playback: P) -> Result<Option<P>, P> {
        let mut slot = self.slot.lock();
        if slot.closed {
            return Err(playback);
        }
        let replaced = slot.pending.replace(playback);
        drop(slot);
        self.ready.notify_one();
        Ok(replaced)
    }

    /// Stop the worker once it has taken what is pending.
    fn close(&self) {
        self.slot.lock().closed = true;
        self.ready.notify_one();
    }

    /// The newest playback, waiting for one until `until` (or for ever).
    fn take(&self, until: Option<Instant>) -> Taken<P> {
        let mut slot = self.slot.lock();
        loop {
            if let Some(playback) = slot.pending.take() {
                return Taken::Newer(playback);
            }
            if slot.closed {
                return Taken::Closed;
            }
            match until {
                Some(until) => {
                    if self.ready.wait_until(&mut slot, until).timed_out() {
                        return slot.pending.take().map_or(Taken::Timeout, Taken::Newer);
                    }
                }
                None => self.ready.wait(&mut slot),
            }
        }
    }
}

/// A worker thread playing what its mailbox is handed; `None` when the
/// thread did not start.
fn spawn_worker<S: PenSink>() -> Option<Arc<Mailbox<Playback<S>>>> {
    let mailbox = Arc::new(Mailbox::new());
    let worked = Arc::clone(&mailbox);
    std::thread::Builder::new()
        .name("flui-pen-haptics".into())
        .spawn(move || play(&worked))
        .map_err(|error| {
            tracing::warn!(
                target: "flui_native::windows",
                %error,
                "the pen haptics thread did not start; pen haptics are unavailable"
            );
        })
        .ok()
        .map(|_| mailbox)
}

/// `duration` in `TimeSpan`'s 100 ns ticks, saturating.
fn time_span(duration: Duration) -> TimeSpan {
    TimeSpan {
        Duration: i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX),
    }
}

/// The worker: plays the newest playback, each call at its time, until its
/// window's haptics are dropped.
fn play<S: PenSink>(mailbox: &Mailbox<Playback<S>>) {
    // SAFETY: no pointer arguments. This thread was spawned for this loop
    // and entered no apartment before; a success is balanced by the single
    // `CoUninitialize` below, on the same thread, after the last WinRT call.
    let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let mut current: Option<Playback<S>> = None;
    loop {
        match mailbox.take(current.as_ref().and_then(Playback::due)) {
            Taken::Newer(newer) => {
                if let Some(older) = current.replace(newer) {
                    older.stop();
                }
            }
            Taken::Timeout => {}
            Taken::Closed => break,
        }
        if let Some(playback) = current.as_mut() {
            playback.run_due(Instant::now());
        }
    }
    if let Some(playback) = current.take() {
        playback.stop();
    }
    if entered {
        // SAFETY: balances the successful `CoInitializeEx` above on this
        // thread; every WinRT object this thread used was dropped above.
        unsafe { CoUninitialize() };
    }
}

/// One window's haptic actuator.
pub struct WindowsHaptics {
    /// The window, reached only on its owner thread and only while its
    /// handle still names it.
    window: WindowRef,
    /// The worker's mailbox, started on the first playback.
    worker: OnceLock<Option<Arc<Mailbox<Playback<SimpleHapticsController>>>>>,
}

impl Drop for WindowsHaptics {
    fn drop(&mut self) {
        // The worker stops what still plays and exits.
        if let Some(Some(mailbox)) = self.worker.get() {
            mailbox.close();
        }
    }
}

impl std::fmt::Debug for WindowsHaptics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsHaptics")
            .field("window", &self.window)
            .finish_non_exhaustive()
    }
}

/// Each Win32 window's actuator is the pen in range of it.
impl crate::host::Haptics<WindowRef> for WindowsHaptics {
    fn for_window(window: &WindowRef) -> Arc<dyn PlatformHaptics> {
        WindowsHaptics::new(*window)
    }
}

impl WindowsHaptics {
    pub(crate) fn new(window: WindowRef) -> Arc<Self> {
        Arc::new(Self {
            window,
            worker: OnceLock::new(),
        })
    }

    /// The pointer of the pen `source` names while it is in range of the
    /// window, else of the pen read most recently: `None` with no pen in
    /// range, off the window's owner thread, or after the window closed.
    fn pen_pointer(&self, source: Option<DeviceId>) -> Option<u32> {
        let inputs = crate::owner::windows::window_inputs()?;
        let actuator = actuator_for(source, |device| {
            inputs.pen_in_range(self.window, Some(device)).is_some()
        });
        let pointer = inputs.pen_in_range(self.window, actuator)?;
        u32::try_from(pointer).ok()
    }

    /// The controller of [`Self::pen_pointer`]'s pen, or what a request
    /// reports for want of one: [`NOT_NOW`] with no pen in range,
    /// [`NOT_ON_THIS_PEN`] for a pen the OS has no controller for.
    fn pen(&self, source: Option<DeviceId>) -> Result<Pen, HapticOutcome> {
        let pointer = self.pen_pointer(source).ok_or(NOT_NOW)?;
        Pen::of_pointer(pointer).ok_or(NOT_ON_THIS_PEN)
    }

    /// Hand `commands` to the worker and report `outcome`, or
    /// [`HapticOutcome::Unsupported`] when they cannot be played.
    fn submit(
        &self,
        pen: &Pen,
        commands: Vec<PenCommand>,
        outcome: HapticOutcome,
    ) -> HapticOutcome {
        let Some(playback) = pen.playback(commands) else {
            return NOT_ON_THIS_PEN;
        };
        let Some(mailbox) = self.worker.get_or_init(spawn_worker) else {
            return NOT_NOW;
        };
        match mailbox.post(playback) {
            // The replaced request never played; this one replaces it.
            Ok(replaced) => {
                drop(replaced);
                outcome
            }
            Err(_) => NOT_NOW,
        }
    }
}

impl PlatformHaptics for WindowsHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, request: &HapticRequest) -> HapticOutcome {
        let pen = match self.pen(request.source) {
            Ok(pen) => pen,
            Err(outcome) => return outcome,
        };
        match windows_feedback(feedback, Some(&pen.facts)) {
            Some((waveform, outcome)) => self.submit(
                &pen,
                pen_timeline(&[pen_feedback_step(waveform)], &pen.facts),
                outcome,
            ),
            None => NOT_ON_THIS_PEN,
        }
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let pen = match self.pen(request.source) {
            Ok(pen) => pen,
            Err(outcome) => return outcome,
        };
        match windows_pattern(pattern, Some(&pen.facts)) {
            Some((steps, HapticOutcome::Approximated)) => self.submit(
                &pen,
                pen_timeline(&steps, &pen.facts),
                HapticOutcome::Approximated,
            ),
            Some(_) => match self.perform_with(fallback, request) {
                outcome if outcome.played() => HapticOutcome::FellBack,
                outcome => outcome,
            },
            None => NOT_ON_THIS_PEN,
        }
    }

    fn capabilities(&self, source: Option<DeviceId>) -> HapticCapabilities {
        windows_capabilities(self.pen(source).ok().as_ref().map(|pen| &pen.facts))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A real window's actuator on a host with no haptic pen: the window has
/// one, and with no pen in range, or a pen pointer the OS has no
/// `PenDevice` for, every request of both tiers is unavailable.
#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::Size;
    use flui_platform::EventTime;
    use flui_platform::haptics::{
        HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, HapticTiming,
    };
    use flui_platform::pointer::{PenTool, PointerKind};

    use crate::WindowsPlatform;
    use crate::input::assembly::{
        NativeContact, NativeDevice, NativeReading, ReadingAction, ReadingTime,
    };
    use crate::input::windows::testing::decode_into;
    use crate::owner::windows::{WindowRef, window_inputs};
    use crate::platform::Platform as _;
    use crate::window::windows::WindowsWindow;
    use flui_platform::HapticFeedback;
    use flui_platform::WindowOptions;

    fn taps() -> HapticPattern {
        HapticPattern::builder()
            .transient(Duration::ZERO, 1.0, 0.8)
            .transient(Duration::from_millis(60), 0.4, 0.2)
            .build()
            .expect("a valid pattern")
    }

    /// Every request of both tiers reports `refused`, and the actuator claims
    /// nothing.
    fn assert_unavailable(
        haptics: &dyn flui_platform::PlatformHaptics,
        refused: HapticOutcome,
        when: &str,
    ) {
        for timing in [HapticTiming::Immediate, HapticTiming::FramePresented] {
            let request = HapticRequest::new(timing);
            for kind in HapticFeedback::ALL {
                assert_eq!(
                    haptics.perform_with(kind, &request),
                    refused,
                    "{kind:?} {when}"
                );
            }
            assert_eq!(
                haptics.play(&taps(), HapticFeedback::Drop, &request),
                refused,
                "a pattern {when}"
            );
        }
        assert_eq!(
            haptics.capabilities(None),
            HapticCapabilities::NONE,
            "{when}"
        );
    }

    #[test]
    fn a_window_without_a_haptic_pen_in_range_reports_unavailable() {
        let platform = WindowsPlatform::try_new().expect("platform");
        let window = platform
            .open_window(
                WindowOptions::default()
                    .with_title("flui haptics")
                    .with_size(Size::new(160.0, 120.0)),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("window");
        let haptics = window.haptics().expect("a Win32 window has an actuator");
        assert_unavailable(haptics.as_ref(), super::NOT_NOW, "with no pen in range");

        let win32 = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 window");
        let this = WindowRef::new(win32.hwnd(), win32.identity);
        let pen = PointerKind::Pen { tool: PenTool::Tip };
        let hovering = NativeReading::new(
            NativeContact(0x4242),
            pen,
            ReadingAction::Enter,
            ReadingTime::epoch(EventTime::from_nanos(1_000_000)),
        )
        .at(10.0, 10.0, 1.0)
        .from_device(NativeDevice(0x51));
        decode_into(this.hwnd(), hovering).expect("a valid reading");
        assert_eq!(
            window_inputs()
                .expect("the class's input")
                .pen_in_range(this, None),
            Some(0x4242)
        );
        assert_unavailable(
            haptics.as_ref(),
            super::NOT_ON_THIS_PEN,
            "for a pen pointer the OS has no pen device for",
        );
    }

    /// An actuator reads the pen of the window it was made for, never of a
    /// window the OS later built behind the same handle. Red-check: read
    /// the context by handle alone in `pen_pointer`.
    #[test]
    fn an_actuator_reads_only_its_own_windows_pen() {
        let platform = WindowsPlatform::try_new().expect("platform");
        let open = |title: &str| {
            platform
                .open_window(
                    WindowOptions::default()
                        .with_title(title)
                        .with_size(Size::new(160.0, 120.0)),
                    flui_testing::window_handler::RecordingWindowHandler::new(),
                )
                .expect("window")
        };
        let (first, second) = (open("flui haptics a"), open("flui haptics b"));
        let win32 = |window: &std::rc::Rc<dyn crate::host_window::HostWindow>| {
            let window = window
                .as_any()
                .downcast_ref::<WindowsWindow>()
                .expect("Win32 window");
            (window.hwnd(), window.identity)
        };
        let ((_, first_identity), (hwnd, identity)) = (win32(&first), win32(&second));
        let hovering = NativeReading::new(
            NativeContact(0x4242),
            PointerKind::Pen { tool: PenTool::Tip },
            ReadingAction::Enter,
            ReadingTime::epoch(EventTime::from_nanos(1_000_000)),
        )
        .at(10.0, 10.0, 1.0)
        .from_device(NativeDevice(0x51));
        decode_into(hwnd, hovering).expect("a valid reading");

        let own = super::WindowsHaptics::new(WindowRef::new(hwnd, identity));
        assert_eq!(own.pen_pointer(None), Some(0x4242));
        // The handle names the second window; the identity is the first's,
        // as for an actuator whose window closed and whose handle the OS
        // recycled.
        let recycled = super::WindowsHaptics::new(WindowRef::new(hwnd, first_identity));
        assert_eq!(recycled.pen_pointer(None), None);
    }

    /// A controller that records each waveform it plays and blocks in the
    /// first until released, as a pen's HID write can.
    struct Stalling {
        played: Arc<parking_lot::Mutex<Vec<u32>>>,
        release: Arc<(parking_lot::Mutex<bool>, parking_lot::Condvar)>,
    }

    impl super::PenSink for Stalling {
        type Feedback = u32;

        fn call(&self, call: &super::Call<u32>) -> windows_core::Result<()> {
            if let super::Call::Play { feedback, .. } = call {
                let first = {
                    let mut played = self.played.lock();
                    played.push(*feedback);
                    played.len() == 1
                };
                if first {
                    let (released, ready) = &*self.release;
                    let mut released = released.lock();
                    while !*released {
                        ready.wait(&mut released);
                    }
                }
            }
            Ok(())
        }
    }

    fn click(
        id: u32,
        started: Instant,
        played: &Arc<parking_lot::Mutex<Vec<u32>>>,
        release: &Arc<(parking_lot::Mutex<bool>, parking_lot::Condvar)>,
    ) -> super::Playback<Stalling> {
        super::Playback {
            sink: Stalling {
                played: Arc::clone(played),
                release: Arc::clone(release),
            },
            started,
            calls: vec![(
                Duration::ZERO,
                super::Call::Play {
                    feedback: id,
                    intensity: None,
                    length: None,
                },
            )],
            next: 0,
        }
    }

    /// Requests made while the worker is stuck in a controller call never
    /// queue up to play in a burst afterwards: only the newest plays, and a
    /// waveform reached past its latest start is skipped (ADR-0210 §4, §8).
    #[test]
    fn a_stalled_pen_worker_plays_only_the_newest_request_and_never_late() {
        let played = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let release = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));
        let mailbox = super::spawn_worker::<Stalling>().expect("the worker starts");
        let posted = |id, started| {
            mailbox
                .post(click(id, started, &played, &release))
                .ok()
                .expect("the mailbox is open")
        };
        drop(posted(0, Instant::now()));
        let deadline = Instant::now() + Duration::from_secs(5);
        while played.lock().is_empty() {
            assert!(Instant::now() < deadline, "the worker never played");
            std::thread::yield_now();
        }
        // Twenty detents while the controller is stuck in the first click;
        // the last is due a little later, so a loaded host cannot make it
        // late.
        for id in 1..20 {
            drop(posted(id, Instant::now()));
        }
        drop(posted(20, Instant::now() + Duration::from_millis(100)));
        {
            let (released, ready) = &*release;
            *released.lock() = true;
            ready.notify_all();
        }
        while !played.lock().contains(&20) {
            assert!(Instant::now() < deadline, "the newest request never played");
            std::thread::yield_now();
        }
        // A request taken a second after its time is not played.
        let stale = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("an earlier instant");
        drop(posted(99, stale));
        std::thread::sleep(Duration::from_millis(100));
        mailbox.close();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(*played.lock(), [0, 20]);
    }

    /// The waveform ids the controller's `SupportedFeedback` is matched
    /// against are the HID haptic usages, read through WinRT on this host.
    #[test]
    fn known_pen_waveforms_resolve_through_winrt() {
        use windows::Devices::Haptics::KnownSimpleHapticsControllerWaveforms as Known;
        let ids = [
            Known::Click(),
            Known::BuzzContinuous(),
            Known::Press(),
            Known::Release(),
            Known::Success(),
            Known::Error(),
        ]
        .map(|id| id.expect("a known waveform"));
        assert_eq!(ids, [0x1003, 0x1004, 0x1006, 0x1007, 0x1009, 0x100A]);
    }
}
