//! What the seats' objects report, as plain wire events, and the one
//! assembly that turns them into owned input (see the parent module for what
//! it decides).
//!
//! The live dispatch copies each protocol event into a [`SeatWire`] — no
//! `wayland-client` type — and [`SeatAssembly::assemble`] turns it into the
//! input it delivers, each event with the window surface it is for, against
//! one [`ContactRegistry`]. A native recording of a session is the journal of
//! that registry.

#![forbid(unsafe_code)]

use flui_platform::input_capabilities::PointerKindSet;
use flui_platform::keyboard::Modifiers;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerKind, ScrollDelta, ScrollPrecision,
    ScrollUnit,
};
use flui_platform::{EventTime, PlatformInput};
use smallvec::SmallVec;

use crate::input::assembly::{
    Assembled, ContactRegistry, ContactScroll, DecodeError, GestureComponent, GestureDelta,
    GesturePhase, GestureReading, NativeCancel, NativeClock, NativeContact, NativeDevice,
    NativeReading, NativeSurface, NativeTime, PenAngles, RawSensors, ReadingAction, ReadingTime,
    ScrollPhases, ScrollReading, decode, framed,
};
use crate::input::scroll::{WheelAmount, wheel_step};

/// The cursor of a pointer device.
pub const MOUSE: NativeContact = NativeContact(u64::MAX);
/// The pinch of a pointer device's gesture object.
pub const PINCH: NativeContact = NativeContact(u64::MAX - 1);
/// A tablet tool, the one contact of its device.
pub const TOOL: NativeContact = NativeContact(0);

/// How far a compositor's millisecond stamp may lie from the monotonic clock
/// read on arrival and still be read as a stamp on that clock.
pub const TRUSTED_SKEW_MS: i64 = 1_000;

/// When a wire event happened, as the dispatch saw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WireTime {
    /// The compositor's millisecond stamp, when the event carries one.
    pub stamp: Option<u32>,
    /// `CLOCK_MONOTONIC` in milliseconds, read on arrival.
    pub monotonic_ms: u64,
    /// The event timeline, read on arrival.
    pub arrival: EventTime,
}

impl WireTime {
    /// When an event that carries the compositor's `stamp` arrived.
    #[must_use]
    pub const fn stamped(stamp: u32, monotonic_ms: u64, arrival: EventTime) -> Self {
        Self {
            stamp: Some(stamp),
            monotonic_ms,
            arrival,
        }
    }

    /// When an event that carries no stamp arrived.
    #[must_use]
    pub const fn arrived(monotonic_ms: u64, arrival: EventTime) -> Self {
        Self {
            stamp: None,
            monotonic_ms,
            arrival,
        }
    }

    /// The reading time this is: the compositor's stamp on the monotonic
    /// clock when it lies within [`TRUSTED_SKEW_MS`] of the arrival, wrap
    /// included; the arrival otherwise.
    #[must_use]
    pub fn reading_time(self) -> ReadingTime {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the stamp is the low 32 bits of the millisecond clock"
        )]
        let offset = self
            .stamp
            .map(|stamp| i64::from(stamp.wrapping_sub(self.monotonic_ms as u32).cast_signed()));
        match (self.stamp, offset) {
            (Some(tick), Some(offset)) if offset.abs() <= TRUSTED_SKEW_MS => {
                ReadingTime::coarse(NativeTime::TickMillis {
                    tick,
                    observed_ms: self.monotonic_ms,
                })
            }
            _ => ReadingTime::epoch(self.arrival),
        }
    }
}

/// A scroll axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// `vertical_scroll`.
    Vertical,
    /// `horizontal_scroll`.
    Horizontal,
}

impl Axis {
    const fn index(self) -> usize {
        match self {
            Self::Vertical => 1,
            Self::Horizontal => 0,
        }
    }
}

/// What turned the axes of a pointer frame (`wl_pointer.axis_source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisSource {
    /// A wheel's notches, or its high-resolution steps.
    Wheel,
    /// Fingers on a touchpad: scrolls in phases, with a stop at the lift.
    Finger,
    /// A continuous device, such as a trackball's scroll mode.
    Continuous,
    /// A wheel tilted sideways.
    WheelTilt,
}

