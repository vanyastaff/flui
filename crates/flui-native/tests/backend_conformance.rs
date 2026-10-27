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
use flui_native::haptics::degradation::{
    AndroidHapticsApi, AndroidVibrator, CoreHapticsEngine, PenController, PenWaveform,
    WebVibration, android_feedback, android_pattern, ios_feedback, ios_pattern, macos_feedback,
    macos_pattern, web_feedback, web_pattern, windows_capabilities, windows_feedback,
    windows_pattern,
};
use flui_native::input::android::keys as android_keys;
use flui_native::input::apple::keys::{keycode_to_key, kvk};
use flui_native::input::assembly::{
    ClockChoice, ContactRegistry, CounterAnchor, DecodeError, GestureComponent, GestureDelta,
    GesturePhase, GestureReading, NativeCancel, NativeClock, NativeContact, NativeDevice,
    NativeReading, NativeSurface, NativeTime, QpcTicks, ReadingAction, ReadingTime, ScrollPhases,
    ScrollReading, decode,
};
use flui_native::input::availability::{
    self as input_availability, AndroidInputApis, WinitSession,
};
use flui_native::input::keys::vk_to_key;
use flui_native::input::web::keys::key_composition;
use flui_native::{HeadlessPlatform, HostWindow, MockWindow, OwnerPlatform, Platform as _};
use flui_platform::Availability;
use flui_platform::EventTime;
use flui_platform::WindowOptions;
use flui_platform::drag::{DragError, OsDragRequest, OsDragStart, PlatformDragSource};
use flui_platform::haptics::{
    HapticCapabilities, HapticOutcome, HapticPattern, HapticRequest, HapticTicket, PatternPlayback,
};
use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_platform::pointer::{
    CancelReason, PenTool, PointerButton, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerRole, ScrollDelta, ScrollEvent, ScrollPrecision, ScrollUnit,
};
use flui_platform::{DeviceId, HapticFeedback, PlatformHaptics, Unsupported, UnsupportedReason};

/// What a table answers for a request its actuator cannot play: this device
/// has none for it.
const NO_ACTUATOR: HapticOutcome =
    HapticOutcome::Unsupported(Unsupported::new(UnsupportedReason::NotOnThisDevice));
use flui_platform::PlatformInput;
use flui_testing::backend_conformance::HostInput;
use flui_testing::backend_conformance::{
    BackendUnderTest, ClockProfile, ContactAction, ContactStep, KeyStep, PanStep, PinchChange,
    PinchStep, RowOutcome, Stamp, Step, StepKey, StepPhase, assert_backend_conforms,
    run_backend_conformance,
};

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
    /// The shared key decoder its OS layer feeds, for a backend that has one.
    keys: Option<KeyDecoder>,
}

/// A shared key decoder: Win32's virtual-key table, AppKit's key-code
/// table, or the web's rule for a key of the input method (its key names
/// come from the DOM, not a table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyDecoder {
    Win32,
    AppKit,
    Web,
    Android,
}

const ALL_KINDS: &[PointerKind] = &[PointerKind::Touch, PointerKind::Mouse, PEN];

fn win32() -> Profile {
    Profile {
        clock: ClockShape::TickAndCounter,
        kinds: ALL_KINDS,
        names_devices: true,
        capabilities: input_availability::windows(true),
        haptics: HapticsTable::Win32 { pen: false },
        drag: Some(input_availability::DESKTOP_DRAG_START),
        keys: Some(KeyDecoder::Win32),
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
        keys: Some(KeyDecoder::AppKit),
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
        keys: None,
    }
}

fn android() -> Profile {
    Profile {
        clock: ClockShape::Epoch,
        kinds: ALL_KINDS,
        names_devices: true,
        capabilities: input_availability::android(AndroidInputApis {
            gesture_exclusion: true,
            touchpad_gestures: true,
            drag_source: true,
        }),
        haptics: HapticsTable::Android,
        drag: Some(input_availability::DESKTOP_DRAG_START),
        keys: Some(KeyDecoder::Android),
    }
}

