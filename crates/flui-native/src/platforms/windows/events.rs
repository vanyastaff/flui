//! Win32 message conversion to owned platform input.
//!
//! `window_proc` unpacks every mouse and keyboard message through these
//! functions:
//!
//! - **Pointers** are copied out of each message into plain
//!   [`NativeReading`]s and assembled by the shared
//!   [`pointer_assembly`](crate::shared::pointer_assembly), which owns contact
//!   and device identity, held buttons, click counting and the clock choice.
//!   This file only reads the OS.
//! - **Modifiers** are the queue-synchronized key state (`GetKeyState`), which
//!   Win32 advances as each input message is retrieved — the state that held
//!   when this message was generated. `GetAsyncKeyState` would report the
//!   keyboard at processing time instead, so a queued Shift+click whose Shift
//!   was released before the click was processed would lose its Shift.
//! - **Capture** belongs to a press sequence: [`capture_on_press`] captures
//!   the mouse on a button press and [`release_capture_after`] releases it
//!   once no button stays held, so a drag released outside the client area
//!   still delivers its release (at client coordinates outside the client
//!   rectangle, sign-extended by `get_x_lparam`). A capture taken away while
//!   a button is held ends the sequence with a cancel
//!   ([`capture_changed_input`]). That is the legacy `WM_*BUTTON*` route; on
//!   the `WM_POINTER` route the system captures the pointer for its press.
//! - Mouse samples retain the retrieved message's generation time, rebased
//!   onto the shared monotonic epoch before any reentrant callbacks run.
//!   `WM_POINTER*` readings are stamped from their performance count when
//!   the source reports one, so a coalesced burst keeps its own spacing.

use std::cell::RefCell;

use flui_platform::{
    EventTime, Modifiers as KeyboardModifiers,
    keyboard::{Code, Key, KeyEvent, KeyRepeat, KeyState, Location, NamedKey},
    pointer::{PenTool, PointerButton, PointerButtons, PointerKind},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    System::{
        SystemInformation::GetTickCount64,
        SystemServices::{MK_LBUTTON, MK_MBUTTON, MK_RBUTTON, MK_XBUTTON1, MK_XBUTTON2},
    },
    UI::{
        Input::{
            KeyboardAndMouse::{
                GetCapture, GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_CONTROL,
                VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU, VK_RBUTTON, VK_RWIN, VK_SHIFT,
                VK_XBUTTON1, VK_XBUTTON2,
            },
            Pointer::{
                POINTER_BUTTON_CHANGE_TYPE, POINTER_CHANGE_FIFTHBUTTON_DOWN,
                POINTER_CHANGE_FIFTHBUTTON_UP, POINTER_CHANGE_FIRSTBUTTON_DOWN,
                POINTER_CHANGE_FIRSTBUTTON_UP, POINTER_CHANGE_FOURTHBUTTON_DOWN,
                POINTER_CHANGE_FOURTHBUTTON_UP, POINTER_CHANGE_SECONDBUTTON_DOWN,
                POINTER_CHANGE_SECONDBUTTON_UP, POINTER_CHANGE_THIRDBUTTON_DOWN,
                POINTER_CHANGE_THIRDBUTTON_UP, POINTER_INFO, POINTER_PEN_INFO, POINTER_TOUCH_INFO,
            },
        },
        WindowsAndMessaging::{
            GetMessageTime, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
        },
    },
};

use crate::shared::pointer_assembly::{
    Assembled, ClickSettings, ClockChoice, ContactRegistry, CounterAnchor, DecodeError, MoveBatch,
    NativeCancel, NativeClock, NativeContact, NativeDevice, NativeReading, NativeSurface,
    NativeTime, PenAngles, QpcTicks, RawSensors, ReadingAction, ReadingTime, WheelAxis,
    WheelReading, decode,
};

/// Rebases the queue's wrapping millisecond timestamp, and pointer readings'
/// performance-counter timestamps, onto the shared epoch.
/// The owner snapshots a sample before callback-capable native or user calls.
pub(super) struct MessageClock {
    native: NativeClock,
    /// The latest stamp the window delivered, coarse or precise: a message
    /// stamp never precedes it. A key in the same system tick as a
    /// `WM_POINTER` sample before it reads an earlier millisecond tick than
    /// that sample's performance count.
    last_ns: std::cell::Cell<u64>,
}

/// The performance counter read now, with its frequency.
fn performance_anchor() -> Option<CounterAnchor> {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut count, mut frequency) = (0_i64, 0_i64);
    // SAFETY: both out pointers address initialized locals that outlive
    // the calls, which only write one `i64` each.
    unsafe {
        QueryPerformanceFrequency(&raw mut frequency).ok()?;
        QueryPerformanceCounter(&raw mut count).ok()?;
    }
    Some(CounterAnchor {
        count: QpcTicks(u64::try_from(count).ok()?),
        frequency: u64::try_from(frequency).ok()?.try_into().ok()?,
    })
}

impl MessageClock {
    pub(super) fn new() -> Self {
        // SAFETY: argument-free monotonic system uptime query.
        let uptime_ms = unsafe { GetTickCount64() };
        // The counter and the epoch are read back to back: `Instant` is the
        // same counter on Windows, so their offset stays sub-microsecond.
        let counter = performance_anchor();
        Self::anchored(uptime_ms, counter, event_timestamp_ns())
    }

    fn anchored(uptime_ms: u64, counter: Option<CounterAnchor>, epoch_ns: u64) -> Self {
        Self {
            native: NativeClock::new(
                EventTime::from_nanos(epoch_ns),
                std::time::Duration::from_millis(uptime_ms),
                counter,
            ),
            last_ns: std::cell::Cell::new(0),
        }
    }

    /// The anchors pointer readings are stamped against.
    pub(super) const fn native(&self) -> NativeClock {
        self.native
    }

    pub(super) fn message_time(&self) -> u64 {
        // SAFETY: argument-free queries of the owner thread's retrieved
        // message and system uptime. No callback can run between them.
        let (tick, now) = unsafe { (GetMessageTime() as u32, GetTickCount64()) };
        self.stamp_at(tick, now)
    }

    fn stamp_at(&self, tick: u32, now_ms: u64) -> u64 {
        let time = ReadingTime::coarse(NativeTime::TickMillis {
            tick,
            observed_ms: now_ms,
        });
        let stamp = self
            .native
            .stamp(time, ClockChoice::Coarse)
            .expect("BUG: a millisecond tick is always a finite time")
            .as_nanos()
            .max(self.last_ns.get());
        self.last_ns.set(stamp);
        stamp
    }

    /// Raise the floor to the latest stamp `inputs` carry: pointer readings
    /// stamped from their performance count, which a later message's coarse
    /// stamp must not precede.
    pub(super) fn observe(&self, inputs: &[PlatformInput]) {
        let latest = inputs
            .iter()
            .filter_map(PlatformInput::as_pointer)
            .filter_map(pointer_time)
            .map(EventTime::as_nanos)
            .max();
        if let Some(latest) = latest {
            self.last_ns.set(self.last_ns.get().max(latest));
        }
    }
}

/// When `event` happened; `None` for a revision, whose time names the
/// reading it replaces.
fn pointer_time(event: &flui_platform::pointer::PointerEvent) -> Option<EventTime> {
    use flui_platform::pointer::{ButtonChange, PointerEvent};
    Some(match event {
        PointerEvent::Down(press) | PointerEvent::ButtonChange(ButtonChange::Pressed(press)) => {
            press.sample.time
        }
        PointerEvent::Up(release) | PointerEvent::ButtonChange(ButtonChange::Released(release)) => {
            release.sample.time
        }
        PointerEvent::Move(movement) => movement.current().time,
        PointerEvent::Cancel(cancel) => cancel.time,
        PointerEvent::Enter(signal)
        | PointerEvent::Leave(signal)
        | PointerEvent::ScrollInertiaCancel(signal) => signal.time,
        PointerEvent::Scroll(scroll) => scroll.time,
        PointerEvent::PanZoom(pan_zoom) => pan_zoom.time,
        PointerEvent::PenSqueeze(squeeze) => squeeze.time,
        PointerEvent::DeviceAdded(change) | PointerEvent::DeviceRemoved(change) => change.time,
        // A revision names the reading it replaces; the vocabulary may grow.
        _ => return None,
    })
}

