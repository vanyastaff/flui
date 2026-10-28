use flui_platform::EventTime;
use flui_platform::PlatformInput;
use flui_platform::input_capabilities::PointerKindSet;
use flui_platform::pointer::{
    CancelReason, PenTool, PointerButton, PointerEvent, PointerKind, ScrollPrecision, ScrollUnit,
};

use super::{
    Axis, AxisLabel, DeviceDescription, DeviceRole, ScrollAxis, X11Pointers, XiAction, XiEvent,
    button, layout_name, role,
};
use crate::input::assembly::NativeSurface;

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
    let mut pointers = X11Pointers::new();
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

fn event(action: XiAction, source: u16, at: (f64, f64)) -> XiEvent {
    XiEvent {
        action,
        device: MASTER,
        source,
        detail: 0,
        position: at,
        emulated: false,
        modifiers: 0,
        valuators: Vec::new(),
        time: EventTime::from_nanos(1_000),
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
    let mut pointers = X11Pointers::new();
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
                (None, PlatformInput::Pointer(PointerEvent::DeviceRemoved(_))),
            ] if cancel.reason == CancelReason::DeviceRemoved
        ),
        "{unplugged:?}"
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

#[test]
fn the_layout_is_the_keyboards_group_of_the_rules_names() {
    let names = b"evdev\0pc105\0us,de,ru\0,nodeadkeys,\0grp:alt_shift_toggle\0";
    // (group, name)
    let table = [(0, "us"), (1, "de(nodeadkeys)"), (2, "ru"), (3, "")];
    for (group, expected) in table {
        assert_eq!(layout_name(names, group), expected, "group {group}");
    }
    assert_eq!(layout_name(b"", 0), "");
}
