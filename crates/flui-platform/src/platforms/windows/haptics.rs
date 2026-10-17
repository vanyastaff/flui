//! Win32 haptics: the pen in range of the window (ADR-0210 §5, §7, §8).
//!
//! The window's own actuator, `InputHapticsManager`, is in no published
//! `windows` crate, so a window plays on a pen: the request's source while
//! that pen is in range of the window (touching or hovering), otherwise the
//! pen read most recently, through the `SimpleHapticsController` that
//! `PenDevice::GetFromPointerId` hands out for its pointer. With no pen in
//! range, or a pen without a controller, both tiers are
//! [`HapticOutcome::Unavailable`].
//!
//! What plays is the degradation table's (`shared::haptic_degradation`); the
//! controller is read on each request, since the pen behind a pointer is the
//! OS's to change. Its calls go to one worker thread per window, which makes
//! each at its time and stops a continuous waveform the controller cannot
//! be given a length for; a newer request replaces what is left of an older
//! one. No user code runs on that thread.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows::Devices::Haptics::{
    KnownSimpleHapticsControllerWaveforms, SimpleHapticsController, SimpleHapticsControllerFeedback,
};
use windows::Devices::Input::PenDevice;
use windows::Foundation::TimeSpan;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use crate::shared::haptic_degradation::{
    PenCommand, PenController, PenWaveform, actuator_for, pen_feedback_step, pen_timeline,
    windows_capabilities, windows_feedback, windows_pattern,
};
use flui_platform_api::haptics::{HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};

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
    fn playback(&self, commands: Vec<PenCommand>) -> Option<Playback> {
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
            controller: self.controller.clone(),
            started: Instant::now(),
            calls,
            next: 0,
        })
    }
}

/// One controller call.
enum Call {
    Play {
        feedback: SimpleHapticsControllerFeedback,
        intensity: Option<f32>,
        length: Option<Duration>,
    },
    Stop,
}

/// A request's calls on one controller, each at its time after `started`.
struct Playback {
    controller: SimpleHapticsController,
    started: Instant,
    calls: Vec<(Duration, Call)>,
    next: usize,
}

impl Playback {
    /// When the next call is due, if one is left.
    fn due(&self) -> Option<Instant> {
        let (at, _) = self.calls.get(self.next)?;
        Some(self.started.checked_add(*at).unwrap_or(self.started))
    }

