//! `input::gestures`: the one composition of each gesture method into an action sequence, which
//! the app's agent and the driver's OS lane both play (ADR-0400 §2); and what it refuses.

use flui_protocol::geometry::{LogicalPoint, LogicalRect};
use flui_protocol::input::gestures::{self, Aim, GestureError};
use flui_protocol::input::{
    ActionSequence, Direction, InputSource, KeyAction, PointerAction, PointerButton, PointerDevice,
    ScrollUnit, WheelAction,
};

fn at(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y)
}

/// One pointer source's device and actions, each written `move x,y/ms`, `down`, `up`, `pause ms`.
fn written(sequence: &ActionSequence) -> Vec<(PointerDevice, Vec<String>)> {
    sequence
        .sources
        .iter()
        .map(|source| match source {
            InputSource::Pointer {
                device, actions, ..
            } => (
                *device,
                actions
                    .iter()
                    .map(|action| match action {
                        PointerAction::Move {
                            to, duration_ms, ..
                        } => format!("move {},{}/{duration_ms}", to.x, to.y),
                        PointerAction::Down { button, .. } => format!("down {button:?}"),
                        PointerAction::Up { button } => format!("up {button:?}"),
                        PointerAction::Pause { duration_ms } => format!("pause {duration_ms}"),
                        other => format!("{other:?}"),
                    })
                    .collect(),
            ),
            other => panic!("a pointer source: {other:?}"),
        })
        .collect()
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

/// Each pointer gesture, landing at (50, 40) on an element 100 × 40 wide where it has one, is the
/// sequence its method performs.
#[test]
fn each_pointer_gesture_composes_to_its_sequence() {
    let point = at(50.0, 40.0);
    let aim = Aim::new(point, Some(LogicalRect::new(0.0, 20.0, 100.0, 40.0)));
    let cases: [(&str, ActionSequence, Vec<(PointerDevice, Vec<String>)>); 9] = [
        (
            "tap",
            gestures::tap(point, PointerButton::Secondary),
            vec![(
                PointerDevice::Mouse,
                strings(&["move 50,40/0", "down Secondary", "up Secondary"]),
            )],
        ),
        (
            "double tap",
            gestures::double_tap(point, PointerButton::Primary),
            vec![(
                PointerDevice::Mouse,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "up Primary",
                    "pause 60",
                    "down Primary",
                    "up Primary",
                ]),
            )],
        ),
        (
            "long press, its duration named",
            gestures::long_press(point, Some(1_200)),
            vec![(
                PointerDevice::Touch,
                strings(&["move 50,40/0", "down Primary", "pause 1200", "up Primary"]),
            )],
        ),
        (
            "long press, the platform's delay and a margin",
            gestures::long_press(point, None),
            vec![(
                PointerDevice::Touch,
                strings(&["move 50,40/0", "down Primary", "pause 800", "up Primary"]),
            )],
        ),
        (
            "drag",
            gestures::drag(point, at(10.0, 90.0)),
            vec![(
                PointerDevice::Mouse,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move 10,90/300",
                    "up Primary",
                ]),
            )],
        ),
        (
            "swipe: most of the extent along the stroke, at the fling velocity, held still",
            gestures::swipe(aim, Direction::Up, None, None, false).expect("a swipe"),
            vec![(
                PointerDevice::Touch,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move 50,8/12",
                    "pause 120",
                    "up Primary",
                ]),
            )],
        ),
        (
            "fling: the distance and velocity named, lifted moving",
            gestures::swipe(aim, Direction::Right, Some(200.0), Some(1_000.0), true)
                .expect("a fling"),
            vec![(
                PointerDevice::Touch,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move 250,40/200",
                    "up Primary",
                ]),
            )],
        ),
        (
            "a slow stroke is at most ten seconds",
            gestures::swipe(aim, Direction::Left, Some(1_000.0), Some(1.0), true)
                .expect("a slow fling"),
            vec![(
                PointerDevice::Touch,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move -950,40/10000",
                    "up Primary",
                ]),
            )],
        ),
        (
            "pinch: two contacts a quarter of the shorter side out, spread by the scale",
            gestures::pinch(aim, 2.0).expect("a pinch"),
            vec![
                (
                    PointerDevice::Touch,
                    strings(&[
                        "move 40,40/0",
                        "down Primary",
                        "move 30,40/300",
                        "up Primary",
                    ]),
                ),
                (
                    PointerDevice::Touch,
                    strings(&[
                        "move 60,40/0",
                        "down Primary",
                        "move 70,40/300",
                        "up Primary",
                    ]),
                ),
            ],
        ),
    ];
    for (case, sequence, expected) in cases {
        assert_eq!(written(&sequence), expected, "{case}");
        assert!(
            flui_protocol::input::plan(&sequence).is_ok(),
            "{case} plans: {sequence:?}"
        );
    }
}

