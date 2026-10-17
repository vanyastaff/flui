//! Backend conformance (ADR-0215 §7) over the pure decoders every backend
//! shares, and over the headless backend's delivery path.
//!
//! Each native backend copies its OS events into the shared pointer
//! assembly's `NativeReading`s on its own clock and claims its availability
//! table. [`SharedAssembly`] is that decoder with one backend's clock shape,
//! contact kinds and table, so Linux CI runs the table for Win32, AppKit,
//! UIKit, Android and the web; the OS translation in front of it runs only on
//! a native host. The headless backend runs the same decoder and delivers its
//! output through a real window's input callback, after the capability
//! snapshot that window reports.
//!
//! Each row of the table also fails against a deliberately broken backend
//! ([`Fault`]), so a row that cannot fail is caught.

use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::Point;
use flui_platform::WindowOptions;
use flui_platform::shared::haptic_degradation::{
    AndroidVibrator, CoreHapticsEngine, PenController, PenWaveform, WebVibration, android_feedback,
    android_pattern, ios_feedback, ios_pattern, macos_feedback, macos_pattern, web_feedback,
    web_pattern, windows_capabilities, windows_feedback, windows_pattern,
};
use flui_platform::shared::input_availability::{self, WinitSession};
use flui_platform::shared::pointer_assembly::{
    ClockChoice, ContactRegistry, CounterAnchor, DecodeError, GestureComponent, GestureDelta,
    GesturePhase, GestureReading, NativeCancel, NativeClock, NativeContact, NativeDevice,
    NativeReading, NativeSurface, NativeTime, QpcTicks, ReadingAction, ReadingTime, ScrollPhases,
    ScrollReading, decode,
};
use flui_platform::traits::{DispatchEventResult, PlatformInput};
use flui_platform::{HeadlessPlatform, HostWindow, MockWindow, OwnerPlatform, Platform as _};
use flui_platform_api::EventTime;
use flui_platform_api::drag::{DragError, OsDragRequest, OsDragStart, PlatformDragSource};
use flui_platform_api::haptics::{
    HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, HapticTicket, PatternPlayback,
};
use flui_platform_api::input_capabilities::{Availability, InputCapabilities};
use flui_platform_api::pointer::{
    CancelReason, PenTool, PointerButton, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerRole, ScrollDelta, ScrollEvent, ScrollPrecision, ScrollUnit,
};
use flui_platform_api::{DeviceId, HapticFeedback, PlatformHaptics};
use flui_testing::backend_conformance::HostInput;
use flui_testing::backend_conformance::{
    BackendUnderTest, ClockProfile, ContactAction, ContactStep, PanStep, PinchChange, PinchStep,
    RowOutcome, Stamp, Step, StepPhase, assert_backend_conforms, run_backend_conformance,
};
use parking_lot::Mutex;

const PEN: PointerKind = PointerKind::Pen { tool: PenTool::Tip };
const GESTURE: NativeContact = NativeContact(u64::MAX - 1);
/// Where every backend's event epoch starts, so no stamp saturates at zero.
const EPOCH: EventTime = EventTime::from_nanos(1_000_000_000);
/// Win32's `QueryPerformanceFrequency` on current hardware.
const QPC_HZ: u64 = 10_000_000;

/// The clock a backend's readings carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClockShape {
    /// Win32: the 32-bit millisecond tick with the 64-bit uptime read on
    /// arrival, and the performance counter on pointer messages.
    TickAndCounter,
    /// AppKit and UIKit: seconds of uptime as `f64`.
    UptimeSeconds,
    /// Android, the web and headless: already on the event epoch.
    Epoch,
}

/// What one backend's OS layer feeds the shared decoder.
#[derive(Debug, Clone, Copy)]
struct Profile {
    clock: ClockShape,
    kinds: &'static [PointerKind],
    names_devices: bool,
    capabilities: InputCapabilities,
    /// The degradation table its haptic actuator plays by.
    haptics: HapticsTable,
    /// When its windows start an OS drag session, for a backend that does.
    drag: Option<OsDragStart>,
}

const ALL_KINDS: &[PointerKind] = &[PointerKind::Touch, PointerKind::Mouse, PEN];