/// Android below API 24, where `FluiView` has no `startDragAndDrop`: every
/// drag stays in its window.
fn android_api_23() -> Profile {
    Profile {
        capabilities: input_availability::android(AndroidInputApis::default()),
        drag: None,
        ..android()
    }
}

/// Android whose `FluiView` could not be installed: no outgoing drag.
fn android_without_view() -> Profile {
    Profile {
        capabilities: input_availability::android(AndroidInputApis {
            gesture_exclusion: true,
            touchpad_gestures: true,
            drag_source: false,
        }),
        drag: None,
        ..android()
    }
}

fn web() -> Profile {
    Profile {
        clock: ClockShape::Epoch,
        kinds: ALL_KINDS,
        names_devices: false,
        capabilities: input_availability::web(false),
        haptics: HapticsTable::Web,
        drag: Some(input_availability::WEB_DRAG_START),
        keys: Some(KeyDecoder::Web),
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
        keys: None,
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
    /// The menu key decodes to no key at all.
    MenuKeyUnidentified,
    /// A key during a composition is read only from what its event says.
    ComposingKeyFromEventOnly,
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
        AndroidVibrator::new(
            AndroidHapticsApi::SegmentFeedback,
            false,
            vec![1, 2, 7, 8],
            true,
        )
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
                return Unsupported::new(UnsupportedReason::Unavailable).into();
            }
            _ => {}
        }
        match self.table {
            HapticsTable::Win32 { pen } => windows_feedback(kind, Self::pen(pen).as_ref())
                .map_or(NO_ACTUATOR, |(_, outcome)| outcome),
            HapticsTable::AppKit => macos_feedback(kind).1,
            HapticsTable::UiKit => ios_feedback(kind, true).1,
            HapticsTable::Android => android_feedback(kind, AndroidHapticsApi::SegmentFeedback).1,
            HapticsTable::Web => web_feedback(kind, WebVibration::Available)
                .map_or(NO_ACTUATOR, |(_, outcome)| outcome),
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
                .map_or(NO_ACTUATOR, |(_, outcome)| outcome),
            HapticsTable::AppKit => macos_pattern(),
            HapticsTable::UiKit => ios_pattern(CoreHapticsEngine::Running),
            HapticsTable::Android => android_pattern(pattern, &Self::vibrator()).1,
            HapticsTable::Web => web_pattern(pattern, WebVibration::Available)
                .map_or(NO_ACTUATOR, |(_, outcome)| outcome),
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
    /// A key decoded by the backend's shared table, with the Shift the OS
    /// reported, pressed then released.
    Key {
        key: NamedKey,
        shift: bool,
    },
    /// A key the window's input method composes through, as WebKit reports
    /// it after `compositionend`: the event says nothing composes and
    /// carries the key's own code.
    ComposingKey {
        key: NamedKey,
    },
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
            Step::Key(step) => self.key_reading(step),
            Step::ComposingKey(step) => (matches!(
                self.profile.keys,
                Some(KeyDecoder::Web | KeyDecoder::Android)
            ) && step.key == StepKey::Escape)
                .then_some(Reading::ComposingKey {
                    key: NamedKey::Escape,
                }),
            _ => None,
        }
    }

    /// The key the backend's table decodes `step` to: Win32's virtual-key
    /// codes (`VK_APPS` 0x5D, `VK_F10` 0x79), AppKit's key codes.
    fn key_reading(&self, step: KeyStep) -> Option<Reading> {
        let decoder = self.profile.keys?;
        let key = match (decoder, step.key) {
            (KeyDecoder::Win32, StepKey::Menu) => vk_to_key(0x5D, step.shift),
            (KeyDecoder::Win32, StepKey::F10) => vk_to_key(0x79, step.shift),
            (KeyDecoder::AppKit, StepKey::Menu) => keycode_to_key(kvk::PC_MENU)?,
            (KeyDecoder::AppKit, StepKey::F10) => keycode_to_key(kvk::F10)?,
            _ => return None,
        };
        let key = match key {
            Key::Named(named)
                if !(self.faulty(Fault::MenuKeyUnidentified) && step.key == StepKey::Menu) =>
            {
                named
            }
            _ => NamedKey::Unidentified,
        };
        Some(Reading::Key {
            key,
            shift: step.shift,
        })
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
            Reading::Key { key, shift } => {
                let modifiers = if shift {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                [KeyState::Down, KeyState::Up]
                    .into_iter()
                    .map(|state| {
                        PlatformInput::Keyboard(
                            KeyEvent::new(state, Key::Named(key), Code::Unidentified, EPOCH)
                                .with_modifiers(modifiers),
                        )
                    })
                    .collect()
            }
            Reading::ComposingKey { key } => {
                const ESCAPE_KEY_CODE: u32 = 27;
                // The host composes; the key event itself says nothing does
                // (WebKit after `compositionend`, and every Android key).
                let host_composing = !self.faulty(Fault::ComposingKeyFromEventOnly);
                let composing = if self.profile.keys == Some(KeyDecoder::Android) {
                    android_keys::key_composition(host_composing)
                } else {
                    key_composition(false, ESCAPE_KEY_CODE, host_composing)
                };
                [KeyState::Down, KeyState::Up]
                    .into_iter()
                    .map(|state| {
                        PlatformInput::Keyboard(
                            KeyEvent::new(state, Key::Named(key), Code::Unidentified, EPOCH)
                                .with_composition(composing),
                        )
                    })
                    .collect()
            }
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
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into())
    }
}

