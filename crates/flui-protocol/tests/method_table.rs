//! The method table (ADR-0238 §1, R28a): one row per method, each named `group.verb`, sorted,
//! in exactly one tier, with at most 13 in the core set MCP lists by default.

use std::collections::BTreeSet;

use flui_protocol::method::{self, MAX_CORE};
use flui_protocol::version::BREAKING;
use flui_protocol::{
    method::METHODS, method::Method, method::MethodEffect, method::MethodRow,
    method::SemanticsRead, method::ServedBy, method::Tier, version::ProtocolVersion,
};

#[test]
fn every_method_is_named_group_dot_verb_once_and_listed_in_order() {
    let lower_snake = |part: &str| {
        !part.is_empty()
            && part.starts_with(|c: char| c.is_ascii_lowercase())
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    for row in METHODS {
        let (group, verb) = row
            .name
            .split_once('.')
            .unwrap_or_else(|| panic!("`{}` is not `group.verb`", row.name));
        assert!(
            lower_snake(group) && lower_snake(verb),
            "`{}` is not `group.verb` in lower snake case",
            row.name
        );
        assert_eq!((row.group(), row.verb()), (group, verb));
        assert_eq!(row.tool_name(), format!("{group}_{verb}"));
    }
    let names: Vec<&str> = METHODS.iter().map(|row| row.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        names, sorted,
        "METHODS is sorted by wire name with no name twice, the order MCP lists tools in"
    );
    let tools: BTreeSet<String> = METHODS.iter().map(MethodRow::tool_name).collect();
    assert_eq!(
        tools.len(),
        METHODS.len(),
        "no two methods share a tool name"
    );
}

#[test]
fn the_core_set_is_the_thirteen_tools_mcp_lists_by_default() {
    let core: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.tier == Tier::Core)
        .map(|row| row.name)
        .collect();
    assert!(
        core.len() <= MAX_CORE,
        "{} core methods: {core:?}",
        core.len()
    );
    assert_eq!(
        core,
        [
            "audit.screen",
            "capture.presented",
            "element.press_keys",
            "element.scroll_into_view",
            "element.tap",
            "element.type_text",
            "semantics.act",
            "semantics.locate",
            "semantics.read",
            "session.close",
            "session.launch",
            "wait.until",
            "windows.list",
        ],
    );
    // Each row has one tier by construction; every tier but the core is a `--caps` group a
    // client can name, so none may be empty of purpose here.
    for row in METHODS {
        assert!(
            Tier::ALL.contains(&row.tier),
            "{} has a known tier",
            row.name
        );
    }
}

#[test]
fn a_methods_row_is_what_its_type_declares() {
    let row = method::find("semantics.read").expect("semantics.read is a method");
    assert_eq!(row.name, SemanticsRead::NAME);
    assert_eq!(row.served_by, SemanticsRead::SERVED_BY);
    assert_eq!(row.effect, SemanticsRead::EFFECT);
    assert_eq!(row.tier, SemanticsRead::TIER);
    assert_eq!(
        (row.served_by, row.effect, row.tier),
        (ServedBy::Agent, MethodEffect::ReadOnly, Tier::Core)
    );
    assert!(method::find("semantics.write").is_none());
}

#[test]
fn what_ends_the_app_is_destructive_and_reads_never_act() {
    let destructive: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.effect == MethodEffect::Destructive)
        .map(|row| row.name)
        .collect();
    // A reload may replace the process, losing its state, as closing the session ends it.
    assert_eq!(destructive, ["run.reload", "session.close"]);
    for row in METHODS {
        let reads = [
            "read",
            "locate",
            "list",
            "journal",
            "placement",
            "capabilities",
        ]
        .contains(&row.verb())
            || ["inspect", "capture", "audit", "wait"].contains(&row.group());
        if reads {
            assert_eq!(
                row.effect,
                MethodEffect::ReadOnly,
                "{} only reads",
                row.name
            );
        }
    }
}