fn win32() -> Profile {
    Profile {
        clock: ClockShape::TickAndCounter,
        kinds: ALL_KINDS,
        names_devices: true,
        capabilities: input_availability::windows(true),
        haptics: HapticsTable::Win32 { pen: false },
        drag: None,
    }
}

/// Win32 with a haptic pen in range of the window.
fn win32_pen() -> Profile {
    Profile {
        haptics: HapticsTable::Win32 { pen: true },
        ..win32()
    }
}

fn appkit() -> Profile {
    Profile {
        clock: ClockShape::UptimeSeconds,
        kinds: &[PointerKind::Mouse],
        names_devices: false,
        capabilities: input_availability::macos(),
        haptics: HapticsTable::AppKit,
        drag: None,
    }
}

fn uikit() -> Profile {
    Profile {
        clock: ClockShape::UptimeSeconds,
        kinds: &[PointerKind::Touch, PEN],
        names_devices: false,
        capabilities: input_availability::ios(),
        haptics: HapticsTable::UiKit,
        drag: None,
    }
}

fn android() -> Profile {
    Profile {
        clock: ClockShape::Epoch,
        kinds: ALL_KINDS,
        names_devices: true,
        capabilities: input_availability::android(34),
        haptics: HapticsTable::Android,
        drag: None,
    }
}

fn web() -> Profile {
    Profile {
        clock: ClockShape::Epoch,
        kinds: ALL_KINDS,
        names_devices: false,
        capabilities: input_availability::web(),
        haptics: HapticsTable::Web,
        drag: None,
    }
}

/// An X11 window: winit reports no outgoing drag there.
fn x11() -> Profile {
    Profile {
        clock: ClockShape::Epoch,
        kinds: &[PointerKind::Mouse],
        names_devices: false,
        capabilities: input_availability::winit(WinitSession::X11),
        haptics: HapticsTable::Web,
        drag: None,
    }
}

/// `profile` once its backend starts OS drag sessions at `start`.
fn with_os_drag(profile: Profile, start: OsDragStart) -> Profile {
    let c = profile.capabilities;
    Profile {
        drag: Some(start),
        capabilities: InputCapabilities::builder()
            .pen(c.pen())
            .precise_scroll(c.precise_scroll())
            .trackpad_gestures(c.trackpad_gestures())
            .input_regions(c.input_regions())
            .outgoing_drag(Availability::Native)
            .build(),
        ..profile
    }
}

/// A broken backend: each breaks the rule one row pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// Releases decode to nothing.
    DropRelease,
    /// Every contact is labelled alike.
    OneLabel,
    /// Device labels are dropped.
    NoDevice,
    /// The clock runs at twice the OS's rate.
    SkewedClock,
    /// An `Up` keeps its reading's raw time, before the contact's last.
    Unfloored,
    /// The tick is read without the uptime that places it across the wrap.
    TickOnly,
    /// The performance counter is dropped.
    CoarseOnly,
    /// Each reading is stamped on its own best clock before decoding, so a
    /// sequence or device switches clocks.
    StampEachReading,
    /// Scrolls carry no phase.
    UnphasedPan,
    /// Pinch readings after the first carry no focal point.
    FrozenFocal,
    /// An undecodable position is read as the origin.
    LenientDecode,
    /// A platform cancel is rewritten to `Claimed` after decoding: a reading
    /// cannot carry it (`NativeCancel` has no such reason).
    ClaimedCancel,
    /// Claims pen input it does not deliver as unavailable.
    UnderclaimedPen,
    /// Claims precise scrolling it has no path for.
    OverclaimedScroll,
    /// Claims no trackpad gestures, yet delivers them.
    UnderclaimedGestures,
    /// The actuator queues requests itself.
    QueuingActuator,
    /// Semantic feedback reports a fallback.
    FeedbackFallsBack,
    /// A source the actuator does not know is refused.
    StaleSourceRefused,
    /// The actuator claims exact patterns it falls back from.
    OverclaimedPatterns,
    /// Claims outgoing drags with no drag source behind the claim.
    OverclaimedDrag,
    /// The drag source starts its sessions at the other moment than the backend declares.
    WrongDragTiming,
}

/// The degradation table a backend's window actuator plays by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HapticsTable {
    /// The window's actuator is the pen in range of it (ADR-0210 §5), for
    /// a request with no source or a stale one: with `pen`, one that lists
    /// every waveform the table uses and takes an intensity; without one,
    /// nothing plays (`InputHapticsManager` is unpublished).
    Win32 {
        pen: bool,
    },
    AppKit,
    /// iOS 17.5+, with a running Core Haptics engine.
    UiKit,
    /// API 34 with every primitive and amplitude control.
    Android,
    /// Chromium with user activation.
    Web,
}