/// The headless backend: the shared decoder's output delivered through a
/// headless window's handler, whose first input is the window's
/// capability snapshot.
struct Headless {
    assembly: SharedAssembly,
    _owner: OwnerPlatform,
    window: Arc<dyn HostWindow>,
    handler: std::rc::Rc<flui_testing::window_handler::RecordingWindowHandler>,
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
        let handler = flui_testing::window_handler::RecordingWindowHandler::new();
        let window = owner
            .open_window(WindowOptions::default(), handler.clone())
            .expect("open")
            .try_ready()
            .expect("ready");
        let capabilities = handler
            .inputs()
            .into_iter()
            .find_map(|input| match input {
                PlatformInput::Capabilities(capabilities) => Some(capabilities),
                _ => None,
            })
            .expect("the snapshot is the handler's first input");
        handler.take_calls();
        let profile = Profile {
            clock: ClockShape::Epoch,
            kinds: ALL_KINDS,
            names_devices: true,
            capabilities,
            // Unused: the headless rows play on the window's own actuator.
            haptics: HapticsTable::Win32 { pen: false },
            drag: None,
            keys: None,
        };
        Self {
            assembly: SharedAssembly::new(profile),
            _owner: owner,
            window,
            handler,
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
        self.handler.take_calls();
    }

    fn reading(&mut self, step: Step) -> Option<Reading> {
        self.assembly.step_reading(step)
    }

    fn decode(&mut self, reading: Reading) -> impl Iterator<Item = HostInput> + use<'_> {
        for input in self.assembly.decode_reading(reading) {
            self.mock().inject_event(input);
        }
        self.handler
            .take_calls()
            .into_iter()
            .filter_map(|call| match call {
                flui_testing::window_handler::HandlerCall::Input(input) => {
                    Some(HostInput::Platform(input))
                }
                _ => None,
            })
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

/// The native Win32 backend on this host: a real window's capability
/// snapshot, its OLE drag source and its haptic actuator, with the shared
/// decoder standing in for the `WM_POINTER` translation in front of it.
#[cfg(target_os = "windows")]
struct Win32Window {
    assembly: SharedAssembly,
    // Field order: the window closes before its platform goes.
    window: Arc<dyn HostWindow>,
    _platform: flui_native::WindowsPlatform,
    capabilities: InputCapabilities,
}

#[cfg(target_os = "windows")]
impl Win32Window {
    fn new() -> Self {
        let platform = flui_native::WindowsPlatform::new().expect("native Windows platform");
        let handler = flui_testing::window_handler::RecordingWindowHandler::new();
        let window = platform
            .open_window(WindowOptions::default(), handler.clone())
            .expect("a Win32 window");
        let capabilities = handler
            .inputs()
            .into_iter()
            .find_map(|input| match input {
                PlatformInput::Capabilities(capabilities) => Some(capabilities),
                _ => None,
            })
            .expect("the snapshot is the handler's first input");
        Self {
            assembly: SharedAssembly::new(Profile {
                capabilities,
                ..win32()
            }),
            window,
            _platform: platform,
            capabilities,
        }
    }
}

#[cfg(target_os = "windows")]
impl BackendUnderTest for Win32Window {
    type Reading = Reading;

