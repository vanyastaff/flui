use flui_platform::EventTime;
use flui_platform::PlatformInput;
use flui_platform::input_capabilities::PointerKindSet;
use flui_platform::pointer::{
    CancelReason, PenTool, PointerButton, PointerEvent, PointerKind, ScrollPrecision, ScrollUnit,
};

use super::{
    Axis, AxisLabel, DeviceDescription, DeviceRole, ScrollAxis, X11Pointers, XiAction, XiEvent,
    XiMaster, XiSlave, XiTime, button, role, valuators,
};
use crate::input::assembly::{NativeCancel, NativeClock, NativeSurface};

const MASTER: u16 = 2;
const XTEST: u16 = 4;
const TOUCHPAD: u16 = 9;
const SCREEN: u16 = 10;
const STYLUS: u16 = 11;
const SURFACE: NativeSurface = NativeSurface(1);

fn device(id: u16, name: &str) -> DeviceDescription {
    DeviceDescription {
        id,
        master: false,
        pointer: true,
        name: name.to_owned(),
        direct_touch: false,
        axes: Vec::new(),
        scrolls: Vec::new(),
    }
}

fn master() -> DeviceDescription {
    DeviceDescription {
        master: true,
        pointer: false,
        ..device(MASTER, "Virtual core pointer")
    }
}

/// A touchpad whose valuators 2 and 3 scroll, 15 to a detent, as
/// xf86-input-libinput reports one.
fn touchpad() -> DeviceDescription {
    DeviceDescription {
        scrolls: vec![
            ScrollAxis {
                number: 2,
                horizontal: true,
                increment: 15.0,
            },
            ScrollAxis {
                number: 3,
                horizontal: false,
                increment: 15.0,
            },
        ],
        ..device(TOUCHPAD, "SynPS/2 Synaptics TouchPad")
    }
}

fn screen() -> DeviceDescription {
    DeviceDescription {
        direct_touch: true,
        ..device(SCREEN, "ELAN Touchscreen")
    }
}

/// A Wacom stylus: pressure 0..65535 and tilt in degrees.
fn stylus() -> DeviceDescription {
    let axis = |number, label, min, max| Axis {
        number,
        label,
        min,
        max,
    };
    DeviceDescription {
        axes: vec![
            axis(0, AxisLabel::Other, 0.0, 30_000.0),
            axis(1, AxisLabel::Other, 0.0, 20_000.0),
            axis(2, AxisLabel::Pressure, 0.0, 65_535.0),
            axis(3, AxisLabel::TiltX, -64.0, 63.0),
            axis(4, AxisLabel::TiltY, -64.0, 63.0),
        ],
        ..device(STYLUS, "Wacom Intuos S Pen stylus")
    }
}

fn pointers() -> X11Pointers {
    let mut pointers = X11Pointers::new(NativeClock::epoch_only());
    let _ = pointers.devices(
        &[
            master(),
            device(XTEST, "Virtual core XTEST pointer"),
            touchpad(),
            screen(),
            stylus(),
        ],
        EventTime::from_nanos(1),
    );
    pointers
}

/// An event's time: the server's `ms` read as it arrives at the same
/// millisecond of the monotonic clock.
fn at_ms(ms: u32) -> XiTime {
    XiTime {
        server_ms: ms,
        observed_ms: u64::from(ms),
        arrival: EventTime::from_nanos(u64::from(ms) * 1_000_000),
    }
}

fn event(action: XiAction, source: u16, at: (f64, f64)) -> XiEvent {
    XiEvent {
        action,
        master: XiMaster(MASTER),
        source: XiSlave(source),
        detail: 0,
        position: at,
        emulated: false,
        modifiers: 0,
        valuators: Vec::new(),
        time: at_ms(1),
    }
}

fn pointer_events(inputs: Vec<PlatformInput>) -> Vec<PointerEvent> {
    inputs
        .into_iter()
        .map(|input| match input {
            PlatformInput::Pointer(event) => event,
            other => panic!("not a pointer event: {other:?}"),
        })
        .collect()
}

