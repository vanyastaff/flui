//! `input::plan`, the lowering the driver's OS lane plays (ADR-0400 §2): one row per gesture kind,
//! each with the timeline it plans to, the sequences it refuses and the bounds it accepts exactly.

use std::time::Duration;

use flui_protocol::geometry::LogicalPoint;
use flui_protocol::input::{
    ActionSequence, ImeAction, InputSource, KeyAction, MAX_COORDINATE, MAX_INPUTS, MAX_SOURCES,
    MAX_SPAN, MAX_STEPS, MAX_TICKS, PlanError, PlannedInput, PointerAction, PointerButton,
    PointerDevice, STEP, ScrollUnit, SourceKind, TimedInput, TrackpadAction, WheelAction, plan,
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

fn pause(duration_ms: u64) -> PointerAction {
    PointerAction::Pause { duration_ms }
}

/// The milliseconds of `duration`, which every bound here is a whole number of.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).expect("small")
}

/// Where a pointer is, `x,y`, or `nowhere` before its first step.
fn point(at: Option<LogicalPoint>) -> String {
    at.map_or_else(|| "nowhere".to_owned(), |at| format!("{},{}", at.x, at.y))
}

/// A pressure the input carries, ` p<pressure>`, or nothing.
fn pressed(pressure: Option<f64>) -> String {
    pressure.map_or_else(String::new, |pressure| format!(" p{pressure}"))
}

/// One planned input as a line: its instant in milliseconds, its tick, its source and what it is,
/// with every value it carries.
fn line(timed: &TimedInput) -> String {
    let what = match &timed.input {
        PlannedInput::PointerDown {
            button,
            pressure,
            at,
        } => format!("down {button:?} {}{}", point(*at), pressed(*pressure)),
        PlannedInput::PointerUp { button, at } => format!("up {button:?} {}", point(*at)),
        PlannedInput::PointerMove { to, pressure } => {
            format!("move {}{}", point(Some(*to)), pressed(*pressure))
        }
        PlannedInput::PointerCancel { at } => format!("cancel {}", point(*at)),
        PlannedInput::Scroll {
            at,
            delta_x,
            delta_y,
            unit,
        } => format!("scroll {} {delta_x} {delta_y} {unit:?}", point(Some(*at))),
        PlannedInput::KeyDown(key) => format!("key_down {key}"),
        PlannedInput::KeyUp(key) => format!("key_up {key}"),
        PlannedInput::Text(text) => format!("text {text}"),
        PlannedInput::Ime(action) => format!("ime {action:?}"),
        PlannedInput::Trackpad(action) => format!("trackpad {action:?}"),
        other => format!("{other:?}"),
    };
    format!(
        "{} t{} s{} {what}",
        timed.at.as_millis(),
        timed.tick,
        timed.source
    )
}

fn timeline(sequence: &ActionSequence) -> (Vec<String>, Duration) {
    let planned = plan(sequence).expect("the sequence plans");
    let lines = planned.inputs().iter().map(line).collect();
    (lines, planned.span())
}

fn lines(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|line| (*line).to_owned()).collect()
}