    fn capabilities(&self) -> InputCapabilities {
        self.capabilities
    }

    fn clock(&self) -> ClockProfile {
        BackendUnderTest::clock(&self.assembly)
    }

    fn restart(&mut self) {
        self.assembly.restart();
    }

    fn reading(&mut self, step: Step) -> Option<Reading> {
        self.assembly.step_reading(step)
    }

    fn decode(&mut self, reading: Reading) -> impl Iterator<Item = HostInput> + use<'_> {
        self.assembly
            .decode_reading(reading)
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

#[cfg(target_os = "windows")]
fn win32_window_conforms() {
    assert_backend_conforms(&mut Win32Window::new());
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

fn android_api_23_conforms() {
    assert_backend_conforms(&mut SharedAssembly::new(android_api_23()));
}

fn android_without_view_conforms() {
    assert_backend_conforms(&mut SharedAssembly::new(android_without_view()));
}

fn web_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(web()));
}

fn x11_decoders_conform() {
    assert_backend_conforms(&mut SharedAssembly::new(x11()));
}

fn uikit_os_drag_conforms() {
    assert_backend_conforms(&mut SharedAssembly::new(uikit_os_drag()));
}

/// UIKit once its native drag session lands, at lift. The web starts its HTML
/// drag at `dragstart`; Win32 its OLE session when the pointer leaves the window.
fn uikit_os_drag() -> Profile {
    with_os_drag(uikit(), input_availability::IOS_DRAG_START)
}

#[test]
fn every_backend_passes_the_conformance_table() {
    super::run_table(
        "backend conformance",
        &[
            ("headless", headless_conforms),
            ("win32", win32_decoders_conform),
            #[cfg(target_os = "windows")]
            ("win32 window", win32_window_conforms),
            ("win32_with_a_haptic_pen", win32_pen_decoders_conform),
            ("appkit", appkit_decoders_conform),
            ("uikit", uikit_decoders_conform),
            ("android", android_decoders_conform),
            ("android below API 24", android_api_23_conforms),
            ("android without FluiView", android_without_view_conforms),
            ("web", web_decoders_conform),
            ("x11", x11_decoders_conform),
            ("uikit with OS drag", uikit_os_drag_conforms),
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
        win32,
    ),
    (
        "an_at_start_backend_never_escalates_mid_drag",
        Fault::WrongDragTiming,
        uikit_os_drag,
    ),
    (
        "start_decision_includes_sibling_presentation_with_identity_only_offer",
        Fault::WrongDragTiming,
        web,
    ),
    (
        "a_leaving_backend_escalates_when_the_pointer_leaves_the_window",
        Fault::WrongDragTiming,
        win32,
    ),
    (
        "menu_keys_arrive_as_the_bindings_name_them",
        Fault::MenuKeyUnidentified,
        win32,
    ),
    (
        "a_key_during_composition_is_the_input_methods",
        Fault::ComposingKeyFromEventOnly,
        web,
    ),
    (
        "a_key_during_composition_is_the_input_methods",
        Fault::ComposingKeyFromEventOnly,
        android,
    ),
    (
        "outgoing_drag_is_delivered_as_claimed",
        Fault::OverclaimedDrag,
        android_api_23,
    ),
    (
        "a_leaving_backend_escalates_when_the_pointer_leaves_the_window",
        Fault::WrongDragTiming,
        android,
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

#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod cross_backend_physical_key_agreement {
    //! Cross-checks a curated set of physical keys that the winit, Win32,
    //! and AppKit conversions all claim to name explicitly (see
    //! `flui_native::input::keys::scancode_to_code`'s and
    //! `flui_native::input::apple::keys::keycode_to_code`'s own module docs), so a
    //! future edit to any one of the three tables that silently retargets a
    //! shared physical key is caught here instead of only shipping as a
    //! per-backend behavior change. Win32/AppKit scancodes and keycodes are
    //! transcribed directly from those modules' own match arms — read there
    //! before adding a row, since only the values THOSE tables actually use
    //! are meaningful cross-checks.
    //!
    //! Coverage is representative, not exhaustive: only keys where a
    //! meaning-preserving correspondence between all three native id spaces
    //! (a winit `KeyCode` variant, a Win32 scancode, an AppKit keycode) is
    //! unambiguous. Two families are deliberately excluded with a reason
    //! rather than silently omitted:
    //! - Win32's `KanaMode` (scancode `0x70`) has no AppKit counterpart —
    //!   the AppKit table maps its Kana key straight to `Lang1` (matching
    //!   Win32's *separate* `Lang1` scancode `0x72`), per the W3C code
    //!   registry's own platform-conditional reuse of the `Lang1`/`Lang2`
    //!   slots (Japanese Mac Eisu/Kana vs. Korean Hangul/Hanja) — not a
    //!   defect, so `KanaMode` stays a Win32-only row below.
    //! - Browser/media/launch keys (`BrowserBack`, `MediaPlayPause`, …) have
    //!   no AppKit table entries at all; those rows carry a Win32
    //!   scancode only.

    use ui_events_winit::keyboard::from_winit_code;
    use winit::keyboard::{KeyCode, PhysicalKey};

    use flui_native::input::apple::keys::keycode_to_code;
    use flui_native::input::keys::scancode_to_code;

    /// `(label, winit KeyCode, Win32 (scancode, extended), AppKit keycode)`.
    /// `None` means that backend has no table entry for this key.
    type PhysicalKeyRow = (&'static str, KeyCode, Option<(u16, bool)>, Option<u16>);

    const PHYSICAL_KEYS: &[PhysicalKeyRow] = &[
        // Letters/digits/punctuation (a sample, not all 26+10).
        ("KeyA", KeyCode::KeyA, Some((0x001E, false)), Some(0x00)),
        ("Digit1", KeyCode::Digit1, Some((0x0002, false)), Some(0x12)),
        ("Minus", KeyCode::Minus, Some((0x000C, false)), Some(0x1B)),
        ("Equal", KeyCode::Equal, Some((0x000D, false)), Some(0x18)),
        (
            "BracketLeft",
            KeyCode::BracketLeft,
            Some((0x001A, false)),
            Some(0x21),
        ),
        (
            "BracketRight",
            KeyCode::BracketRight,
            Some((0x001B, false)),
            Some(0x1E),
        ),
        (
            "Semicolon",
            KeyCode::Semicolon,
            Some((0x0027, false)),
            Some(0x29),
        ),
        ("Quote", KeyCode::Quote, Some((0x0028, false)), Some(0x27)),
        (
            "Backquote",
            KeyCode::Backquote,
            Some((0x0029, false)),
            Some(0x32),
        ),
        (
            "Backslash",
            KeyCode::Backslash,
            Some((0x002B, false)),
            Some(0x2A),
        ),
        ("Comma", KeyCode::Comma, Some((0x0033, false)), Some(0x2B)),
        ("Period", KeyCode::Period, Some((0x0034, false)), Some(0x2F)),
        ("Slash", KeyCode::Slash, Some((0x0035, false)), Some(0x2C)),
        // Editing / whitespace.
        ("Enter", KeyCode::Enter, Some((0x001C, false)), Some(0x24)),
        ("Escape", KeyCode::Escape, Some((0x0001, false)), Some(0x35)),
        ("Tab", KeyCode::Tab, Some((0x000F, false)), Some(0x30)),
        ("Space", KeyCode::Space, Some((0x0039, false)), Some(0x31)),
        (
            "Backspace",
            KeyCode::Backspace,
            Some((0x000E, false)),
            Some(0x33),
        ),
        (
            "CapsLock",
            KeyCode::CapsLock,
            Some((0x003A, false)),
            Some(0x39),
        ),
        // Sided modifiers.
        (
            "ShiftLeft",
            KeyCode::ShiftLeft,
            Some((0x002A, false)),
            Some(0x38),
        ),
        (
            "ShiftRight",
            KeyCode::ShiftRight,
            Some((0x0036, false)),
            Some(0x3C),
        ),
        (
            "ControlLeft",
            KeyCode::ControlLeft,
            Some((0x001D, false)),
            Some(0x3B),
        ),
        (
            "ControlRight",
            KeyCode::ControlRight,
            Some((0x001D, true)),
            Some(0x3E),
        ),
        (
            "AltLeft",
            KeyCode::AltLeft,
            Some((0x0038, false)),
            Some(0x3A),
        ),
        (
            "AltRight",
            KeyCode::AltRight,
            Some((0x0038, true)),
            Some(0x3D),
        ),
        (
            "MetaLeft",
            KeyCode::SuperLeft,
            Some((0x005B, true)),
            Some(0x37),
        ),
        (
            "MetaRight",
            KeyCode::SuperRight,
            Some((0x005C, true)),
            Some(0x36),
        ),
        // Navigation cluster.
        (
            "ArrowLeft",
            KeyCode::ArrowLeft,
            Some((0x004B, true)),
            Some(0x7B),
        ),
        (
            "ArrowRight",
            KeyCode::ArrowRight,
            Some((0x004D, true)),
            Some(0x7C),
        ),
        (
            "ArrowUp",
            KeyCode::ArrowUp,
            Some((0x0048, true)),
            Some(0x7E),
        ),
        (
            "ArrowDown",
            KeyCode::ArrowDown,
            Some((0x0050, true)),
            Some(0x7D),
        ),
        ("Home", KeyCode::Home, Some((0x0047, true)), Some(0x73)),
        ("End", KeyCode::End, Some((0x004F, true)), Some(0x77)),
        ("PageUp", KeyCode::PageUp, Some((0x0049, true)), Some(0x74)),
        (
            "PageDown",
            KeyCode::PageDown,
            Some((0x0051, true)),
            Some(0x79),
        ),
        // Apple's Insert/Delete labeling is a documented cross-platform
        // divergence in position, not in the `Code` value each produces.
        ("Insert", KeyCode::Insert, Some((0x0052, true)), Some(0x72)),
        ("Delete", KeyCode::Delete, Some((0x0053, true)), Some(0x75)),
        // NumLock: Win32's own key vs. the Mac numpad Clear key.
        (
            "NumLock",
            KeyCode::NumLock,
            Some((0x0045, true)),
            Some(0x47),
        ),
        // Numpad cluster.
        (
            "Numpad0",
            KeyCode::Numpad0,
            Some((0x0052, false)),
            Some(0x52),
        ),
        (
            "Numpad1",
            KeyCode::Numpad1,
            Some((0x004F, false)),
            Some(0x53),
        ),
        (
            "Numpad2",
            KeyCode::Numpad2,
            Some((0x0050, false)),
            Some(0x54),
        ),
        (
            "Numpad3",
            KeyCode::Numpad3,
            Some((0x0051, false)),
            Some(0x55),
        ),
        (
            "Numpad4",
            KeyCode::Numpad4,
            Some((0x004B, false)),
            Some(0x56),
        ),
        (
            "Numpad5",
            KeyCode::Numpad5,
            Some((0x004C, false)),
            Some(0x57),
        ),
        (
            "Numpad6",
            KeyCode::Numpad6,
            Some((0x004D, false)),
            Some(0x58),
        ),
        (
            "Numpad7",
            KeyCode::Numpad7,
            Some((0x0047, false)),
            Some(0x59),
        ),
        (
            "Numpad8",
            KeyCode::Numpad8,
            Some((0x0048, false)),
            Some(0x5B),
        ),
        (
            "Numpad9",
            KeyCode::Numpad9,
            Some((0x0049, false)),
            Some(0x5C),
        ),
        (
            "NumpadAdd",
            KeyCode::NumpadAdd,
            Some((0x004E, false)),
            Some(0x45),
        ),
        (
            "NumpadSubtract",
            KeyCode::NumpadSubtract,
            Some((0x004A, false)),
            Some(0x4E),
        ),
        (
            "NumpadMultiply",
            KeyCode::NumpadMultiply,
            Some((0x0037, false)),
            Some(0x43),
        ),
        (
            "NumpadDivide",
            KeyCode::NumpadDivide,
            Some((0x0035, true)),
            Some(0x4B),
        ),
        (
            "NumpadDecimal",
            KeyCode::NumpadDecimal,
            Some((0x0053, false)),
            Some(0x41),
        ),
        (
            "NumpadEnter",
            KeyCode::NumpadEnter,
            Some((0x001C, true)),
            Some(0x4C),
        ),
        (
            "NumpadEqual",
            KeyCode::NumpadEqual,
            Some((0x0059, false)),
            Some(0x51),
        ),
        (
            "NumpadComma",
            KeyCode::NumpadComma,
            Some((0x007E, false)),
            Some(0x5F),
        ),
        // System / IME keys.
        (
            "ContextMenu",
            KeyCode::ContextMenu,
            Some((0x005D, true)),
            Some(0x6E),
        ),
        ("Lang1", KeyCode::Lang1, Some((0x0072, false)), Some(0x68)),
        ("Lang2", KeyCode::Lang2, Some((0x0071, false)), Some(0x66)),
        (
            "IntlBackslash",
            KeyCode::IntlBackslash,
            Some((0x0056, false)),
            Some(0x0A),
        ),
        ("IntlRo", KeyCode::IntlRo, Some((0x0073, false)), Some(0x5E)),
        (
            "IntlYen",
            KeyCode::IntlYen,
            Some((0x007D, false)),
            Some(0x5D),
        ),
        // Media keys.
        (
            "AudioVolumeUp",
            KeyCode::AudioVolumeUp,
            Some((0x0030, true)),
            Some(0x48),
        ),
        (
            "AudioVolumeDown",
            KeyCode::AudioVolumeDown,
            Some((0x002E, true)),
            Some(0x49),
        ),
        (
            "AudioVolumeMute",
            KeyCode::AudioVolumeMute,
            Some((0x0020, true)),
            Some(0x4A),
        ),
        // Function keys, including the F13-F20 band all three tables cover.
        ("F1", KeyCode::F1, Some((0x003B, false)), Some(0x7A)),
        ("F12", KeyCode::F12, Some((0x0058, false)), Some(0x6F)),
        ("F13", KeyCode::F13, Some((0x0064, false)), Some(0x69)),
        ("F14", KeyCode::F14, Some((0x0065, false)), Some(0x6B)),
        ("F15", KeyCode::F15, Some((0x0066, false)), Some(0x71)),
        ("F16", KeyCode::F16, Some((0x0067, false)), Some(0x6A)),
        ("F17", KeyCode::F17, Some((0x0068, false)), Some(0x40)),
        ("F18", KeyCode::F18, Some((0x0069, false)), Some(0x4F)),
        ("F19", KeyCode::F19, Some((0x006A, false)), Some(0x50)),
        ("F20", KeyCode::F20, Some((0x006B, false)), Some(0x5A)),
        // Win32-only families: no AppKit table entry exists for these.
        (
            "ScrollLock",
            KeyCode::ScrollLock,
            Some((0x0046, false)),
            None,
        ),
        ("Pause", KeyCode::Pause, Some((0x0045, false)), None),
        (
            "PrintScreen",
            KeyCode::PrintScreen,
            Some((0x0037, true)),
            None,
        ),
        ("Power", KeyCode::Power, Some((0x005E, true)), None),
        ("KanaMode", KeyCode::KanaMode, Some((0x0070, false)), None),
        ("Convert", KeyCode::Convert, Some((0x0079, false)), None),
        (
            "NonConvert",
            KeyCode::NonConvert,
            Some((0x007B, false)),
            None,
        ),
        ("F21", KeyCode::F21, Some((0x006C, false)), None),
        ("F24", KeyCode::F24, Some((0x0076, false)), None),
        (
            "BrowserBack",
            KeyCode::BrowserBack,
            Some((0x006A, true)),
            None,
        ),
        (
            "BrowserForward",
            KeyCode::BrowserForward,
            Some((0x0069, true)),
            None,
        ),
        (
            "BrowserRefresh",
            KeyCode::BrowserRefresh,
            Some((0x0067, true)),
            None,
        ),
        (
            "BrowserSearch",
            KeyCode::BrowserSearch,
            Some((0x0065, true)),
            None,
        ),
        (
            "BrowserFavorites",
            KeyCode::BrowserFavorites,
            Some((0x0066, true)),
            None,
        ),
        (
            "BrowserStop",
            KeyCode::BrowserStop,
            Some((0x0068, true)),
            None,
        ),
        (
            "BrowserHome",
            KeyCode::BrowserHome,
            Some((0x0032, true)),
            None,
        ),
        (
            "MediaPlayPause",
            KeyCode::MediaPlayPause,
            Some((0x0022, true)),
            None,
        ),
        ("MediaStop", KeyCode::MediaStop, Some((0x0024, true)), None),
        (
            "MediaTrackNext",
            KeyCode::MediaTrackNext,
            Some((0x0019, true)),
            None,
        ),
        (
            "MediaTrackPrevious",
            KeyCode::MediaTrackPrevious,
            Some((0x0010, true)),
            None,
        ),
        (
            "MediaSelect",
            KeyCode::MediaSelect,
            Some((0x006D, true)),
            None,
        ),
        (
            "LaunchApp1",
            KeyCode::LaunchApp1,
            Some((0x006B, true)),
            None,
        ),
        (
            "LaunchApp2",
            KeyCode::LaunchApp2,
            Some((0x0021, true)),
            None,
        ),
        (
            "LaunchMail",
            KeyCode::LaunchMail,
            Some((0x006C, true)),
            None,
        ),
    ];

    /// The winit, Win32 and AppKit tables name every shared physical key alike.
    #[test]
    fn physical_keys_agree_across_backends() {
        let mut disagreements = Vec::new();
        for &(label, winit_code, win32, appkit) in PHYSICAL_KEYS {
            let winit_result = from_winit_code(PhysicalKey::Code(winit_code));
            if let Some((scancode, extended)) = win32 {
                let win32_result = scancode_to_code(scancode, extended);
                if win32_result.as_str() != winit_result.to_string() {
                    disagreements.push(format!(
                        "{label}: winit={winit_result:?} win32={win32_result:?}"
                    ));
                }
            }
            if let Some(keycode) = appkit {
                let appkit_result = keycode_to_code(keycode);
                if appkit_result.as_str() != winit_result.to_string() {
                    disagreements.push(format!(
                        "{label}: winit={winit_result:?} appkit={appkit_result:?}"
                    ));
                }
            }
        }
        assert!(
            disagreements.is_empty(),
            "{} cross-backend disagreement(s):\n{}",
            disagreements.len(),
            disagreements.join("\n")
        );
    }
}