fn feed(pointers: &mut X11Pointers, events: &[XiEvent]) -> Vec<PointerEvent> {
    pointer_events(
        events
            .iter()
            .flat_map(|event| pointers.event(SURFACE, 1.0, event).inputs)
            .collect(),
    )
}

/// The detents and precision of each scroll in `events`.
fn scrolls(events: &[PointerEvent]) -> Vec<(f64, f64, ScrollPrecision)> {
    events
        .iter()
        .filter_map(|event| match event {
            PointerEvent::Scroll(scroll) => {
                assert_eq!(scroll.delta.unit(), ScrollUnit::Detents);
                assert_eq!(scroll.phase, None, "X11 scrolling has no phases");
                Some((scroll.delta.x(), scroll.delta.y(), scroll.precision))
            }
            _ => None,
        })
        .collect()
}

/// A scroll valuator's change is that many detents of its increment,
/// fractional and precise for a touchpad; the first value after the pointer
/// enters only sets where it starts. A legacy wheel button of a device with
/// no scroll valuator is a whole detent, and one the server emulated from a
/// valuator is not read twice.
#[test]
fn a_wheel_is_detents_and_a_touchpad_scrolls_precisely() {
    let mut pointers = pointers();
    let at = (10.0, 10.0);
    let scrolled = |vertical: f64, horizontal: f64| XiEvent {
        valuators: vec![(2, horizontal), (3, vertical)],
        ..event(XiAction::Motion, TOUCHPAD, at)
    };
    let wheel = |button: u32, emulated: bool| XiEvent {
        detail: button,
        emulated,
        ..event(XiAction::ButtonPress, XTEST, at)
    };
    let events = feed(
        &mut pointers,
        &[
            event(XiAction::Enter, TOUCHPAD, at),
            scrolled(1000.0, 500.0),
            scrolled(1015.0, 500.0),
            scrolled(1022.5, 470.0),
            wheel(5, false),
            wheel(4, true),
            wheel(6, false),
        ],
    );
    assert_eq!(
        scrolls(&events),
        [
            (0.0, 1.0, ScrollPrecision::Notched),
            (-2.0, 0.5, ScrollPrecision::Precise),
            (0.0, 1.0, ScrollPrecision::Notched),
            (-1.0, 0.0, ScrollPrecision::Notched),
        ]
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, PointerEvent::Move(_))),
        "a scroll that leaves the cursor where it was moves nothing: {events:?}"
    );
}

/// A pen moves the master's cursor, but is a contact of its own: the cursor
/// leaves as the pen takes over, and the pen as the mouse takes it back. The
/// pen's pressure and tilt come from its valuators.
#[test]
fn a_pen_takes_the_cursor_over_and_gives_it_back() {
    let mut pointers = pointers();
    let pen = |action, at| XiEvent {
        valuators: vec![(2, 32_767.5), (3, 30.0), (4, -64.0)],
        ..event(action, STYLUS, at)
    };
    let events = feed(
        &mut pointers,
        &[
            event(XiAction::Enter, XTEST, (5.0, 5.0)),
            event(XiAction::Motion, XTEST, (6.0, 5.0)),
            pen(XiAction::Motion, (40.0, 40.0)),
            XiEvent {
                detail: 1,
                ..pen(XiAction::ButtonPress, (40.0, 40.0))
            },
            XiEvent {
                detail: 1,
                ..pen(XiAction::ButtonRelease, (41.0, 40.0))
            },
            event(XiAction::Motion, XTEST, (7.0, 5.0)),
        ],
    );
    let kinds: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            PointerEvent::Enter(signal) => Some(("enter", signal.pointer.kind)),
            PointerEvent::Leave(signal) => Some(("leave", signal.pointer.kind)),
            PointerEvent::Down(press) => Some(("down", press.pointer.kind)),
            PointerEvent::Up(release) => Some(("up", release.pointer.kind)),
            PointerEvent::Move(movement) => Some(("move", movement.pointer.kind)),
            _ => None,
        })
        .collect();
    let tip = PointerKind::Pen { tool: PenTool::Tip };
    assert_eq!(
        kinds,
        [
            ("enter", PointerKind::Mouse),
            ("move", PointerKind::Mouse),
            ("leave", PointerKind::Mouse),
            ("enter", tip),
            ("move", tip),
            ("down", tip),
            ("up", tip),
            ("leave", tip),
            ("enter", PointerKind::Mouse),
            ("move", PointerKind::Mouse),
        ],
        "{events:?}"
    );
    let down = events
        .iter()
        .find_map(|event| match event {
            PointerEvent::Down(press) => Some(press),
            _ => None,
        })
        .expect("the pen's press");
    let pressure = down.sample.pressure.expect("pressure").get();
    assert!((pressure - 0.5).abs() < 1e-9, "{pressure}");
    assert!(down.sample.orientation.is_some(), "the tilt is read");
}