/// A window actuator that plays by its backend's degradation table.
struct TableHaptics {
    table: HapticsTable,
    fault: Option<Fault>,
}

impl TableHaptics {
    fn vibrator() -> AndroidVibrator {
        AndroidVibrator::new(34, false, vec![1, 2, 7, 8], true)
    }

    fn pen(pen: bool) -> Option<PenController> {
        pen.then(|| {
            PenController::new(
                vec![
                    PenWaveform::Click,
                    PenWaveform::Press,
                    PenWaveform::Release,
                    PenWaveform::BuzzContinuous,
                    PenWaveform::Success,
                    PenWaveform::Error,
                ],
                true,
            )
        })
    }
}

impl PlatformHaptics for TableHaptics {
    fn perform(&self, _feedback: HapticFeedback) {}

    fn perform_with(&self, kind: HapticFeedback, request: &HapticRequest) -> HapticOutcome {
        match self.fault {
            Some(Fault::QueuingActuator) => {
                return HapticOutcome::Queued(HapticTicket::new(NonZeroU64::MIN));
            }
            Some(Fault::FeedbackFallsBack) => return HapticOutcome::FellBack,
            Some(Fault::StaleSourceRefused) if request.source.is_some() => {
                return HapticOutcome::Unavailable;
            }
            _ => {}
        }
        match self.table {
            HapticsTable::Win32 { pen } => windows_feedback(kind, Self::pen(pen).as_ref())
                .map_or(HapticOutcome::Unavailable, |(_, outcome)| outcome),
            HapticsTable::AppKit => macos_feedback(kind).1,
            HapticsTable::UiKit => ios_feedback(kind, true).1,
            HapticsTable::Android => android_feedback(kind, 34).1,
            HapticsTable::Web => web_feedback(kind, WebVibration::Available)
                .map_or(HapticOutcome::Unavailable, |(_, outcome)| outcome),
        }
    }

    fn play(
        &self,
        pattern: &HapticPattern,
        _fallback: HapticFeedback,
        _request: &HapticRequest,
    ) -> HapticOutcome {
        match self.table {
            HapticsTable::Win32 { pen } => windows_pattern(pattern, Self::pen(pen).as_ref())
                .map_or(HapticOutcome::Unavailable, |(_, outcome)| outcome),
            HapticsTable::AppKit => macos_pattern(),
            HapticsTable::UiKit => ios_pattern(CoreHapticsEngine::Running),
            HapticsTable::Android => android_pattern(pattern, &Self::vibrator()).1,
            HapticsTable::Web => web_pattern(pattern, WebVibration::Available)
                .map_or(HapticOutcome::Unavailable, |(_, outcome)| outcome),
        }
    }