// Native queue clocks cannot be advanced through a consumer API. These rows
// pin the wrapping/rebasing arithmetic used by the actual Win32 producer;
// queued dispatch itself is covered through a live public window contract.
#[cfg(test)]
mod message_clock_contract {
    use super::MessageClock;

    #[test]
    fn message_clock_preserves_wrapping_samples_and_window_epochs() {
        let wrap = u64::from(u32::MAX) + 1;
        for (name, anchor_tick, anchor_ns, now, samples, expected) in [
            (
                "ordinary queued gap",
                100,
                1_000_000,
                200,
                [120, 160],
                [21_000_000, 61_000_000],
            ),
            (
                "32-bit uptime wrap",
                wrap - 10,
                1_000_000,
                wrap + 30,
                [u32::MAX - 4, 7],
                [6_000_000, 18_000_000],
            ),
            (
                "older sent timestamp",
                100,
                1_000_000,
                200,
                [150, 120],
                [51_000_000, 51_000_000],
            ),
            (
                "pre-epoch queued sample",
                100,
                20_000_000,
                200,
                [90, 110],
                [10_000_000, 30_000_000],
            ),
            (
                "nanosecond exhaustion",
                100,
                u64::MAX - 1,
                200,
                [101, 102],
                [u64::MAX, u64::MAX],
            ),
            (
                "native reading ahead of coarse uptime",
                100,
                1_000_000,
                200,
                [200, 201],
                [101_000_000, 102_000_000],
            ),
        ] {
            let clock = MessageClock::anchored(anchor_tick, None, anchor_ns);
            assert_eq!(
                samples.map(|tick| clock.stamp_at(tick, now)),
                expected,
                "{name}"
            );
        }
        let first = MessageClock::anchored(100, None, 1_000_000);
        let second = MessageClock::anchored(120, None, 21_000_000);
        assert_eq!(
            first.stamp_at(150, 200),
            second.stamp_at(150, 200),
            "independent windows share the process epoch"
        );
    }

    /// A message stamped from its millisecond tick after a pointer reading
    /// stamped from its performance count, in the same system tick, is not
    /// placed before that reading.
    #[test]
    fn a_coarse_stamp_never_precedes_a_delivered_precise_one() {
        use flui_platform::EventTime;
        use flui_platform::pointer::{
            CancelReason, PointerCancel, PointerEvent, PointerId, PointerInfo, PointerKind,
        };
        let clock = MessageClock::anchored(100, None, 1_000_000);
        let precise = PointerEvent::Cancel(PointerCancel::new(
            PointerInfo::new(
                PointerId::new(std::num::NonZeroU64::MIN),
                PointerKind::Touch,
            ),
            EventTime::from_nanos(25_400_000),
            CancelReason::Platform,
        ));
        clock.observe(&[flui_platform::PlatformInput::Pointer(precise)]);
        assert_eq!(
            [clock.stamp_at(110, 200), clock.stamp_at(130, 200)],
            [25_400_000, 31_000_000],
            "floored to the reading, then the tick again once it is later"
        );
    }
}

use crate::{shared::events::event_timestamp_ns, shared::keys};
use flui_platform::PlatformInput;

/// Convert LPARAM to X coordinate
#[inline]
pub(super) fn get_x_lparam(lparam: LPARAM) -> i32 {
    (lparam.0 & 0xFFFF) as i16 as i32
}

/// Convert LPARAM to Y coordinate
#[inline]
pub(super) fn get_y_lparam(lparam: LPARAM) -> i32 {
    ((lparam.0 >> 16) & 0xFFFF) as i16 as i32
}

/// Every Win32 registry serves one window.
pub(crate) const SURFACE: NativeSurface = NativeSurface(0);

/// Each mouse button: its virtual key, the `MK_*` bit a mouse message's
/// `WPARAM` and OLE's key state carry for it, and the button it is.
pub(crate) const MOUSE_BUTTONS: [(VIRTUAL_KEY, u32, PointerButton); 5] = [
    (VK_LBUTTON, MK_LBUTTON.0, PointerButton::PRIMARY),
    (VK_RBUTTON, MK_RBUTTON.0, PointerButton::SECONDARY),
    (VK_MBUTTON, MK_MBUTTON.0, PointerButton::AUXILIARY),
    (VK_XBUTTON1, MK_XBUTTON1.0, PointerButton::BACK),
    (VK_XBUTTON2, MK_XBUTTON2.0, PointerButton::FORWARD),
];

/// The button a pointer packet's `ButtonChangeType` names and whether it
/// went down; `None` when no button changed.
fn button_change(change: POINTER_BUTTON_CHANGE_TYPE) -> Option<(PointerButton, bool)> {
    Some(match change {
        POINTER_CHANGE_FIRSTBUTTON_DOWN => (PointerButton::PRIMARY, true),
        POINTER_CHANGE_FIRSTBUTTON_UP => (PointerButton::PRIMARY, false),
        POINTER_CHANGE_SECONDBUTTON_DOWN => (PointerButton::SECONDARY, true),
        POINTER_CHANGE_SECONDBUTTON_UP => (PointerButton::SECONDARY, false),
        POINTER_CHANGE_THIRDBUTTON_DOWN => (PointerButton::AUXILIARY, true),
        POINTER_CHANGE_THIRDBUTTON_UP => (PointerButton::AUXILIARY, false),
        POINTER_CHANGE_FOURTHBUTTON_DOWN => (PointerButton::BACK, true),
        POINTER_CHANGE_FOURTHBUTTON_UP => (PointerButton::BACK, false),
        POINTER_CHANGE_FIFTHBUTTON_DOWN => (PointerButton::FORWARD, true),
        POINTER_CHANGE_FIFTHBUTTON_UP => (PointerButton::FORWARD, false),
        _ => return None,
    })
}

/// The mouse's contact, given the primary mouse identity: outside the 32-bit
/// range of `WM_POINTER` ids. Every route a mouse reaches the window by —
/// `WM_POINTER` under `EnableMouseInPointer`, the legacy `WM_MOUSE*`
/// messages (a host that keeps them, or a cursor moved by `SetCursorPos`)
/// and the wheel, which always comes as `WM_MOUSEWHEEL` — reads as this one
/// contact, so its hover, presses and scrolls share one pointer.
const MOUSE_CONTACT: NativeContact = NativeContact(1 << 32);

/// The contact a `WM_POINTER` id reads as, `mouse` when it is the mouse's.
fn pointer_contact(mouse: bool, raw: u32) -> NativeContact {
    if mouse {
        MOUSE_CONTACT
    } else {
        NativeContact(u64::from(raw))
    }
}

/// A window's pointer registry on `clock`, with the OS double-click
/// thresholds at `scale`.
pub(super) fn pointer_registry(clock: NativeClock, scale: f64) -> ContactRegistry {
    let mut registry = ContactRegistry::new(clock).with_reserved_contact(MOUSE_CONTACT);
    registry.set_click_settings(click_settings(scale));
    registry
}

/// The OS double-click interval and rectangle at `scale`, in device pixels;
/// `None` when the metrics cannot be read.
pub(super) fn click_settings(scale: f64) -> Option<ClickSettings> {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a finite positive scale times 96 is a small DPI"
    )]
    let dpi = (scale * 96.0).round() as u32;
    let area = super::preferences::mouse_geometry(dpi)
        .inspect_err(|error| tracing::debug!(%error, "double-click area unavailable"))
        .ok()?
        .double_click_area();
    // SAFETY: this getter takes no pointers and only reads the system setting.
    let interval = unsafe { GetDoubleClickTime() };
    Some(ClickSettings {
        interval: std::time::Duration::from_millis(u64::from(interval)),
        area: (f64::from(area.width), f64::from(area.height)),
    })
}

/// One native pointer packet, copied out of the OS before any callback runs.
/// A pen's carries its device's himetric mapping, when it could be read.
#[derive(Clone, Copy)]
enum PointerPacket {
    Pointer(POINTER_INFO),
    Pen(POINTER_PEN_INFO, Option<HimetricMap>),
    Touch(POINTER_TOUCH_INFO),
}

impl PointerPacket {
    fn info(self) -> POINTER_INFO {
        match self {
            Self::Pointer(info) => info,
            Self::Pen(info, _) => info.pointerInfo,
            Self::Touch(info) => info.pointerInfo,
        }
    }