/// A touch screen's touches are contacts of their own, by touch id; the
/// pointer events the server emulates from the first one are not read.
#[test]
fn a_touch_is_its_own_contact() {
    let mut pointers = pointers();
    let touch = |action, id, x| XiEvent {
        detail: id,
        ..event(action, SCREEN, (x, 30.0))
    };
    let events = feed(
        &mut pointers,
        &[
            touch(XiAction::TouchBegin, 7, 10.0),
            XiEvent {
                emulated: true,
                detail: 1,
                ..event(XiAction::ButtonPress, SCREEN, (10.0, 30.0))
            },
            touch(XiAction::TouchBegin, 8, 50.0),
            touch(XiAction::TouchUpdate, 7, 12.0),
            touch(XiAction::TouchEnd, 7, 12.0),
            touch(XiAction::TouchEnd, 8, 50.0),
        ],
    );
    let downs: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            PointerEvent::Down(press) => Some(press.pointer),
            _ => None,
        })
        .collect();
    assert_eq!(downs.len(), 2, "{events:?}");
    assert!(
        downs
            .iter()
            .all(|pointer| pointer.kind == PointerKind::Touch)
    );
    assert_ne!(downs[0].id, downs[1].id, "two touches, two pointers");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, PointerEvent::Up(_)))
            .count(),
        2
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, PointerEvent::Down(press) if press.pointer.kind == PointerKind::Mouse)),
        "an emulated press is not read: {events:?}"
    );
}

/// A grab that takes the pointer mid-press ends the press as the platform's;
/// a leave after the release only leaves.
#[test]
fn a_grab_that_takes_a_pressed_pointer_cancels_it() {
    let mut pointers = pointers();
    let at = (20.0, 20.0);
    let pressed = |action| XiEvent {
        detail: 1,
        ..event(action, XTEST, at)
    };
    let grabbed = feed(
        &mut pointers,
        &[
            event(XiAction::Enter, XTEST, at),
            pressed(XiAction::ButtonPress),
            event(XiAction::Leave { grabbed: true }, XTEST, at),
        ],
    );
    assert!(
        matches!(
            grabbed.as_slice(),
            [
                PointerEvent::Enter(_),
                PointerEvent::Down(_),
                PointerEvent::Cancel(cancel),
                PointerEvent::Leave(_),
            ] if cancel.reason == CancelReason::Platform
        ),
        "{grabbed:?}"
    );
    let released = feed(
        &mut pointers,
        &[
            event(XiAction::Enter, XTEST, at),
            pressed(XiAction::ButtonPress),
            pressed(XiAction::ButtonRelease),
            event(XiAction::Leave { grabbed: false }, XTEST, at),
        ],
    );
    assert!(
        !released
            .iter()
            .any(|event| matches!(event, PointerEvent::Cancel(_))),
        "{released:?}"
    );
}

