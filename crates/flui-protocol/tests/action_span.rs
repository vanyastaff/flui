//! How long an action sequence plays: its ticks one after another, each as long as its longest
//! pause or move across the sources, which is what a server adds to an `input.inject` reply's
//! deadline.

use std::time::Duration;

use flui_protocol::method::Addressed;
use flui_protocol::{
    ActionSequence, InputParams, InputSource, KeyAction, LogicalPoint, PointerAction,
    PointerDevice, WindowId,
};

fn pointer(actions: Vec<PointerAction>) -> InputSource {
    InputSource::Pointer {
        id: "finger".to_owned(),
        device: PointerDevice::Touch,
        actions,
    }
}

fn keys(actions: Vec<KeyAction>) -> InputSource {
    InputSource::Keys {
        id: "keyboard".to_owned(),
        actions,
    }
}

fn glide(duration_ms: u64) -> PointerAction {
    PointerAction::Move {
        to: LogicalPoint::new(1.0, 1.0),
        duration_ms,
        pressure: None,
    }
}

#[test]
fn a_sequence_lasts_the_sum_of_its_ticks_longest_pause_or_move() {
    let cases: [(&str, Vec<InputSource>, usize, Duration); 5] = [
        ("nothing", Vec::new(), 0, Duration::ZERO),
        (
            "text is one tick of no time",
            vec![InputSource::Text {
                text: "hi".to_owned(),
            }],
            1,
            Duration::ZERO,
        ),
        (
            "each tick as long as its longest source",
            vec![
                pointer(vec![
                    glide(0),
                    PointerAction::Pause { duration_ms: 100 },
                    glide(50),
                ]),
                keys(vec![
                    KeyAction::Pause { duration_ms: 30 },
                    KeyAction::Down {
                        key: "a".to_owned(),
                    },
                    KeyAction::Pause { duration_ms: 200 },
                ]),
            ],
            3,
            Duration::from_millis(30 + 100 + 200),
        ),
        (
            "a shorter source leaves the longer one's ticks",
            vec![
                pointer(vec![glide(10), glide(20), glide(40)]),
                keys(vec![KeyAction::Pause { duration_ms: 5 }]),
            ],
            3,
            Duration::from_millis(10 + 20 + 40),
        ),
        (
            "durations past any clock saturate",
            vec![pointer(vec![
                PointerAction::Pause {
                    duration_ms: u64::MAX,
                };
                1001
            ])],
            1001,
            Duration::MAX,
        ),
    ];
    for (case, sources, ticks, span) in cases {
        let sequence = ActionSequence::new(sources);
        assert_eq!(sequence.ticks(), ticks, "{case}");
        assert_eq!(sequence.span(), span, "{case}");
        let params = InputParams::new(WindowId::from_u64(1).expect("non-zero"), sequence);
        assert_eq!(
            params.holds_for(),
            span,
            "{case}: the reply waits for the input"
        );
    }
}