    /// Make every call due by `now`.
    fn run_due(&mut self, now: Instant) {
        while let Some(due) = self.due()
            && due <= now
        {
            let (_, call) = &self.calls[self.next];
            self.next += 1;
            let result = match call {
                Call::Play {
                    feedback,
                    intensity,
                    length: Some(length),
                } => self.controller.SendHapticFeedbackForDuration(
                    feedback,
                    intensity.map_or(1.0, f64::from),
                    time_span(*length),
                ),
                Call::Play {
                    feedback,
                    intensity: Some(intensity),
                    length: None,
                } => self
                    .controller
                    .SendHapticFeedbackWithIntensity(feedback, f64::from(*intensity)),
                Call::Play {
                    feedback,
                    intensity: None,
                    length: None,
                } => self.controller.SendHapticFeedback(feedback),
                Call::Stop => self.controller.StopFeedback(),
            };
            if let Err(error) = result {
                tracing::debug!(
                    target: "flui_platform::windows",
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
            let _ = self.controller.StopFeedback();
        }
    }
}

/// `duration` in `TimeSpan`'s 100 ns ticks, saturating.
fn time_span(duration: Duration) -> TimeSpan {
    TimeSpan {
        Duration: i64::try_from(duration.as_nanos() / 100).unwrap_or(i64::MAX),
    }
}

/// The worker: plays the newest playback, each call at its time, until its
/// window's haptics are dropped.
fn play(playbacks: &Receiver<Playback>) {
    // SAFETY: no pointer arguments. This thread was spawned for this loop
    // and entered no apartment before; a success is balanced by the single
    // `CoUninitialize` below, on the same thread, after the last WinRT call.
    let entered = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let mut current: Option<Playback> = None;
    loop {
        let received = match current.as_ref().and_then(Playback::due) {
            Some(due) => playbacks.recv_timeout(due.saturating_duration_since(Instant::now())),
            None => playbacks.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(newer) => {
                if let Some(older) = current.replace(newer) {
                    older.stop();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
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
pub(super) struct WindowsHaptics {
    /// The window, as its address: read only through `with_window_context`,
    /// which refuses any thread but the window's owner.
    hwnd: isize,
    /// The worker's queue, started on the first playback.
    worker: OnceLock<Option<Sender<Playback>>>,
}

impl std::fmt::Debug for WindowsHaptics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsHaptics")
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

impl WindowsHaptics {
    pub(super) fn new(hwnd: HWND) -> Arc<Self> {
        Arc::new(Self {
            hwnd: hwnd.0 as isize,
            worker: OnceLock::new(),
        })
    }

    /// The pen `source` names while it is in range of the window, else the
    /// pen read most recently: `None` with no pen in range, off the window's
    /// owner thread, or after the window closed.
    fn pen(&self, source: Option<DeviceId>) -> Option<Pen> {
        let pointer = super::platform::with_window_context(
            HWND(self.hwnd as *mut core::ffi::c_void),
            "haptics",
            |context| {
                let registry = context.pointer_registry.try_borrow().ok()?;
                let actuator = actuator_for(source, |device| {
                    registry.pen_contact(Some(device)).is_some()
                });
                registry.pen_contact(actuator)
            },
        )
        .flatten()?;
        Pen::of_pointer(u32::try_from(pointer.0).ok()?)
    }

    /// Hand `commands` to the worker and report `outcome`, or
    /// [`HapticOutcome::Unavailable`] when they cannot be played.
    fn submit(
        &self,
        pen: &Pen,
        commands: Vec<PenCommand>,
        outcome: HapticOutcome,
    ) -> HapticOutcome {
        let Some(playback) = pen.playback(commands) else {
            return HapticOutcome::Unavailable;
        };
        let worker = self.worker.get_or_init(|| {
            let (sender, receiver) = mpsc::channel();
            std::thread::Builder::new()
                .name("flui-pen-haptics".into())
                .spawn(move || play(&receiver))
                .map_err(|error| {
                    tracing::warn!(
                        target: "flui_platform::windows",
                        %error,
                        "the pen haptics thread did not start; pen haptics are unavailable"
                    );
                })
                .ok()
                .map(|_| sender)
        });
        match worker {
            Some(sender) if sender.send(playback).is_ok() => outcome,
            _ => HapticOutcome::Unavailable,
        }
    }
}

impl PlatformHaptics for WindowsHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        let _ = self.perform_with(feedback, &HapticRequest::default());
    }

    fn perform_with(&self, feedback: HapticFeedback, request: &HapticRequest) -> HapticOutcome {
        let Some(pen) = self.pen(request.source) else {
            return HapticOutcome::Unavailable;
        };
        match windows_feedback(feedback, Some(&pen.facts)) {
            Some((waveform, outcome)) => self.submit(
                &pen,
                pen_timeline(&[pen_feedback_step(waveform)], &pen.facts),
                outcome,
            ),
            None => HapticOutcome::Unavailable,
        }
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        fallback: HapticFeedback,
        request: &HapticRequest,
    ) -> HapticOutcome {
        let Some(pen) = self.pen(request.source) else {
            return HapticOutcome::Unavailable;
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
            None => HapticOutcome::Unavailable,
        }
    }

    fn capabilities(&self, source: Option<DeviceId>) -> HapticCapabilities {
        windows_capabilities(self.pen(source).as_ref().map(|pen| &pen.facts))
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
    use std::time::Duration;

    use flui_foundation::geometry::Size;
    use flui_platform_api::EventTime;
    use flui_platform_api::haptics::{
        HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, HapticTiming,
    };
    use flui_platform_api::pointer::{PenTool, PointerKind};

    use super::super::platform::with_window_context;
    use super::super::{WindowsPlatform, WindowsWindow};
    use crate::shared::pointer_assembly::{
        NativeContact, NativeDevice, NativeReading, NativeSurface, ReadingAction, ReadingTime,
        decode,
    };
    use crate::traits::{Platform as _, WindowOptions};
    use flui_platform_api::HapticFeedback;

    fn taps() -> HapticPattern {
        HapticPattern::builder()
            .transient(Duration::ZERO, 1.0, 0.8)
            .transient(Duration::from_millis(60), 0.4, 0.2)
            .build()
            .expect("a valid pattern")
    }

    fn assert_unavailable(haptics: &dyn flui_platform_api::PlatformHaptics, when: &str) {
        for timing in [HapticTiming::Immediate, HapticTiming::FramePresented] {
            let request = HapticRequest::new(timing);
            for kind in HapticFeedback::ALL {
                assert_eq!(
                    haptics.perform_with(kind, &request),
                    HapticOutcome::Unavailable,
                    "{kind:?} {when}"
                );
            }
            assert_eq!(
                haptics.play(&taps(), HapticFeedback::Drop, &request),
                HapticOutcome::Unavailable,
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
        let platform = WindowsPlatform::new().expect("platform");
        let window = platform
            .open_window(WindowOptions {
                title: "flui haptics".into(),
                size: Size::new(160.0, 120.0),
                ..Default::default()
            })
            .expect("window");
        let haptics = window.haptics().expect("a Win32 window has an actuator");
        assert_unavailable(haptics.as_ref(), "with no pen in range");

        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("Win32 window")
            .hwnd();
        let pen = PointerKind::Pen { tool: PenTool::Tip };
        let hovering = NativeReading::new(
            NativeContact(0x4242),
            pen,
            ReadingAction::Enter,
            ReadingTime::epoch(EventTime::from_nanos(1_000_000)),
        )
        .on(NativeSurface(0))
        .at(10.0, 10.0, 1.0)
        .from_device(NativeDevice(0x51));
        with_window_context(hwnd, "test", |context| {
            decode(&mut context.pointer_registry.borrow_mut(), &hovering).expect("a valid reading");
            assert_eq!(
                context.pointer_registry.borrow().pen_contact(None),
                Some(NativeContact(0x4242))
            );
        })
        .expect("the window's context");
        assert_unavailable(
            haptics.as_ref(),
            "for a pen pointer the OS has no pen device for",
        );
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