#[test]
fn the_ops_protocol_0_5_renamed_are_methods_now() {
    let break_0_5 = BREAKING
        .iter()
        .find(|change| change.version == ProtocolVersion::new(0, 5))
        .expect("0.5 is listed in BREAKING");
    assert_eq!(break_0_5.adr, "ADR-0238");
    for (old, new) in break_0_5.renamed {
        assert!(
            method::find(new).is_some(),
            "`{old}` became `{new}`, a method"
        );
        assert!(method::find(old).is_none(), "`{old}` is no method");
    }
}

#[test]
fn every_method_the_agent_serves_is_a_window_method_but_the_apps_own() {
    macro_rules! names {
        ($($ty:ident = $name:literal, $effect:ident, $tier:ident, $params:ty => $output:ty;)+) => {
            [$($name),+]
        };
    }
    let window_methods: &[&str] = &flui_protocol::for_each_window_method!(names);
    let apps_own = [
        "events.subscribe",
        "events.unsubscribe",
        "record.start",
        "record.stop",
        "windows.list",
    ];
    let expected: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.served_by == ServedBy::Agent && !apps_own.contains(&row.name))
        .map(|row| row.name)
        .collect();
    assert_eq!(window_methods, expected.as_slice());
}

#[test]
fn a_window_methods_parameters_name_the_window_they_address() {
    use std::time::Duration;

    use flui_protocol::{
        act::ActParams, geometry::LogicalPoint, locator::LocateParams, locator::Locator,
        locator::Target, method::Addressed, tree::ElementId, tree::ElementRef, tree::WindowId,
        wait::Condition, wait::WaitParams, wire::ActionName,
    };

    let window = WindowId::from_u64(3).expect("non-zero");
    let element = ElementRef::new(window, ElementId::from_u64(9).expect("non-zero"));
    let act = |target| ActParams::new(target, ActionName::Invoke);
    let mut named = Locator::default();
    named.window = Some(window);
    let table: &[(&str, &dyn Addressed, Option<WindowId>)] = &[
        ("an element", &act(Target::Element(element)), Some(window)),
        (
            "a point",
            &act(Target::Point {
                window,
                at: LogicalPoint::new(1.0, 2.0),
            }),
            Some(window),
        ),
        (
            "a locator naming none",
            &act(Target::Locator(Locator::default())),
            None,
        ),
        (
            "a locator naming one",
            &LocateParams::new(named),
            Some(window),
        ),
        (
            "a wait",
            &WaitParams::new(window, Condition::Settled),
            Some(window),
        ),
    ];
    for (case, params, expected) in table {
        assert_eq!(params.window(), *expected, "{case}");
    }
    assert_eq!(
        WaitParams::new(window, Condition::Settled)
            .timeout_ms(250)
            .holds_for(),
        Duration::from_millis(250),
        "a wait holds for its timeout"
    );
    assert_eq!(
        LocateParams::new(Locator::default()).holds_for(),
        Duration::ZERO,
        "a locate answers at the drain"
    );
}

/// Addressing parameters names the window where they name none, and keeps the one they name: a
/// locator's, a key press's with no target, never an element's, a point's or a field's.
#[cfg(feature = "serde")]
#[test]
fn addressing_names_a_window_only_where_the_parameters_name_none() {
    use flui_protocol::{
        act::ActParams, geometry::LogicalPoint, input::InputParams, input::PressKeysParams,
        input::TapParams, locator::LocateParams, locator::Locator, locator::Target,
        method::Addressed, tree::ElementId, tree::ElementRef, tree::WindowId, wire::ActionName,
    };

    let connection = WindowId::from_u64(3).expect("non-zero");
    let other = WindowId::from_u64(4).expect("non-zero");
    let element = ElementRef::new(other, ElementId::from_u64(9).expect("non-zero"));
    let point = Target::Point {
        window: other,
        at: LogicalPoint::new(1.0, 2.0),
    };
    let keys = |window: Option<&str>| -> PressKeysParams {
        serde_json::from_value(serde_json::json!({ "window": window, "keys": ["Enter"] }))
            .expect("press keys reads")
    };
    let table: Vec<(&str, Box<dyn Addressed>, WindowId)> = vec![
        (
            "a locator naming none",
            Box::new(LocateParams::new(Locator::default())),
            connection,
        ),
        (
            "a locator naming one",
            Box::new(LocateParams::new(Locator::default().in_window(other))),
            other,
        ),
        (
            "a gesture on a locator naming none",
            Box::new(TapParams::new(Locator::default())),
            connection,
        ),
        (
            "a gesture on an element",
            Box::new(TapParams::new(element)),
            other,
        ),
        (
            "an act at a point",
            Box::new(ActParams::new(point, ActionName::Invoke)),
            other,
        ),
        ("keys with no target", Box::new(keys(None)), connection),
        ("keys in a window", Box::new(keys(Some("w4"))), other),
        (
            "an injection",
            Box::new(InputParams::new(
                other,
                flui_protocol::input::ActionSequence::new(Vec::new()),
            )),
            other,
        ),
    ];
    for (case, mut params, expected) in table {
        params.address(connection);
        assert_eq!(params.window(), Some(expected), "{case}");
    }
}