    /// The packet's raw screen position in device pixels. A pen reports its
    /// position in himetric units, finer than a pixel at its 120–240 Hz, and
    /// it is mapped through its device's rectangles; every other packet, or
    /// a pen whose mapping is missing or disagrees with the pixel the system
    /// reported, has the integer pixel.
    fn screen_position(self) -> (f64, f64) {
        let pixel = self.info().ptPixelLocationRaw;
        let whole = (f64::from(pixel.x), f64::from(pixel.y));
        match self {
            Self::Pen(pen, Some(map)) => map
                .to_screen(pen.pointerInfo.ptHimetricLocationRaw)
                .filter(|(x, y)| (x - whole.0).abs() <= 1.0 && (y - whole.1).abs() <= 1.0)
                .unwrap_or(whole),
            Self::Pointer(_) | Self::Pen(_, None) | Self::Touch(_) => whole,
        }
    }
}

/// How a pointer device's himetric coordinates map onto the screen
/// (`GetPointerDeviceRects`): its own rectangle and the display rectangle it
/// covers, in screen pixels.
#[derive(Clone, Copy)]
struct HimetricMap {
    device: RECT,
    display: RECT,
}

impl HimetricMap {
    /// `device`'s mapping now; `None` when the system cannot report it.
    fn of(device: windows::Win32::Foundation::HANDLE) -> Option<Self> {
        use windows::Win32::UI::Input::Pointer::GetPointerDeviceRects;
        let mut map = Self {
            device: RECT::default(),
            display: RECT::default(),
        };
        // SAFETY: both out pointers address fields of a live local; the
        // system refuses a stale or invented device handle.
        unsafe { GetPointerDeviceRects(device, &raw mut map.device, &raw mut map.display) }.ok()?;
        Some(map)
    }

    /// The screen pixel position of `himetric`, linearly between the two
    /// rectangles; `None` for an empty rectangle.
    fn to_screen(self, himetric: POINT) -> Option<(f64, f64)> {
        let axis =
            |value: i32, (from_start, from_end): (i32, i32), (to_start, to_end): (i32, i32)| {
                let from = f64::from(from_end) - f64::from(from_start);
                let to = f64::from(to_end) - f64::from(to_start);
                (from > 0.0 && to > 0.0).then(|| {
                    f64::from(to_start) + (f64::from(value) - f64::from(from_start)) * to / from
                })
            };
        let (device, display) = (self.device, self.display);
        Some((
            axis(
                himetric.x,
                (device.left, device.right),
                (display.left, display.right),
            )?,
            axis(
                himetric.y,
                (device.top, device.bottom),
                (display.top, display.bottom),
            )?,
        ))
    }
}

/// The plain reading a `message` packet makes. `client_offset` moves raw
/// screen pixels into the client area; `now_ms` is the uptime its tick is
/// placed against.
fn pointer_reading(
    packet: PointerPacket,
    message: u32,
    client_offset: POINT,
    scale: f64,
    now_ms: u64,
    modifiers: KeyboardModifiers,
) -> NativeReading {
    use windows::Win32::UI::{
        Input::Pointer::{
            POINTER_FLAG_CANCELED, POINTER_FLAG_FIFTHBUTTON, POINTER_FLAG_FIRSTBUTTON,
            POINTER_FLAG_FOURTHBUTTON, POINTER_FLAG_PRIMARY, POINTER_FLAG_SECONDBUTTON,
            POINTER_FLAG_THIRDBUTTON,
        },
        WindowsAndMessaging::{
            PEN_FLAG_ERASER, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X,
            PEN_MASK_TILT_Y, PT_MOUSE, PT_TOUCHPAD, TOUCH_MASK_CONTACTAREA, TOUCH_MASK_PRESSURE,
            WM_POINTERDOWN, WM_POINTERENTER, WM_POINTERLEAVE, WM_POINTERUP,
        },
    };
    let native = packet.info();
    let screen = packet.screen_position();
    let flags = native.pointerFlags.0;
    let mut sensors = RawSensors::default();
    let kind = match packet {
        PointerPacket::Pen(pen, _) => {
            if pen.penMask & PEN_MASK_PRESSURE != 0 {
                sensors.pressure = Some(f64::from(pen.pressure) / 1024.0);
            }
            if pen.penMask & PEN_MASK_ROTATION != 0 {
                sensors.twist_degrees = Some(f64::from(pen.rotation));
            }
            if pen.penMask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y)
                == (PEN_MASK_TILT_X | PEN_MASK_TILT_Y)
            {
                sensors.angles = Some(PenAngles::TiltDegrees {
                    x: f64::from(pen.tiltX),
                    y: f64::from(pen.tiltY),
                });
            }
            PointerKind::Pen {
                tool: if pen.penFlags & PEN_FLAG_ERASER != 0 {
                    PenTool::Eraser
                } else {
                    PenTool::Tip
                },
            }
        }
        PointerPacket::Touch(touch) => {
            if touch.touchMask & TOUCH_MASK_PRESSURE != 0 {
                sensors.pressure = Some(f64::from(touch.pressure) / 1024.0);
            }
            if touch.touchMask & TOUCH_MASK_CONTACTAREA != 0 {
                let contact = touch.rcContactRaw;
                sensors.contact_size = Some((
                    f64::from(contact.right) - f64::from(contact.left),
                    f64::from(contact.bottom) - f64::from(contact.top),
                ));
            }
            PointerKind::Touch
        }
        PointerPacket::Pointer(info) => match info.pointerType {
            PT_MOUSE => PointerKind::Mouse,
            PT_TOUCHPAD => PointerKind::Trackpad,
            _ => PointerKind::Unknown,
        },
    };
    let mut buttons = PointerButtons::NONE;
    for (flag, button) in [
        (POINTER_FLAG_FIRSTBUTTON, PointerButton::PRIMARY),
        (POINTER_FLAG_SECONDBUTTON, PointerButton::SECONDARY),
        (POINTER_FLAG_THIRDBUTTON, PointerButton::AUXILIARY),
        (POINTER_FLAG_FOURTHBUTTON, PointerButton::BACK),
        (POINTER_FLAG_FIFTHBUTTON, PointerButton::FORWARD),
    ] {
        if flags & flag.0 != 0 {
            buttons = buttons.with(button);
        }
    }
    let change = button_change(native.ButtonChangeType);
    // A DOWN or UP that names no button is the primary's.
    let changed = change.map_or(PointerButton::PRIMARY, |(button, _)| button);
    // ENTER can follow DOWN while both notifications refer to the same native
    // packet: its cached ButtonChangeType is not a second press.
    let action = if message == WM_POINTERENTER {
        ReadingAction::Enter
    } else if message == WM_POINTERLEAVE {
        ReadingAction::Leave
    } else if flags & POINTER_FLAG_CANCELED.0 != 0 {
        ReadingAction::Cancel(NativeCancel::Platform)
    } else if change.is_some_and(|(_, down)| down) || message == WM_POINTERDOWN {
        ReadingAction::Press(changed)
    } else if change.is_some_and(|(_, down)| !down) || message == WM_POINTERUP {
        ReadingAction::Release(changed)
    } else {
        ReadingAction::Move
    };
    let time = ReadingTime {
        coarse: NativeTime::TickMillis {
            tick: native.dwTime,
            observed_ms: now_ms,
        },
        counter: (native.PerformanceCount != 0).then_some(QpcTicks(native.PerformanceCount)),
    };
    let mut reading = NativeReading::new(
        pointer_contact(kind == PointerKind::Mouse, native.pointerId),
        kind,
        action,
        time,
    )
    .on(SURFACE)
    .at(
        screen.0 + f64::from(client_offset.x),
        screen.1 + f64::from(client_offset.y),
        scale,
    )
    .with_primary(flags & POINTER_FLAG_PRIMARY.0 != 0)
    .with_buttons(buttons)
    .with_modifiers(modifiers)
    .with_sensors(sensors);
    if !native.sourceDevice.is_invalid() {
        reading = reading.from_device(NativeDevice(native.sourceDevice.0 as usize as u64));
    }
    reading
}