/// Key chords go down in order and come up in reverse, one chord after another; text is one
/// commit; a scroll step turns the wheel by the distance towards its direction.
#[test]
fn keys_text_and_a_scroll_step_compose_to_their_sequences() {
    let chords =
        gestures::chords(&["Control+a".to_owned(), "Enter".to_owned()]).expect("two chords");
    let [InputSource::Keys { actions, .. }] = chords.sources.as_slice() else {
        panic!("one keyboard: {chords:?}");
    };
    let key = |down: bool, key: &str| {
        if down {
            KeyAction::Down {
                key: key.to_owned(),
            }
        } else {
            KeyAction::Up {
                key: key.to_owned(),
            }
        }
    };
    assert_eq!(
        actions,
        &[
            key(true, "Control"),
            key(true, "a"),
            key(false, "a"),
            key(false, "Control"),
            key(true, "Enter"),
            key(false, "Enter"),
        ]
    );
    assert_eq!(
        gestures::text("héllo").sources,
        [InputSource::Text {
            text: "héllo".to_owned()
        }]
    );
    let step = gestures::scroll(at(5.0, 6.0), Direction::Down, 120.0);
    let [InputSource::Wheel { actions, .. }] = step.sources.as_slice() else {
        panic!("one wheel: {step:?}");
    };
    assert_eq!(
        actions,
        &[WheelAction::Scroll {
            at: at(5.0, 6.0),
            delta_x: 0.0,
            delta_y: 120.0,
            unit: ScrollUnit::Pixel,
        }]
    );
}

/// What makes no sequence is refused, naming why: a stroke that goes nowhere, a pinch to no
/// scale, a chord of no key.
#[test]
fn parameters_that_make_no_gesture_are_refused() {
    let aim = Aim::new(at(1.0, 1.0), Some(LogicalRect::new(0.0, 0.0, 0.0, 0.0)));
    let nowhere = gestures::swipe(aim, Direction::Down, None, None, false)
        .expect_err("an element with no extent and no distance named");
    assert!(
        matches!(nowhere, GestureError::StrokeGoesNowhere { distance, .. } if distance == 0.0),
        "{nowhere:?}"
    );
    for velocity in [0.0, f64::NAN, f64::INFINITY] {
        let refused = gestures::swipe(aim, Direction::Down, Some(10.0), Some(velocity), true);
        assert!(
            matches!(refused, Err(GestureError::StrokeGoesNowhere { .. })),
            "{velocity}: {refused:?}"
        );
    }
    for scale in [0.0, -1.0, f64::NAN] {
        let refused = gestures::pinch(aim, scale);
        assert!(
            matches!(refused, Err(GestureError::NoScale { .. })),
            "{scale}: {refused:?}"
        );
    }
    let refused =
        gestures::chords(&["Shift+".to_owned(), "+".to_owned()]).expect_err("a chord of no key");
    assert_eq!(
        refused,
        GestureError::EmptyChord {
            chord: "+".to_owned()
        },
        "a trailing `+` names no key of its own; a lone one names none"
    );
}
