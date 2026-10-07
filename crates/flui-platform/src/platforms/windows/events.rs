//! Win32 message conversion to W3C ui-events (0.3 API).
//!
//! `window_proc` unpacks every mouse and keyboard message through these
//! functions:
//!
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
//!   ([`capture_changed_event`]).
//! - Mouse samples retain the retrieved message's generation time, rebased
//!   onto the shared monotonic epoch before any reentrant callbacks run.

use dpi::{PhysicalPosition, PhysicalSize};
use keyboard_types::{Key, Modifiers as KeyboardModifiers, NamedKey};
use ui_events::{
    keyboard::{Code, KeyState, KeyboardEvent, Location},
    pointer::{
        PointerButton, PointerButtonEvent, PointerButtons, PointerEvent, PointerOrientation,
        PointerState, PointerUpdate,
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, WPARAM},
    Graphics::Gdi::ScreenToClient,
    System::SystemInformation::GetTickCount64,
    UI::{
        Input::KeyboardAndMouse::{
            GetCapture, GetKeyState, ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_CONTROL,
            VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU, VK_RBUTTON, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::{
            GetMessageTime, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
            WM_RBUTTONDOWN, WM_RBUTTONUP,
        },
    },
};

/// Rebases the queue's wrapping millisecond timestamp onto the shared epoch.
/// The owner snapshots a sample before callback-capable native or user calls.
pub(super) struct MessageClock {
    uptime_ms: u64,
    epoch_ns: u64,
    last_ns: std::cell::Cell<u64>,
}

impl MessageClock {
    pub(super) fn new() -> Self {
        // SAFETY: argument-free monotonic system uptime query.
        let uptime_ms = unsafe { GetTickCount64() };
        Self::anchored(uptime_ms, event_timestamp_ns())
    }

    fn anchored(uptime_ms: u64, epoch_ns: u64) -> Self {
        Self {
            uptime_ms,
            epoch_ns,
            last_ns: std::cell::Cell::new(0),
        }
    }

    pub(super) fn message_time(&self) -> u64 {
        // SAFETY: argument-free queries of the owner thread's retrieved
        // message and system uptime. No callback can run between them.
        let (tick, now) = unsafe { (GetMessageTime() as u32, GetTickCount64()) };
        self.stamp_at(tick, now)
    }

    fn stamp_at(&self, tick: u32, now_ms: u64) -> u64 {
        // GetMessageTime carries the low 32 bits of uptime. Subtract in
        // that domain before extending, including across its 49.7-day wrap.
        // A message older than one complete wrap cannot be distinguished.
        let age_ms = (now_ms as u32).wrapping_sub(tick);
        let sample_ms = now_ms.saturating_sub(u64::from(age_ms));
        let stamp = if sample_ms >= self.uptime_ms {
            self.epoch_ns.saturating_add(
                sample_ms
                    .saturating_sub(self.uptime_ms)
                    .saturating_mul(1_000_000),
            )
        } else {
            self.epoch_ns.saturating_sub(
                self.uptime_ms
                    .saturating_sub(sample_ms)
                    .saturating_mul(1_000_000),
            )
        };
        // Sent messages can inherit an older retrieved-message timestamp;
        // nested dispatch must not move this window's clock backwards.
        let stamp = stamp.max(self.last_ns.get());
        self.last_ns.set(stamp);
        stamp
    }
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
        ] {
            let clock = MessageClock::anchored(anchor_tick, anchor_ns);
            assert_eq!(
                samples.map(|tick| clock.stamp_at(tick, now)),
                expected,
                "{name}"
            );
        }
        let first = MessageClock::anchored(100, 1_000_000);
        let second = MessageClock::anchored(120, 21_000_000);
        assert_eq!(
            first.stamp_at(150, 200),
            second.stamp_at(150, 200),
            "independent windows share the process epoch"
        );
    }
}

use super::util::{get_x_lparam, get_y_lparam};
use crate::{
    shared::events::{event_timestamp_ns, primary_mouse_info},
    shared::{
        input_vocabulary::{keyboard_input, pointer_input},
        keys,
    },
    traits::{PlatformInput, device_to_logical},
};