/// The server's device list is what is present: a pen plugged in is
/// announced, one unplugged mid-stroke ends its stroke and is removed, and
/// an id the server gives a different device is a new device.
#[test]
fn devices_come_and_go_with_the_hierarchy() {
    let time = EventTime::from_nanos(5);
    let mut pointers = X11Pointers::new(NativeClock::epoch_only());
    let added = pointers.devices(
        &[master(), device(XTEST, "Virtual core XTEST pointer")],
        time,
    );
    assert_eq!(added.len(), 1, "the master's cursor: {added:?}");
    assert_eq!(pointers.present(), PointerKindSet::MOUSE);
    let plugged = pointers.devices(
        &[
            master(),
            device(XTEST, "Virtual core XTEST pointer"),
            stylus(),
        ],
        time,
    );
    assert!(matches!(
        plugged.as_slice(),
        [(None, PlatformInput::Pointer(PointerEvent::DeviceAdded(change)))]
            if matches!(change.kind, PointerKind::Pen { .. })
    ));
    let down = XiEvent {
        detail: 1,
        ..event(XiAction::ButtonPress, STYLUS, (3.0, 3.0))
    };
    let _ = pointers.event(SURFACE, 1.0, &down);
    let unplugged = pointers.devices(
        &[master(), device(XTEST, "Virtual core XTEST pointer")],
        time,
    );
    assert!(
        matches!(
            unplugged.as_slice(),
            [
                (Some(SURFACE), PlatformInput::Pointer(PointerEvent::Cancel(cancel))),
                (Some(SURFACE), PlatformInput::Pointer(PointerEvent::Leave(_))),
                (None, PlatformInput::Pointer(PointerEvent::DeviceRemoved(_))),
            ] if cancel.reason == CancelReason::DeviceRemoved
        ),
        "the stroke ends, the pen leaves the window it entered: {unplugged:?}"
    );
    assert_eq!(pointers.present(), PointerKindSet::MOUSE);
    // The stylus's id, now an eraser's.
    let eraser = DeviceDescription {
        name: "Wacom Intuos S Pen eraser".to_owned(),
        ..stylus()
    };
    let _ = pointers.devices(&[master(), stylus()], time);
    let swapped = pointers.devices(&[master(), eraser], time);
    assert!(
        matches!(
            swapped.as_slice(),
            [
                (None, PlatformInput::Pointer(PointerEvent::DeviceRemoved(_))),
                (None, PlatformInput::Pointer(PointerEvent::DeviceAdded(change))),
            ] if change.kind == PointerKind::Pen { tool: PenTool::Eraser }
        ),
        "{swapped:?}"
    );
}

#[test]
fn a_device_s_role_is_read_from_its_classes_and_name() {
    // (case, device, role)
    let table = [
        ("a touch screen", screen(), DeviceRole::Touchscreen),
        ("a stylus", stylus(), DeviceRole::Pen(PenTool::Tip)),
        (
            "an eraser",
            device(12, "Wacom Intuos S Pen eraser"),
            DeviceRole::Pen(PenTool::Eraser),
        ),
        (
            "a pen by its valuators alone",
            DeviceDescription {
                name: "tablet".to_owned(),
                ..stylus()
            },
            DeviceRole::Pen(PenTool::Tip),
        ),
        ("a touchpad", touchpad(), DeviceRole::Cursor),
        (
            "XTest",
            device(XTEST, "Virtual core XTEST pointer"),
            DeviceRole::Cursor,
        ),
    ];
    for (case, device, expected) in table {
        assert_eq!(role(&device), expected, "{case}");
    }
}

#[test]
fn x_buttons_are_flui_buttons() {
    // (X button, FLUI button)
    let table = [
        (1, PointerButton::PRIMARY),
        (2, PointerButton::AUXILIARY),
        (3, PointerButton::SECONDARY),
        (8, PointerButton::BACK),
        (9, PointerButton::FORWARD),
    ];
    for (x, expected) in table {
        assert_eq!(button(x), expected, "button {x}");
    }
    // Vendor buttons past 9 share the non-actuating band, and wrap in it.
    assert_eq!(button(10), button(36));
    assert_ne!(button(10), button(11));
}