/// A `wl_pointer` event. Positions are surface-local logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerWire {
    /// The pointer entered `surface`.
    Enter {
        /// The window surface.
        surface: NativeSurface,
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// The pointer left its surface.
    Leave,
    /// It moved.
    Motion {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// A button changed: the kernel's button `code`.
    Button {
        /// `BTN_LEFT` and its kin.
        code: u32,
        /// Down rather than up.
        pressed: bool,
    },
    /// An axis turned by `value` (positive scrolls toward later content).
    Axis {
        /// Which.
        axis: Axis,
        /// By how much, in surface coordinates.
        value: f64,
    },
    /// What turned the frame's axes.
    AxisSource(AxisSource),
    /// The fingers on an axis lifted.
    AxisStop(Axis),
    /// A wheel's steps, 120 to a notch.
    AxisValue120 {
        /// Which.
        axis: Axis,
        /// How far, 120 to a notch.
        value120: i32,
    },
    /// A wheel's whole notches, from a compositor without `value120`.
    AxisDiscrete {
        /// Which.
        axis: Axis,
        /// How many notches.
        discrete: i32,
    },
    /// The events since the last frame belong together.
    Frame,
}

/// A `wl_touch` event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchWire {
    /// Touch `id` went down on `surface`.
    Down {
        /// The window surface.
        surface: NativeSurface,
        /// The touch.
        id: i32,
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// Touch `id` lifted.
    Up {
        /// The touch.
        id: i32,
    },
    /// Touch `id` moved.
    Motion {
        /// The touch.
        id: i32,
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// Touch `id`'s contact ellipse.
    Shape {
        /// The touch.
        id: i32,
        /// The major axis, in surface coordinates.
        major: f64,
        /// The minor axis, in surface coordinates.
        minor: f64,
    },
    /// The touches since the last frame changed together.
    Frame,
    /// The compositor took every touch of the device.
    Cancel,
}

/// A tablet tool's event (`zwp_tablet_tool_v2`); a frame closes each set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolWire {
    /// The tool came into range over `surface`.
    ProximityIn {
        /// The window surface.
        surface: NativeSurface,
    },
    /// It left range.
    ProximityOut,
    /// Its tip touched.
    Down,
    /// Its tip lifted.
    Up,
    /// It moved.
    Motion {
        /// Where.
        x: f64,
        /// Where.
        y: f64,
    },
    /// Its pressure, `0` to `65535`.
    Pressure(u32),
    /// Its tilt in degrees.
    Tilt {
        /// Toward positive x.
        x: f64,
        /// Toward positive y.
        y: f64,
    },
    /// Its rotation in degrees, clockwise.
    Rotation(f64),
    /// An airbrush's finger slider, `-65535` to `65535`.
    Slider(i32),
    /// A button on the tool changed: the kernel's button `code`.
    Button {
        /// `BTN_STYLUS` and its kin.
        code: u32,
        /// Down rather than up.
        pressed: bool,
    },
    /// The events since the last frame happened together, at the frame's
    /// time.
    Frame,
}

/// A pinch's event (`zwp_pointer_gesture_pinch_v1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PinchWire {
    /// A pinch began over `surface`.
    Begin {
        /// The window surface.
        surface: NativeSurface,
    },
    /// It changed.
    Update {
        /// The fingers' centre moved by this much.
        dx: f64,
        /// The fingers' centre moved by this much.
        dy: f64,
        /// The scale since the pinch began.
        scale: f64,
        /// Degrees, clockwise, since the last update.
        rotation: f64,
    },
    /// It ended, or the compositor cancelled it.
    End {
        /// Cancelled rather than ended.
        cancelled: bool,
    },
}

/// A seat, by a key minted when the input bound it: a `wl_seat` global's
/// name can be announced again for another seat after its removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeatKey(pub u64);

/// What one seat object reported.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeatEvent {
    /// A device of `seat` came: a pointer, a touchscreen or a tablet tool.
    DeviceAdded {
        /// Its label.
        device: NativeDevice,
        /// What it is.
        kind: PointerKind,
        /// The seat it belongs to, whose keyboard's modifiers its readings
        /// carry and whose removal removes it.
        seat: SeatKey,
    },
    /// A device went.
    DeviceRemoved {
        /// Its label.
        device: NativeDevice,
    },
    /// A pointer's event.
    Pointer {
        /// The pointer.
        device: NativeDevice,
        /// What it said.
        event: PointerWire,
    },
    /// A touchscreen's event.
    Touch {
        /// The touchscreen.
        device: NativeDevice,
        /// What it said.
        event: TouchWire,
    },
    /// A tablet tool's event.
    Tool {
        /// The tool.
        device: NativeDevice,
        /// What kind of tool it is.
        kind: PointerKind,
        /// What it said.
        event: ToolWire,
    },
    /// A pinch's event.
    Pinch {
        /// The gesture object.
        device: NativeDevice,
        /// The pointer it belongs to, whose cursor is its focal point.
        pointer: NativeDevice,
        /// What it said.
        event: PinchWire,
    },
    /// The modifiers of `seat`'s keyboard changed.
    Modifiers {
        /// The seat.
        seat: SeatKey,
        /// What its keyboard holds.
        modifiers: Modifiers,
    },
    /// `seat`'s keyboard left its window: the modifiers it held no longer
    /// reach the window, whose next readings carry none until it returns.
    KeyboardLeft(SeatKey),
    /// The compositor removed `seat`: every device of it goes, the
    /// sequences its devices held open ending as removed.
    SeatRemoved(SeatKey),
    /// The window of `surface` closed: what it held open ends unheard.
    SurfaceGone(NativeSurface),
}

/// One wire event, with when it happened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeatWire {
    /// When.
    pub time: WireTime,
    /// What.
    pub event: SeatEvent,
}