#[test]
fn each_gesture_kind_plans_to_one_timeline() {
    let table: Vec<(&str, ActionSequence, Vec<String>, Duration)> = vec![
        (
            "a tap: the move lasts one step, the press and release share the next tick's start",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![move_to(10.0, 20.0, 0), down(), up()],
            )]),
            lines(&[
                "0 t0 s0 move 10,20",
                "16 t1 s0 down Primary 10,20",
                "16 t2 s0 up Primary 10,20",
            ]),
            STEP,
        ),
        (
            "a long press: the hold is a pause",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Touch,
                vec![move_to(10.0, 20.0, 0), down(), pause(800), up()],
            )]),
            lines(&[
                "0 t0 s0 move 10,20",
                "16 t1 s0 down Primary 10,20",
                "816 t3 s0 up Primary 10,20",
            ]),
            ms(816),
        ),
        (
            "a drag: a move of 40 ms is three steps, the last at its end point",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Pen,
                vec![move_to(0.0, 0.0, 0), down(), move_to(30.0, 0.0, 40), up()],
            )]),
            lines(&[
                "0 t0 s0 move 0,0",
                "16 t1 s0 down Primary 0,0",
                "16 t2 s0 move 10,0",
                "29 t2 s0 move 20,0",
                "42 t2 s0 move 30,0",
                "56 t3 s0 up Primary 30,0",
            ]),
            ms(56),
        ),
        (
            "a pen stroke: the press and every step of a move carry their pressure",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Pen,
                vec![
                    move_to(0.0, 0.0, 0),
                    PointerAction::Down {
                        button: PointerButton::Primary,
                        pressure: Some(0.25),
                    },
                    PointerAction::Move {
                        to: at(20.0, 0.0),
                        duration_ms: 32,
                        pressure: Some(0.75),
                    },
                    up(),
                ],
            )]),
            lines(&[
                "0 t0 s0 move 0,0",
                "16 t1 s0 down Primary 0,0 p0.25",
                "16 t2 s0 move 10,0 p0.75",
                "32 t2 s0 move 20,0 p0.75",
                "48 t3 s0 up Primary 20,0",
            ]),
            ms(48),
        ),
        (
            "a cancel: the platform takes the contact where its last step put it",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Touch,
                vec![move_to(7.0, 8.0, 0), down(), PointerAction::Cancel],
            )]),
            lines(&[
                "0 t0 s0 move 7,8",
                "16 t1 s0 down Primary 7,8",
                "16 t2 s0 cancel 7,8",
            ]),
            STEP,
        ),
        (
            "a press before any move: the pointer is nowhere yet",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![down(), up(), PointerAction::Cancel],
            )]),
            lines(&[
                "0 t0 s0 down Primary nowhere",
                "0 t1 s0 up Primary nowhere",
                "0 t2 s0 cancel nowhere",
            ]),
            Duration::ZERO,
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
            lines(&[
                "0 t0 s0 move 40,0",
                "0 t0 s1 move 60,0",
                "16 t1 s0 down Primary 40,0",
                "16 t1 s1 down Primary 60,0",
                "16 t2 s0 move 30,0",
                "16 t2 s1 move 70,0",
                "32 t2 s0 move 20,0",
                "32 t2 s1 move 80,0",
                "48 t3 s0 up Primary 20,0",
                "48 t3 s1 up Primary 80,0",
            ]),
            ms(48),
        ),
        (
            "a scroll, a chord and text: instants at their tick's start, in source order",
            ActionSequence::new(vec![
                InputSource::Wheel {
                    id: "w".to_owned(),
                    actions: vec![WheelAction::Scroll {
                        at: at(5.0, 6.0),
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
            lines(&[
                "0 t0 s0 scroll 5,6 0 120 Pixel",
                "0 t0 s1 key_down Control",
                "0 t0 s2 text hi",
                "30 t2 s1 key_up Control",
            ]),
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
                format!(
                    "0 t0 s0 ime {:?}",
                    ImeAction::SetComposition {
                        text: "に".to_owned(),
                        cursor: None,
                        caret_hidden: false,
                    }
                ),
                format!(
                    "0 t0 s1 trackpad {:?}",
                    TrackpadAction::Begin { at: at(1.0, 1.0) }
                ),
                format!(
                    "0 t1 s0 ime {:?}",
                    ImeAction::Commit {
                        text: "に".to_owned(),
                    }
                ),
                format!(
                    "0 t1 s1 trackpad {:?}",
                    TrackpadAction::Pinch { scale: 1.5 }
                ),
                format!("0 t2 s1 trackpad {:?}", TrackpadAction::End),
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
            lines(&[
                "0 t0 s0 move 10,20",
                "16 t1 s0 down Primary 10,20",
                "16 t2 s0 up Primary 10,20",
            ]),
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

/// A plan says where each tick ends, so a player can hold each until then: a move a step at
/// least, a pause its duration, presses and releases nothing; the last end is the plan's span.
#[test]
fn a_plan_says_where_each_tick_ends() {
    let cases: [(&str, ActionSequence, &[u64]); 3] = [
        ("nothing", ActionSequence::default(), &[]),
        (
            "a press held",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Touch,
                vec![move_to(1.0, 1.0, 0), down(), pause(100), up()],
            )]),
            &[16, 16, 116, 116],
        ),
        (
            "two sources, the longer action of a tick setting its end",
            ActionSequence::new(vec![
                pointer(
                    "p",
                    PointerDevice::Mouse,
                    vec![pause(0), move_to(5.0, 5.0, 40)],
                ),
                InputSource::Keys {
                    id: "k".to_owned(),
                    actions: vec![
                        KeyAction::Pause { duration_ms: 30 },
                        KeyAction::Pause { duration_ms: 70 },
                    ],
                },
            ]),
            &[30, 100],
        ),
    ];
    for (case, sequence, ends) in cases {
        let planned = plan(&sequence).expect("plans");
        let planned_ends: Vec<u64> = planned.tick_ends().iter().copied().map(millis).collect();
        assert_eq!(planned_ends, ends, "{case}");
        assert_eq!(
            ends.last().copied().unwrap_or(0),
            millis(planned.span()),
            "{case}"
        );
    }
}

#[test]
fn a_long_move_takes_at_most_the_step_bound() {
    let long = millis(STEP) * u64::from(MAX_STEPS) * 4;
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

/// A sequence of one mouse whose ticks are a pause of `pause_ms` and then `moves` moves that name
/// no duration, each played for one [`STEP`].
fn paused_then_moved(pause_ms: u64, moves: usize) -> ActionSequence {
    let mut actions = vec![pause(pause_ms)];
    actions.extend((0..moves).map(|_| move_to(0.0, 0.0, 0)));
    ActionSequence::new(vec![pointer("p", PointerDevice::Mouse, actions)])
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
    let trackpad = |action: TrackpadAction| {
        ActionSequence::new(vec![InputSource::Trackpad {
            id: "t".to_owned(),
            actions: vec![action],
        }])
    };
    let out_of_range = |tick: usize, field: &'static str| PlanError::OutOfRange {
        source: 0,
        tick,
        field,
    };
    let past = MAX_COORDINATE + 1.0;
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
                vec![pause(0); MAX_TICKS + 1],
            )]),
            PlanError::TooManyTicks {
                ticks: MAX_TICKS + 1,
            },
        ),
        (
            "more inputs than the bound, within the span: 32 sources of 36 moves of 1024 steps",
            ActionSequence::new(
                (0..MAX_SOURCES)
                    .map(|index| {
                        pointer(
                            &format!("p{index}"),
                            PointerDevice::Mouse,
                            vec![move_to(1.0, 1.0, 16_384); 36],
                        )
                    })
                    .collect(),
            ),
            PlanError::TooManyInputs {
                inputs: MAX_SOURCES * 36 * 1024,
            },
        ),
        (
            "a pause past the span",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![pause(millis(MAX_SPAN) + 1)],
            )]),
            PlanError::TooLong {
                span: MAX_SPAN + ms(1),
            },
        ),
        (
            "moves that name no duration, played past the span the sequence names",
            paused_then_moved(millis(MAX_SPAN), 1),
            PlanError::TooLong {
                span: MAX_SPAN + STEP,
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
            out_of_range(1, "to"),
        ),
        (
            "a point past the coordinate bound",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![move_to(0.0, -past, 0)],
            )]),
            out_of_range(0, "to"),
        ),
        (
            "a press's pressure above one",
            pressed(1.0 + f64::EPSILON),
            out_of_range(0, "pressure"),
        ),
        (
            "a press's pressure below zero",
            pressed(-f64::EPSILON),
            out_of_range(0, "pressure"),
        ),
        (
            "a move's pressure above one",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Pen,
                vec![PointerAction::Move {
                    to: at(0.0, 0.0),
                    duration_ms: 0,
                    pressure: Some(1.5),
                }],
            )]),
            out_of_range(0, "pressure"),
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
            out_of_range(0, "delta"),
        ),
        (
            "a scroll past the coordinate bound",
            ActionSequence::new(vec![InputSource::Wheel {
                id: "w".to_owned(),
                actions: vec![WheelAction::Scroll {
                    at: at(past, 0.0),
                    delta_x: 0.0,
                    delta_y: 1.0,
                    unit: ScrollUnit::Pixel,
                }],
            }]),
            out_of_range(0, "at"),
        ),
        (
            "a trackpad gesture begun past the coordinate bound",
            trackpad(TrackpadAction::Begin { at: at(past, 0.0) }),
            out_of_range(0, "trackpad"),
        ),
        (
            "a trackpad pinch to nothing",
            trackpad(TrackpadAction::Pinch { scale: 0.0 }),
            out_of_range(0, "trackpad"),
        ),
        (
            "a trackpad pinch that is not a number",
            trackpad(TrackpadAction::Pinch { scale: f64::NAN }),
            out_of_range(0, "trackpad"),
        ),
        (
            "a trackpad rotation that is infinite",
            trackpad(TrackpadAction::Rotate {
                degrees: f64::INFINITY,
            }),
            out_of_range(0, "trackpad"),
        ),
        (
            "a trackpad pan that is not a number",
            trackpad(TrackpadAction::Pan {
                dx: 0.0,
                dy: f64::NAN,
            }),
            out_of_range(0, "trackpad"),
        ),
    ];
    for (case, sequence, refused) in table {
        assert_eq!(plan(&sequence), Err(refused), "{case}");
    }
}