/// A pen unplugged and plugged back in before the list is read again (the
/// hierarchy reports both, the server lists it under the same id) ends its
/// stroke and comes back as a new device: no press stays down.
#[test]
fn a_pen_plugged_back_in_under_its_id_is_a_new_pen() {
    let mut pointers = pointers();
    let pressed = XiEvent {
        detail: 1,
        ..event(XiAction::ButtonPress, STYLUS, (3.0, 3.0))
    };
    let down = |events: &[PointerEvent]| {
        events.iter().find_map(|event| match event {
            PointerEvent::Down(press) => Some(press.pointer),
            _ => None,
        })
    };
    let first = down(&pointer_events(
        pointers.event(SURFACE, 1.0, &pressed).inputs,
    ))
    .expect("the pen's press");
    let time = EventTime::from_nanos(9);
    let mut changes = pointers.removed(&[XiSlave(STYLUS)], &[], time);
    changes.extend(pointers.devices(
        &[
            master(),
            device(XTEST, "Virtual core XTEST pointer"),
            touchpad(),
            screen(),
            stylus(),
        ],
        time,
    ));
    let events = pointer_events(changes.into_iter().map(|(_, input)| input).collect());
    assert!(
        matches!(
            events.as_slice(),
            [
                PointerEvent::Cancel(cancel),
                PointerEvent::Leave(_),
                PointerEvent::DeviceRemoved(_),
                PointerEvent::DeviceAdded(added),
            ] if cancel.reason == CancelReason::DeviceRemoved
                && cancel.pointer == first
                && Some(added.device) != first.device
        ),
        "{events:?}"
    );
    // The pen's next press is the new device's: a new pointer.
    let again = down(&pointer_events(
        pointers.event(SURFACE, 1.0, &pressed).inputs,
    ))
    .expect("the new pen's press");
    assert_ne!(again.id, first.id);
    assert_ne!(again.device, first.device);
}

/// A window losing the focus ends its own contacts only: the pen hovering
/// another window keeps its contact, and leaves that window when the mouse
/// takes the cursor back.
#[test]
fn a_window_losing_the_focus_leaves_another_windows_pen_alone() {
    let mut pointers = pointers();
    let window_a = NativeSurface(1);
    let window_b = NativeSurface(2);
    let pen = event(XiAction::Motion, STYLUS, (40.0, 40.0));
    let _ = pointers.event(window_b, 1.0, &pen);
    let cancelled =
        pointers.cancel_surface(window_a, NativeCancel::FocusLost, EventTime::from_nanos(2));
    assert!(
        cancelled.is_empty(),
        "nothing moved on window A: {cancelled:?}"
    );
    let mouse = event(XiAction::Motion, XTEST, (5.0, 5.0));
    let events = pointer_events(pointers.event(window_b, 1.0, &mouse).inputs);
    assert!(
        matches!(
            events.first(),
            Some(PointerEvent::Leave(signal)) if matches!(signal.pointer.kind, PointerKind::Pen { .. })
        ),
        "the pen leaves as the mouse takes the cursor back: {events:?}"
    );
}

/// A touch screen's crossing is no contact: it makes no hover pointer.
#[test]
fn a_touch_screens_crossing_is_no_contact() {
    let mut pointers = pointers();
    let decoded = pointers.event(SURFACE, 1.0, &event(XiAction::Enter, SCREEN, (1.0, 1.0)));
    assert!(decoded.inputs.is_empty(), "{:?}", decoded.inputs);
    assert_eq!(decoded.hover, None);
}

/// Motions the loop reads in one turn keep the spacing of the server's
/// times; a server whose clock is not the monotonic one is read by arrival,
/// and the server's time is its own across the wrap.
#[test]
fn a_burst_keeps_the_spacing_of_the_servers_times() {
    use crate::input::assembly::{NativeTime, ReadingTime};
    let mut pointers = pointers();
    let arrived: u64 = 1_000;
    let burst: Vec<XiEvent> = (0..4_u32)
        .map(|step| XiEvent {
            time: XiTime {
                server_ms: 960 + step * 4,
                observed_ms: arrived,
                arrival: EventTime::from_nanos(arrived * 1_000_000),
            },
            ..event(XiAction::Motion, XTEST, (f64::from(step), 0.0))
        })
        .collect();
    let times: Vec<u64> = feed(&mut pointers, &burst)
        .iter()
        .filter_map(|event| match event {
            PointerEvent::Move(movement) => Some(movement.current().time.as_nanos()),
            _ => None,
        })
        .collect();
    let gaps: Vec<u64> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert_eq!(gaps, [4_000_000; 3], "{times:?}");

    let remote = XiTime {
        server_ms: 5,
        observed_ms: 100_000,
        arrival: EventTime::from_nanos(77),
    };
    assert_eq!(
        remote.reading_time(),
        ReadingTime::epoch(EventTime::from_nanos(77))
    );
    let wrapped = XiTime {
        server_ms: 3,
        observed_ms: u64::from(u32::MAX) + 2,
        arrival: EventTime::from_nanos(1),
    };
    assert!(matches!(
        wrapped.reading_time().coarse,
        NativeTime::TickMillis { tick: 3, .. }
    ));
}