/// Where an assembled input goes.
#[derive(Debug, Clone)]
pub enum Routed {
    /// To the window of a surface.
    To(NativeSurface, PlatformInput),
    /// To every window: a device came or went.
    Everywhere(PlatformInput),
}

/// The owned button the kernel's button `code` is (`BTN_LEFT` and its kin,
/// `linux/input-event-codes.h`). Buttons past the five named ones share the
/// vocabulary's non-actuating band, 26 codes to a turn.
#[must_use]
pub fn button_of(code: u32) -> PointerButton {
    const BTN_LEFT: u32 = 0x110;
    match code {
        BTN_LEFT => PointerButton::PRIMARY,
        // BTN_RIGHT, and BTN_STYLUS, a pen's barrel button.
        0x111 | 0x14b => PointerButton::SECONDARY,
        // BTN_MIDDLE, and BTN_STYLUS2.
        0x112 | 0x14c => PointerButton::AUXILIARY,
        // BTN_SIDE and BTN_BACK.
        0x113 | 0x116 => PointerButton::BACK,
        // BTN_EXTRA and BTN_FORWARD.
        0x114 | 0x115 => PointerButton::FORWARD,
        other => {
            let band = u8::try_from(other.wrapping_sub(BTN_LEFT) % 26)
                .expect("BUG: a remainder of 26 fits a byte");
            PointerButton::try_from(7 + band)
                .expect("BUG: a vendor button maps into the non-actuating 7..=32 band")
        }
    }
}

/// A pointer and what its current frame gathered.
#[derive(Debug, Clone)]
struct PointerState {
    device: NativeDevice,
    surface: Option<NativeSurface>,
    position: (f64, f64),
    axes: AxisFrame,
    /// A finger scroll is open.
    fingers: bool,
}

/// The axis events of one pointer frame.
#[derive(Debug, Clone, Copy, Default)]
struct AxisFrame {
    source: Option<AxisSource>,
    /// Horizontal, vertical.
    values: [f64; 2],
    value120: [Option<i32>; 2],
    discrete: [Option<i32>; 2],
    stopped: bool,
    turned: bool,
    time: Option<WireTime>,
}

/// One touch of a frame.
#[derive(Debug, Clone, Copy)]
struct TouchStep {
    id: i32,
    action: ReadingAction,
    position: Option<(f64, f64)>,
    time: WireTime,
}

/// A touchscreen: its open touches and the frame it is gathering.
#[derive(Debug, Clone, Default)]
struct TouchState {
    device: Option<NativeDevice>,
    /// The surface each open touch went down on.
    open: Vec<(i32, NativeSurface)>,
    steps: Vec<TouchStep>,
    shapes: Vec<(i32, (f64, f64))>,
}

/// What a tablet tool's current frame gathered.
#[derive(Debug, Clone, Default)]
struct ToolFrame {
    entered: Option<NativeSurface>,
    left: bool,
    down: bool,
    up: bool,
    moved: bool,
    /// Every button change of the frame, in order: none is dropped, so none
    /// is left held.
    buttons: SmallVec<[(u32, bool); 4]>,
}

/// A tablet tool: where it is, what its axes hold, and its frame.
#[derive(Debug, Clone)]
struct ToolState {
    device: NativeDevice,
    surface: Option<NativeSurface>,
    position: (f64, f64),
    sensors: RawSensors,
    frame: ToolFrame,
}

/// A pinch in progress.
#[derive(Debug, Clone, Copy)]
struct PinchState {
    device: NativeDevice,
    surface: NativeSurface,
    focal: (f64, f64),
    scale: f64,
}

/// The seats' input as one assembly: the registry every reading is decoded
/// against, the scroll phases, and what each device's frame gathered.
#[derive(Debug)]
pub struct SeatAssembly {
    contacts: ContactRegistry,
    scrolling: ScrollPhases,
    /// The seat of each device announced.
    seats: Vec<(NativeDevice, SeatKey)>,
    /// What each seat's keyboard holds, while it is in a window.
    modifiers: Vec<(SeatKey, Modifiers)>,
    pointers: Vec<PointerState>,
    touches: Vec<TouchState>,
    tools: Vec<ToolState>,
    pinches: Vec<PinchState>,
}

/// The output of one wire event.
type Out = SmallVec<[Routed; 2]>;

impl SeatAssembly {
    /// An assembly stamping readings on `clock`, whose pointers' cursors take
    /// pointer identity one.
    #[must_use]
    pub fn new(clock: NativeClock) -> Self {
        Self {
            contacts: ContactRegistry::new(clock).with_reserved_contact(MOUSE),
            scrolling: ScrollPhases::default(),
            seats: Vec::new(),
            modifiers: Vec::new(),
            pointers: Vec::new(),
            touches: Vec::new(),
            tools: Vec::new(),
            pinches: Vec::new(),
        }
    }

    /// The kinds of the devices present.
    #[must_use]
    pub fn present(&self) -> PointerKindSet {
        self.contacts.devices().present()
    }

