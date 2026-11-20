//! What an OS lane is handed: every move one step to its destination with an explicit pause after
//! it, a lasting move split by the driver (ADR-0238 §11).

use std::time::Duration;

use flui_driver::Error;
use flui_driver::Isolation;
use flui_driver::coordinates::{DevicePoint, WindowGeometry};
use flui_driver::lane::send_guarded;
use flui_driver::protocol::geometry::{LogicalPoint, LogicalSize};
use flui_driver::protocol::input::{
    ActionSequence, ImeAction, InputSource, PointerAction, PointerButton, PointerDevice,
    ScrollUnit, WheelAction,
};
use flui_driver::steps::{MAX_STEPS, OsEvent, STEP, os_events};
use flui_driver::testing::ScriptedLane;

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
            "a jump beside a stroke stays a jump, and the stroke keeps its pace",
            ActionSequence::new(vec![
                touch(
                    "a",
                    vec![move_to(100.0, 100.0, 0), move_to(150.0, 100.0, 0)],
                ),
                touch(
                    "b",
                    vec![move_to(200.0, 100.0, 0), move_to(232.0, 100.0, 32)],
                ),
            ]),
            vec![
                a(200),
                b(400),
                OsEvent::Pause(STEP),
                a(300),
                b(432),
                OsEvent::Pause(step),
                b(464),
                OsEvent::Pause(step),
            ],
        ),
        (
            "a short stroke beside a long one is not slowed to the long one's pace",
            ActionSequence::new(vec![
                touch(
                    "a",
                    vec![move_to(100.0, 100.0, 0), move_to(116.0, 100.0, 16)],
                ),
                touch(
                    "b",
                    vec![move_to(200.0, 100.0, 0), move_to(248.0, 100.0, 48)],
                ),
            ]),
            vec![
                a(200),
                b(400),
                OsEvent::Pause(STEP),
                a(232),
                b(432),
                OsEvent::Pause(step),
                b(464),
                OsEvent::Pause(step),
                b(496),
                OsEvent::Pause(step),
            ],
        ),
        (
            "steps of two strokes due within one millisecond go out together, with no shorter pause",
            ActionSequence::new(vec![
                touch(
                    "a",
                    vec![move_to(100.0, 100.0, 0), move_to(120.0, 100.0, 20)],
                ),
                touch(
                    "b",
                    vec![move_to(200.0, 100.0, 0), move_to(221.0, 100.0, 21)],
                ),
            ]),
            vec![
                a(200),
                b(400),
                OsEvent::Pause(STEP),
                a(220),
                b(421),
                OsEvent::Pause(Duration::from_millis(10)),
                a(240),
                b(442),
                OsEvent::Pause(Duration::from_millis(11)),
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
fn no_pause_is_shorter_than_a_millisecond_and_a_tick_keeps_its_length() {
    let strokes = ActionSequence::new(vec![
        touch(
            "a",
            vec![move_to(0.0, 100.0, 0), move_to(300.0, 100.0, 1000)],
        ),
        touch(
            "b",
            vec![move_to(0.0, 200.0, 0), move_to(300.0, 200.0, 1001)],
        ),
    ]);
    let events = os_events(&strokes, &geometry()).expect("two strokes");
    let pauses: Vec<Duration> = events
        .iter()
        .filter_map(|event| match event {
            OsEvent::Pause(pause) => Some(*pause),
            _ => None,
        })
        .collect();
    let short: Vec<&Duration> = pauses
        .iter()
        .filter(|pause| pause.subsec_nanos() % 1_000_000 != 0 || pause.is_zero())
        .collect();
    assert!(
        short.is_empty(),
        "every pause is whole milliseconds, at least one: {short:?}"
    );
    assert_eq!(
        pauses.iter().sum::<Duration>(),
        STEP + Duration::from_millis(1001),
        "the first tick's step, then the longer stroke's length exactly"
    );
    let moves = |pointer: &str| {
        events
            .iter()
            .filter(
                |event| matches!(event, OsEvent::PointerMove { pointer: p, .. } if p == pointer),
            )
            .count()
    };
    assert_eq!(
        (moves("a"), moves("b")),
        (1 + 63, 1 + 63),
        "no step is lost"
    );
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

/// A composition becomes its steps, one a tick, beside the other sources' events; a lane that
/// does not compose is refused it before any event is sent, at `send_guarded`, whatever the lane's
/// own check says.
#[test]
fn a_composition_reaches_only_a_lane_that_composes() {
    let ime = ActionSequence::new(vec![
        InputSource::Pointer {
            id: "p".into(),
            device: PointerDevice::Mouse,
            actions: vec![move_to(1.0, 1.0, 0)],
        },
        InputSource::Ime {
            actions: vec![
                ImeAction::SetComposition {
                    text: "あ".into(),
                    cursor: None,
                    caret_hidden: false,
                },
                ImeAction::Commit { text: "亜".into() },
            ],
        },
    ]);
    let events = os_events(&ime, &geometry()).expect("a composition's steps");
    let steps: Vec<&OsEvent> = events
        .iter()
        .filter(|event| matches!(event, OsEvent::Composition(_)))
        .collect();
    assert_eq!(
        steps,
        [
            &OsEvent::Composition(ImeAction::SetComposition {
                text: "あ".into(),
                cursor: None,
                caret_hidden: false,
            }),
            &OsEvent::Composition(ImeAction::Commit { text: "亜".into() }),
        ]
    );
    let (mut lane, log) = ScriptedLane::new(Isolation::HeadlessBrowser);
    let refused = send_guarded(&mut lane, &events);
    assert!(
        matches!(refused, Err(Error::NotSupported { capability: Some(ref capability), .. }) if capability == "os_ime_composition"),
        "{refused:?}"
    );
    assert!(log.sent().is_empty(), "nothing was sent: {:?}", log.sent());
}

/// A lane that does not compose, called directly rather than through `send_guarded`, refuses a
/// composition with a typed error instead of reporting it sent.
#[test]
fn a_lane_sent_a_composition_directly_refuses_it() {
    use flui_driver::Lane;

    let composition = OsEvent::Composition(ImeAction::Commit { text: "亜".into() });
    let refused = |lane: &mut dyn Lane| {
        let sent = lane.send(&composition);
        assert!(
            matches!(&sent, Err(Error::NotSupported { capability: Some(capability), .. }) if capability == "os_ime_composition"),
            "the {} lane: {sent:?}",
            lane.name()
        );
    };
    // The Android lane runs on every host, against a scripted device: refused before any command.
    let adb = crate::android_lane::scripted(true, true);
    let mut android = flui_driver::lane::android::AndroidLane::connect(
        adb.device(),
        crate::android_lane::PACKAGE,
    )
    .expect("the Android lane connects to the scripted device");
    refused(&mut android);
    let (mut scripted, log) = ScriptedLane::new(Isolation::Xvfb);
    refused(&mut scripted);
    assert!(log.sent().is_empty(), "the scripted lane recorded nothing");
    #[cfg(windows)]
    refused(
        &mut flui_driver::lane::windows::WindowsLane::try_new(
            std::process::id(),
            Isolation::Shared,
        )
        .expect("a Windows lane"),
    );
    #[cfg(target_os = "linux")]
    refused(
        &mut flui_driver::lane::x11::X11Lane::try_new(std::process::id(), None, Isolation::Xvfb)
            .expect("an X11 lane"),
    );
    #[cfg(target_os = "macos")]
    refused(
        &mut flui_driver::lane::macos::MacosLane::try_new(std::process::id(), Isolation::Shared)
            .expect("a macOS lane"),
    );
}

/// What no OS lane sends is refused before any event: a point outside the window.
#[test]
fn input_no_lane_sends_is_refused_before_any_event() {
    let outside = os_events(
        &pointer(vec![move_to(0.0, 0.0, 0), move_to(900.0, 0.0, 32)]),
        &geometry(),
    );
    assert!(
        matches!(outside, Err(Error::OutsideTarget { .. })),
        "{outside:?}"
    );
}

/// A scroll smaller than a device pixel is carried into the next, so small scrolls add up to what
/// they ask for in all, and one that adds up to no whole pixel yet sends nothing.
#[test]
fn small_scrolls_add_up_and_a_part_of_a_pixel_is_carried() {
    let scroll = |delta_y: f64| WheelAction::Scroll {
        at: LogicalPoint::new(10.0, 10.0),
        delta_x: 0.0,
        delta_y,
        unit: ScrollUnit::Pixel,
    };
    // At scale 2, a tenth of a logical pixel is a fifth of a device pixel.
    let wheel = ActionSequence::new(vec![InputSource::Wheel {
        id: "w".into(),
        actions: (0..5).map(|_| scroll(0.1)).collect(),
    }]);
    let scrolls: Vec<i32> = os_events(&wheel, &geometry())
        .expect("events")
        .into_iter()
        .filter_map(|event| match event {
            OsEvent::Scroll { delta_y, .. } => Some(delta_y),
            _ => None,
        })
        .collect();
    assert_eq!(
        scrolls,
        [1],
        "five fifths of a pixel are one pixel, sent once it is whole"
    );
    let alone = ActionSequence::new(vec![InputSource::Wheel {
        id: "w".into(),
        actions: vec![scroll(0.1)],
    }]);
    assert!(
        !os_events(&alone, &geometry())
            .expect("events")
            .iter()
            .any(|event| matches!(event, OsEvent::Scroll { .. })),
        "a part of a pixel alone sends no turn"
    );
}
