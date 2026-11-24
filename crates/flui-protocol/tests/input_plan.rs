//! `input::plan`, the one lowering of an action sequence every path plays (ADR-0400 §2): one row
//! per gesture kind, each with the timeline it plans to, and the sequences it refuses.

use std::time::Duration;

use flui_protocol::geometry::LogicalPoint;
use flui_protocol::input::{
    ActionSequence, ImeAction, InputSource, KeyAction, MAX_SOURCES, MAX_SPAN, MAX_STEPS, MAX_TICKS,
    PlanError, PlannedInput, PointerAction, PointerButton, PointerDevice, STEP, ScrollUnit,
    SourceKind, TrackpadAction, WheelAction, plan,
};

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

fn at(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y)
}

fn pointer(id: &str, device: PointerDevice, actions: Vec<PointerAction>) -> InputSource {
    InputSource::Pointer {
        id: id.to_owned(),
        device,
        actions,
    }
}

fn move_to(x: f64, y: f64, duration_ms: u64) -> PointerAction {
    PointerAction::Move {
        to: at(x, y),
        duration_ms,
        pressure: None,
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

/// One planned input as a line: its instant in milliseconds, its source and what it is.
fn line(at_ms: u128, source: usize, input: &PlannedInput) -> String {
    let what = match input {
        PlannedInput::PointerDown { at, .. } => format!("down {at:?}"),
        PlannedInput::PointerUp { at, .. } => format!("up {at:?}"),
        PlannedInput::PointerMove { to, .. } => format!("move {} {}", to.x, to.y),
        PlannedInput::PointerCancel { .. } => "cancel".to_owned(),
        PlannedInput::Scroll {
            delta_x, delta_y, ..
        } => format!("scroll {delta_x} {delta_y}"),
        PlannedInput::KeyDown(key) => format!("key_down {key}"),
        PlannedInput::KeyUp(key) => format!("key_up {key}"),
        PlannedInput::Text(text) => format!("text {text}"),
        PlannedInput::Ime(_) => "ime".to_owned(),
        PlannedInput::Trackpad(_) => "trackpad".to_owned(),
        other => format!("{other:?}"),
    };
    format!("{at_ms} s{source} {what}")
}

fn timeline(sequence: &ActionSequence) -> (Vec<String>, Duration) {
    let planned = plan(sequence).expect("the sequence plans");
    let lines = planned
        .inputs()
        .iter()
        .map(|input| line(input.at.as_millis(), input.source, &input.input))
        .collect();
    (lines, planned.span())
}

#[test]
fn each_gesture_kind_plans_to_one_timeline() {
    let here = Some(at(10.0, 20.0));
    let table: Vec<(&str, ActionSequence, Vec<String>, Duration)> = vec![
        (
            "a tap: the move lasts one step, the press and release share the next tick's start",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![move_to(10.0, 20.0, 0), down(), up()],
            )]),
            vec![
                "0 s0 move 10 20".to_owned(),
                format!("16 s0 down {here:?}"),
                format!("16 s0 up {here:?}"),
            ],
            STEP,
        ),
        (
            "a long press: the hold is a pause",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Touch,
                vec![
                    move_to(10.0, 20.0, 0),
                    down(),
                    PointerAction::Pause { duration_ms: 800 },
                    up(),
                ],
            )]),
            vec![
                "0 s0 move 10 20".to_owned(),
                format!("16 s0 down {here:?}"),
                format!("816 s0 up {here:?}"),
            ],
            ms(816),
        ),
        (
            "a drag: a move of 40 ms is three steps, the last at its end point",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Pen,
                vec![move_to(0.0, 0.0, 0), down(), move_to(30.0, 0.0, 40), up()],
            )]),
            vec![
                "0 s0 move 0 0".to_owned(),
                "16 s0 down Some(LogicalPoint { x: 0.0, y: 0.0 })".to_owned(),
                "16 s0 move 10 0".to_owned(),
                "29 s0 move 20 0".to_owned(),
                "42 s0 move 30 0".to_owned(),
                "56 s0 up Some(LogicalPoint { x: 30.0, y: 0.0 })".to_owned(),
            ],
            ms(56),
        ),
        (
            "a pinch: two contacts' steps due in one millisecond go out together, in source order",
            ActionSequence::new(vec![
                pointer(
                    "a",
                    PointerDevice::Touch,
                    vec![move_to(40.0, 0.0, 0), down(), move_to(20.0, 0.0, 32), up()],
                ),
                pointer(
                    "b",
                    PointerDevice::Touch,
                    vec![move_to(60.0, 0.0, 0), down(), move_to(80.0, 0.0, 32), up()],
                ),
            ]),
            vec![
                "0 s0 move 40 0".to_owned(),
                "0 s1 move 60 0".to_owned(),
                "16 s0 down Some(LogicalPoint { x: 40.0, y: 0.0 })".to_owned(),
                "16 s1 down Some(LogicalPoint { x: 60.0, y: 0.0 })".to_owned(),
                "16 s0 move 30 0".to_owned(),
                "16 s1 move 70 0".to_owned(),
                "32 s0 move 20 0".to_owned(),
                "32 s1 move 80 0".to_owned(),
                "48 s0 up Some(LogicalPoint { x: 20.0, y: 0.0 })".to_owned(),
                "48 s1 up Some(LogicalPoint { x: 80.0, y: 0.0 })".to_owned(),
            ],
            ms(48),
        ),
        (
            "a scroll, a chord and text: instants at their tick's start, in source order",
            ActionSequence::new(vec![
                InputSource::Wheel {
                    id: "w".to_owned(),
                    actions: vec![WheelAction::Scroll {
                        at: at(5.0, 5.0),
                        delta_x: 0.0,
                        delta_y: 120.0,
                        unit: ScrollUnit::Pixel,
                    }],
                },
                InputSource::Keys {
                    id: "k".to_owned(),
                    actions: vec![
                        KeyAction::Down {
                            key: "Control".to_owned(),
                        },
                        KeyAction::Pause { duration_ms: 30 },
                        KeyAction::Up {
                            key: "Control".to_owned(),
                        },
                    ],
                },
                InputSource::Text {
                    text: "hi".to_owned(),
                },
            ]),
            vec![
                "0 s0 scroll 0 120".to_owned(),
                "0 s1 key_down Control".to_owned(),
                "0 s2 text hi".to_owned(),
                "30 s1 key_up Control".to_owned(),
            ],
            ms(30),
        ),
        (
            "a composition and a trackpad pinch: one step per tick",
            ActionSequence::new(vec![
                InputSource::Ime {
                    actions: vec![
                        ImeAction::SetComposition {
                            text: "に".to_owned(),
                            cursor: None,
                            caret_hidden: false,
                        },
                        ImeAction::Commit {
                            text: "に".to_owned(),
                        },
                    ],
                },
                InputSource::Trackpad {
                    id: "t".to_owned(),
                    actions: vec![
                        TrackpadAction::Begin { at: at(1.0, 1.0) },
                        TrackpadAction::Pinch { scale: 1.5 },
                        TrackpadAction::End,
                    ],
                },
            ]),
            vec![
                "0 s0 ime".to_owned(),
                "0 s1 trackpad".to_owned(),
                "0 s0 ime".to_owned(),
                "0 s1 trackpad".to_owned(),
                "0 s1 trackpad".to_owned(),
            ],
            Duration::ZERO,
        ),
        (
            "a virtual pointer plans as any other pointer",
            ActionSequence::new(vec![pointer(
                "v",
                PointerDevice::Virtual,
                vec![move_to(10.0, 20.0, 0), down(), up()],
            )]),
            vec![
                "0 s0 move 10 20".to_owned(),
                format!("16 s0 down {here:?}"),
                format!("16 s0 up {here:?}"),
            ],
            STEP,
        ),
    ];
    for (case, sequence, expected, span) in table {
        let (lines, planned_span) = timeline(&sequence);
        assert_eq!(lines, expected, "{case}");
        assert_eq!(planned_span, span, "{case}");
    }
}