    /// The input `wire` assembles into, each for its window or for every
    /// window.
    pub fn assemble(&mut self, wire: SeatWire) -> Vec<Routed> {
        let time = wire.time;
        let out = match wire.event {
            SeatEvent::DeviceAdded { device, kind, seat } => {
                self.seats.retain(|(held, _)| *held != device);
                self.seats.push((device, seat));
                self.device_added(device, kind, time)
            }
            SeatEvent::DeviceRemoved { device } => self.device_removed(device, time),
            SeatEvent::Pointer { device, event } => self.pointer(device, event, time),
            SeatEvent::Touch { device, event } => self.touch(device, event, time),
            SeatEvent::Tool {
                device,
                kind,
                event,
            } => self.tool(device, kind, event, time),
            SeatEvent::Pinch {
                device,
                pointer,
                event,
            } => self.pinch(device, pointer, event, time),
            SeatEvent::Modifiers { seat, modifiers } => {
                self.modifiers.retain(|(held, _)| *held != seat);
                self.modifiers.push((seat, modifiers));
                Out::new()
            }
            SeatEvent::KeyboardLeft(seat) => {
                self.modifiers.retain(|(held, _)| *held != seat);
                Out::new()
            }
            SeatEvent::SeatRemoved(seat) => {
                self.modifiers.retain(|(held, _)| *held != seat);
                // Each removal drops the device from the seats, so the next
                // device of the seat is the first left.
                let mut out = Out::new();
                while let Some(device) = self
                    .seats
                    .iter()
                    .find(|(_, held)| *held == seat)
                    .map(|(device, _)| *device)
                {
                    out.extend(self.device_removed(device, time));
                }
                out
            }
            SeatEvent::SurfaceGone(surface) => {
                self.surface_gone(surface, time);
                Out::new()
            }
        };
        out.into_vec()
    }

    /// What the keyboard of `device`'s seat holds; none for a device of no
    /// seat announced, or of one whose keyboard is in no window.
    fn modifiers_of(&self, device: NativeDevice) -> Modifiers {
        self.seats
            .iter()
            .find(|(held, _)| *held == device)
            .and_then(|(_, seat)| self.modifiers.iter().find(|(held, _)| held == seat))
            .map_or(Modifiers::NONE, |(_, modifiers)| *modifiers)
    }

    /// `time` on the event timeline, as the readings are stamped.
    #[must_use]
    pub fn stamp(&self, time: WireTime) -> EventTime {
        self.contacts
            .stamp(time.reading_time())
            .unwrap_or(time.arrival)
    }

    /// Decode `reading`, routed to its surface. A reading that fails ends its
    /// contact with the cancel the failure carries.
    fn decode(&mut self, reading: &NativeReading, out: &mut Out) {
        let surface = reading.surface;
        let inputs = decode(&mut self.contacts, reading).unwrap_or_else(|error| {
            tracing::debug!(%error, "a Wayland reading was not assembled");
            DecodeError::into_inputs(error)
        });
        out.extend(inputs.into_iter().map(|input| Routed::To(surface, input)));
    }

    fn device_added(&mut self, device: NativeDevice, kind: PointerKind, time: WireTime) -> Out {
        let time = self.stamp(time);
        self.contacts
            .add_device(device, kind, time)
            .map(|added| {
                Routed::Everywhere(PlatformInput::Pointer(PointerEvent::DeviceAdded(added)))
            })
            .into_iter()
            .collect()
    }

    fn device_removed(&mut self, device: NativeDevice, time: WireTime) -> Out {
        let time = self.stamp(time);
        self.scrolling.remove_device(device);
        self.pointers.retain(|pointer| pointer.device != device);
        self.touches.retain(|touch| touch.device != Some(device));
        self.tools.retain(|tool| tool.device != device);
        self.pinches.retain(|pinch| pinch.device != device);
        self.seats.retain(|(held, _)| *held != device);
        let (ended, removed) = self.contacts.remove_device(device, time);
        let mut out: Out = ended
            .into_iter()
            .map(|(surface, input)| Routed::To(surface, input))
            .collect();
        out.extend(removed.map(|removed| {
            Routed::Everywhere(PlatformInput::Pointer(PointerEvent::DeviceRemoved(removed)))
        }));
        out
    }

    fn surface_gone(&mut self, surface: NativeSurface, time: WireTime) {
        let time = self.stamp(time);
        // The window is gone: what its contacts end with reaches no one.
        let _ended = self
            .contacts
            .cancel_surface(surface, time, NativeCancel::FocusLost);
        self.scrolling.cancel_surface(surface);
        for pointer in &mut self.pointers {
            if pointer.surface == Some(surface) {
                pointer.surface = None;
                pointer.fingers = false;
                pointer.axes = AxisFrame::default();
            }
        }
        for touch in &mut self.touches {
            touch.open.retain(|(_, open)| *open != surface);
        }
        for tool in &mut self.tools {
            if tool.surface == Some(surface) {
                tool.surface = None;
            }
        }
        self.pinches.retain(|pinch| pinch.surface != surface);
    }

    // ---- Pointers -------------------------------------------------------

