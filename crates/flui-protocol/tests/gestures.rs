//! `input::gestures`: the one composition of each gesture method into an action sequence, which
//! the app's agent and the driver's OS lane both play (ADR-0400 §2); and what it refuses.

use flui_protocol::geometry::{LogicalPoint, LogicalRect};
use flui_protocol::input::gestures::{self, Aim, Defaults, GestureError};
use flui_protocol::input::{
    ActionSequence, Direction, InputSource, KeyAction, PointerAction, PointerButton, PointerDevice,
    ScrollUnit, WheelAction,
};

fn at(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y)
}

/// A pointer source as [`written`] writes it: its device and its actions.
type Written = (PointerDevice, Vec<String>);

/// One pointer source's device and actions, each written `move x,y/ms`, `down`, `up`, `pause ms`.
fn written(sequence: &ActionSequence) -> Vec<Written> {
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
    let mut touch = Defaults::WINDOWLESS;
    touch.pointer = PointerDevice::Touch;
    touch.min_drag = 18.0;
    let cases: [(&str, ActionSequence, Vec<Written>); 12] = [
        (
            "tap",
            gestures::tap(point, PointerButton::Secondary, &Defaults::WINDOWLESS),
            vec![(
                PointerDevice::Mouse,
                strings(&["move 50,40/0", "down Secondary", "up Secondary"]),
            )],
        ),
        (
            "tap where the primary pointer is touch",
            gestures::tap(point, PointerButton::Primary, &touch),
            vec![(
                PointerDevice::Touch,
                strings(&["move 50,40/0", "down Primary", "up Primary"]),
            )],
        ),
        (
            "drag where the primary pointer is touch",
            gestures::drag(point, at(50.0, 90.0), &touch).expect("past the touch slop"),
            vec![(
                PointerDevice::Touch,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move 50,90/300",
                    "pause 120",
                    "up Primary",
                ]),
            )],
        ),
        (
            "double tap",
            gestures::double_tap(point, PointerButton::Primary, &Defaults::WINDOWLESS),
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
            gestures::long_press(point, Some(1_200), &Defaults::WINDOWLESS),
            vec![(
                PointerDevice::Touch,
                strings(&["move 50,40/0", "down Primary", "pause 1200", "up Primary"]),
            )],
        ),
        (
            "long press, the windowless hold",
            gestures::long_press(point, None, &Defaults::WINDOWLESS),
            vec![(
                PointerDevice::Touch,
                strings(&["move 50,40/0", "down Primary", "pause 800", "up Primary"]),
            )],
        ),
        (
            "drag",
            gestures::drag(point, at(10.0, 90.0), &Defaults::WINDOWLESS).expect("a drag"),
            vec![(
                PointerDevice::Mouse,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move 10,90/300",
                    "pause 120",
                    "up Primary",
                ]),
            )],
        ),
        (
            "swipe: most of the room to the edge of what it aims at, at the fling velocity, held still",
            gestures::swipe(
                aim,
                Direction::Right,
                None,
                None,
                false,
                &Defaults::WINDOWLESS,
            )
            .expect("a swipe"),
            vec![(
                PointerDevice::Touch,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move 90,40/16",
                    "pause 120",
                    "up Primary",
                ]),
            )],
        ),
        (
            "swipe with less room than the least stroke: the least stroke",
            gestures::swipe(aim, Direction::Up, None, None, false, &Defaults::WINDOWLESS)
                .expect("a swipe"),
            vec![(
                PointerDevice::Touch,
                strings(&[
                    "move 50,40/0",
                    "down Primary",
                    "move 50,4/14",
                    "pause 120",
                    "up Primary",
                ]),
            )],
        ),
        (
            "fling: the distance and velocity named, lifted moving",
            gestures::swipe(
                aim,
                Direction::Right,
                Some(200.0),
                Some(1_000.0),
                true,
                &Defaults::WINDOWLESS,
            )
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
            gestures::swipe(
                aim,
                Direction::Left,
                Some(1_000.0),
                Some(1.0),
                true,
                &Defaults::WINDOWLESS,
            )
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
/// scale, a chord of no key, a chord that does not read as `+`-joined keys.
#[test]
fn parameters_that_make_no_gesture_are_refused() {
    let aim = Aim::new(at(1.0, 1.0), Some(LogicalRect::new(0.0, 0.0, 0.0, 0.0)));
    let nowhere = gestures::swipe(
        aim,
        Direction::Down,
        None,
        None,
        false,
        &Defaults::WINDOWLESS,
    )
    .expect_err("an element with no extent and no distance named");
    assert!(
        matches!(nowhere, GestureError::StrokeGoesNowhere { distance, .. } if distance == 0.0),
        "{nowhere:?}"
    );
    for velocity in [0.0, f64::NAN, f64::INFINITY] {
        let refused = gestures::swipe(
            aim,
            Direction::Down,
            Some(10.0),
            Some(velocity),
            true,
            &Defaults::WINDOWLESS,
        );
        assert!(
            matches!(refused, Err(GestureError::StrokeGoesNowhere { .. })),
            "{velocity}: {refused:?}"
        );
    }
    // A drag no longer than the slop of the pointer it plays with starts no drag.
    let mut touch = Defaults::WINDOWLESS;
    touch.pointer = PointerDevice::Touch;
    touch.min_drag = 18.0;
    for (defaults, to, refused) in [
        (Defaults::WINDOWLESS, at(3.0, 1.0), true),
        (Defaults::WINDOWLESS, at(3.1, 1.0), false),
        (touch, at(19.0, 1.0), true),
        (touch, at(19.5, 1.0), false),
    ] {
        let drag = gestures::drag(at(1.0, 1.0), to, &defaults);
        assert_eq!(
            matches!(drag, Err(GestureError::DragTooShort { .. })),
            refused,
            "{defaults:?} to {to:?}: {drag:?}"
        );
    }
    // A fling whose stroke the window chose is over in fewer than three moves of 16 ms is
    // refused, naming what to give; one whose distance and velocity the call named is played.
    let room = Aim::new(
        at(50.0, 50.0),
        Some(LogicalRect::new(0.0, 0.0, 100.0, 100.0)),
    );
    for (distance, velocity, refused) in [
        (None, None, true),
        (Some(40.0), None, true),
        (None, Some(1_000.0), false),
        (Some(40.0), Some(2_500.0), false),
        (Some(150.0), None, false),
    ] {
        let fling = gestures::swipe(
            room,
            Direction::Down,
            distance,
            velocity,
            true,
            &Defaults::WINDOWLESS,
        );
        assert_eq!(
            matches!(fling, Err(GestureError::FlingTooShort { .. })),
            refused,
            "{distance:?} at {velocity:?}: {fling:?}"
        );
        if let Err(error) = fling {
            assert!(
                error
                    .to_string()
                    .contains("name a longer distance or a slower velocity"),
                "{error}"
            );
        }
    }
    for scale in [0.0, -1.0, f64::NAN] {
        let refused = gestures::pinch(aim, scale);
        assert!(
            matches!(refused, Err(GestureError::NoScale { .. })),
            "{scale}: {refused:?}"
        );
    }
    let refused = gestures::chords(&["Enter".to_owned(), String::new()]).expect_err("no key");
    assert_eq!(
        refused,
        GestureError::EmptyChord {
            chord: String::new()
        }
    );
    // A `+` with no key after it, a key after a `+` key with no `+` between, and a key name with
    // space around it do not read as keys; none is dropped or typed as text.
    for chord in ["Shift+", "+a", "a++b", "Control + a", "Control+ a"] {
        let refused = gestures::chords(&[chord.to_owned()]).expect_err("an unreadable chord");
        assert_eq!(
            refused,
            GestureError::UnreadableChord {
                chord: chord.to_owned()
            },
            "{chord:?}"
        );
    }
    // A key that is neither a key name nor the text one key types is a mistyped name, refused
    // rather than typed into the focused field as its whole text.
    let not_keys = [
        ("Ctrl-a", "Ctrl-a"),
        ("Control-a", "Control-a"),
        ("Arrow Down", "Arrow Down"),
        ("Page_Down", "Page_Down"),
        ("Shift+ctrl", "ctrl"),
        ("Control+ab", "ab"),
    ];
    for (chord, key) in not_keys {
        let refused = gestures::chords(&[chord.to_owned()]).expect_err("not a key");
        assert_eq!(
            refused,
            GestureError::NotAKey {
                chord: chord.to_owned(),
                key: key.to_owned(),
            },
            "{chord:?}"
        );
    }
}

/// One key is a key name (an upper-case ASCII letter, then ASCII letters and digits) or the text
/// one key types (one character, and only the non-ASCII marks that join it); nothing else is.
#[test]
fn a_key_is_a_key_name_or_the_text_of_one_key() {
    let rows = [
        ("Enter", true),
        ("ArrowDown", true),
        ("F12", true),
        ("Ctrl", true),
        ("a", true),
        ("A", true),
        ("+", true),
        (" ", true),
        ("é", true),
        ("e\u{301}", true),
        ("E\u{301}", true),
        ("\u{1f44d}\u{1f3fd}", true),
        ("", false),
        ("ctrl", false),
        ("Ctrl-a", false),
        ("Page_Down", false),
        ("Arrow Down", false),
        ("ab", false),
        ("aé", false),
        ("e ", false),
        ("日本", false),
    ];
    for (key, one) in rows {
        assert_eq!(gestures::is_one_key(key), one, "{key:?}");
    }
}

/// A `+` where a key stands is the plus key, alone or after a modifier; a space is the space key.
#[test]
fn the_plus_key_and_the_space_key_read_as_keys() {
    let cases: [(&str, &[&str]); 5] = [
        ("+", &["+"]),
        ("Control++", &["Control", "+"]),
        ("Shift+Control++", &["Shift", "Control", "+"]),
        (" ", &[" "]),
        ("Shift+ ", &["Shift", " "]),
    ];
    for (chord, keys) in cases {
        let sequence = gestures::chords(&[chord.to_owned()]).expect("a chord that reads");
        let [InputSource::Keys { actions, .. }] = sequence.sources.as_slice() else {
            panic!("one keyboard: {sequence:?}");
        };
        let mut wanted: Vec<KeyAction> = keys
            .iter()
            .map(|key| KeyAction::Down {
                key: (*key).to_owned(),
            })
            .collect::<Vec<KeyAction>>();
        wanted.extend(keys.iter().rev().map(|key| KeyAction::Up {
            key: (*key).to_owned(),
        }));
        assert_eq!(actions, &wanted, "{chord:?}");
    }
}

/// A swipe naming no distance goes most of the way from its point to the edge of what it aims at,
/// whichever way it goes, and at least the least stroke; a stroke over in under a millisecond
/// lasts one.
#[test]
fn a_swipe_takes_its_room_in_each_direction() {
    let defaults = Defaults::WINDOWLESS;
    let rows = [
        (
            "left",
            Aim::new(
                at(50.0, 40.0),
                Some(LogicalRect::new(0.0, 20.0, 100.0, 40.0)),
            ),
            Direction::Left,
            "move 10,40/16",
        ),
        (
            "down",
            Aim::new(
                at(50.0, 25.0),
                Some(LogicalRect::new(10.0, 20.0, 100.0, 100.0)),
            ),
            Direction::Down,
            "move 50,101/30",
        ),
        (
            "up",
            Aim::new(
                at(50.0, 100.0),
                Some(LogicalRect::new(10.0, 20.0, 100.0, 100.0)),
            ),
            Direction::Up,
            "move 50,36/25",
        ),
    ];
    for (case, aim, direction, stroke) in rows {
        let swipe = gestures::swipe(aim, direction, None, None, false, &defaults)
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        let written = written(&swipe);
        let [(_, actions)] = written.as_slice() else {
            panic!("{case}: one source: {written:?}");
        };
        assert_eq!(actions[2], stroke, "{case}: {actions:?}");
    }
    let quick = gestures::swipe(
        Aim::new(at(50.0, 40.0), None),
        Direction::Right,
        Some(1.0),
        Some(10_000.0),
        false,
        &defaults,
    )
    .expect("a stroke");
    assert_eq!(written(&quick)[0].1[2], "move 51,40/1", "{quick:?}");
}

/// How long each gesture's parameters let a window hold the request past its timeout counts
/// what those parameters play: a stroke of no distance or no velocity lasts nothing; the focusing
/// tap counts only where keys have a target to focus.
#[test]
#[cfg(feature = "serde")]
fn a_gestures_play_bound_counts_what_its_parameters_play() {
    use flui_protocol::method::Addressed;
    use std::time::Duration;
    let target = serde_json::json!({ "element": "w3/e9" });
    let swipe = |distance: f64, velocity: f64| {
        serde_json::from_value::<flui_protocol::input::SwipeParams>(serde_json::json!({
            "target": target, "direction": "up", "distance": distance, "velocity": velocity,
            "timeout_ms": 1_000,
        }))
        .expect("a swipe reads")
        .holds_for()
    };
    let settle = Duration::from_millis(gestures::SWIPE_SETTLE_MS);
    // A move of no time, the stillness and the lift, each a 16 ms frame, past the timeout.
    let still = Duration::from_millis(1_000) + settle + Duration::from_millis(5 * 16);
    assert_eq!(swipe(0.0, 100.0), still, "no distance");
    assert_eq!(swipe(10.0, 0.0), still, "no velocity");
    let keys = |target: Option<&serde_json::Value>| {
        let mut params = serde_json::json!({
            "window": "w3", "keys": ["a"], "timeout_ms": 1_000,
        });
        if let Some(target) = target {
            params["target"] = target.clone();
        }
        serde_json::from_value::<flui_protocol::input::PressKeysParams>(params)
            .expect("press keys reads")
            .holds_for()
    };
    let tap =
        gestures::tap(at(0.0, 0.0), PointerButton::Primary, &Defaults::WINDOWLESS).plays_within();
    let untargeted = keys(None);
    assert_eq!(
        keys(Some(&target)),
        untargeted + tap,
        "keys with a target count the tap that focuses it"
    );
}