#[test]
fn a_plan_names_its_sources_in_order() {
    let sequence = ActionSequence::new(vec![
        pointer("v", PointerDevice::Virtual, vec![move_to(1.0, 1.0, 0)]),
        InputSource::Text {
            text: "a".to_owned(),
        },
    ]);
    let planned = plan(&sequence).expect("plans");
    let kinds: Vec<(Option<&str>, SourceKind)> = planned
        .sources()
        .iter()
        .map(|source| (source.id.as_deref(), source.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            (Some("v"), SourceKind::Pointer(PointerDevice::Virtual)),
            (None, SourceKind::Text),
        ]
    );
}

#[test]
fn a_long_move_takes_at_most_the_step_bound() {
    let long = u64::try_from(STEP.as_millis()).expect("small") * u64::from(MAX_STEPS) * 4;
    let sequence = ActionSequence::new(vec![pointer(
        "p",
        PointerDevice::Mouse,
        vec![move_to(0.0, 0.0, 0), move_to(100.0, 0.0, long)],
    )]);
    let planned = plan(&sequence).expect("plans");
    let moves = planned
        .inputs()
        .iter()
        .filter(|input| matches!(input.input, PlannedInput::PointerMove { .. }))
        .count();
    assert_eq!(moves, 1 + MAX_STEPS as usize);
    assert_eq!(planned.span(), STEP + ms(long));
}