    fn capabilities(&self, _source: Option<DeviceId>) -> HapticCapabilities {
        if self.fault == Some(Fault::OverclaimedPatterns) {
            return HapticCapabilities::FULL;
        }
        match self.table {
            HapticsTable::Win32 { pen } => windows_capabilities(Self::pen(pen).as_ref()),
            HapticsTable::AppKit => HapticCapabilities::FEEDBACK_ONLY,
            HapticsTable::UiKit => HapticCapabilities::FULL,
            HapticsTable::Android => {
                HapticCapabilities::new(true, PatternPlayback::Approximated, true, false)
            }
            HapticsTable::Web => {
                HapticCapabilities::new(true, PatternPlayback::Approximated, false, false)
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// One reading of the shared decoder: a pointer reading, or a scroll, which
/// each backend assembles itself around the shared phase grammar.
#[derive(Debug, Clone, Copy)]
enum Reading {
    Pointer(NativeReading),
    Scroll {
        reading: ScrollReading,
        time: ReadingTime,
        delta: (f64, f64),
    },
}

/// The pure decoders a backend shares, on its profile.
struct SharedAssembly {
    profile: Profile,
    fault: Option<Fault>,
    contacts: ContactRegistry,
    scrolling: ScrollPhases,
}

impl SharedAssembly {
    fn new(profile: Profile) -> Self {
        Self {
            profile,
            fault: None,
            contacts: ContactRegistry::new(native_clock(profile.clock)),
            scrolling: ScrollPhases::default(),
        }
    }

    fn broken(profile: Profile, fault: Fault) -> Self {
        Self {
            fault: Some(fault),
            ..Self::new(profile)
        }
    }

    fn faulty(&self, fault: Fault) -> bool {
        self.fault == Some(fault)
    }

    fn time(&self, stamp: Stamp) -> ReadingTime {
        let uptime = if self.faulty(Fault::SkewedClock) {
            stamp.uptime * 2
        } else {
            stamp.uptime
        };
        let nanos = u64::try_from(uptime.as_nanos()).expect("uptime fits");
        let time = match self.profile.clock {
            ClockShape::TickAndCounter => {
                let observed_ms = nanos / 1_000_000;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "the tick is the low 32 bits of the uptime"
                )]
                let tick = observed_ms as u32;
                let observed_ms = if self.faulty(Fault::TickOnly) {
                    u64::from(tick)
                } else {
                    observed_ms
                };
                let with_counter = stamp.counter && !self.faulty(Fault::CoarseOnly);
                ReadingTime {
                    coarse: NativeTime::TickMillis { tick, observed_ms },
                    counter: with_counter.then_some(QpcTicks(nanos / (1_000_000_000 / QPC_HZ))),
                }
            }
            ClockShape::UptimeSeconds => {
                ReadingTime::coarse(NativeTime::UptimeSeconds(uptime.as_secs_f64()))
            }
            ClockShape::Epoch => {
                ReadingTime::epoch(EventTime::from_nanos(EPOCH.as_nanos() + nanos))
            }
        };
        if self.faulty(Fault::StampEachReading) {
            let clock = self.contacts.clock();
            let choice = if clock.supports(time, ClockChoice::Counter) {
                ClockChoice::Counter
            } else {
                ClockChoice::Coarse
            };
            return ReadingTime::epoch(clock.stamp(time, choice).expect("finite"));
        }
        time
    }

    fn contact_reading(&self, step: ContactStep, finite: bool) -> Option<NativeReading> {
        // A backend whose OS names no device has no reading from a named one.
        if !self.profile.kinds.contains(&step.kind)
            || (step.device.is_some() && !self.profile.names_devices)
        {
            return None;
        }
        let action = match step.action {
            ContactAction::Press => ReadingAction::Press(PointerButton::PRIMARY),
            ContactAction::Move => ReadingAction::Move,
            ContactAction::Release => ReadingAction::Release(PointerButton::PRIMARY),
            ContactAction::Cancel => ReadingAction::Cancel(NativeCancel::Platform),
            _ => return None,
        };
        let label = if self.faulty(Fault::OneLabel) {
            0
        } else {
            u64::from(step.contact)
        };
        let (x, y) = if finite {
            (step.x, step.y)
        } else if self.faulty(Fault::LenientDecode) {
            (0.0, 0.0)
        } else {
            (f64::NAN, step.y)
        };
        let mut reading =
            NativeReading::new(NativeContact(label), step.kind, action, self.time(step.at))
                .at(x, y, 1.0);
        if let Some(device) = step.device
            && self.profile.names_devices
            && !self.faulty(Fault::NoDevice)
        {
            reading = reading.from_device(NativeDevice(u64::from(device)));
        }
        Some(reading)
    }

    fn capabilities(&self) -> InputCapabilities {
        let mut builder = InputCapabilities::builder()
            .pen(self.profile.capabilities.pen())
            .precise_scroll(self.profile.capabilities.precise_scroll())
            .trackpad_gestures(self.profile.capabilities.trackpad_gestures())
            .input_regions(self.profile.capabilities.input_regions())
            .outgoing_drag(self.profile.capabilities.outgoing_drag());
        if self.faulty(Fault::UnderclaimedPen) {
            builder = builder.pen(Availability::Unavailable);
        }
        if self.faulty(Fault::OverclaimedScroll) {
            builder = builder.precise_scroll(Availability::Native);
        }
        if self.faulty(Fault::UnderclaimedGestures) {
            builder = builder.trackpad_gestures(Availability::Unavailable);
        }
        if self.faulty(Fault::OverclaimedDrag) {
            builder = builder.outgoing_drag(Availability::Native);
        }
        builder.build()
    }

    fn step_reading(&mut self, step: Step) -> Option<Reading> {
        let claimed = self.profile.capabilities;
        match step {
            Step::Contact(step) => self.contact_reading(step, true).map(Reading::Pointer),
            Step::Undecodable(step) => self.contact_reading(step, false).map(Reading::Pointer),
            Step::Pan(step) => {
                (claimed.precise_scroll() != Availability::Unavailable).then(|| self.pan(step))
            }
            Step::Pinch(step) => (claimed.trackpad_gestures() != Availability::Unavailable)
                .then(|| Reading::Pointer(self.pinch(step))),
            _ => None,
        }
    }

    fn pan(&self, step: PanStep) -> Reading {
        let phase = gesture_phase(step.phase);
        let reading = if step.momentum {
            ScrollReading::momentum(phase)
        } else {
            ScrollReading::fingers(phase, true)
        };
        Reading::Scroll {
            reading,
            time: self.time(step.at),
            delta: (step.dx, step.dy),
        }
    }

    fn pinch(&self, step: PinchStep) -> NativeReading {
        let (component, delta) = match step.change {
            PinchChange::Rotation(radians) => {
                (GestureComponent::Rotation, GestureDelta::Rotation(radians))
            }
            PinchChange::Scale(factor) => (GestureComponent::Scale, GestureDelta::Scale(factor)),
            _ => (GestureComponent::Scale, GestureDelta::None),
        };
        let phase = gesture_phase(step.phase);
        let reading = NativeReading::new(
            GESTURE,
            PointerKind::Trackpad,
            ReadingAction::Gesture(GestureReading {
                component,
                phase,
                delta,
            }),
            self.time(step.at),
        )
        .with_primary(true);
        if self.faulty(Fault::FrozenFocal) && phase != GesturePhase::Began {
            reading
        } else {
            reading.at(step.x, step.y, 1.0)
        }
    }

    fn decode_reading(&mut self, reading: Reading) -> Vec<PlatformInput> {
        match reading {
            Reading::Pointer(reading) => {
                if self.faulty(Fault::DropRelease)
                    && matches!(reading.action, ReadingAction::Release(_))
                {
                    return Vec::new();
                }
                let mut inputs = decode(&mut self.contacts, &reading)
                    .unwrap_or_else(DecodeError::into_inputs)
                    .into_vec();
                if self.faulty(Fault::ClaimedCancel) {
                    for input in &mut inputs {
                        if let PlatformInput::Pointer(PointerEvent::Cancel(cancel)) = input {
                            cancel.reason = CancelReason::Claimed;
                        }
                    }
                }
                if self.faulty(Fault::Unfloored) {
                    let raw = self
                        .contacts
                        .clock()
                        .stamp(reading.time, ClockChoice::Coarse)
                        .expect("finite");
                    for input in &mut inputs {
                        if let PlatformInput::Pointer(PointerEvent::Up(up)) = input {
                            up.sample.time = raw;
                        }
                    }
                }
                inputs
            }
            Reading::Scroll {
                reading,
                time,
                delta,
            } => {
                let time = self.contacts.stamp(time).expect("finite");
                let pointer =
                    PointerInfo::new(PointerId::new(NonZeroU64::MIN), PointerKind::Trackpad)
                        .with_role(PointerRole::Primary);
                let position = PointerPosition::try_new(Point::new(10.0, 20.0)).expect("finite");
                let delta =
                    ScrollDelta::try_new(ScrollUnit::Pixels, delta.0, delta.1).expect("finite");
                let mut scroll = ScrollEvent::new(pointer, time, position, delta)
                    .with_precision(ScrollPrecision::Precise);
                let phase = self
                    .scrolling
                    .phase(NativeSurface::default(), None, reading)
                    .filter(|_| !self.faulty(Fault::UnphasedPan));
                if let Some(phase) = phase {
                    scroll = scroll.with_phase(phase);
                }
                vec![PlatformInput::Pointer(PointerEvent::Scroll(scroll))]
            }
        }
    }

    fn restart(&mut self) {
        self.contacts = ContactRegistry::new(native_clock(self.profile.clock));
        self.scrolling = ScrollPhases::default();
    }
}

fn native_clock(shape: ClockShape) -> NativeClock {
    match shape {
        ClockShape::TickAndCounter => NativeClock::new(
            EPOCH,
            Duration::ZERO,
            Some(CounterAnchor {
                count: QpcTicks(0),
                frequency: NonZeroU64::new(QPC_HZ).expect("nonzero"),
            }),
        ),
        ClockShape::UptimeSeconds => NativeClock::new(EPOCH, Duration::ZERO, None),
        ClockShape::Epoch => NativeClock::epoch_only(),
    }
}

const fn gesture_phase(phase: StepPhase) -> GesturePhase {
    match phase {
        StepPhase::Began => GesturePhase::Began,
        StepPhase::Changed => GesturePhase::Changed,
        StepPhase::Ended => GesturePhase::Ended,
        _ => GesturePhase::Cancelled,
    }
}

impl BackendUnderTest for SharedAssembly {
    type Reading = Reading;

    fn capabilities(&self) -> InputCapabilities {
        SharedAssembly::capabilities(self)
    }

    fn clock(&self) -> ClockProfile {
        match self.profile.clock {
            ClockShape::TickAndCounter => ClockProfile::coarse(Duration::from_millis(1))
                .with_counter(Duration::from_nanos(1_000_000_000 / QPC_HZ))
                .wrapping_after(Duration::from_millis(1 << 32)),
            ClockShape::UptimeSeconds => ClockProfile::coarse(Duration::from_micros(1)),
            ClockShape::Epoch => ClockProfile::coarse(Duration::from_nanos(1)),
        }
    }

    fn restart(&mut self) {
        SharedAssembly::restart(self);
    }

    fn reading(&mut self, step: Step) -> Option<Reading> {
        self.step_reading(step)
    }

    fn decode(&mut self, reading: Reading) -> impl Iterator<Item = HostInput> + use<'_> {
        self.decode_reading(reading)
            .into_iter()
            .map(HostInput::Platform)
    }

    fn haptics(&mut self) -> Option<Arc<dyn PlatformHaptics>> {
        Some(Arc::new(TableHaptics {
            table: self.profile.haptics,
            fault: self.fault,
        }))
    }

    fn drag_source(&mut self) -> Option<Arc<dyn PlatformDragSource>> {
        let declared = self.profile.drag?;
        let timing = if self.faulty(Fault::WrongDragTiming) {
            match declared {
                OsDragStart::OnLeavingWindow => OsDragStart::AtStart { eager_data: false },
                OsDragStart::AtStart { .. } => OsDragStart::OnLeavingWindow,
            }
        } else {
            declared
        };
        Some(Arc::new(ProfileDragSource(timing)))
    }

    fn drag_start(&self) -> OsDragStart {
        self.profile.drag.unwrap_or(OsDragStart::OnLeavingWindow)
    }
}

/// A drag source that only declares when it starts: the decoders have no OS behind them.
struct ProfileDragSource(OsDragStart);

impl PlatformDragSource for ProfileDragSource {
    fn start_timing(&self) -> OsDragStart {
        self.0
    }