/// Snapshot the OS query before committing state or calling presentation code.
#[derive(Clone, Copy)]
enum NativeReading {
    Pointer(windows::Win32::UI::Input::Pointer::POINTER_INFO),
    Pen(windows::Win32::UI::Input::Pointer::POINTER_PEN_INFO),
    Touch(windows::Win32::UI::Input::Pointer::POINTER_TOUCH_INFO),
}

struct DecodedPointer {
    kind: flui_platform_api::pointer::PointerKind,
    role: flui_platform_api::pointer::PointerRole,
    device: Option<flui_platform_api::pointer::DeviceId>,
    sample: flui_platform_api::pointer::PointerSample,
}

fn decode_native_reading(
    _reading: NativeReading,
    _client_offset: POINT,
    _scale: f64,
    _time: flui_platform_api::EventTime,
) -> Option<DecodedPointer> {
    None
}

#[derive(Default)]
pub(super) struct NativePointerRegistry {
    next_contact: u64,
    contacts: std::collections::BTreeMap<u32, flui_platform_api::pointer::PointerInfo>,
}

impl NativePointerRegistry {
    fn commit(
        &mut self,
        _native: &windows::Win32::UI::Input::Pointer::POINTER_INFO,
        _decoded: DecodedPointer,
        _message: u32,
    ) -> Vec<PlatformInput> {
        Vec::new()
    }

    fn cancel(
        &mut self,
        _raw: u32,
        _time: flui_platform_api::EventTime,
        _reason: flui_platform_api::pointer::CancelReason,
    ) -> Vec<PlatformInput> {
        Vec::new()
    }

    fn remove_device(
        &mut self,
        _device: flui_platform_api::pointer::DeviceId,
        _time: flui_platform_api::EventTime,
    ) -> Vec<PlatformInput> {
        Vec::new()
    }
}