/// Native history is newest first and includes current. Read it before any
/// callback, then decode every measured sample against one uptime snapshot.
fn native_history(raw: u32) -> Option<Vec<PointerPacket>> {
    use windows::Win32::UI::{
        Input::Pointer::{
            GetPointerInfo, GetPointerInfoHistory, GetPointerPenInfoHistory,
            GetPointerTouchInfoHistory,
        },
        WindowsAndMessaging::{PT_PEN, PT_TOUCH},
    };
    let mut info = POINTER_INFO::default();
    // SAFETY: `info` is a live, writable local. The native ID is obtained
    // from the message; Windows rejects stale or invented IDs.
    unsafe { GetPointerInfo(raw, &raw mut info) }.ok()?;
    let count = info.historyCount.max(1);
    match info.pointerType {
        PT_PEN => read_history(count, |count, buffer: &mut [POINTER_PEN_INFO]| {
            // SAFETY: `*count` is `buffer.len()` (`read_history`), so the
            // call writes at most the entries `buffer` holds.
            unsafe { GetPointerPenInfoHistory(raw, count, Some(buffer.as_mut_ptr())) }
        })
        .map(|readings| {
            // One mapping for the message: its device is the same throughout.
            let map = HimetricMap::of(info.sourceDevice);
            readings
                .into_iter()
                .map(|pen| PointerPacket::Pen(pen, map))
                .collect()
        }),
        PT_TOUCH => read_history(count, |count, buffer: &mut [POINTER_TOUCH_INFO]| {
            // SAFETY: as for the pen history.
            unsafe { GetPointerTouchInfoHistory(raw, count, Some(buffer.as_mut_ptr())) }
        })
        .map(|readings| readings.into_iter().map(PointerPacket::Touch).collect()),
        _ => read_history(count, |count, buffer: &mut [POINTER_INFO]| {
            // SAFETY: as for the pen history.
            unsafe { GetPointerInfoHistory(raw, count, Some(buffer.as_mut_ptr())) }
        })
        .map(|readings| readings.into_iter().map(PointerPacket::Pointer).collect()),
    }
}

/// Read up to `count` history entries: `query` is handed a buffer of
/// `count` entries and that same count, which it rewrites to the number it
/// filled. A count larger than the buffer is refused.
fn read_history<T: Default + Clone>(
    count: u32,
    query: impl FnOnce(&mut u32, &mut [T]) -> windows::core::Result<()>,
) -> Option<Vec<T>> {
    let len = usize::try_from(count).ok()?;
    let mut values = Vec::new();
    values.try_reserve_exact(len).ok()?;
    values.resize(len, T::default());
    let mut filled = count;
    query(&mut filled, &mut values).ok()?;
    let filled = usize::try_from(filled)
        .ok()
        .filter(|filled| *filled <= len)?;
    values.truncate(filled);
    Some(values)
}

/// Assemble a newest-first native history: an update carries the older
/// readings as its coalesced batch.
fn assemble_history(
    registry: &mut ContactRegistry,
    packets: &[PointerPacket],
    message: u32,
    client_offset: POINT,
    scale: f64,
    now_ms: u64,
    modifiers: KeyboardModifiers,
) -> Assembled {
    use windows::Win32::UI::WindowsAndMessaging::WM_POINTERUPDATE;
    let reading =
        |packet| pointer_reading(packet, message, client_offset, scale, now_ms, modifiers);
    let Some((&current, older)) = packets.split_first() else {
        return Assembled::new();
    };
    let assembled = if message == WM_POINTERUPDATE && !older.is_empty() {
        let mut batch = MoveBatch::new();
        for &packet in older.iter().rev() {
            batch.push_coalesced(reading(packet));
        }
        batch.decode(registry, &reading(current))
    } else {
        decode(registry, &reading(current))
    };
    assembled.unwrap_or_else(DecodeError::into_inputs)
}

/// Whether the `WM_POINTER` id `raw` is the mouse's. A stale or invented id
/// is not.
pub(super) fn is_mouse_pointer(raw: u32) -> bool {
    use windows::Win32::UI::{
        Input::Pointer::GetPointerType,
        WindowsAndMessaging::{POINTER_INPUT_TYPE, PT_MOUSE},
    };
    let mut native_kind = POINTER_INPUT_TYPE::default();
    // SAFETY: an initialized out value owned by this call; Windows refuses a
    // stale or invented id.
    unsafe { GetPointerType(raw, &raw mut native_kind) }.is_ok() && native_kind == PT_MOUSE
}

/// The input a `WM_POINTER*` `message` for pointer `raw` makes; `mouse` is
/// [`is_mouse_pointer`]`(raw)`.
pub(super) fn native_pointer_input(
    hwnd: HWND,
    message: u32,
    (raw, mouse): (u32, bool),
    scale: f64,
    clock: &MessageClock,
    registry: &RefCell<ContactRegistry>,
) -> Assembled {
    use windows::Win32::UI::WindowsAndMessaging::WM_POINTERCAPTURECHANGED;
    let contact = pointer_contact(mouse, raw);
    let cancel = |reason| {
        let time = EventTime::from_nanos(clock.message_time());
        registry
            .borrow_mut()
            .cancel_contact(SURFACE, contact, time, reason)
            .into_iter()
            .collect()
    };
    if message == WM_POINTERCAPTURECHANGED {
        return cancel(NativeCancel::CaptureLost);
    }
    let Some(packets) = native_history(raw) else {
        tracing::warn!(raw, "pointer history unavailable; its contact is cancelled");
        return cancel(NativeCancel::InvalidInput);
    };
    let mut offset = POINT::default();
    // SAFETY: the live owner HWND and initialized client translation buffer
    // remain valid under the window procedure's ContextGuard.
    let now = unsafe {
        if !ScreenToClient(hwnd, &raw mut offset).as_bool() {
            tracing::warn!(raw, "client origin unavailable; the pointer is cancelled");
            return cancel(NativeCancel::InvalidInput);
        }
        GetTickCount64()
    };
    let modifiers = message_modifiers();
    let assembled = assemble_history(
        &mut registry.borrow_mut(),
        &packets,
        message,
        offset,
        scale,
        now,
        modifiers,
    );
    clock.observe(&assembled);
    assembled
}

pub(super) fn native_device_kind(
    handle: windows::Win32::Foundation::HANDLE,
) -> Option<PointerKind> {
    use windows::Win32::UI::{
        Controls::{
            POINTER_DEVICE_INFO, POINTER_DEVICE_TYPE_EXTERNAL_PEN,
            POINTER_DEVICE_TYPE_INTEGRATED_PEN, POINTER_DEVICE_TYPE_TOUCH,
            POINTER_DEVICE_TYPE_TOUCH_PAD,
        },
        Input::Pointer::GetPointerDevice,
    };
    let mut info = POINTER_DEVICE_INFO::default();
    // SAFETY: the notification carries the device handle and the initialized
    // output lives through the query. Stale/removed handles are refused by Windows.
    unsafe {
        GetPointerDevice(handle, &raw mut info).ok()?;
    }
    Some(match info.pointerDeviceType {
        POINTER_DEVICE_TYPE_INTEGRATED_PEN | POINTER_DEVICE_TYPE_EXTERNAL_PEN => {
            PointerKind::Pen { tool: PenTool::Tip }
        }
        POINTER_DEVICE_TYPE_TOUCH => PointerKind::Touch,
        POINTER_DEVICE_TYPE_TOUCH_PAD => PointerKind::Trackpad,
        _ => PointerKind::Unknown,
    })
}

/// The device a `WM_POINTERDEVICECHANGE` names.
pub(super) fn native_device(lparam: LPARAM) -> NativeDevice {
    NativeDevice(lparam.0 as usize as u64)
}

#[cfg(test)]
mod native_pointer_contracts {
    use super::*;
    use flui_platform::pointer::{PointerEvent, PointerRole, PointerSample, Pressure};
    use windows::Win32::{
        Foundation::HANDLE,
        UI::{
            Input::Pointer::{
                POINTER_CHANGE_FIRSTBUTTON_DOWN, POINTER_FLAG_DOWN, POINTER_FLAG_FIRSTBUTTON,
                POINTER_FLAG_INCONTACT, POINTER_FLAG_PRIMARY, POINTER_FLAG_UP, POINTER_FLAGS,
            },
            WindowsAndMessaging::{
                PEN_FLAG_ERASER, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X,
                PEN_MASK_TILT_Y, PT_PEN, PT_TOUCH, TOUCH_MASK_CONTACTAREA, TOUCH_MASK_PRESSURE,
                WM_POINTERDOWN, WM_POINTERENTER, WM_POINTERUP, WM_POINTERUPDATE,
            },
        },
    };