    fn pointer_state(&mut self, device: NativeDevice) -> &mut PointerState {
        entry(
            &mut self.pointers,
            |state| state.device == device,
            || PointerState {
                device,
                surface: None,
                position: (0.0, 0.0),
                axes: AxisFrame::default(),
                fingers: false,
            },
        )
    }

    fn mouse_reading(
        &self,
        device: NativeDevice,
        surface: NativeSurface,
        action: ReadingAction,
        time: WireTime,
    ) -> NativeReading {
        NativeReading::new(MOUSE, PointerKind::Mouse, action, time.reading_time())
            .on(surface)
            .from_device(device)
            .with_modifiers(self.modifiers_of(device))
    }

    fn pointer(&mut self, device: NativeDevice, event: PointerWire, time: WireTime) -> Out {
        let mut out = Out::new();
        match event {
            PointerWire::Enter { surface, x, y } => {
                let state = self.pointer_state(device);
                state.surface = Some(surface);
                state.position = (x, y);
                state.fingers = false;
                let reading = self
                    .mouse_reading(device, surface, ReadingAction::Enter, time)
                    .at(x, y, 1.0);
                self.decode(&reading, &mut out);
            }
            PointerWire::Leave => {
                let state = self.pointer_state(device);
                let Some(surface) = state.surface.take() else {
                    return out;
                };
                let fingers = std::mem::take(&mut state.fingers);
                state.axes = AxisFrame::default();
                if fingers {
                    self.finger_step(
                        device,
                        surface,
                        GesturePhase::Cancelled,
                        [0.0; 2],
                        time,
                        &mut out,
                    );
                }
                // The compositor took the pointer mid-press: no release comes.
                let held = self
                    .contacts
                    .buttons(surface, Some(device), MOUSE)
                    .is_some_and(|held| held != PointerButtons::NONE);
                if held {
                    let cancel = self.mouse_reading(
                        device,
                        surface,
                        ReadingAction::Cancel(NativeCancel::CaptureLost),
                        time,
                    );
                    self.decode(&cancel, &mut out);
                }
                let reading = self.mouse_reading(device, surface, ReadingAction::Leave, time);
                self.decode(&reading, &mut out);
            }
            PointerWire::Motion { x, y } => {
                let state = self.pointer_state(device);
                state.position = (x, y);
                let Some(surface) = state.surface else {
                    return out;
                };
                let reading = self
                    .mouse_reading(device, surface, ReadingAction::Move, time)
                    .at(x, y, 1.0);
                self.decode(&reading, &mut out);
            }
            PointerWire::Button { code, pressed } => {
                let Some(surface) = self.pointer_state(device).surface else {
                    return out;
                };
                let button = button_of(code);
                let action = if pressed {
                    ReadingAction::Press(button)
                } else {
                    ReadingAction::Release(button)
                };
                let reading = self.mouse_reading(device, surface, action, time);
                self.decode(&reading, &mut out);
            }
            PointerWire::Axis { axis, value } => {
                let axes = &mut self.pointer_state(device).axes;
                axes.values[axis.index()] += value;
                axes.turned = true;
                axes.time = Some(time);
            }
            PointerWire::AxisSource(source) => {
                self.pointer_state(device).axes.source = Some(source);
            }
            PointerWire::AxisStop(_) => {
                let axes = &mut self.pointer_state(device).axes;
                axes.stopped = true;
                axes.time.get_or_insert(time);
            }
            PointerWire::AxisValue120 { axis, value120 } => {
                let axes = &mut self.pointer_state(device).axes;
                let held = axes.value120[axis.index()].get_or_insert(0);
                *held = held.saturating_add(value120);
            }
            PointerWire::AxisDiscrete { axis, discrete } => {
                let axes = &mut self.pointer_state(device).axes;
                let held = axes.discrete[axis.index()].get_or_insert(0);
                *held = held.saturating_add(discrete);
            }
            PointerWire::Frame => self.axis_frame(device, time, &mut out),
        }
        out
    }

    /// The scroll a pointer frame's axes make.
    fn axis_frame(&mut self, device: NativeDevice, time: WireTime, out: &mut Out) {
        let state = self.pointer_state(device);
        let axes = std::mem::take(&mut state.axes);
        let Some(surface) = state.surface else {
            return;
        };
        let time = axes.time.unwrap_or(time);
        match axes.source {
            Some(AxisSource::Finger) => {
                if axes.turned {
                    let phase = if self.pointer_state(device).fingers {
                        GesturePhase::Changed
                    } else {
                        GesturePhase::Began
                    };
                    self.pointer_state(device).fingers = true;
                    self.finger_step(device, surface, phase, axes.values, time, out);
                }
                if axes.stopped && std::mem::take(&mut self.pointer_state(device).fingers) {
                    self.finger_step(device, surface, GesturePhase::Ended, [0.0; 2], time, out);
                }
            }
            Some(AxisSource::Continuous) if axes.turned => {
                let delta = step(ScrollUnit::Pixels, axes.values);
                self.scroll(
                    device,
                    surface,
                    None,
                    delta,
                    ScrollPrecision::Precise,
                    time,
                    out,
                );
            }
            Some(AxisSource::Wheel | AxisSource::WheelTilt) | None
                if axes.turned || axes.value120.iter().any(Option::is_some) =>
            {
                let (delta, precision) = wheel(&axes);
                self.scroll(device, surface, None, delta, precision, time, out);
            }
            _ => {}
        }
    }