    fn start(&self, _request: OsDragRequest) -> Result<(), DragError> {
        Err(DragError::Unavailable)
    }
}

/// The headless backend: the shared decoder's output delivered through a
/// headless window's input callback, which first reports the window's
/// capability snapshot.
struct Headless {
    assembly: SharedAssembly,
    _owner: OwnerPlatform,
    window: Arc<dyn HostWindow>,
    delivered: Arc<Mutex<Vec<PlatformInput>>>,
    capabilities: InputCapabilities,
}

impl Headless {
    fn new() -> Self {
        let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
        let installed = std::rc::Rc::clone(&saved);
        Box::new(HeadlessPlatform::new().with_os_drag(input_availability::DESKTOP_DRAG_START))
            .run(Box::new(move |owner| {
                *installed.borrow_mut() = Some(owner);
                Ok(())
            }))
            .expect("headless owner");
        let owner: OwnerPlatform = saved.borrow_mut().take().expect("started owner");
        let window = owner
            .open_window(WindowOptions::default())
            .expect("open")
            .try_ready()
            .expect("ready");
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&delivered);
        window.on_input(Box::new(move |input| {
            sink.lock().push(input);
            DispatchEventResult::default()
        }));
        let capabilities = delivered
            .lock()
            .drain(..)
            .find_map(|input| match input {
                PlatformInput::Capabilities(capabilities) => Some(capabilities),
                _ => None,
            })
            .expect("the snapshot is delivered on registration");
        let profile = Profile {
            clock: ClockShape::Epoch,
            kinds: ALL_KINDS,
            names_devices: true,
            capabilities,
            // Unused: the headless rows play on the window's own actuator.
            haptics: HapticsTable::Win32 { pen: false },
            drag: None,
        };
        Self {
            assembly: SharedAssembly::new(profile),
            _owner: owner,
            window,
            delivered,
            capabilities,
        }
    }