    fn info() -> POINTER_INFO {
        POINTER_INFO {
            pointerId: 7,
            sourceDevice: HANDLE(0x3450_usize as *mut core::ffi::c_void),
            pointerFlags: POINTER_FLAG_PRIMARY,
            ptPixelLocationRaw: POINT { x: 120, y: 220 },
            ptPixelLocation: POINT { x: 900, y: 800 },
            ..POINTER_INFO::default()
        }
    }

    fn registry() -> ContactRegistry {
        ContactRegistry::new(MessageClock::anchored(0, None, 0).native())
            .with_reserved_contact(MOUSE_CONTACT)
    }

    /// `packet` assembled as `message` at client offset (-100, -200) and
    /// scale two.
    fn assemble(
        registry: &mut ContactRegistry,
        packets: &[PointerPacket],
        message: u32,
    ) -> Vec<PointerEvent> {
        assemble_history(
            registry,
            packets,
            message,
            POINT { x: -100, y: -200 },
            2.0,
            0,
            KeyboardModifiers::NONE,
        )
        .into_iter()
        .filter_map(|input| input.as_pointer().cloned())
        .collect()
    }

    /// The sample of the move `packet` makes for a hovering contact.
    fn sampled(packet: PointerPacket) -> (flui_platform::pointer::PointerInfo, PointerSample) {
        let events = assemble(&mut registry(), &[packet], WM_POINTERUPDATE);
        events
            .into_iter()
            .find_map(|event| match event {
                PointerEvent::Move(movement) => Some((movement.pointer, *movement.current())),
                _ => None,
            })
            .expect("a valid native reading is delivered")
    }

    fn native_raw_position_and_identity_survive() {
        let (pointer, sample) = sampled(PointerPacket::Pointer(info()));
        assert_eq!(
            sample.position.get(),
            flui_foundation::geometry::Point::new(10.0, 10.0)
        );
        assert!(pointer.device.is_some(), "the source device is minted");
        assert_eq!(pointer.role, PointerRole::Primary);
        assert_eq!(sample.pressure, None);
        let mut secondary = info();
        secondary.pointerFlags = POINTER_FLAGS::default();
        secondary.sourceDevice = HANDLE::default();
        let (pointer, _) = sampled(PointerPacket::Pointer(secondary));
        assert_eq!(pointer.role, PointerRole::Additional);
        assert_eq!(
            pointer.device, None,
            "missing hardware identity stays absent"
        );
    }

