//! One composition, two paths (ADR-0400 §2, "Confirmation"): each gesture kind's action sequence,
//! planned once by `input::plan`, gives the same routed events through the app agent's injection,
//! read back from the window's input journal, and through the driver's OS lowering
//! (`steps::os_events`), wherever both paths carry the event.
//!
//! What the two cannot be compared on: the instants (the journal keeps the runtime's clock, the
//! lowering only the pauses between events), a key's name (the journal records its kind), and a
//! touch contact's moves while it is up, which a window never sees and an OS lane sends to place
//! the contact. The row a conformance table replays is a later comparison.

use flui_driver::coordinates::WindowGeometry;
use flui_driver::protocol::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use flui_driver::protocol::input::gestures::{self, Aim, Defaults};
use flui_driver::protocol::input::{
    ActionSequence, DeviceClass, Direction, InputKind, InputParams, PointerButton, PointerDevice,
};
use flui_driver::protocol::method;
use flui_driver::steps::{OsEvent, os_events};
use flui_driver::{Driver, Launch};

use crate::fixtures::{Counter, Probe};

/// One event as both paths can say it: what it is, from what kind of device, and where for a
/// pointer, in the whole device pixels a window at scale 1 and origin 0 maps it to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Routed {
    kind: InputKind,
    device: DeviceClass,
    at: Option<(i64, i64)>,
}

/// The device pixel `at` falls in at scale 1, as `coordinates::to_device` maps it: its floor.
fn device_pixel(at: LogicalPoint) -> (i64, i64) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a point inside an 800 × 600 window"
    )]
    let pixel = (at.x.floor() as i64, at.y.floor() as i64);
    pixel
}

fn class_of(device: PointerDevice) -> DeviceClass {
    match device {
        PointerDevice::Touch => DeviceClass::Touch,
        PointerDevice::Pen => DeviceClass::Pen,
        _ => DeviceClass::Mouse,
    }
}

/// The events the OS lowering sends for `sequence` at scale 1, as the window would see them: a
/// press and a release where its pointer is, a move of a mouse or of a contact that is down, a
/// key, committed text and a scroll.
fn through_the_lane(sequence: &ActionSequence) -> Vec<Routed> {
    let geometry = WindowGeometry::new(
        LogicalPoint::new(0.0, 0.0),
        LogicalSize::new(800.0, 600.0),
        1.0,
    );
    let events = os_events(sequence, &geometry).expect("the lane sends it");
    let mut at: Vec<(String, (i64, i64))> = Vec::new();
    let mut down: Vec<String> = Vec::new();
    let mut routed = Vec::new();
    let place = |at: &Vec<(String, (i64, i64))>, pointer: &str| {
        at.iter()
            .find(|(id, _)| id == pointer)
            .map(|(_, point)| *point)
    };
    for event in events {
        match event {
            OsEvent::PointerMove {
                pointer,
                device,
                to,
            } => {
                let point = (i64::from(to.x), i64::from(to.y));
                at.retain(|(id, _)| *id != pointer);
                at.push((pointer.clone(), point));
                if device != PointerDevice::Touch || down.contains(&pointer) {
                    routed.push(Routed {
                        kind: InputKind::PointerMove,
                        device: class_of(device),
                        at: Some(point),
                    });
                }
            }
            OsEvent::PointerDown {
                pointer, device, ..
            } => {
                routed.push(Routed {
                    kind: InputKind::PointerDown,
                    device: class_of(device),
                    at: place(&at, &pointer),
                });
                down.push(pointer);
            }
            OsEvent::PointerUp {
                pointer, device, ..
            } => {
                routed.push(Routed {
                    kind: InputKind::PointerUp,
                    device: class_of(device),
                    at: place(&at, &pointer),
                });
                down.retain(|id| *id != pointer);
            }
            OsEvent::KeyDown(_) => routed.push(Routed {
                kind: InputKind::KeyDown,
                device: DeviceClass::Keyboard,
                at: None,
            }),
            OsEvent::KeyUp(_) => routed.push(Routed {
                kind: InputKind::KeyUp,
                device: DeviceClass::Keyboard,
                at: None,
            }),
            OsEvent::Text(_) => routed.push(Routed {
                kind: InputKind::Text,
                device: DeviceClass::Ime,
                at: None,
            }),
            OsEvent::Scroll { at, .. } => routed.push(Routed {
                kind: InputKind::Scroll,
                device: DeviceClass::Mouse,
                at: Some((i64::from(at.x), i64::from(at.y))),
            }),
            _ => {}
        }
    }
    routed
}

/// The events the app's agent delivered for `sequence`, as its window's journal recorded them.
fn through_the_agent(driver: &mut Driver, sequence: ActionSequence) -> Vec<Routed> {
    let window = driver
        .windows()
        .expect("the app lists its windows")
        .into_iter()
        .next()
        .expect("one window");
    let before = driver
        .window(window)
        .journal()
        .expect("the journal reads")
        .span
        .map_or(0, |span| span.to);
    driver
        .call::<method::InputInject>(InputParams::new(window, sequence))
        .expect("the agent plays it");
    driver
        .window(window)
        .journal()
        .expect("the journal reads")
        .entries
        .into_iter()
        .filter(|entry| entry.seq > before)
        .map(|entry| Routed {
            kind: entry.kind,
            device: entry.device,
            at: entry.at.map(device_pixel),
        })
        .collect()
}

/// Each gesture kind's sequence, as `gestures` composes it, routes the same events through the
/// agent's injection and the OS lowering.
#[test]
fn each_gesture_kind_routes_the_same_events_through_the_agent_and_the_os_lowering() {
    let point = LogicalPoint::new(200.0, 150.0);
    let aim = Aim::new(point, Some(LogicalRect::new(150.0, 100.0, 100.0, 100.0)));
    let cases: [(&str, ActionSequence); 9] = [
        ("tap", gestures::tap(point, PointerButton::Primary)),
        (
            "double tap",
            gestures::double_tap(point, PointerButton::Primary),
        ),
        (
            "long press",
            gestures::long_press(point, Some(100), &Defaults::WINDOWLESS),
        ),
        (
            "drag",
            gestures::drag(point, LogicalPoint::new(300.0, 260.0)),
        ),
        (
            "swipe",
            gestures::swipe(aim, Direction::Up, None, None, false, &Defaults::WINDOWLESS)
                .expect("a swipe"),
        ),
        (
            "fling",
            gestures::swipe(
                aim,
                Direction::Left,
                Some(120.0),
                None,
                true,
                &Defaults::WINDOWLESS,
            )
            .expect("a fling"),
        ),
        ("pinch", gestures::pinch(aim, 0.5).expect("a pinch")),
        (
            "chords",
            gestures::chords(&["Shift+a".to_owned(), "Enter".to_owned()]).expect("chords"),
        ),
        ("text", gestures::text("hi")),
    ];
    for (case, sequence) in cases {
        let probe = Probe::default();
        let mut driver = Driver::launch(Launch::headless(move || Counter { probe }))
            .expect("the headless app starts");
        let lane = through_the_lane(&sequence);
        let agent = through_the_agent(&mut driver, sequence);
        assert!(!lane.is_empty(), "{case} sends something");
        assert_eq!(agent, lane, "{case}");
    }
}
