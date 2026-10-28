//! The Wayland wire translation: what each kind of seat object's events
//! become, as a transcript of the input each window hears.

use std::time::Duration;

use flui_platform::pointer::{PanZoomPhase, PenTool, PointerEvent};

use super::*;

const WINDOW: NativeSurface = NativeSurface(1);
const OTHER: NativeSurface = NativeSurface(2);
const POINTER: NativeDevice = NativeDevice(1);
const TOUCHSCREEN: NativeDevice = NativeDevice(2);
const PEN: NativeDevice = NativeDevice(3);
const PINCHER: NativeDevice = NativeDevice(4);
const EPOCH: EventTime = EventTime::from_nanos(1_000_000_000);

/// A seat's assembly whose clock's epoch is monotonic time zero.
fn assembly() -> SeatAssembly {
    SeatAssembly::new(NativeClock::new(EPOCH, Duration::ZERO, None))
}

/// The time of an event stamped at `ms` that arrived then.
fn at(ms: u32) -> WireTime {
    WireTime::stamped(
        ms,
        u64::from(ms),
        EventTime::from_nanos(EPOCH.as_nanos() + u64::from(ms) * 1_000_000),
    )
}

/// An assembly whose seat announced its pointer, touchscreen and eraser, as
/// the dispatch does when the seat's capabilities and tools arrive.
fn seated() -> SeatAssembly {
    let mut assembly = assembly();
    for (device, kind) in [
        (POINTER, PointerKind::Mouse),
        (TOUCHSCREEN, PointerKind::Touch),
        (PEN, ERASER),
    ] {
        let _announced = assembly.assemble(SeatWire {
            time: at(1),
            event: SeatEvent::DeviceAdded { device, kind },
        });
    }
    assembly
}

/// Feed `events` at increasing times and transcribe what each window heard.
fn feed(assembly: &mut SeatAssembly, events: &[SeatEvent]) -> Vec<String> {
    let mut lines = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let ms = 10 + 2 * u32::try_from(index).expect("few events");
        for routed in assembly.assemble(SeatWire {
            time: at(ms),
            event: *event,
        }) {
            match routed {
                Routed::To(surface, input) => {
                    lines.push(format!("s{} {}", surface.0, describe(&input)))
                }
                Routed::Everywhere(input) => lines.push(format!("all {}", describe(&input))),
            }
        }
    }
    lines
}