    fn pen(pen: POINTER_PEN_INFO) -> PointerPacket {
        PointerPacket::Pen(
            POINTER_PEN_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_PEN,
                    ..info()
                },
                ..pen
            },
            None,
        )
    }

    /// A pen's himetric position, mapped through its device's rectangles,
    /// keeps the fraction of a pixel the integer pixel drops; a mapping that
    /// is empty, or that puts the pen more than a pixel from where the system
    /// says it is (a rotated display maps the axes otherwise), leaves the
    /// pixel.
    fn native_pen_position_keeps_himetric_precision() {
        let rect = |right, bottom| RECT {
            left: 0,
            top: 0,
            right,
            bottom,
        };
        // 10 himetric units to the pixel; `info()` is at pixel (120, 220),
        // read at client offset (-100, -200) and scale two.
        let tenth = HimetricMap {
            device: rect(10_000, 10_000),
            display: rect(1_000, 1_000),
        };
        let at = |himetric: POINT, map: Option<HimetricMap>| {
            let packet = POINTER_PEN_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_PEN,
                    ptHimetricLocationRaw: himetric,
                    ..info()
                },
                ..Default::default()
            };
            let position = sampled(PointerPacket::Pen(packet, map)).1.position.get();
            (position.x, position.y)
        };
        let rows = [
            (
                "mapped",
                POINT { x: 1_205, y: 2_207 },
                Some(tenth),
                (10.25, 10.35),
            ),
            (
                "no mapping",
                POINT { x: 1_205, y: 2_207 },
                None,
                (10.0, 10.0),
            ),
            (
                "mapping disagrees with the pixel",
                POINT { x: 2_207, y: 1_205 },
                Some(tenth),
                (10.0, 10.0),
            ),
            (
                "empty device rectangle",
                POINT { x: 1_205, y: 2_207 },
                Some(HimetricMap {
                    device: rect(0, 10_000),
                    ..tenth
                }),
                (10.0, 10.0),
            ),
        ];
        for (name, himetric, map, (x, y)) in rows {
            let (got_x, got_y) = at(himetric, map);
            assert!(
                (got_x - x).abs() < 1e-9 && (got_y - y).abs() < 1e-9,
                "{name}: ({got_x}, {got_y}), expected ({x}, {y})"
            );
        }
    }

    fn native_pen_masks_preserve_sensor_presence() {
        let base = POINTER_PEN_INFO {
            pressure: 1024,
            rotation: 180,
            tiltX: 45,
            tiltY: 0,
            ..Default::default()
        };
        let (pointer, missing) = sampled(pen(base));
        assert_eq!(pointer.kind, PointerKind::Pen { tool: PenTool::Tip });
        assert_eq!(missing.pressure, None);
        assert_eq!(missing.orientation, None);
        assert_eq!(missing.twist, None);
        let (pointer, present) = sampled(pen(POINTER_PEN_INFO {
            penFlags: PEN_FLAG_ERASER,
            penMask: PEN_MASK_PRESSURE | PEN_MASK_ROTATION | PEN_MASK_TILT_X | PEN_MASK_TILT_Y,
            ..base
        }));
        assert_eq!(
            pointer.kind,
            PointerKind::Pen {
                tool: PenTool::Eraser
            }
        );
        assert_eq!(present.pressure.map(Pressure::get), Some(1.0));
        let orientation = present.orientation.expect("both native tilt axes");
        assert!(
            (orientation.altitude().expect("altitude") - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        );
        assert_eq!(orientation.azimuth(), Some(0.0));
        assert!(
            (present.twist.expect("reported rotation").radians() - std::f64::consts::PI).abs()
                < 1e-12
        );
        let (_, partial) = sampled(pen(POINTER_PEN_INFO {
            penMask: PEN_MASK_TILT_X,
            ..base
        }));
        assert_eq!(
            partial.orientation, None,
            "one tilt axis does not determine a pen orientation"
        );
    }

    fn native_touch_contact_and_pressure_are_measured() {
        let touch = POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerFlags: POINTER_FLAG_PRIMARY
                    | POINTER_FLAG_DOWN
                    | POINTER_FLAG_FIRSTBUTTON
                    | POINTER_FLAG_INCONTACT,
                ..info()
            },
            touchMask: TOUCH_MASK_CONTACTAREA | TOUCH_MASK_PRESSURE,
            rcContactRaw: windows::Win32::Foundation::RECT {
                left: 100,
                top: 200,
                right: 140,
                bottom: 260,
            },
            rcContact: windows::Win32::Foundation::RECT {
                left: 100,
                top: 200,
                right: 180,
                bottom: 320,
            },
            pressure: 512,
            ..Default::default()
        };
        let down = |touch| {
            assemble(
                &mut registry(),
                &[PointerPacket::Touch(touch)],
                WM_POINTERDOWN,
            )
            .into_iter()
            .find_map(|event| match event {
                PointerEvent::Down(press) => Some(press),
                _ => None,
            })
            .expect("touch down")
        };
        let press = down(touch);
        assert_eq!(press.pointer.kind, PointerKind::Touch);
        assert_eq!(press.sample.pressure.map(Pressure::get), Some(0.5));
        assert_eq!(
            press.sample.contact_size.expect("reported contact").get(),
            flui_foundation::geometry::Size::new(20.0, 30.0)
        );
        let absent = down(POINTER_TOUCH_INFO {
            touchMask: 0,
            ..touch
        });
        assert_eq!(absent.sample.pressure, None);
        assert_eq!(absent.sample.contact_size, None);
    }

    fn invalid_native_sensor_and_scale_are_refused() {
        for (pressure, scale) in [(1025, 1.0), (0, 0.0), (0, f64::NAN)] {
            let packet = pen(POINTER_PEN_INFO {
                penMask: PEN_MASK_PRESSURE,
                pressure,
                ..Default::default()
            });
            let reading = pointer_reading(
                packet,
                WM_POINTERUPDATE,
                POINT::default(),
                scale,
                0,
                KeyboardModifiers::NONE,
            );
            assert!(decode(&mut registry(), &reading).is_err());
        }
        let (_, sample) = sampled(pen(POINTER_PEN_INFO {
            penMask: PEN_MASK_PRESSURE,
            pressure: 0,
            ..Default::default()
        }));
        assert_eq!(
            sample.pressure.map(Pressure::get),
            Some(0.0),
            "reported zero is a sensor reading"
        );
    }

    fn touch_packet(id: u32, flags: POINTER_FLAGS) -> PointerPacket {
        PointerPacket::Touch(POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerId: id,
                pointerFlags: flags,
                ..info()
            },
            ..Default::default()
        })
    }

    fn down(registry: &mut ContactRegistry, id: u32) -> flui_platform::pointer::PointerInfo {
        assemble(
            registry,
            &[touch_packet(
                id,
                POINTER_FLAG_DOWN | POINTER_FLAG_FIRSTBUTTON | POINTER_FLAG_INCONTACT,
            )],
            WM_POINTERDOWN,
        )
        .into_iter()
        .find_map(|event| match event {
            PointerEvent::Down(press) => Some(press.pointer),
            _ => None,
        })
        .expect("native contact must be admitted")
    }

    fn native_terminal_identity_survives_callback_readmission() {
        let mut registry = registry();
        let old = down(&mut registry, 7);
        let terminal = assemble(
            &mut registry,
            &[touch_packet(7, POINTER_FLAG_UP)],
            WM_POINTERUP,
        );
        // Presentation delivery can re-admit the native ID after retirement.
        let fresh = down(&mut registry, 7);
        assert_ne!(old.id, fresh.id);
        assert!(
            terminal
                .iter()
                .any(|event| matches!(event, PointerEvent::Up(release) if release.pointer == old))
        );
        let cancelled = registry.cancel_contact(
            SURFACE,
            NativeContact(7),
            EventTime::from_nanos(4),
            NativeCancel::Platform,
        );
        assert!(cancelled.iter().any(|input| matches!(input, PlatformInput::Pointer(PointerEvent::Cancel(cancel)) if cancel.pointer == fresh)));
    }

    fn enter_after_native_down_is_not_another_button_press() {
        let mut registry = registry();
        let native = PointerPacket::Pointer(POINTER_INFO {
            pointerFlags: POINTER_FLAG_DOWN | POINTER_FLAG_FIRSTBUTTON | POINTER_FLAG_INCONTACT,
            ButtonChangeType: POINTER_CHANGE_FIRSTBUTTON_DOWN,
            ..info()
        });
        let admitted = assemble(&mut registry, &[native], WM_POINTERDOWN);
        let entered = assemble(&mut registry, &[native], WM_POINTERENTER);
        assert!(
            admitted
                .iter()
                .any(|event| matches!(event, PointerEvent::Down(_)))
        );
        assert!(
            matches!(entered.as_slice(), [PointerEvent::Enter(_)]),
            "the native DOWN packet also backs the following ENTER notification: {entered:?}"
        );
    }

    fn native_enter_before_down_preserves_first_contact_admission() {
        let mut registry = registry();
        let native = PointerPacket::Pointer(POINTER_INFO {
            pointerFlags: POINTER_FLAG_DOWN | POINTER_FLAG_FIRSTBUTTON | POINTER_FLAG_INCONTACT,
            ButtonChangeType: POINTER_CHANGE_FIRSTBUTTON_DOWN,
            ..info()
        });
        let entered = assemble(&mut registry, &[native], WM_POINTERENTER);
        let enter_info = entered
            .iter()
            .find_map(|event| match event {
                PointerEvent::Enter(signal) => Some(signal.pointer),
                _ => None,
            })
            .expect("native entry");
        let contact = assemble(&mut registry, &[native], WM_POINTERDOWN);
        assert!(
            matches!(contact.as_slice(), [PointerEvent::Down(press)] if press.pointer == enter_info),
            "ENTER observes the identity but does not admit its first press: {contact:?}"
        );
    }

    /// 100 ns counter ticks anchored at uptime 5000 ms and epoch 1 s.
    fn performance_clock() -> MessageClock {
        MessageClock::anchored(
            5_000,
            Some(CounterAnchor {
                count: QpcTicks(1_000_000),
                frequency: std::num::NonZeroU64::new(10_000_000).expect("nonzero"),
            }),
            1_000_000_000,
        )
    }

    fn timed_touch(dw_time: u32, count: u64, x: i32) -> PointerPacket {
        PointerPacket::Touch(POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
                pointerFlags: POINTER_FLAG_PRIMARY
                    | POINTER_FLAG_FIRSTBUTTON
                    | POINTER_FLAG_INCONTACT,
                dwTime: dw_time,
                PerformanceCount: count,
                ptPixelLocationRaw: POINT { x, y: 220 },
                ..info()
            },
            ..Default::default()
        })
    }

    /// A 240 Hz burst inside one system tick: every reading carries the same
    /// `dwTime`, and only the performance count tells them apart. Native
    /// history is newest first; the contact went down at the burst's start.
    fn assembled_burst(clock: &MessageClock) -> flui_platform::pointer::PointerMove {
        let mut registry = ContactRegistry::new(clock.native());
        let start = timed_touch(5_016, 1_160_000, 130);
        assemble_history(
            &mut registry,
            &[start],
            WM_POINTERDOWN,
            POINT { x: -100, y: -200 },
            2.0,
            5_020,
            KeyboardModifiers::NONE,
        );
        let burst = [
            timed_touch(5_016, 1_285_001, 160),
            timed_touch(5_016, 1_243_334, 150),
            timed_touch(5_016, 1_201_667, 140),
            timed_touch(5_016, 1_160_000, 130),
        ];
        let events = assemble_history(
            &mut registry,
            &burst,
            WM_POINTERUPDATE,
            POINT { x: -100, y: -200 },
            2.0,
            5_020,
            KeyboardModifiers::NONE,
        );
        match events.as_slice() {
            [PlatformInput::Pointer(PointerEvent::Move(movement))] => movement.clone(),
            other => panic!("one batched move expected, got {other:?}"),
        }
    }

    fn coalesced_burst_is_ordered_by_performance_count() {
        let movement = assembled_burst(&performance_clock());
        assert_eq!(
            movement
                .coalesced()
                .iter()
                .map(|sample| sample.time.as_nanos())
                .collect::<Vec<_>>(),
            [1_016_000_000, 1_020_166_700, 1_024_333_400],
            "oldest first, at counter resolution"
        );
        assert_eq!(
            movement
                .coalesced()
                .iter()
                .map(|sample| sample.position.get().x)
                .collect::<Vec<_>>(),
            [15.0, 20.0, 25.0],
            "each time stays with its own reading"
        );
    }

    fn current_reading_is_stamped_from_its_performance_count() {
        let movement = assembled_burst(&performance_clock());
        assert_eq!(movement.current().time.as_nanos(), 1_028_500_100);
        assert_eq!(movement.current().position.get().x, 30.0);
    }

    fn reading_time(clock: &MessageClock, count: u64) -> u64 {
        sampled_on(clock, timed_touch(5_016, count, 0))
    }

    fn sampled_on(clock: &MessageClock, packet: PointerPacket) -> u64 {
        let mut registry = ContactRegistry::new(clock.native());
        let events = assemble_history(
            &mut registry,
            &[packet],
            WM_POINTERDOWN,
            POINT::default(),
            1.0,
            5_020,
            KeyboardModifiers::NONE,
        );
        events
            .iter()
            .find_map(|input| match input.as_pointer() {
                Some(PointerEvent::Down(press)) => Some(press.sample.time.as_nanos()),
                _ => None,
            })
            .expect("down")
    }

    fn reading_without_count_uses_system_tick() {
        assert_eq!(reading_time(&performance_clock(), 0), 1_016_000_000);
    }

    fn reading_without_counter_anchor_uses_system_tick() {
        let clock = MessageClock::anchored(5_000, None, 1_000_000_000);
        assert_eq!(reading_time(&clock, 1_160_000), 1_016_000_000);
    }

    fn count_before_the_epoch_precedes_it() {
        assert_eq!(reading_time(&performance_clock(), 990_000), 999_000_000);
    }

    fn count_past_the_epoch_range_saturates() {
        assert_eq!(reading_time(&performance_clock(), u64::MAX), u64::MAX);
    }

    #[test]
    fn native_pointer_decoding_contracts() {
        for case in [
            native_raw_position_and_identity_survive as fn(),
            native_pen_masks_preserve_sensor_presence,
            native_pen_position_keeps_himetric_precision,
            native_touch_contact_and_pressure_are_measured,
            invalid_native_sensor_and_scale_are_refused,
            native_terminal_identity_survives_callback_readmission,
            enter_after_native_down_is_not_another_button_press,
            native_enter_before_down_preserves_first_contact_admission,
            coalesced_burst_is_ordered_by_performance_count,
            current_reading_is_stamped_from_its_performance_count,
            reading_without_count_uses_system_tick,
            reading_without_counter_anchor_uses_system_tick,
            count_before_the_epoch_precedes_it,
            count_past_the_epoch_range_saturates,
        ] {
            case();
        }
    }
}