#[test]
fn a_sequence_no_path_plays_is_refused_whole() {
    let named = |id: &str| pointer(id, PointerDevice::Mouse, vec![down()]);
    let pressed = |pressure: f64| {
        ActionSequence::new(vec![pointer(
            "p",
            PointerDevice::Pen,
            vec![PointerAction::Down {
                button: PointerButton::Primary,
                pressure: Some(pressure),
            }],
        )])
    };
    let table: Vec<(&str, ActionSequence, PlanError)> = vec![
        (
            "too many sources",
            ActionSequence::new(
                (0..=MAX_SOURCES)
                    .map(|index| named(&format!("p{index}")))
                    .collect(),
            ),
            PlanError::TooManySources {
                sources: MAX_SOURCES + 1,
            },
        ),
        (
            "too many ticks",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![PointerAction::Pause { duration_ms: 0 }; MAX_TICKS + 1],
            )]),
            PlanError::TooManyTicks {
                ticks: MAX_TICKS + 1,
            },
        ),
        (
            "too long",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![PointerAction::Pause {
                    duration_ms: u64::try_from(MAX_SPAN.as_millis()).expect("small") + 1,
                }],
            )]),
            PlanError::TooLong {
                span: MAX_SPAN + ms(1),
            },
        ),
        (
            "two sources of one name",
            ActionSequence::new(vec![named("p"), named("p")]),
            PlanError::SharedName { id: "p".to_owned() },
        ),
        (
            "a point that is not a number",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![down(), move_to(f64::NAN, 0.0, 0)],
            )]),
            PlanError::OutOfRange {
                source: 0,
                tick: 1,
                field: "to",
            },
        ),
        (
            "a pressure above one",
            pressed(1.0 + f64::EPSILON),
            PlanError::OutOfRange {
                source: 0,
                tick: 0,
                field: "pressure",
            },
        ),
        (
            "a scroll whose delta is infinite",
            ActionSequence::new(vec![InputSource::Wheel {
                id: "w".to_owned(),
                actions: vec![WheelAction::Scroll {
                    at: at(0.0, 0.0),
                    delta_x: f64::INFINITY,
                    delta_y: 0.0,
                    unit: ScrollUnit::Line,
                }],
            }]),
            PlanError::OutOfRange {
                source: 0,
                tick: 0,
                field: "delta",
            },
        ),
    ];
    for (case, sequence, refused) in table {
        assert_eq!(plan(&sequence), Err(refused), "{case}");
    }
    for pressure in [0.0, 1.0] {
        assert!(
            plan(&pressed(pressure)).is_ok(),
            "a pressure of {pressure} is in range"
        );
    }
}