/// What an observer may call and what a timeout lets a client resend is one rule over the
/// effects (ADR-0395): a read and an annotation leave the app as it was, an act and a destructive
/// call change it.
#[test]
fn only_an_act_or_a_destructive_call_changes_the_app() {
    let changes: Vec<(MethodEffect, bool)> = MethodEffect::ALL
        .iter()
        .map(|effect| (*effect, effect.changes_app()))
        .collect();
    assert_eq!(
        changes,
        [
            (MethodEffect::ReadOnly, false),
            (MethodEffect::Annotates, false),
            (MethodEffect::Acts, true),
            (MethodEffect::Destructive, true),
        ]
    );
}

/// A gesture is the agent's on the synthetic and semantic paths (ADR-0400 §2): every acting
/// `element.*` row is a window method, and what the window may hold it for is its act timeout and
/// then the time its input takes to play, which the conversation counts in the reply's deadline
/// (ADR-0288 §4).
#[cfg(feature = "serde")]
#[test]
fn every_gesture_is_a_window_method_held_for_its_act_timeout_and_its_play() {
    use std::time::Duration;

    use flui_protocol::{
        input::DragParams, input::GestureOptions, input::LongPressParams, input::PressKeysParams,
        input::ScrollUntilParams, input::TapParams, locator::Locator, locator::Target,
        method::Addressed, tree::ElementId, tree::ElementRef, tree::WindowId,
    };

    let gestures: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.group() == "element" && row.effect == MethodEffect::Acts)
        .map(|row| row.name)
        .collect();
    assert_eq!(
        gestures,
        [
            "element.double_tap",
            "element.drag_to",
            "element.fling",
            "element.long_press",
            "element.pinch",
            "element.press_keys",
            "element.scroll_into_view",
            "element.scroll_until",
            "element.swipe",
            "element.tap",
            "element.type_text",
        ]
    );
    for name in &gestures {
        let row = method::find(name).expect("a gesture is a method");
        assert_eq!(row.served_by, ServedBy::Agent, "{name}");
    }
    let driver_composed: Vec<&str> = METHODS
        .iter()
        .filter(|row| row.served_by == ServedBy::Driver)
        .map(|row| row.name)
        .collect();
    assert_eq!(
        driver_composed,
        [
            "audit.screen",
            "input.perform",
            "perf.expect",
            "session.attach",
            "session.capabilities",
            "session.close",
            "session.launch",
        ],
        "what only the driver composes: the `os` path of a gesture is `input.perform`'s"
    );

    let window = WindowId::from_u64(3).expect("non-zero");
    let element = Target::Element(ElementRef::new(
        window,
        ElementId::from_u64(9).expect("non-zero"),
    ));
    let mut tap = TapParams::new(element);
    assert_eq!(tap.window(), Some(window));
    // The parameters of a tap are a double tap's too, whose input plays the longer: six ticks of
    // a frame each, and the 60 ms gap between its taps.
    let double_tap_plays = Duration::from_millis(6 * 16 + 60);
    assert_eq!(
        tap.holds_for(),
        Duration::from_millis(GestureOptions::DEFAULT_TIMEOUT_MS) + double_tap_plays,
        "an act that names no timeout holds for the default, then plays"
    );
    tap.options.timeout_ms = Some(u64::MAX);
    assert_eq!(
        tap.holds_for(),
        Duration::from_millis(GestureOptions::MAX_TIMEOUT_MS) + double_tap_plays,
        "an act holds out for a day at most, then plays"
    );
    assert_eq!(
        tap.options.named_timeout_ms(),
        Some(GestureOptions::MAX_TIMEOUT_MS),
        "the timeout a composed act takes is bounded as the window's hold is"
    );
    tap.options.timeout_ms = None;
    assert_eq!(tap.options.named_timeout_ms(), None);
    let until = ScrollUntilParams::new(
        Target::Locator(Locator::identifier("inbox")),
        Locator::root(),
    );
    assert_eq!(
        until.window(),
        None,
        "a locator naming no window searches the only one"
    );

    let long_press: LongPressParams = serde_json::from_value(serde_json::json!({
        "target": {"element": "w3/e9"},
        "duration_ms": 800,
        "timeout_ms": 1000,
    }))
    .expect("a long press reads");
    assert_eq!(
        long_press.holds_for(),
        Duration::from_millis(1_000 + 800 + 4 * 16),
        "a long press holds for its timeout, then its press and a frame for each of its ticks"
    );
    let keys: PressKeysParams = serde_json::from_value(serde_json::json!({
        "window": "w3",
        "keys": ["Enter"],
    }))
    .expect("press keys reads");
    assert_eq!(keys.window(), Some(window), "no target: the window named");
    let drag: DragParams = serde_json::from_value(serde_json::json!({
        "target": {"point": {"window": "w3", "at": {"x": 1.0, "y": 2.0}}},
        "to": {"element": "w3/e9"},
    }))
    .expect("a drag reads");
    assert_eq!(drag.window(), Some(window));
}