// ============================================================================
// Message-time modifiers
// ============================================================================
//
// The translation decisions for keys (vk -> Key fallback, scancode -> Code,
// the WM_CHAR merge) are pure functions in `crate::shared::keys`, where their
// tests execute on every host; this file only unpacks the Win32 message
// parameters and feeds them through. The WM_CHAR pairing model is documented
// on that module.

/// Whether `key` was down when the message being processed was generated.
fn key_down_in_queue(key: VIRTUAL_KEY) -> bool {
    // SAFETY: `GetKeyState` takes a plain virtual-key code and reads only the
    // calling thread's queue-synchronized key state; no pointers are involved.
    unsafe { GetKeyState(i32::from(key.0)) < 0 }
}

/// The keyboard modifiers held when the message being processed was
/// generated (see the module doc for why this is not `GetAsyncKeyState`).
pub(super) fn message_modifiers() -> KeyboardModifiers {
    let mut mods = KeyboardModifiers::NONE;
    for (key, modifier) in [
        (VK_SHIFT, KeyboardModifiers::SHIFT),
        (VK_CONTROL, KeyboardModifiers::CONTROL),
        (VK_MENU, KeyboardModifiers::ALT),
        (VK_LWIN, KeyboardModifiers::META),
        (VK_RWIN, KeyboardModifiers::META),
    ] {
        if key_down_in_queue(key) {
            mods |= modifier;
        }
    }
    mods
}

// ============================================================================
// Pointer Event Conversion (W3C ui-events 0.3 API)
// ============================================================================

/// The set of mouse buttons Win32 reports as held in a message's `WPARAM`.
///
/// Every mouse message (`WM_MOUSEMOVE`, `WM_*BUTTONDOWN`, `WM_*BUTTONUP`,
/// `WM_MOUSEWHEEL`) carries the `MK_*` mask in the low word of `WPARAM`, and
/// Win32 already applies the W3C rule for us: the bit for a button is set on
/// its DOWN message and clear on its UP message, i.e. the set held *after*
/// the event. That makes this stateless — unlike the winit backend, which has
/// to track the transition itself because winit does not surface a mask.
///
/// This matters beyond fidelity: the framework tells a drag-move from a hover
/// by asking whether any button is held. A move that always reports an empty
/// set is delivered as a hover, so no gesture recognizer ever sees it and the
/// drag is silently re-interpreted as a tap on release.
///
/// The bits are [`MOUSE_BUTTONS`]'.
#[inline]
fn held_buttons(wparam: WPARAM) -> PointerButtons {
    let mask = wparam.0 & 0xffff;
    MOUSE_BUTTONS
        .iter()
        .filter(|(_, bit, _)| mask & *bit as usize != 0)
        .fold(PointerButtons::NONE, |held, (_, _, button)| {
            held.with(*button)
        })
}

/// The button a button message reports and whether it is a press, or `None`
/// for any other message.
pub(super) fn button_message(msg: u32, wparam: WPARAM) -> Option<(PointerButton, bool)> {
    match msg {
        WM_LBUTTONDOWN => Some((PointerButton::PRIMARY, true)),
        WM_LBUTTONUP => Some((PointerButton::PRIMARY, false)),
        WM_RBUTTONDOWN => Some((PointerButton::SECONDARY, true)),
        WM_RBUTTONUP => Some((PointerButton::SECONDARY, false)),
        WM_MBUTTONDOWN => Some((PointerButton::AUXILIARY, true)),
        WM_MBUTTONUP => Some((PointerButton::AUXILIARY, false)),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let button = match (wparam.0 >> 16) & 0xffff {
                1 => PointerButton::BACK,
                2 => PointerButton::FORWARD,
                _ => return None,
            };
            Some((button, msg == WM_XBUTTONDOWN))
        }
        _ => None,
    }
}

/// Captures the mouse for `hwnd` on a button press, so the rest of the press
/// sequence — its moves and its release — reaches this window even when the
/// cursor leaves the client area. A press while the window already holds the
/// capture (a second button) keeps it.
pub(super) fn capture_on_press(hwnd: HWND) {
    // SAFETY: plain handle calls on the window's owner thread, where
    // `window_proc` runs; `SetCapture` captures for this thread's own window.
    unsafe {
        if GetCapture() != hwnd {
            SetCapture(hwnd);
        }
    }
}

/// Releases `hwnd`'s mouse capture after a button release that leaves no
/// button held (`wparam` is the release's `MK_*` mask, which no longer has
/// the released button). The release itself sends `WM_CAPTURECHANGED`, which
/// the `WM_CAPTURECHANGED` handler recognizes as the sequence's own end.
pub(super) fn release_capture_after(hwnd: HWND, wparam: WPARAM) {
    if !held_buttons(wparam).is_empty() {
        return;
    }
    // SAFETY: plain calls on the window's owner thread; the capture is only
    // released while this window holds it, never another window's.
    unsafe {
        if GetCapture() == hwnd
            && let Err(error) = ReleaseCapture()
        {
            tracing::warn!(%error, "ReleaseCapture failed after the last button release");
        }
    }
}

/// The pointer cancel for a `WM_CAPTURECHANGED` that took the capture from a
/// press sequence still in progress; nothing when the sequence ended on its
/// own.
///
/// `lparam` is the window gaining the capture. The sequence is in progress
/// while a button is still down in the queue-synchronized key state. After
/// the last release — the one that makes [`release_capture_after`] let go —
/// every button there is up. Another window, a modal loop or `WM_CANCELMODE`
/// taking the capture mid-drag leaves a button down, and without the cancel
/// this window would never see that sequence's release.
pub(super) fn capture_changed_input(
    hwnd: HWND,
    lparam: LPARAM,
    time: u64,
    registry: &RefCell<ContactRegistry>,
) -> Vec<PlatformInput> {
    let gaining = HWND(lparam.0 as *mut core::ffi::c_void);
    let held = MOUSE_BUTTONS
        .iter()
        .any(|(key, _, _)| key_down_in_queue(*key));
    if gaining == hwnd || !held {
        return Vec::new();
    }
    registry.borrow_mut().cancel_contact(
        SURFACE,
        MOUSE_CONTACT,
        EventTime::from_nanos(time),
        NativeCancel::CaptureLost,
    )
}