    fn finger_step(
        &mut self,
        device: NativeDevice,
        surface: NativeSurface,
        phase: GesturePhase,
        values: [f64; 2],
        time: WireTime,
        out: &mut Out,
    ) {
        let phase =
            self.scrolling
                .phase(surface, Some(device), ScrollReading::fingers(phase, true));
        let delta = step(ScrollUnit::Pixels, values);
        self.scroll(
            device,
            surface,
            phase,
            delta,
            ScrollPrecision::Precise,
            time,
            out,
        );
    }

    #[expect(clippy::too_many_arguments, reason = "one scroll step, field by field")]
    fn scroll(
        &self,
        device: NativeDevice,
        surface: NativeSurface,
        phase: Option<flui_platform::pointer::ScrollPhase>,
        delta: ScrollDelta,
        precision: ScrollPrecision,
        time: WireTime,
        out: &mut Out,
    ) {
        let reading = ContactScroll {
            surface,
            device: Some(device),
            contact: MOUSE,
            phase,
            delta,
            precision,
            time: self.stamp(time),
            modifiers: self.modifiers_of(device),
        };
        if let Some(input) = self.contacts.scroll(&reading) {
            out.push(Routed::To(surface, input));
        }
    }

    // ---- Touch ----------------------------------------------------------

    fn touch_state(&mut self, device: NativeDevice) -> &mut TouchState {
        entry(
            &mut self.touches,
            |state| state.device == Some(device),
            || TouchState {
                device: Some(device),
                ..TouchState::default()
            },
        )
    }

    fn touch(&mut self, device: NativeDevice, event: TouchWire, time: WireTime) -> Out {
        let mut out = Out::new();
        let state = self.touch_state(device);
        match event {
            TouchWire::Down { surface, id, x, y } => {
                state.open.retain(|(open, _)| *open != id);
                state.open.push((id, surface));
                state.steps.push(TouchStep {
                    id,
                    action: ReadingAction::Press(PointerButton::PRIMARY),
                    position: Some((x, y)),
                    time,
                });
            }
            TouchWire::Up { id } => state.steps.push(TouchStep {
                id,
                action: ReadingAction::Release(PointerButton::PRIMARY),
                position: None,
                time,
            }),
            TouchWire::Motion { id, x, y } => state.steps.push(TouchStep {
                id,
                action: ReadingAction::Move,
                position: Some((x, y)),
                time,
            }),
            TouchWire::Shape { id, major, minor } => {
                state.shapes.retain(|(shaped, _)| *shaped != id);
                state.shapes.push((id, (major, minor)));
            }
            TouchWire::Frame => self.touch_frame(device, &mut out),
            TouchWire::Cancel => {
                self.touch_frame(device, &mut out);
                let open = std::mem::take(&mut self.touch_state(device).open);
                for (id, surface) in open {
                    let reading = touch_reading(
                        device,
                        surface,
                        id,
                        ReadingAction::Cancel(NativeCancel::Platform),
                        time,
                    )
                    .with_modifiers(self.modifiers_of(device));
                    self.decode(&reading, &mut out);
                }
            }
        }
        out
    }

    /// The touches a frame closed, as one pointer frame per surface.
    fn touch_frame(&mut self, device: NativeDevice, out: &mut Out) {
        let state = self.touch_state(device);
        let steps = std::mem::take(&mut state.steps);
        let shapes = std::mem::take(&mut state.shapes);
        let mut per_surface: Vec<(NativeSurface, Vec<Assembled>)> = Vec::new();
        for step in steps {
            let state = self.touch_state(device);
            let Some(surface) = state
                .open
                .iter()
                .find(|(id, _)| *id == step.id)
                .map(|(_, surface)| *surface)
            else {
                continue;
            };
            if matches!(step.action, ReadingAction::Release(_)) {
                state.open.retain(|(id, _)| *id != step.id);
            }
            let mut reading = touch_reading(device, surface, step.id, step.action, step.time)
                .with_modifiers(self.modifiers_of(device));
            if let Some((x, y)) = step.position {
                reading = reading.at(x, y, 1.0);
            }
            if let Some((_, size)) = shapes.iter().find(|(id, _)| *id == step.id) {
                reading = reading.with_sensors(RawSensors {
                    contact_size: Some(*size),
                    ..RawSensors::default()
                });
            }
            let assembled = decode(&mut self.contacts, &reading).unwrap_or_else(|error| {
                tracing::debug!(%error, "a Wayland touch was not assembled");
                DecodeError::into_inputs(error)
            });
            match per_surface.iter_mut().find(|(held, _)| *held == surface) {
                Some((_, frame)) => frame.push(assembled),
                None => per_surface.push((surface, vec![assembled])),
            }
        }
        for (surface, frame) in per_surface {
            out.extend(
                framed(frame)
                    .into_iter()
                    .map(|input| Routed::To(surface, input)),
            );
        }
    }