/// A touchpad pinch is one pan-zoom: each update's own factor, compounding
/// to the scale the server says, about the cursor.
#[test]
fn a_touchpad_pinch_is_one_pan_zoom() {
    use flui_platform::pointer::PanZoomPhase;
    let mut pointers = pointers();
    let at = (30.0, 30.0);
    let update = |scale, rotation| event(XiAction::PinchUpdate { scale, rotation }, TOUCHPAD, at);
    let events = feed(
        &mut pointers,
        &[
            event(XiAction::PinchBegin, TOUCHPAD, at),
            update(1.5, 0.0),
            update(3.0, 90.0),
            event(XiAction::PinchEnd { cancelled: false }, TOUCHPAD, at),
        ],
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            PointerEvent::DeviceAdded(change) if change.kind == PointerKind::Trackpad
        )),
        "the touchpad's gestures are a device: {events:?}"
    );
    let mut scale = 0.0;
    let mut phases = Vec::new();
    for event in &events {
        if let PointerEvent::PanZoom(gesture) = event {
            match gesture.phase {
                PanZoomPhase::Start => phases.push("start"),
                PanZoomPhase::Update(transform) => {
                    scale = transform.scale();
                    phases.push("update");
                }
                PanZoomPhase::End => phases.push("end"),
                _ => phases.push("other"),
            }
        }
    }
    assert_eq!(phases.first(), Some(&"start"), "{events:?}");
    assert_eq!(phases.last(), Some(&"end"), "{events:?}");
    assert!(
        (scale - 3.0).abs() < 1e-9,
        "the last update is the scale since the pinch began: {scale}"
    );
}

/// The valuators a mask names: a bit past the 16 bits of a valuator's
/// number skips its value without a panic, as does a value the event lacks.
#[test]
fn a_valuator_mask_past_its_numbers_is_skipped() {
    assert_eq!(valuators(&[0b101], &[1.0, 2.0]), [(0, 1.0), (2, 2.0)]);
    assert_eq!(valuators(&[0, 1], &[7.0]), [(32, 7.0)]);
    // Word 2048 starts at number 65 536, past a `u16`.
    let mut mask = vec![0_u32; 2049];
    mask[0] = 1;
    mask[2048] = 1;
    assert_eq!(valuators(&mask, &[1.0, 2.0]), [(0, 1.0)]);
    assert_eq!(valuators(&[0b11], &[1.0]), [(0, 1.0)]);
}

#[test]
/// A server that lists devices and valuators without end has the decoder
/// know `MAX_DEVICES` devices and read `MAX_VALUATORS` valuators of each.
fn the_devices_and_valuators_known_are_bounded() {
    let time = EventTime::from_nanos(5);
    let mut pointers = X11Pointers::new(NativeClock::epoch_only());
    let scroll = |number| ScrollAxis {
        number,
        horizontal: false,
        increment: 15.0,
    };
    let wide = DeviceDescription {
        scrolls: (0..10_000).map(scroll).collect(),
        ..touchpad()
    };
    let listed: Vec<DeviceDescription> = std::iter::once(master())
        .chain(std::iter::once(wide))
        .chain((100..10_100).map(|id| device(id, "Spurious pointer")))
        .collect();
    let _ = pointers.devices(&listed, time);
    let known = pointers.devices.len() + pointers.masters.len();
    assert!(known <= super::MAX_DEVICES, "{known} devices known");
    let scrolls = pointers.devices[&XiSlave(TOUCHPAD)].scrolls.len();
    assert!(scrolls <= super::MAX_VALUATORS, "{scrolls} valuators read");
}