    fn mock(&self) -> &MockWindow {
        self.window
            .as_any()
            .downcast_ref::<MockWindow>()
            .expect("headless window")
    }
}

impl BackendUnderTest for Headless {
    type Reading = Reading;

    fn capabilities(&self) -> InputCapabilities {
        self.capabilities
    }

    fn clock(&self) -> ClockProfile {
        ClockProfile::coarse(Duration::from_nanos(1))
    }

    fn restart(&mut self) {
        self.assembly.restart();
        self.delivered.lock().clear();
    }

    fn reading(&mut self, step: Step) -> Option<Reading> {
        self.assembly.step_reading(step)
    }

    fn decode(&mut self, reading: Reading) -> impl Iterator<Item = HostInput> + use<'_> {
        for input in self.assembly.decode_reading(reading) {
            self.mock().inject_event(input);
        }
        std::mem::take(&mut *self.delivered.lock())
            .into_iter()
            .map(HostInput::Platform)
    }

    fn haptics(&mut self) -> Option<Arc<dyn PlatformHaptics>> {
        self.window.haptics()
    }

    fn drag_source(&mut self) -> Option<Arc<dyn PlatformDragSource>> {
        self.window.drag_source()
    }

    fn drag_start(&self) -> OsDragStart {
        input_availability::DESKTOP_DRAG_START
    }
}