/// A legacy `WM_MOUSE*` message as a reading of the primary mouse.
fn legacy_mouse_reading(
    action: ReadingAction,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> NativeReading {
    NativeReading::new(
        MOUSE_CONTACT,
        PointerKind::Mouse,
        action,
        ReadingTime::epoch(EventTime::from_nanos(time)),
    )
    .on(SURFACE)
    .at(
        f64::from(get_x_lparam(lparam)),
        f64::from(get_y_lparam(lparam)),
        scale_factor,
    )
    .with_primary(true)
    .with_buttons(held_buttons(wparam))
    .with_modifiers(message_modifiers())
}

/// Assemble a `WM_[LRMX]BUTTON*` (see [`button_message`]).
pub(super) fn mouse_button_input(
    registry: &RefCell<ContactRegistry>,
    button: PointerButton,
    is_down: bool,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> Assembled {
    let action = if is_down {
        ReadingAction::Press(button)
    } else {
        ReadingAction::Release(button)
    };
    let reading = legacy_mouse_reading(action, wparam, lparam, scale_factor, time);
    decode(&mut registry.borrow_mut(), &reading).unwrap_or_else(DecodeError::into_inputs)
}

/// Assemble a `WM_MOUSEMOVE`.
pub(super) fn mouse_move_input(
    registry: &RefCell<ContactRegistry>,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> Assembled {
    let reading = legacy_mouse_reading(ReadingAction::Move, wparam, lparam, scale_factor, time);
    decode(&mut registry.borrow_mut(), &reading).unwrap_or_else(DecodeError::into_inputs)
}

/// The signed scroll distance both wheel messages carry in the high word of
/// `wParam` (`GET_WHEEL_DELTA_WPARAM`), in multiples of `WHEEL_DELTA` (120).
fn wheel_distance(wparam: WPARAM) -> i16 {
    ((wparam.0 as i32) >> 16) as i16
}

/// A wheel message as a [`WheelReading`], assembled through `registry`.
///
/// Unlike every other client-area mouse message, `WM_MOUSEWHEEL` and
/// `WM_MOUSEHWHEEL` deliver the cursor position in SCREEN coordinates
/// (both messages' `lParam` docs:
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel>),
/// so the point is converted to client space here before the registry
/// DPI-scales it — otherwise scroll hit-testing targets the wrong child
/// whenever the window's client origin is not the desktop origin.
///
/// Win32's vertical sign (positive = wheel rotated away from the user) is the
/// inverse of the cross-backend convention — positive = content scrolls down
/// — and its horizontal sign (positive = tilted right) already matches it;
/// `crate::shared::scroll` holds the sign and unit table with citations.
pub(super) fn mouse_wheel_input(
    registry: &RefCell<ContactRegistry>,
    hwnd: HWND,
    axis: WheelAxis,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> Option<PlatformInput> {
    let mut point = POINT {
        x: get_x_lparam(lparam),
        y: get_y_lparam(lparam),
    };
    // SAFETY: `point` is a live, writable local; `ScreenToClient` writes
    // nothing else. On failure (invalid `hwnd`) it returns FALSE and leaves
    // `point` unchanged.
    let converted = unsafe { ScreenToClient(hwnd, &raw mut point) };
    if !converted.as_bool() {
        return None;
    }
    let reading = WheelReading {
        axis,
        distance: wheel_distance(wparam),
        position: (f64::from(point.x), f64::from(point.y)),
        scale: scale_factor,
        time: EventTime::from_nanos(time),
        modifiers: message_modifiers(),
    };
    registry.borrow().wheel(&reading)
}

// ============================================================================
// Keyboard events (simple wrappers)
// ============================================================================

/// Convert WM_KEYDOWN to W3C KeyboardEvent.
///
/// `translated_text` is the drained `WM_CHAR` burst for this keydown (see
/// `window_proc`'s `WM_KEYDOWN` arm and `crate::shared::keys`'s module doc
/// for the pairing model); when present and typeable it becomes the event's
/// `Key::Character`, otherwise the layout-independent virtual-key fallback
/// applies. `time` is the message's generation time on the shared epoch
/// ([`MessageClock::message_time`]), so a key and a click queued around it
/// keep their order.
pub fn key_down_event(
    wparam: WPARAM,
    lparam: LPARAM,
    stroke: Keystroke,
    held_dead: &mut HeldDeadKeys,
    time: u64,
) -> PlatformInput {
    let vk = wparam.0 as u16;
    let (scan_code, extended, is_repeat) = keys::parse_key_lparam(lparam.0);
    let (dead, translated_text) = match stroke {
        Keystroke::Dead => (true, None),
        Keystroke::Text(text) => (false, text),
    };
    // A press is also proof the key was released before: a dead press held
    // across a focus change whose release went to another window is forgotten
    // here, when the same physical key goes down again. A key that stays held
    // across a transient focus loss keeps its identity for its release.
    if dead {
        held_dead.press(scan_code, extended);
    } else if !is_repeat {
        held_dead.release(scan_code, extended);
    }

    let modifiers = message_modifiers();
    let fallback = keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT));
    let code = keys::scancode_to_code(scan_code, extended);
    // A dead key with nothing typed yet is reported as one, not as its
    // unshifted fallback character: otherwise a shortcut bound to that
    // character fires while the user composes an accented letter. Its second
    // press (or a space after it) types the accent itself and arrives as text.
    let key = if dead {
        Key::Named(NamedKey::Dead)
    } else {
        keys::key_for_keydown(
            fallback,
            translated_text,
            modifiers.contains(KeyboardModifiers::ALT),
            modifiers.contains(KeyboardModifiers::CONTROL),
            code,
        )
    };

    PlatformInput::Keyboard(
        KeyEvent::new(KeyState::Down, key, code, EventTime::from_nanos(time))
            .with_location(keys::location_for_code(code))
            .with_modifiers(modifiers)
            .with_repeat(if is_repeat {
                KeyRepeat::AutoRepeat
            } else {
                KeyRepeat::First
            }),
    )
}

/// Convert WM_KEYUP to W3C KeyboardEvent.
///
/// No `WM_CHAR` pairs with a keyup, so the key is always the virtual-key
/// fallback — for a letter that is its shift-respecting character, for OEM
/// punctuation the US-layout position. Consumers that type text act on
/// `KeyState::Down` only, so this asymmetry never reaches a text field.
/// `time` is the message's, as for [`key_down_event`].
pub fn key_up_event(
    wparam: WPARAM,
    lparam: LPARAM,
    held_dead: &mut HeldDeadKeys,
    time: u64,
) -> PlatformInput {
    let vk = wparam.0 as u16;
    let (scan_code, extended, _) = keys::parse_key_lparam(lparam.0);

    let modifiers = message_modifiers();
    // The release of a dead key is `Dead` too (W3C UI Events dead-key
    // sequence), so a pressed-key set sees the same identity go down and up.
    // The press decided it; the layout may have changed while the key was held.
    let key = if held_dead.release(scan_code, extended) {
        Key::Named(NamedKey::Dead)
    } else {
        keys::vk_to_key(vk, modifiers.contains(KeyboardModifiers::SHIFT))
    };
    let code = keys::scancode_to_code(scan_code, extended);

    PlatformInput::Keyboard(
        KeyEvent::new(KeyState::Up, key, code, EventTime::from_nanos(time))
            .with_location(keys::location_for_code(code))
            .with_modifiers(modifiers),
    )
}

/// Build the KeyboardEvent for an out-of-band `WM_CHAR` — one that reached
/// `window_proc` instead of being drained by a `WM_KEYDOWN` (Alt+numpad
/// composition, or a directly-sent message). There is no owning physical
/// key, so it is dispatched as a key-down with `Code::Unidentified` and no
/// paired key-up; the text lane consumes `Key::Character` on key-down only.
/// `time` is the message's, as for [`key_down_event`].
pub fn stray_char_event(text: String, time: u64) -> PlatformInput {
    let modifiers = message_modifiers();

    PlatformInput::Keyboard(
        KeyEvent::new(
            KeyState::Down,
            Key::character(text),
            Code::Unidentified,
            EventTime::from_nanos(time),
        )
        .with_location(Location::Standard)
        .with_modifiers(modifiers),
    )
}

/// What `TranslateMessage` produced for one keydown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keystroke {
    /// The drained `WM_CHAR` burst, or `None` for a key with no typeable
    /// translation (navigation keys, Ctrl chords).
    Text(Option<String>),
    /// A dead key: `TranslateMessage` queued `WM_DEADCHAR` (or
    /// `WM_SYSDEADCHAR`) for it on the layer the held modifiers select.
    Dead,
}

/// The physical keys currently held down that were pressed as dead keys, so
/// each one's release reports `Dead` like its press, even if the keyboard
/// layout or the modifiers changed while it was held. Owned per window.
#[derive(Debug, Default)]
pub struct HeldDeadKeys(Vec<(u16, bool)>);

impl HeldDeadKeys {
    fn press(&mut self, scan_code: u16, extended: bool) {
        if !self.0.contains(&(scan_code, extended)) {
            self.0.push((scan_code, extended));
        }
    }

    /// Whether the released key was pressed as a dead key; forgets it.
    fn release(&mut self, scan_code: u16, extended: bool) -> bool {
        let before = self.0.len();
        self.0.retain(|held| *held != (scan_code, extended));
        self.0.len() != before
    }
}