#[cfg(test)]
mod native_pointer_contracts {
    use super::*;
    use flui_platform_api::{
        EventTime,
        pointer::{PenTool, PointerKind, PointerRole},
    };
    use windows::Win32::{
        Foundation::HANDLE,
        UI::{
            Input::Pointer::{
                POINTER_FLAG_PRIMARY, POINTER_INFO, POINTER_PEN_INFO, POINTER_TOUCH_INFO,
            },
            WindowsAndMessaging::{
                PEN_FLAG_ERASER, PEN_MASK_PRESSURE, PEN_MASK_ROTATION, PEN_MASK_TILT_X,
                PEN_MASK_TILT_Y, PT_PEN, PT_TOUCH, TOUCH_MASK_CONTACTAREA, TOUCH_MASK_PRESSURE,
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
    fn decoded(reading: NativeReading) -> DecodedPointer {
        decode_native_reading(
            reading,
            POINT { x: -100, y: -200 },
            2.0,
            EventTime::from_nanos(0),
        )
        .expect("valid native reading must be delivered")
    }
    fn native_raw_position_and_identity_survive() {
        let packet = decoded(NativeReading::Pointer(info()));
        assert_eq!(
            packet.sample.position.get(),
            flui_foundation::geometry::Point::new(10.0, 10.0)
        );
        assert_eq!(packet.sample.time.as_nanos(), 0);
        assert_eq!(packet.device.map(|device| device.get().get()), Some(0x3450));
        assert_eq!(packet.role, PointerRole::Primary);
        assert_eq!(packet.sample.pressure, None);
        let mut secondary = info();
        secondary.pointerFlags = Default::default();
        secondary.sourceDevice = HANDLE::default();
        let packet = decoded(NativeReading::Pointer(secondary));
        assert_eq!(packet.role, PointerRole::Additional);
        assert_eq!(
            packet.device, None,
            "missing hardware identity stays absent"
        );
    }
    fn native_pen_masks_preserve_sensor_presence() {
        let pen = POINTER_PEN_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_PEN,
                ..info()
            },
            pressure: 1024,
            rotation: 180,
            tiltX: 45,
            tiltY: 0,
            ..Default::default()
        };
        let missing = decoded(NativeReading::Pen(pen));
        assert_eq!(missing.kind, PointerKind::Pen { tool: PenTool::Tip });
        assert_eq!(missing.sample.pressure, None);
        assert_eq!(missing.sample.orientation, None);
        assert_eq!(missing.sample.twist, None);
        let present = decoded(NativeReading::Pen(POINTER_PEN_INFO {
            penFlags: PEN_FLAG_ERASER,
            penMask: PEN_MASK_PRESSURE | PEN_MASK_ROTATION | PEN_MASK_TILT_X | PEN_MASK_TILT_Y,
            ..pen
        }));
        assert_eq!(
            present.kind,
            PointerKind::Pen {
                tool: PenTool::Eraser
            }
        );
        assert_eq!(
            present.sample.pressure.map(|pressure| pressure.get()),
            Some(1.0)
        );
        let orientation = present.sample.orientation.expect("both native tilt axes");
        assert!(
            (orientation.altitude().expect("altitude") - std::f64::consts::FRAC_PI_4).abs() < 1e-12
        );
        assert_eq!(orientation.azimuth(), Some(0.0));
        assert!(
            (present.sample.twist.expect("reported rotation").radians() - std::f64::consts::PI)
                .abs()
                < 1e-12
        );
        let partial = decoded(NativeReading::Pen(POINTER_PEN_INFO {
            penMask: PEN_MASK_TILT_X,
            ..pen
        }));
        assert_eq!(
            partial.sample.orientation, None,
            "one tilt axis does not determine a pen orientation"
        );
    }
    fn native_touch_contact_and_pressure_are_measured() {
        let touch = POINTER_TOUCH_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_TOUCH,
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
        let packet = decoded(NativeReading::Touch(touch));
        assert_eq!(packet.kind, PointerKind::Touch);
        assert_eq!(
            packet.sample.pressure.map(|pressure| pressure.get()),
            Some(0.5)
        );
        assert_eq!(
            packet.sample.contact_size.expect("reported contact").get(),
            flui_foundation::geometry::Size::new(20.0, 30.0)
        );
        let absent = decoded(NativeReading::Touch(POINTER_TOUCH_INFO {
            touchMask: 0,
            ..touch
        }));
        assert_eq!(absent.sample.pressure, None);
        assert_eq!(absent.sample.contact_size, None);
    }
    fn invalid_native_sensor_and_scale_are_refused() {
        for (pressure, scale) in [(1025, 1.0), (0, 0.0), (0, f64::NAN)] {
            let pen = NativeReading::Pen(POINTER_PEN_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_PEN,
                    ..info()
                },
                penMask: PEN_MASK_PRESSURE,
                pressure,
                ..Default::default()
            });
            assert!(
                decode_native_reading(pen, POINT::default(), scale, EventTime::from_nanos(1))
                    .is_none()
            );
        }
        let packet = decoded(NativeReading::Pen(POINTER_PEN_INFO {
            pointerInfo: POINTER_INFO {
                pointerType: PT_PEN,
                ..info()
            },
            penMask: PEN_MASK_PRESSURE,
            pressure: 0,
            ..Default::default()
        }));
        assert_eq!(
            packet.sample.pressure.map(|pressure| pressure.get()),
            Some(0.0),
            "reported zero is a sensor reading"
        );
    }

    fn packet(device: u64, time: u64) -> DecodedPointer {
        use flui_platform_api::pointer::{DeviceId, PointerPosition, PointerSample};
        DecodedPointer {
            kind: PointerKind::Touch,
            role: PointerRole::Additional,
            device: Some(DeviceId::try_from(device).expect("nonzero device")),
            sample: PointerSample::new(
                EventTime::from_nanos(time),
                PointerPosition::try_new(flui_foundation::geometry::Point::new(10.0, 20.0))
                    .expect("finite native geometry"),
            ),
        }
    }
    fn down(
        registry: &mut NativePointerRegistry,
        raw: u32,
        device: u64,
        time: u64,
    ) -> flui_platform_api::pointer::PointerInfo {
        use flui_platform_api::pointer::PointerEvent;
        use windows::Win32::UI::{
            Input::Pointer::{POINTER_FLAG_DOWN, POINTER_FLAG_FIRSTBUTTON, POINTER_FLAG_INCONTACT},
            WindowsAndMessaging::WM_POINTERDOWN,
        };
        let native = POINTER_INFO {
            pointerId: raw,
            pointerFlags: POINTER_FLAG_DOWN | POINTER_FLAG_FIRSTBUTTON | POINTER_FLAG_INCONTACT,
            ..info()
        };
        registry
            .commit(&native, packet(device, time), WM_POINTERDOWN)
            .into_iter()
            .find_map(|input| match input {
                PlatformInput::Pointer(PointerEvent::Down(press)) => Some(press.pointer),
                _ => None,
            })
            .expect("native contact must be admitted")
    }
    fn native_contacts_survive_independent_device_removal() {
        use flui_platform_api::pointer::{CancelReason, DeviceId, PointerEvent};
        let mut registry = NativePointerRegistry::default();
        let first = down(&mut registry, 7, 0x3450, 1);
        let second = down(&mut registry, 8, 0x4560, 2);
        assert_ne!(first.id, second.id);
        let removed = registry.remove_device(
            DeviceId::try_from(0x3450_u64).expect("device"),
            EventTime::from_nanos(3),
        );
        let cancelled: Vec<_> = removed
            .iter()
            .filter_map(|input| match input {
                PlatformInput::Pointer(PointerEvent::Cancel(cancel)) => Some(cancel),
                _ => None,
            })
            .collect();
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].pointer, first);
        assert_eq!(cancelled[0].reason, CancelReason::DeviceRemoved);
        let surviving = registry.cancel(8, EventTime::from_nanos(4), CancelReason::CaptureLost);
        assert!(surviving.iter().any(|input| matches!(input, PlatformInput::Pointer(PointerEvent::Cancel(cancel)) if cancel.pointer == second && cancel.reason == CancelReason::CaptureLost)));
        assert!(
            registry
                .cancel(7, EventTime::from_nanos(5), CancelReason::CaptureLost)
                .is_empty()
        );
    }
    fn native_terminal_identity_survives_callback_readmission() {
        use flui_platform_api::pointer::PointerEvent;
        use windows::Win32::UI::{
            Input::Pointer::POINTER_FLAG_UP, WindowsAndMessaging::WM_POINTERUP,
        };
        let mut registry = NativePointerRegistry::default();
        let old = down(&mut registry, 7, 0x3450, 1);
        let terminal = registry.commit(
            &POINTER_INFO {
                pointerId: 7,
                pointerFlags: POINTER_FLAG_UP,
                ..info()
            },
            packet(0x3450, 2),
            WM_POINTERUP,
        );
        // Presentation delivery can re-admit the native ID after retirement.
        let fresh = down(&mut registry, 7, 0x3450, 3);
        assert_ne!(old.id, fresh.id);
        assert!(terminal.iter().any(|input| matches!(input, PlatformInput::Pointer(PointerEvent::Up(release)) if release.pointer == old)));
        let cancelled = registry.cancel(
            7,
            EventTime::from_nanos(4),
            flui_platform_api::pointer::CancelReason::Platform,
        );
        assert!(cancelled.iter().any(|input| matches!(input, PlatformInput::Pointer(PointerEvent::Cancel(cancel)) if cancel.pointer == fresh)));
    }
    fn exhausted_native_identity_never_wraps() {
        use flui_platform_api::pointer::PointerEvent;
        use windows::Win32::UI::WindowsAndMessaging::WM_POINTERDOWN;
        let mut registry = NativePointerRegistry {
            next_contact: u64::MAX - 1,
            ..Default::default()
        };
        let final_contact = down(&mut registry, 7, 0x3450, 1);
        assert_eq!(final_contact.id.get().get(), u64::MAX);
        for raw in [8, 9] {
            let native = POINTER_INFO {
                pointerId: raw,
                ..info()
            };
            assert!(
                !registry
                    .commit(&native, packet(0x3450, 2), WM_POINTERDOWN)
                    .iter()
                    .any(|input| matches!(input, PlatformInput::Pointer(PointerEvent::Down(_))))
            );
        }
        registry.cancel(
            7,
            EventTime::from_nanos(3),
            flui_platform_api::pointer::CancelReason::Platform,
        );
        assert!(
            !registry
                .commit(
                    &POINTER_INFO {
                        pointerId: 10,
                        ..info()
                    },
                    packet(0x3450, 4),
                    WM_POINTERDOWN
                )
                .iter()
                .any(|input| matches!(input, PlatformInput::Pointer(PointerEvent::Down(_))))
        );
    }
    #[test]
    fn native_pointer_decoding_contracts() {
        for case in [
            native_raw_position_and_identity_survive as fn(),
            native_pen_masks_preserve_sensor_presence,
            native_touch_contact_and_pressure_are_measured,
            invalid_native_sensor_and_scale_are_refused,
            native_contacts_survive_independent_device_removal,
            native_terminal_identity_survives_callback_readmission,
            exhausted_native_identity_never_wraps,
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
    let mut mods = KeyboardModifiers::empty();
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
/// `MK_LBUTTON`/`MK_RBUTTON`/`MK_MBUTTON` are `0x0001`/`0x0002`/`0x0010`
/// (`winuser.h`); the X buttons have no `PointerButton` mapping here.
#[inline]
fn held_buttons(wparam: WPARAM) -> PointerButtons {
    const MK_LBUTTON: usize = 0x0001;
    const MK_RBUTTON: usize = 0x0002;
    const MK_MBUTTON: usize = 0x0010;

    let mask = wparam.0 & 0xffff;
    let mut buttons = PointerButtons::default();
    if mask & MK_LBUTTON != 0 {
        buttons.insert(PointerButton::Primary);
    }
    if mask & MK_RBUTTON != 0 {
        buttons.insert(PointerButton::Secondary);
    }
    if mask & MK_MBUTTON != 0 {
        buttons.insert(PointerButton::Auxiliary);
    }
    buttons
}

/// The button a button message reports and whether it is a press, or `None`
/// for any other message.
pub(super) fn button_message(msg: u32) -> Option<(PointerButton, bool)> {
    match msg {
        WM_LBUTTONDOWN => Some((PointerButton::Primary, true)),
        WM_LBUTTONUP => Some((PointerButton::Primary, false)),
        WM_RBUTTONDOWN => Some((PointerButton::Secondary, true)),
        WM_RBUTTONUP => Some((PointerButton::Secondary, false)),
        WM_MBUTTONDOWN => Some((PointerButton::Auxiliary, true)),
        WM_MBUTTONUP => Some((PointerButton::Auxiliary, false)),
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
/// [`capture_changed_event`] recognizes as the sequence's own end.
pub(super) fn release_capture_after(hwnd: HWND, wparam: WPARAM) {
    if held_buttons(wparam) != PointerButtons::default() {
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
/// press sequence still in progress, or `None` when the sequence ended on its
/// own.
///
/// `lparam` is the window gaining the capture. The sequence is in progress
/// while a button is still down in the queue-synchronized key state. After
/// the last release — the one that makes [`release_capture_after`] let go —
/// every button there is up. Another window, a modal loop or `WM_CANCELMODE`
/// taking the capture mid-drag leaves a button down, and without the cancel
/// this window would never see that sequence's release.
pub(super) fn capture_changed_event(
    hwnd: HWND,
    lparam: LPARAM,
    time: u64,
) -> Option<PlatformInput> {
    let gaining = HWND(lparam.0 as *mut core::ffi::c_void);
    let held = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
        .into_iter()
        .any(key_down_in_queue);
    (gaining != hwnd && held)
        .then(|| pointer_input(PointerEvent::Cancel(primary_mouse_info()), time))
        .flatten()
}

/// Build a `PointerState` from LPARAM coordinates and scale factor.
///
/// `buttons` is the held set from [`held_buttons`]; it is a required argument
/// rather than a defaulted field so a new message arm cannot silently ship an
/// empty set.
#[inline]
fn pointer_state(
    lparam: LPARAM,
    scale_factor: f64,
    pressure: f64,
    buttons: PointerButtons,
    count: u8,
    time: u64,
) -> PointerState {
    pointer_state_at(
        get_x_lparam(lparam),
        get_y_lparam(lparam),
        scale_factor,
        pressure,
        buttons,
        count,
        time,
    )
}

/// [`pointer_state`] with the CLIENT-space device coordinates already in
/// hand — for the wheel messages, whose `lParam` needs a screen-to-client
/// conversion first (see [`wheel_pointer_state`]).
///
/// `count` is the W3C click count — `1` on Down/Up transitions, `0`
/// elsewhere (the cross-wire contract in flui-interaction's module doc).
#[inline]
fn pointer_state_at(
    x: i32,
    y: i32,
    scale_factor: f64,
    pressure: f64,
    buttons: PointerButtons,
    count: u8,
    time: u64,
) -> PointerState {
    let logical_x = device_to_logical(x as f64, scale_factor);
    let logical_y = device_to_logical(y as f64, scale_factor);

    PointerState {
        time,
        position: PhysicalPosition::new(logical_x as f64, logical_y as f64),
        buttons,
        modifiers: message_modifiers(),
        count,
        contact_geometry: PhysicalSize::new(1.0, 1.0),
        orientation: PointerOrientation::default(),
        pressure: pressure as f32,
        tangential_pressure: 0.0,
        scale_factor,
    }
}

/// Convert a `WM_[LRM]BUTTONDOWN`/`UP` (see [`button_message`]) to a W3C
/// pointer Down/Up.
pub fn mouse_button_event(
    button: PointerButton,
    is_down: bool,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> Option<PlatformInput> {
    let state = pointer_state(
        lparam,
        scale_factor,
        if is_down { 0.5 } else { 0.0 },
        held_buttons(wparam),
        1,
        time,
    );
    let event = PointerButtonEvent {
        pointer: primary_mouse_info(),
        state,
        button: Some(button),
    };
    pointer_input(
        if is_down {
            PointerEvent::Down(event)
        } else {
            PointerEvent::Up(event)
        },
        time,
    )
}

/// Convert WM_MOUSEMOVE to a W3C pointer Move.
pub fn mouse_move_event(
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> Option<PlatformInput> {
    let held = held_buttons(wparam);
    // Sensor-less pressure rule: 0.5 while any button is held (a drag),
    // 0.0 on a hover.
    let pressure = if held == PointerButtons::default() {
        0.0
    } else {
        0.5
    };
    pointer_input(
        PointerEvent::Move(PointerUpdate {
            pointer: primary_mouse_info(),
            current: pointer_state(lparam, scale_factor, pressure, held, 0, time),
            coalesced: Vec::new(),
            predicted: Vec::new(),
        }),
        time,
    )
}

/// The signed scroll distance both wheel messages carry in the high word of
/// `wParam` (`GET_WHEEL_DELTA_WPARAM`), in multiples of `WHEEL_DELTA` (120).
fn wheel_distance(wparam: WPARAM) -> i16 {
    ((wparam.0 as i32) >> 16) as i16
}

/// Build the pointer state for a wheel message.
///
/// Unlike every other client-area mouse message, `WM_MOUSEWHEEL` and
/// `WM_MOUSEHWHEEL` deliver the cursor position in SCREEN coordinates
/// (both messages' `lParam` docs:
/// <https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mousewheel>),
/// so the point is converted to client space here before the shared
/// [`pointer_state_at`] path DPI-scales it — otherwise scroll hit-testing
/// targets the wrong child whenever the window's client origin is not the
/// desktop origin.
fn wheel_pointer_state(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> PointerState {
    let mut point = POINT {
        x: get_x_lparam(lparam),
        y: get_y_lparam(lparam),
    };
    // SAFETY: `point` is a live, writable local; `ScreenToClient` writes
    // nothing else. On failure (invalid `hwnd`) it returns FALSE and leaves
    // `point` unchanged.
    let converted = unsafe { ScreenToClient(hwnd, &raw mut point) };
    if !converted.as_bool() {
        tracing::warn!(
            "ScreenToClient failed for a wheel message; scroll position stays in screen space"
        );
    }
    pointer_state_at(
        point.x,
        point.y,
        scale_factor,
        0.0,
        held_buttons(wparam),
        0,
        time,
    )
}

/// Convert WM_MOUSEWHEEL to a W3C pointer Scroll.
///
/// Win32's vertical sign (positive = wheel rotated away from the user) is the
/// inverse of the cross-backend convention — positive = content scrolls down —
/// so `from_win32_wheel` negates it at this boundary; see
/// `crate::shared::scroll` for the sign/unit table and citations. The cursor
/// position arrives in screen coordinates; see [`wheel_pointer_state`].
pub fn mouse_wheel_event(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> Option<PlatformInput> {
    pointer_input(
        PointerEvent::Scroll(ui_events::pointer::PointerScrollEvent {
            pointer: primary_mouse_info(),
            state: wheel_pointer_state(hwnd, wparam, lparam, scale_factor, time),
            delta: crate::shared::scroll::from_win32_wheel(wheel_distance(wparam)),
        }),
        time,
    )
}

/// Convert WM_MOUSEHWHEEL to a W3C pointer Scroll.
///
/// Win32's horizontal sign (positive = wheel tilted right) already matches
/// the cross-backend convention — positive = content scrolls right — so only
/// the `WHEEL_DELTA` division applies; see `crate::shared::scroll`. The
/// cursor position arrives in screen coordinates; see
/// [`wheel_pointer_state`].
pub fn mouse_hwheel_event(
    hwnd: HWND,
    wparam: WPARAM,
    lparam: LPARAM,
    scale_factor: f64,
    time: u64,
) -> Option<PlatformInput> {
    pointer_input(
        PointerEvent::Scroll(ui_events::pointer::PointerScrollEvent {
            pointer: primary_mouse_info(),
            state: wheel_pointer_state(hwnd, wparam, lparam, scale_factor, time),
            delta: crate::shared::scroll::from_win32_hwheel(wheel_distance(wparam)),
        }),
        time,
    )
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
/// applies.
pub fn key_down_event(
    wparam: WPARAM,
    lparam: LPARAM,
    stroke: Keystroke,
    held_dead: &mut HeldDeadKeys,
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

    keyboard_input(
        KeyboardEvent {
            state: KeyState::Down,
            key,
            code,
            location: keys::location_for_code(code),
            modifiers,
            repeat: is_repeat,
            is_composing: false,
        },
        event_timestamp_ns(),
    )
}

/// Convert WM_KEYUP to W3C KeyboardEvent.
///
/// No `WM_CHAR` pairs with a keyup, so the key is always the virtual-key
/// fallback — for a letter that is its shift-respecting character, for OEM
/// punctuation the US-layout position. Consumers that type text act on
/// `KeyState::Down` only, so this asymmetry never reaches a text field.
pub fn key_up_event(wparam: WPARAM, lparam: LPARAM, held_dead: &mut HeldDeadKeys) -> PlatformInput {
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

    keyboard_input(
        KeyboardEvent {
            state: KeyState::Up,
            key,
            code,
            location: keys::location_for_code(code),
            modifiers,
            repeat: false,
            is_composing: false,
        },
        event_timestamp_ns(),
    )
}

/// Build the KeyboardEvent for an out-of-band `WM_CHAR` — one that reached
/// `window_proc` instead of being drained by a `WM_KEYDOWN` (Alt+numpad
/// composition, or a directly-sent message). There is no owning physical
/// key, so it is dispatched as a key-down with `Code::Unidentified` and no
/// paired key-up; the text lane consumes `Key::Character` on key-down only.
pub fn stray_char_event(text: String) -> PlatformInput {
    let modifiers = message_modifiers();

    keyboard_input(
        KeyboardEvent {
            state: KeyState::Down,
            key: Key::Character(text),
            code: Code::Unidentified,
            location: Location::Standard,
            modifiers,
            repeat: false,
            is_composing: false,
        },
        event_timestamp_ns(),
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