fn headless_conforms() {
    assert_backend_conforms(&mut Headless::new());
}

fn win32_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(win32()));
}

fn win32_pen_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(win32_pen()));
}

fn appkit_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(appkit()));
}

fn uikit_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(uikit()));
}

fn android_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(android()));
}

fn web_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(web()));
}

fn x11_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(x11()));
}

fn win32_os_drag_conforms() {
    assert_backend_conforms(&mut SharedAssembly::new(win32_os_drag()));
}

fn uikit_os_drag_conforms() {
    assert_backend_conforms(&mut SharedAssembly::new(uikit_os_drag()));
}

fn web_os_drag_conforms() {
    assert_backend_conforms(&mut SharedAssembly::new(web_os_drag()));
}

/// The backends once their native drag sessions land: Win32 starts when the pointer leaves
/// the window, iOS at lift and the web at `dragstart`.
fn win32_os_drag() -> Profile {
    with_os_drag(win32(), input_availability::DESKTOP_DRAG_START)
}

fn uikit_os_drag() -> Profile {
    with_os_drag(uikit(), input_availability::IOS_DRAG_START)
}

fn web_os_drag() -> Profile {
    with_os_drag(web(), input_availability::WEB_DRAG_START)
}

#[test]
fn every_backend_passes_the_conformance_table() {
    super::run_table(
        "backend conformance",
        &[
            ("headless", headless_conforms),
            ("win32", win32_decoders_conform),
            ("win32_with_a_haptic_pen", win32_pen_decoders_conform),
            ("appkit", appkit_decoders_conform),
            ("uikit", uikit_decoders_conform),
            ("android", android_decoders_conform),
            ("web", web_decoders_conform),
            ("x11", x11_decoders_conform),
            ("win32 with OS drag", win32_os_drag_conforms),
            ("uikit with OS drag", uikit_os_drag_conforms),
            ("web with OS drag", web_os_drag_conforms),
        ],
    );
}