#[test]
fn every_bound_is_inclusive() {
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
    let table: Vec<(&str, ActionSequence, Duration)> = vec![
        (
            "as many sources as the bound",
            ActionSequence::new(
                (0..MAX_SOURCES)
                    .map(|index| named(&format!("p{index}")))
                    .collect(),
            ),
            Duration::ZERO,
        ),
        (
            "as many ticks as the bound",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![pause(0); MAX_TICKS],
            )]),
            Duration::ZERO,
        ),
        (
            "a pause of the whole span",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![pause(millis(MAX_SPAN))],
            )]),
            MAX_SPAN,
        ),
        (
            "moves that name no duration, played to the span exactly",
            paused_then_moved(millis(MAX_SPAN) - 2 * millis(STEP), 2),
            MAX_SPAN,
        ),
        (
            "a point on the coordinate bound",
            ActionSequence::new(vec![pointer(
                "p",
                PointerDevice::Mouse,
                vec![
                    PointerAction::Move {
                        to: at(MAX_COORDINATE, -MAX_COORDINATE),
                        duration_ms: 1,
                        pressure: None,
                    },
                    down(),
                ],
            )]),
            ms(1),
        ),
        ("a pressure of zero", pressed(0.0), Duration::ZERO),
        ("a pressure of one", pressed(1.0), Duration::ZERO),
        (
            "as many inputs as the bound: two sources of 32 moves split into the most steps",
            ActionSequence::new(
                ["a", "b"]
                    .iter()
                    .map(|id| {
                        pointer(
                            id,
                            PointerDevice::Mouse,
                            vec![move_to(1.0, 1.0, 16_384); MAX_INPUTS / 2 / 1024],
                        )
                    })
                    .collect(),
            ),
            ms(16_384 * 32),
        ),
    ];
    for (case, sequence, span) in table {
        let planned = plan(&sequence).unwrap_or_else(|error| panic!("{case}: {error}"));
        assert_eq!(planned.span(), span, "{case}");
    }
}