fn describe(input: &PlatformInput) -> String {
    match input {
        PlatformInput::Pointer(event) => pointer(event),
        PlatformInput::PointerFrame(frame) => format!(
            "frame[{}]",
            frame
                .events()
                .iter()
                .map(pointer)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    }
}

fn pointer(event: &PointerEvent) -> String {
    let head = |verb: &str, info: &flui_platform::pointer::PointerInfo| {
        format!("{verb} p{} {:?}", info.id.get(), info.kind)
    };
    match event {
        PointerEvent::Enter(signal) => head("enter", &signal.pointer),
        PointerEvent::Leave(signal) => head("leave", &signal.pointer),
        PointerEvent::Down(press) => {
            let pressure = press
                .sample
                .pressure
                .map(|pressure| format!(" pressure {}", pressure.get()))
                .unwrap_or_default();
            let size = press
                .sample
                .contact_size
                .map(|size| format!(" size {:?}", size.get()))
                .unwrap_or_default();
            format!(
                "{} {:?} at {:?}{pressure}{size}",
                head("down", &press.pointer),
                press.button(),
                press.sample.position.get()
            )
        }
        PointerEvent::Up(release) => {
            format!("{} {:?}", head("up", &release.pointer), release.button())
        }
        PointerEvent::Move(movement) => format!(
            "{} at {:?}",
            head("move", &movement.pointer),
            movement.current().position.get()
        ),
        PointerEvent::Cancel(cancel) => {
            format!("{} {:?}", head("cancel", &cancel.pointer), cancel.reason)
        }
        PointerEvent::Scroll(scroll) => format!(
            "{} {:?} {},{} {:?} {:?}",
            head("scroll", &scroll.pointer),
            scroll.delta.unit(),
            scroll.delta.x(),
            scroll.delta.y(),
            scroll.precision,
            scroll.phase
        ),
        PointerEvent::PanZoom(gesture) => {
            let position = gesture.position.get();
            let phase = match gesture.phase {
                PanZoomPhase::Start => "start".to_owned(),
                PanZoomPhase::Update(transform) => {
                    format!(
                        "update {:.3} {:.3}",
                        transform.scale(),
                        transform.rotation()
                    )
                }
                PanZoomPhase::End => "end".to_owned(),
                _ => "cancelled".to_owned(),
            };
            format!(
                "{} {phase} at {},{}",
                head("pinch", gesture.pointer()),
                position.x,
                position.y
            )
        }
        PointerEvent::DeviceAdded(change) => format!("added {:?}", change.kind),
        PointerEvent::DeviceRemoved(change) => format!("removed {:?}", change.kind),
        other => format!("{other:?}"),
    }
}

fn mouse(event: PointerWire) -> SeatEvent {
    SeatEvent::Pointer {
        device: POINTER,
        event,
    }
}

fn touch(event: TouchWire) -> SeatEvent {
    SeatEvent::Touch {
        device: TOUCHSCREEN,
        event,
    }
}

const ERASER: PointerKind = PointerKind::Pen {
    tool: PenTool::Eraser,
};

fn eraser(event: ToolWire) -> SeatEvent {
    SeatEvent::Tool {
        device: PEN,
        kind: ERASER,
        event,
    }
}

fn pinch(event: PinchWire) -> SeatEvent {
    SeatEvent::Pinch {
        device: PINCHER,
        pointer: POINTER,
        event,
    }
}

const BTN_LEFT: u32 = 0x110;

/// A mouse is one pointer from its enter to its leave, its buttons pressed
/// and released where it is.
#[test]
fn a_mouse_is_one_pointer_from_enter_to_leave() {
    let lines = feed(
        &mut seated(),
        &[
            mouse(PointerWire::Enter {
                surface: WINDOW,
                x: 10.0,
                y: 20.0,
            }),
            mouse(PointerWire::Frame),
            mouse(PointerWire::Motion { x: 12.5, y: 21.0 }),
            mouse(PointerWire::Frame),
            mouse(PointerWire::Button {
                code: BTN_LEFT,
                pressed: true,
            }),
            mouse(PointerWire::Frame),
            mouse(PointerWire::Button {
                code: BTN_LEFT,
                pressed: false,
            }),
            mouse(PointerWire::Frame),
            mouse(PointerWire::Leave),
            mouse(PointerWire::Frame),
        ],
    );
    assert_eq!(
        lines,
        [
            "s1 enter p1 Mouse",
            "s1 move p1 Mouse at Point { x: 12.5, y: 21.0 }",
            "s1 down p1 Mouse PRIMARY at Point { x: 12.5, y: 21.0 }",
            "s1 up p1 Mouse PRIMARY",
            "s1 leave p1 Mouse",
        ]
    );
}

/// A compositor that takes the pointer while a button is held sends no
/// release: the press is cancelled at the leave, before it.
#[test]
fn a_leave_with_a_button_held_cancels_the_press() {
    let lines = feed(
        &mut seated(),
        &[
            mouse(PointerWire::Enter {
                surface: WINDOW,
                x: 1.0,
                y: 1.0,
            }),
            mouse(PointerWire::Button {
                code: BTN_LEFT,
                pressed: true,
            }),
            mouse(PointerWire::Leave),
            mouse(PointerWire::Frame),
        ],
    );
    assert_eq!(
        &lines[2..],
        ["s1 cancel p1 Mouse CaptureLost", "s1 leave p1 Mouse"],
        "{lines:?}"
    );
}

/// A wheel frame is one step in detents from its `value120`, notched when
/// whole and precise when not, or from its discrete notches; its pixel
/// value is not added on top.
#[test]
fn a_wheel_frame_is_one_step_in_detents() {
    let enter = mouse(PointerWire::Enter {
        surface: WINDOW,
        x: 5.0,
        y: 5.0,
    });
    for (case, events, expected) in [
        (
            "a notch down",
            vec![
                PointerWire::AxisSource(AxisSource::Wheel),
                PointerWire::Axis {
                    axis: Axis::Vertical,
                    value: 15.0,
                },
                PointerWire::AxisValue120 {
                    axis: Axis::Vertical,
                    value120: 120,
                },
            ],
            "s1 scroll p1 Mouse Detents 0,1 Notched None",
        ),
        (
            "half a notch right",
            vec![
                PointerWire::AxisSource(AxisSource::WheelTilt),
                PointerWire::Axis {
                    axis: Axis::Horizontal,
                    value: 7.5,
                },
                PointerWire::AxisValue120 {
                    axis: Axis::Horizontal,
                    value120: 60,
                },
            ],
            "s1 scroll p1 Mouse Detents 0.5,0 Precise None",
        ),
        (
            "two notches up, discrete",
            vec![
                PointerWire::AxisSource(AxisSource::Wheel),
                PointerWire::Axis {
                    axis: Axis::Vertical,
                    value: -30.0,
                },
                PointerWire::AxisDiscrete {
                    axis: Axis::Vertical,
                    discrete: -2,
                },
            ],
            "s1 scroll p1 Mouse Detents 0,-2 Notched None",
        ),
        (
            "a continuous source",
            vec![
                PointerWire::AxisSource(AxisSource::Continuous),
                PointerWire::Axis {
                    axis: Axis::Vertical,
                    value: 3.25,
                },
            ],
            "s1 scroll p1 Mouse Pixels 0,3.25 Precise None",
        ),
    ] {
        let mut wire = vec![enter, mouse(PointerWire::Frame)];
        wire.extend(events.into_iter().map(mouse));
        wire.push(mouse(PointerWire::Frame));
        let lines = feed(&mut seated(), &wire);
        assert_eq!(&lines[1..], [expected], "{case}: {lines:?}");
    }
}

/// Fingers on a touchpad scroll in phases: the first frame begins, the next
/// change, and the stop at the lift ends; a leave mid-scroll cancels it.
#[test]
fn a_finger_scroll_is_phased_until_its_stop() {
    let fingers = |value: f64| {
        [
            mouse(PointerWire::AxisSource(AxisSource::Finger)),
            mouse(PointerWire::Axis {
                axis: Axis::Vertical,
                value,
            }),
            mouse(PointerWire::Frame),
        ]
    };
    let mut wire = vec![
        mouse(PointerWire::Enter {
            surface: WINDOW,
            x: 5.0,
            y: 5.0,
        }),
        mouse(PointerWire::Frame),
    ];
    wire.extend(fingers(4.0));
    wire.extend(fingers(8.0));
    wire.extend([
        mouse(PointerWire::AxisSource(AxisSource::Finger)),
        mouse(PointerWire::AxisStop(Axis::Vertical)),
        mouse(PointerWire::Frame),
    ]);
    wire.extend(fingers(2.0));
    wire.push(mouse(PointerWire::Leave));
    let lines = feed(&mut seated(), &wire);
    assert_eq!(
        &lines[1..],
        [
            "s1 scroll p1 Mouse Pixels 0,4 Precise Some(Began)",
            "s1 scroll p1 Mouse Pixels 0,8 Precise Some(Changed)",
            "s1 scroll p1 Mouse Pixels 0,0 Precise Some(Ended)",
            "s1 scroll p1 Mouse Pixels 0,2 Precise Some(Began)",
            "s1 scroll p1 Mouse Pixels 0,0 Precise Some(Cancelled)",
            "s1 leave p1 Mouse",
        ],
        "{lines:?}"
    );
}

/// The touches one frame closes are one pointer frame, each with the
/// contact size its shape gave; a lifted label is a new pointer when it
/// comes back; a cancel ends every touch still down.
#[test]
fn the_touches_of_a_frame_are_one_pointer_frame() {
    let lines = feed(
        &mut seated(),
        &[
            touch(TouchWire::Down {
                surface: WINDOW,
                id: 0,
                x: 10.0,
                y: 10.0,
            }),
            touch(TouchWire::Down {
                surface: WINDOW,
                id: 1,
                x: 30.0,
                y: 10.0,
            }),
            touch(TouchWire::Shape {
                id: 1,
                major: 8.0,
                minor: 6.0,
            }),
            touch(TouchWire::Frame),
            touch(TouchWire::Up { id: 0 }),
            touch(TouchWire::Frame),
            touch(TouchWire::Down {
                surface: WINDOW,
                id: 0,
                x: 12.0,
                y: 12.0,
            }),
            touch(TouchWire::Frame),
            touch(TouchWire::Cancel),
        ],
    );
    assert_eq!(
        lines,
        [
            "s1 frame[down p2 Touch PRIMARY at Point { x: 10.0, y: 10.0 }, \
             down p3 Touch PRIMARY at Point { x: 30.0, y: 10.0 } size Size { width: 8.0, height: 6.0 }]",
            "s1 up p2 Touch PRIMARY",
            "s1 down p4 Touch PRIMARY at Point { x: 12.0, y: 12.0 }",
            "s1 cancel p3 Touch Platform",
            "s1 cancel p4 Touch Platform",
        ],
        "{lines:?}"
    );
}

/// A tablet tool's frame becomes its enter, its press with the axes it
/// holds, its lift and its leave; an eraser is the pen's eraser.
#[test]
fn a_tablet_frame_is_one_reading_per_change() {
    let lines = feed(
        &mut seated(),
        &[
            eraser(ToolWire::ProximityIn { surface: WINDOW }),
            eraser(ToolWire::Motion { x: 40.0, y: 50.0 }),
            eraser(ToolWire::Frame),
            eraser(ToolWire::Pressure(65_535)),
            eraser(ToolWire::Down),
            eraser(ToolWire::Frame),
            eraser(ToolWire::Up),
            eraser(ToolWire::ProximityOut),
            eraser(ToolWire::Frame),
        ],
    );
    assert_eq!(
        lines,
        [
            "s1 enter p2 Pen { tool: Eraser }",
            "s1 move p2 Pen { tool: Eraser } at Point { x: 40.0, y: 50.0 }",
            "s1 down p2 Pen { tool: Eraser } PRIMARY at Point { x: 40.0, y: 50.0 } pressure 1",
            "s1 up p2 Pen { tool: Eraser } PRIMARY",
            "s1 leave p2 Pen { tool: Eraser }",
        ],
        "{lines:?}"
    );
}

/// A pinch is one pan-zoom about the cursor: each update's scale is taken
/// as its own factor of the scale since the start, its rotation in
/// clockwise radians, and its translation moves the focal point.
#[test]
fn a_pinch_is_one_pan_zoom_about_the_cursor() {
    let lines = feed(
        &mut seated(),
        &[
            mouse(PointerWire::Enter {
                surface: WINDOW,
                x: 100.0,
                y: 80.0,
            }),
            mouse(PointerWire::Frame),
            pinch(PinchWire::Begin { surface: WINDOW }),
            pinch(PinchWire::Update {
                dx: 0.0,
                dy: 0.0,
                scale: 1.5,
                rotation: 0.0,
            }),
            pinch(PinchWire::Update {
                dx: 10.0,
                dy: -5.0,
                scale: 3.0,
                rotation: 90.0,
            }),
            pinch(PinchWire::End { cancelled: false }),
        ],
    );
    assert_eq!(
        &lines[1..],
        [
            // A pinch's gesture object is announced by its first reading.
            "s1 added Trackpad",
            "s1 pinch p2 Trackpad start at 100,80",
            "s1 pinch p2 Trackpad update 1.500 0.000 at 100,80",
            "s1 pinch p2 Trackpad update 3.000 0.000 at 110,75",
            "s1 pinch p2 Trackpad update 3.000 1.571 at 110,75",
            "s1 pinch p2 Trackpad end at 110,75",
        ],
        "{lines:?}"
    );
}

/// A device that goes ends its contacts' sequences first; every window
/// hears a device come and go.
#[test]
fn a_device_that_goes_ends_its_contacts() {
    let lines = feed(
        &mut assembly(),
        &[
            SeatEvent::DeviceAdded {
                device: POINTER,
                kind: PointerKind::Mouse,
            },
            mouse(PointerWire::Enter {
                surface: WINDOW,
                x: 1.0,
                y: 1.0,
            }),
            mouse(PointerWire::Button {
                code: BTN_LEFT,
                pressed: true,
            }),
            SeatEvent::DeviceRemoved { device: POINTER },
        ],
    );
    assert_eq!(lines[0], "all added Mouse");
    assert_eq!(
        &lines[3..],
        [
            "s1 cancel p1 Mouse DeviceRemoved",
            "s1 leave p1 Mouse",
            "all removed Mouse"
        ],
        "{lines:?}"
    );
}

/// What a closed window held open ends unheard, and nothing of it reaches
/// another window.
#[test]
fn a_closed_windows_contacts_end_unheard() {
    let lines = feed(
        &mut seated(),
        &[
            mouse(PointerWire::Enter {
                surface: OTHER,
                x: 1.0,
                y: 1.0,
            }),
            mouse(PointerWire::Button {
                code: BTN_LEFT,
                pressed: true,
            }),
            SeatEvent::SurfaceGone(OTHER),
            mouse(PointerWire::Button {
                code: BTN_LEFT,
                pressed: false,
            }),
            mouse(PointerWire::Leave),
            mouse(PointerWire::Frame),
        ],
    );
    assert_eq!(lines.len(), 2, "only the enter and press: {lines:?}");
}

/// The compositor's stamp is placed on the event timeline through the
/// monotonic clock while it lies near it, wrap included; a stamp on another
/// clock, and an event without one, take the arrival.
#[test]
fn a_stamp_off_the_monotonic_clock_takes_the_arrival() {
    let arrival = EventTime::from_nanos(77);
    let wrap = 1_u64 << 32;
    for (case, time, expected) in [
        (
            "on the clock",
            WireTime::stamped(5_000, 5_004, arrival),
            ReadingTime::coarse(NativeTime::TickMillis {
                tick: 5_000,
                observed_ms: 5_004,
            }),
        ),
        (
            "across the wrap",
            WireTime::stamped(u32::MAX - 1, wrap + 2, arrival),
            ReadingTime::coarse(NativeTime::TickMillis {
                tick: u32::MAX - 1,
                observed_ms: wrap + 2,
            }),
        ),
        (
            "another clock",
            WireTime::stamped(5_000, 9_000_000, arrival),
            ReadingTime::epoch(arrival),
        ),
        (
            "no stamp",
            WireTime::arrived(5_000, arrival),
            ReadingTime::epoch(arrival),
        ),
    ] {
        assert_eq!(time.reading_time(), expected, "{case}");
    }
}

/// The kernel's button codes are the owned buttons; a pen's barrel buttons
/// are the secondary and auxiliary ones, and the rest share the band.
#[test]
fn a_button_code_is_its_owned_button() {
    for (code, expected) in [
        (0x110, PointerButton::PRIMARY),
        (0x111, PointerButton::SECONDARY),
        (0x112, PointerButton::AUXILIARY),
        (0x113, PointerButton::BACK),
        (0x116, PointerButton::BACK),
        (0x114, PointerButton::FORWARD),
        (0x115, PointerButton::FORWARD),
        (0x14b, PointerButton::SECONDARY),
        (0x14c, PointerButton::AUXILIARY),
        (0x117, PointerButton::try_from(14).expect("in band")),
    ] {
        assert_eq!(button_of(code), expected, "{code:#x}");
    }
}