/// A row, the fault that breaks it and the backend profile it applies to.
type BrokenRow = (&'static str, Fault, fn() -> Profile);

const BROKEN: &[BrokenRow] = &[
    (
        "a_contact_is_one_pointer_from_down_to_up",
        Fault::DropRelease,
        win32,
    ),
    (
        "a_reused_touch_label_is_a_new_pointer",
        Fault::DropRelease,
        uikit,
    ),
    ("a_second_touch_is_additional", Fault::OneLabel, android),
    (
        "a_named_device_is_announced_before_its_first_contact",
        Fault::NoDevice,
        android,
    ),
    (
        "event_times_follow_the_reading_clock",
        Fault::SkewedClock,
        appkit,
    ),
    ("event_times_never_go_back", Fault::Unfloored, web),
    ("the_coarse_clock_wraps_forward", Fault::TickOnly, win32),
    (
        "precise_readings_are_stamped_from_the_counter",
        Fault::CoarseOnly,
        win32,
    ),
    (
        "a_sequence_keeps_the_clock_it_started_on",
        Fault::StampEachReading,
        win32,
    ),
    (
        "a_device_whose_reading_lacked_the_counter_stays_coarse",
        Fault::StampEachReading,
        win32,
    ),
    (
        "two_finger_pan_is_phased_scroll_on_every_backend",
        Fault::UnphasedPan,
        appkit,
    ),
    (
        "pan_zoom_carries_only_scale_and_rotation",
        Fault::FrozenFocal,
        appkit,
    ),
    (
        "an_undecodable_reading_cancels_its_contact_as_invalid_input",
        Fault::LenientDecode,
        uikit,
    ),
    ("pen_is_delivered_as_claimed", Fault::UnderclaimedPen, win32),
    (
        "precise_scroll_is_delivered_as_claimed",
        Fault::OverclaimedScroll,
        web,
    ),
    (
        "trackpad_gestures_are_delivered_as_claimed",
        Fault::UnderclaimedGestures,
        appkit,
    ),
    (
        "no_backend_cancels_a_contact_as_claimed",
        Fault::ClaimedCancel,
        android,
    ),
    ("a_backend_never_queues", Fault::QueuingActuator, uikit),
    (
        "semantic_feedback_reports_a_tier_one_outcome",
        Fault::FeedbackFallsBack,
        appkit,
    ),
    (
        "a_stale_source_plays_on_the_window_actuator",
        Fault::StaleSourceRefused,
        android,
    ),
    (
        "semantic_feedback_reports_a_tier_one_outcome",
        Fault::FeedbackFallsBack,
        win32_pen,
    ),
    (
        "a_stale_source_plays_on_the_window_actuator",
        Fault::StaleSourceRefused,
        win32_pen,
    ),
    (
        "capabilities_match_what_plays",
        Fault::OverclaimedPatterns,
        win32_pen,
    ),
    (
        "capabilities_match_what_plays",
        Fault::OverclaimedPatterns,
        web,
    ),
    (
        "outgoing_drag_is_delivered_as_claimed",
        Fault::OverclaimedDrag,
        x11,
    ),
    (
        "a_drag_source_starts_when_the_backend_declares",
        Fault::WrongDragTiming,
        win32_os_drag,
    ),
    (
        "an_at_start_backend_never_escalates_mid_drag",
        Fault::WrongDragTiming,
        uikit_os_drag,
    ),
    (
        "start_decision_includes_sibling_presentation_with_identity_only_offer",
        Fault::WrongDragTiming,
        web_os_drag,
    ),
    (
        "a_leaving_backend_escalates_when_the_pointer_leaves_the_window",
        Fault::WrongDragTiming,
        win32_os_drag,
    ),
];

/// Every row of the table fails against the backend its fault breaks, and
/// holds for the same profile unbroken; no row is left without a fault.
#[test]
fn each_conformance_row_fails_against_a_broken_backend() {
    let mut failures = Vec::new();
    for &(row, fault, profile) in BROKEN {
        let healthy = run_backend_conformance(&mut SharedAssembly::new(profile()));
        if healthy.outcome(row) != Some(&RowOutcome::Held) {
            failures.push(format!(
                "{row}: does not hold unbroken: {:?}",
                healthy.outcome(row)
            ));
        }
        let broken = run_backend_conformance(&mut SharedAssembly::broken(profile(), fault));
        if !matches!(broken.outcome(row), Some(RowOutcome::Failed(_))) {
            failures.push(format!(
                "{row}: holds with {fault:?}: {:?}",
                broken.outcome(row)
            ));
        }
    }
    let report = run_backend_conformance(&mut SharedAssembly::new(win32()));
    for (row, _, _) in &report.rows {
        if !BROKEN.iter().any(|(broken, _, _)| broken == row) {
            failures.push(format!("{row}: no broken backend shows it can fail"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
