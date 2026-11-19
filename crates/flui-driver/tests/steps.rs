//! What an OS lane is handed: every move one step to its destination with an explicit pause after
//! it, a lasting move split by the driver (ADR-0238 §11).

use std::time::Duration;

use flui_driver::Error;
use flui_driver::coordinates::{DevicePoint, WindowGeometry};
use flui_driver::protocol::geometry::{LogicalPoint, LogicalSize};
use flui_driver::protocol::input::{
    ActionSequence, ImeAction, InputSource, PointerAction, PointerButton, PointerDevice,
};
use flui_driver::steps::{MAX_STEPS, OsEvent, STEP, os_events};

fn geometry() -> WindowGeometry {
    WindowGeometry::new(
        LogicalPoint::new(0.0, 0.0),
        LogicalSize::new(800.0, 600.0),
        2.0,
    )
}

fn pointer(actions: Vec<PointerAction>) -> ActionSequence {
    ActionSequence::new(vec![InputSource::Pointer {
        id: "p".into(),
        device: PointerDevice::Mouse,
        actions,
    }])
}

fn move_to(x: f64, y: f64, duration_ms: u64) -> PointerAction {
    PointerAction::Move {
        to: LogicalPoint::new(x, y),
        duration_ms,
        pressure: None,
    }
}

fn moved(x: i32, y: i32) -> OsEvent {
    contact_moved("p", PointerDevice::Mouse, x, y)
}

fn contact_moved(pointer: &str, device: PointerDevice, x: i32, y: i32) -> OsEvent {
    OsEvent::PointerMove {
        pointer: pointer.into(),
        device,
        to: DevicePoint::new(x, y),
    }
}

fn touch(id: &str, actions: Vec<PointerAction>) -> InputSource {
    InputSource::Pointer {
        id: id.into(),
        device: PointerDevice::Touch,
        actions,
    }
}

fn down() -> PointerAction {
    PointerAction::Down {
        button: PointerButton::Primary,
        pressure: None,
    }
}

fn up() -> PointerAction {
    PointerAction::Up {
        button: PointerButton::Primary,
    }
}

fn contact(pointer: &str, pressed: bool) -> OsEvent {
    if pressed {
        OsEvent::PointerDown {
            pointer: pointer.into(),
            device: PointerDevice::Touch,
            button: PointerButton::Primary,
        }
    } else {
        OsEvent::PointerUp {
            pointer: pointer.into(),
            device: PointerDevice::Touch,
            button: PointerButton::Primary,
        }
    }
}

#[test]
fn every_move_is_a_single_step_followed_by_a_pause() {
    let step = Duration::from_millis(16);
    let a = |x| contact_moved("a", PointerDevice::Touch, x, 200);
    let b = |x| contact_moved("b", PointerDevice::Touch, x, 200);
    let rows: Vec<(&str, ActionSequence, Vec<OsEvent>)> = vec![
        (
            "a move with no duration is one step and a pause of one step",
            pointer(vec![move_to(10.0, 20.0, 0)]),
            vec![moved(20, 40), OsEvent::Pause(STEP)],
        ),
        (
            "two contacts that move in one tick take each step together, then its pause",
            ActionSequence::new(vec![
                touch(
                    "a",
                    vec![
                        move_to(100.0, 100.0, 0),
                        down(),
                        move_to(40.0, 100.0, 32),
                        up(),
                    ],
                ),
                touch(
                    "b",
                    vec![
                        move_to(200.0, 100.0, 0),
                        down(),
                        move_to(260.0, 100.0, 32),
                        up(),
                    ],
                ),
            ]),
            vec![
                a(200),
                b(400),
                OsEvent::Pause(STEP),
                contact("a", true),
                contact("b", true),
                a(140),
                b(460),
                OsEvent::Pause(step),
                a(80),
                b(520),
                OsEvent::Pause(step),
                contact("a", false),
                contact("b", false),
            ],
        ),
        (
            "a lasting move is split into steps of its duration, each paused",
            pointer(vec![move_to(0.0, 0.0, 0), move_to(30.0, 0.0, 48)]),
            vec![
                moved(0, 0),
                OsEvent::Pause(STEP),
                moved(20, 0),
                OsEvent::Pause(Duration::from_millis(16)),
                moved(40, 0),
                OsEvent::Pause(Duration::from_millis(16)),
                moved(60, 0),
                OsEvent::Pause(Duration::from_millis(16)),
            ],
        ),
        (
            "a press and a release keep their place between the steps",
            pointer(vec![
                move_to(1.0, 1.0, 0),
                PointerAction::Down {
                    button: PointerButton::Primary,
                    pressure: None,
                },
                PointerAction::Pause { duration_ms: 5 },
                PointerAction::Up {
                    button: PointerButton::Primary,
                },
            ]),
            vec![
                moved(2, 2),
                OsEvent::Pause(STEP),
                OsEvent::PointerDown {
                    pointer: "p".into(),
                    device: PointerDevice::Mouse,
                    button: PointerButton::Primary,
                },
                OsEvent::Pause(Duration::from_millis(5)),
                OsEvent::PointerUp {
                    pointer: "p".into(),
                    device: PointerDevice::Mouse,
                    button: PointerButton::Primary,
                },
            ],
        ),
    ];
    for (row, sequence, expected) in rows {
        let events = os_events(&sequence, &geometry()).expect(row);
        assert_eq!(events, expected, "{row}");
    }
}

#[test]
fn a_very_long_move_takes_longer_steps_not_more_of_them() {
    let day = 24 * 60 * 60 * 1000;
    let events = os_events(
        &pointer(vec![move_to(0.0, 0.0, 0), move_to(100.0, 100.0, day)]),
        &geometry(),
    )
    .expect("a long move");
    let moves = events
        .iter()
        .filter(|event| matches!(event, OsEvent::PointerMove { .. }))
        .count();
    assert_eq!(moves, 1 + MAX_STEPS as usize);
    assert_eq!(
        events.last(),
        Some(&OsEvent::Pause(Duration::from_millis(
            day / u64::from(MAX_STEPS)
        )))
    );
}

#[test]
fn input_no_lane_sends_is_refused_before_any_event() {
    let ime = ActionSequence::new(vec![
        InputSource::Pointer {
            id: "p".into(),
            device: PointerDevice::Mouse,
            actions: vec![move_to(1.0, 1.0, 0)],
        },
        InputSource::Ime {
            actions: vec![ImeAction::Commit { text: "あ".into() }],
        },
    ]);
    let refused = os_events(&ime, &geometry());
    assert!(
        matches!(refused, Err(Error::NotSupported { capability: Some(ref capability), .. }) if capability == "os_ime_composition"),
        "{refused:?}"
    );
    let outside = os_events(
        &pointer(vec![move_to(0.0, 0.0, 0), move_to(900.0, 0.0, 32)]),
        &geometry(),
    );
    assert!(
        matches!(outside, Err(Error::OutsideTarget { .. })),
        "{outside:?}"
    );
}