    // ---- Tablet tools ---------------------------------------------------

    fn tool_state(&mut self, device: NativeDevice) -> &mut ToolState {
        entry(
            &mut self.tools,
            |state| state.device == device,
            || ToolState {
                device,
                surface: None,
                position: (0.0, 0.0),
                sensors: RawSensors::default(),
                frame: ToolFrame::default(),
            },
        )
    }

    fn tool(
        &mut self,
        device: NativeDevice,
        kind: PointerKind,
        event: ToolWire,
        time: WireTime,
    ) -> Out {
        let mut out = Out::new();
        let state = self.tool_state(device);
        let frame = &mut state.frame;
        match event {
            ToolWire::ProximityIn { surface } => frame.entered = Some(surface),
            ToolWire::ProximityOut => frame.left = true,
            ToolWire::Down => frame.down = true,
            ToolWire::Up => frame.up = true,
            ToolWire::Motion { x, y } => {
                state.position = (x, y);
                frame.moved = true;
            }
            ToolWire::Pressure(pressure) => {
                state.sensors.pressure = Some(f64::from(pressure) / 65_535.0);
                frame.moved = true;
            }
            ToolWire::Tilt { x, y } => {
                state.sensors.angles = Some(PenAngles::TiltDegrees { x, y });
                frame.moved = true;
            }
            ToolWire::Rotation(degrees) => {
                state.sensors.twist_degrees = Some(degrees.rem_euclid(360.0));
                frame.moved = true;
            }
            ToolWire::Slider(position) => {
                state.sensors.tangential_pressure = Some(f64::from(position) / 65_535.0);
                frame.moved = true;
            }
            ToolWire::Button { code, pressed } => frame.buttons.push((code, pressed)),
            ToolWire::Frame => self.tool_frame(device, kind, time, &mut out),
        }
        out
    }

    /// The readings a tool's frame makes, in the order the module docs give.
    fn tool_frame(
        &mut self,
        device: NativeDevice,
        kind: PointerKind,
        time: WireTime,
        out: &mut Out,
    ) {
        let state = self.tool_state(device);
        let frame = std::mem::take(&mut state.frame);
        if let Some(surface) = frame.entered {
            state.surface = Some(surface);
        }
        let (surface, (x, y), sensors) = (state.surface, state.position, state.sensors);
        let Some(surface) = surface else {
            return;
        };
        if frame.left {
            self.tool_state(device).surface = None;
        }
        let reading = |action| {
            NativeReading::new(TOOL, kind, action, time.reading_time())
                .on(surface)
                .from_device(device)
                .with_modifiers(self.modifiers_of(device))
                .with_sensors(sensors)
        };
        let mut readings: SmallVec<[NativeReading; 6]> = SmallVec::new();
        if frame.entered.is_some() {
            readings.push(reading(ReadingAction::Enter).at(x, y, 1.0));
        }
        if frame.down {
            readings.push(reading(ReadingAction::Press(PointerButton::PRIMARY)).at(x, y, 1.0));
        } else if frame.moved {
            readings.push(reading(ReadingAction::Move).at(x, y, 1.0));
        }
        for (code, pressed) in frame.buttons {
            let button = button_of(code);
            readings.push(reading(if pressed {
                ReadingAction::Press(button)
            } else {
                ReadingAction::Release(button)
            }));
        }
        if frame.up {
            readings.push(reading(ReadingAction::Release(PointerButton::PRIMARY)));
        }
        if frame.left {
            readings.push(reading(ReadingAction::Leave));
        }
        for reading in &readings {
            self.decode(reading, out);
        }
    }

    // ---- Pinches --------------------------------------------------------