/// Each gesture's parameters hold the request for the act's timeout and then for what its input
/// takes to play, from the parameters alone: a long press for the duration it names, a swipe
/// that names no distance for the longest stroke, a swipe that names one for that stroke, a
/// pinch, a drag, typing and keys after their focusing tap, a scroll step after its timeout, and
/// a semantics action for its timeout alone.
#[cfg(feature = "serde")]
#[test]
fn each_gesture_holds_for_its_timeout_and_the_input_its_parameters_play() {
    use std::time::Duration;

    use flui_protocol::geometry::LogicalPoint;
    use flui_protocol::input::gestures::{self, Aim};
    use flui_protocol::input::{
        ActionSequence, Direction, DragParams, InputSource, LongPressParams, PinchParams,
        PointerAction, PointerButton, PointerDevice, PressKeysParams, ScrollIntoViewParams,
        ScrollUntilParams, SwipeParams, TypeTextParams,
    };
    use flui_protocol::method::Addressed;

    /// How long parameters of `$params` read from `$json` hold the request.
    macro_rules! holds {
        ($params:ty, $json:expr) => {
            serde_json::from_value::<$params>($json)
                .expect("the parameters read")
                .holds_for()
        };
    }
    let origin = LogicalPoint::new(0.0, 0.0);
    let timeout = Duration::from_millis(1_000);
    let target = serde_json::json!({ "element": "w3/e9" });
    // A swipe whose move lasts `stroke_ms`, with its stillness before it lifts.
    let swipe = |stroke_ms: u64| {
        ActionSequence::new(vec![InputSource::Pointer {
            id: "s".to_owned(),
            device: PointerDevice::Touch,
            actions: vec![
                PointerAction::Move {
                    to: origin,
                    duration_ms: 0,
                    pressure: None,
                },
                PointerAction::Down {
                    button: PointerButton::Primary,
                    pressure: None,
                },
                PointerAction::Move {
                    to: origin,
                    duration_ms: stroke_ms,
                    pressure: None,
                },
                PointerAction::Pause {
                    duration_ms: gestures::SWIPE_SETTLE_MS,
                },
                PointerAction::Up {
                    button: PointerButton::Primary,
                },
            ],
        }])
        .plays_within()
    };
    let tap = gestures::tap(
        origin,
        PointerButton::Primary,
        &gestures::Defaults::WINDOWLESS,
    )
    .plays_within();
    let rows: [(&str, Duration, Duration); 11] = [
        (
            "a long press naming no duration: the longest a window may choose",
            holds!(
                LongPressParams,
                serde_json::json!({ "target": target, "timeout_ms": 1_000 })
            ),
            gestures::long_press(
                origin,
                Some(gestures::Defaults::LONGEST_LONG_PRESS_MS),
                &gestures::Defaults::WINDOWLESS,
            )
            .plays_within(),
        ),
        (
            "a long press of 5 s",
            holds!(
                LongPressParams,
                serde_json::json!({
                    "target": target, "duration_ms": 5_000, "timeout_ms": 1_000,
                })
            ),
            gestures::long_press(origin, Some(5_000), &gestures::Defaults::WINDOWLESS)
                .plays_within(),
        ),
        (
            "a swipe that names no distance",
            holds!(
                SwipeParams,
                serde_json::json!({
                    "target": target, "direction": "up", "timeout_ms": 1_000,
                })
            ),
            swipe(10_000),
        ),
        (
            "a swipe of 500 px at 1000 px/s",
            holds!(
                SwipeParams,
                serde_json::json!({
                    "target": target, "direction": "up", "distance": 500.0, "velocity": 1_000.0,
                    "timeout_ms": 1_000,
                })
            ),
            swipe(500),
        ),
        (
            "a swipe whose distance goes nowhere",
            holds!(
                SwipeParams,
                serde_json::json!({
                    "target": target, "direction": "up", "distance": -1.0, "velocity": 1_000.0,
                    "timeout_ms": 1_000,
                })
            ),
            swipe(0),
        ),
        (
            "a pinch",
            holds!(
                PinchParams,
                serde_json::json!({
                    "target": target, "scale": 2.0, "timeout_ms": 1_000,
                })
            ),
            gestures::pinch(Aim::new(origin, None), 2.0)
                .expect("a pinch")
                .plays_within(),
        ),
        (
            "a drag",
            holds!(
                DragParams,
                serde_json::json!({
                    "target": target, "to": target, "timeout_ms": 1_000,
                })
            ),
            gestures::drag(
                origin,
                flui_protocol::geometry::LogicalPoint::new(100.0, 0.0),
                &gestures::Defaults::WINDOWLESS,
            )
            .expect("a drag past the slop")
            .plays_within(),
        ),
        (
            "typing",
            holds!(
                TypeTextParams,
                serde_json::json!({
                    "target": target, "text": "hello", "timeout_ms": 1_000,
                })
            ),
            tap + gestures::text("hello").plays_within(),
        ),
        (
            "keys on a target",
            holds!(
                PressKeysParams,
                serde_json::json!({
                    "target": target, "keys": ["Control+a", "Enter"], "timeout_ms": 1_000,
                })
            ),
            tap + gestures::chords(&["Control+a".to_owned(), "Enter".to_owned()])
                .expect("two chords")
                .plays_within(),
        ),
        (
            "a scroll into view",
            holds!(
                ScrollIntoViewParams,
                serde_json::json!({
                    "target": target, "timeout_ms": 1_000,
                })
            ),
            Duration::ZERO,
        ),
        (
            "a scroll until",
            holds!(
                ScrollUntilParams,
                serde_json::json!({
                    "target": target, "until": { "root": true }, "timeout_ms": 1_000,
                })
            ),
            gestures::scroll(origin, Direction::Down, 1.0).plays_within(),
        ),
    ];
    for (case, held, plays) in rows {
        assert_eq!(held, timeout + plays, "{case}");
    }
    assert!(
        rows[1].1 >= timeout + Duration::from_millis(5_000),
        "a long press holds for the press it names"
    );
    assert!(
        rows[2].1 >= timeout + Duration::from_millis(10_000 + gestures::SWIPE_SETTLE_MS),
        "a swipe with no distance holds for the longest stroke"
    );
    assert!(
        rows[0].1 >= timeout + Duration::from_millis(gestures::Defaults::LONGEST_LONG_PRESS_MS),
        "a long press naming no duration holds for the longest press a window may choose"
    );
}