    fn pinch(
        &mut self,
        device: NativeDevice,
        pointer: NativeDevice,
        event: PinchWire,
        time: WireTime,
    ) -> Out {
        let mut out = Out::new();
        let readings: SmallVec<[(GestureComponent, GesturePhase, GestureDelta); 2]> = match event {
            PinchWire::Begin { surface } => {
                let focal = self
                    .pointers
                    .iter()
                    .find(|state| state.device == pointer)
                    .map_or((0.0, 0.0), |state| state.position);
                self.pinches.retain(|pinch| pinch.device != device);
                // The gesture object is its pointer's seat's: that seat's
                // removal ends it too.
                if let Some(seat) = self
                    .seats
                    .iter()
                    .find(|(held, _)| *held == pointer)
                    .map(|(_, seat)| *seat)
                {
                    self.seats.retain(|(held, _)| *held != device);
                    self.seats.push((device, seat));
                }
                self.pinches.push(PinchState {
                    device,
                    surface,
                    focal,
                    scale: 1.0,
                });
                smallvec::smallvec![
                    (
                        GestureComponent::Scale,
                        GesturePhase::Began,
                        GestureDelta::None
                    ),
                    (
                        GestureComponent::Rotation,
                        GesturePhase::Began,
                        GestureDelta::None
                    ),
                ]
            }
            PinchWire::Update {
                dx,
                dy,
                scale,
                rotation,
            } => {
                let Some(pinch) = self.pinches.iter_mut().find(|pinch| pinch.device == device)
                else {
                    return out;
                };
                pinch.focal = (pinch.focal.0 + dx, pinch.focal.1 + dy);
                // The scale is since the pinch began; the assembly takes each
                // update's own factor.
                let factor = if scale.is_finite() && scale > 0.0 && pinch.scale > 0.0 {
                    let factor = scale / pinch.scale;
                    pinch.scale = scale;
                    factor
                } else {
                    1.0
                };
                smallvec::smallvec![
                    (
                        GestureComponent::Scale,
                        GesturePhase::Changed,
                        GestureDelta::Scale(factor)
                    ),
                    (
                        GestureComponent::Rotation,
                        GesturePhase::Changed,
                        GestureDelta::Rotation(rotation.to_radians()),
                    ),
                ]
            }
            // Both components end: the gesture is cancelled whole, not left
            // to whichever component the assembly cancels first.
            PinchWire::End { cancelled: true } => smallvec::smallvec![
                (
                    GestureComponent::Scale,
                    GesturePhase::Cancelled,
                    GestureDelta::None
                ),
                (
                    GestureComponent::Rotation,
                    GesturePhase::Cancelled,
                    GestureDelta::None
                ),
            ],
            PinchWire::End { cancelled: false } => smallvec::smallvec![
                (
                    GestureComponent::Scale,
                    GesturePhase::Ended,
                    GestureDelta::None
                ),
                (
                    GestureComponent::Rotation,
                    GesturePhase::Ended,
                    GestureDelta::None
                ),
            ],
        };
        let Some(pinch) = self
            .pinches
            .iter()
            .find(|pinch| pinch.device == device)
            .copied()
        else {
            return out;
        };
        if matches!(event, PinchWire::End { .. }) {
            self.pinches.retain(|held| held.device != device);
        }
        for (component, phase, delta) in readings {
            let reading = NativeReading::new(
                PINCH,
                PointerKind::Trackpad,
                ReadingAction::Gesture(GestureReading {
                    component,
                    phase,
                    delta,
                }),
                time.reading_time(),
            )
            .on(pinch.surface)
            .from_device(device)
            .with_primary(true)
            .with_modifiers(self.modifiers_of(pointer))
            .at(pinch.focal.0, pinch.focal.1, 1.0);
            self.decode(&reading, &mut out);
        }
        out
    }
}

/// The item of `items` that `found` picks, made by `make` when none is.
fn entry<T>(items: &mut Vec<T>, found: impl Fn(&T) -> bool, make: impl FnOnce() -> T) -> &mut T {
    let index = items.iter().position(found).unwrap_or_else(|| {
        items.push(make());
        items.len() - 1
    });
    &mut items[index]
}

/// A touch reading.
fn touch_reading(
    device: NativeDevice,
    surface: NativeSurface,
    id: i32,
    action: ReadingAction,
    time: WireTime,
) -> NativeReading {
    NativeReading::new(
        NativeContact(u64::from(id.cast_unsigned())),
        PointerKind::Touch,
        action,
        time.reading_time(),
    )
    .on(surface)
    .from_device(device)
}

/// A scroll step of `values` (horizontal, vertical) in `unit`, in FLUI's
/// direction: a Wayland axis value grows the scroll offset as FLUI's delta
/// does. A value that is not finite steps zero.
fn step(unit: ScrollUnit, values: [f64; 2]) -> ScrollDelta {
    wheel_step(
        unit,
        WheelAmount::offset(values[0]),
        WheelAmount::offset(values[1]),
    )
    .unwrap_or_else(|_| ScrollDelta::zero(unit))
}

/// A wheel frame's step: detents from `value120` (precise when they are not
/// whole), else from the discrete notches, else the values in pixels.
fn wheel(axes: &AxisFrame) -> (ScrollDelta, ScrollPrecision) {
    if axes.value120.iter().any(Option::is_some) {
        let detents = axes
            .value120
            .map(|value| f64::from(value.unwrap_or(0)) / 120.0);
        let whole = axes
            .value120
            .iter()
            .all(|value| value.unwrap_or(0) % 120 == 0);
        let precision = if whole {
            ScrollPrecision::Notched
        } else {
            ScrollPrecision::Precise
        };
        return (step(ScrollUnit::Detents, detents), precision);
    }
    if axes.discrete.iter().any(Option::is_some) {
        let detents = axes.discrete.map(|value| f64::from(value.unwrap_or(0)));
        return (step(ScrollUnit::Detents, detents), ScrollPrecision::Notched);
    }
    (
        step(ScrollUnit::Pixels, axes.values),
        ScrollPrecision::Notched,
    )
}

#[cfg(test)]
mod tests;
