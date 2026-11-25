//! The session's input policy, the scenario's window rule and the OS-input announcement
//! (ADR-0238 §5, §11), on the headless target with a scripted OS lane.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_driver::lane;
use flui_driver::os_input::OsDevice;
use flui_driver::protocol::act::ActOutcome;
use flui_driver::protocol::error::{ErrorCode, ErrorObject};
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{
    ActionSequence, InputSource, PointerAction, PointerDevice, ScrollUnit, TapParams, WheelAction,
};
use flui_driver::protocol::locator::Target;
use flui_driver::protocol::method::{self, Method};
use flui_driver::protocol::report::Report;
use flui_driver::protocol::tree::WindowId;
use flui_driver::protocol::verdict::Verdict;
use flui_driver::steps::OsEvent;
use flui_driver::testing::{LaneLog, Reply, ScriptedAgent, ScriptedLane};
use flui_driver::{
    Driver, Error, InputPath, InputPolicy, Isolation, Launch, Locator, Scenario, WindowSpec,
};

use serde_json::json;

use crate::fixtures::{Counter, Probe};

fn counter(probe: &Probe) -> Launch {
    let probe = probe.clone();
    Launch::headless(move || Counter { probe })
}

/// A counter whose session has a guarded scripted lane on the shared desktop, and the lane's log.
fn with_lane(probe: &Probe, dir: &tempfile::TempDir) -> (Driver, LaneLog) {
    let (lane, log) = ScriptedLane::new(Isolation::Shared);
    let driver = Driver::launch(counter(probe))
        .expect("the headless app starts")
        .with_os_input_lock(dir.path().join("os-input.lock"))
        .with_lane(lane.guarded())
        .expect("the lane starts");
    (driver, log)
}

/// The session says what its lane can do, the accessibility API it reads through included; a
/// session with no lane has none.
#[test]
fn a_session_reports_what_its_lane_can_do() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let probe = Probe::default();
    let bare = Driver::launch(counter(&probe)).expect("the headless app starts");
    assert_eq!(bare.lane_capabilities(), None);
    let (driver, _log) = with_lane(&probe, &dir);
    let capabilities = driver.lane_capabilities().expect("a lane is attached");
    assert_eq!(capabilities.isolation, Isolation::Shared);
    assert_eq!(
        capabilities.accessibility, None,
        "the scripted lane reads no tree"
    );
    let read = lane::LaneCapabilities::new(
        vec![PointerDevice::Mouse],
        true,
        Isolation::Xvfb,
        lane::PositionFidelity::Exact,
    )
    .accessibility(lane::AccessibilityApi::AtSpi);
    assert_eq!(read.accessibility, Some(lane::AccessibilityApi::AtSpi));
}

fn a_move() -> ActionSequence {
    ActionSequence::new(vec![InputSource::Pointer {
        id: "p".into(),
        device: PointerDevice::Mouse,
        actions: vec![PointerAction::Move {
            to: LogicalPoint::new(5.0, 5.0),
            duration_ms: 0,
            pressure: None,
        }],
    }])
}

#[test]
fn a_default_session_never_selects_os_input() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let probe = Probe::default();
    let (mut driver, log) = with_lane(&probe, &dir);
    assert_eq!(driver.input_policy(), InputPolicy::default());
    assert_eq!(driver.input_policy().prefer, InputPath::Synthetic);
    assert!(!driver.input_policy().require);
    assert!(
        driver.capabilities().paths.contains(&InputPath::Os),
        "the target can take OS input: the lane is there"
    );

    let mut window = driver.main_window().expect("a window");
    let tapped = window
        .locate(Locator::identifier("increment"))
        .tap()
        .expect("a tap");
    assert_eq!(tapped.via, InputPath::Synthetic);
    let performed = window.perform(a_move()).expect("a move");
    assert_eq!(performed.via, InputPath::Synthetic);
    // A path preferred and missing falls back, and never to the OS: an action sequence has no
    // semantic form, so it goes synthetic.
    let fallen_back = window
        .perform_via(a_move(), InputPolicy::prefer(InputPath::Semantic))
        .expect("a move");
    assert_eq!(fallen_back.via, InputPath::Synthetic);

    assert!(log.sent().is_empty(), "the lane sent nothing");
    assert_eq!(log.arms(), 0, "its guard never armed");
    assert!(
        driver.take_announcements().is_empty(),
        "nothing was announced"
    );
    assert_eq!(probe.presses(), 1);
}

#[test]
fn a_certification_scenario_takes_the_os_path_announced_once_before_its_first_event() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let probe = Probe::default();
    let (mut driver, log) = with_lane(&probe, &dir);
    let scenario = Scenario::new("increments")
        .takes(
            [OsDevice::Mouse, OsDevice::Keyboard],
            Duration::from_secs(40),
        )
        .certification();
    let mut report = Report::new();
    driver.run_scenario(&mut report, "increments", |driver, report| {
        driver.declare(scenario)?;
        assert_eq!(driver.input_policy(), InputPolicy::require(InputPath::Os));
        let mut window = driver.main_window()?;
        for _ in 0..2 {
            let tapped = report.check("tap", || {
                window.locate(Locator::identifier("increment")).tap()
            })?;
            assert_eq!(tapped.via, InputPath::Os);
        }
        Ok(())
    });
    assert_eq!(report.verdict(), Verdict::Pass, "{report}");
    assert_eq!(report.isolation(), Some(Isolation::Shared));
    assert_eq!(report.lane(), Some("scripted"), "the report names the lane");
    assert_eq!(
        serde_json::to_value(&report).expect("a report serializes")["lane"],
        "scripted"
    );
    let announced = report.announcements();
    assert_eq!(
        announced.len(),
        1,
        "announced once per session: {announced:?}"
    );
    assert!(
        announced[0].contains("mouse and keyboard") && announced[0].contains("40 s"),
        "the scenario's devices and duration: {}",
        announced[0]
    );
    let sent = log.sent();
    assert!(
        matches!(sent.first(), Some(OsEvent::PointerMove { .. })),
        "{sent:?}"
    );
    assert_eq!(log.arms(), 2, "the guard watched each run");
}

#[test]
fn a_scenario_written_for_another_window_size_refuses_to_run() {
    let probe = Probe::default();
    let mut driver =
        Driver::launch(counter(&probe).window_size(640, 480)).expect("the headless app starts");
    let refused = driver.declare(Scenario::new("drop").window(WindowSpec::new(800.0, 600.0, 1.0)));
    match refused {
        Err(Error::InvalidArgument { ref message, .. }) => assert!(
            message.contains("800 x 600") && message.contains("640 x 480"),
            "it names what was expected and what the app has: {message}"
        ),
        other => panic!("{other:?}"),
    }
    let other_scale =
        driver.declare(Scenario::new("drop").window(WindowSpec::new(640.0, 480.0, 2.0)));
    assert!(
        matches!(other_scale, Err(Error::InvalidArgument { .. })),
        "{other_scale:?}"
    );
    driver
        .declare(Scenario::new("drop").window(WindowSpec::new(640.0, 480.0, 1.0)))
        .expect("the window it is written for");
}

#[test]
fn os_input_without_a_guard_runs_only_where_it_is_isolated() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let probe = Probe::default();
    for (isolation, admitted) in [
        (Isolation::Shared, false),
        (Isolation::Xvfb, true),
        (Isolation::Emulator, true),
    ] {
        let (lane, log) = ScriptedLane::new(isolation);
        let mut driver = Driver::launch(counter(&probe))
            .expect("the headless app starts")
            .with_os_input_lock(dir.path().join(format!("{}.lock", isolation.name())))
            .with_input_policy(InputPolicy::require(InputPath::Os))
            .with_lane(lane)
            .expect("the lane starts");
        let tapped = driver
            .main_window()
            .expect("a window")
            .locate(Locator::identifier("increment"))
            .tap();
        if admitted {
            assert_eq!(tapped.expect("an isolated run").via, InputPath::Os);
            assert!(
                matches!(log.sent().first(), Some(OsEvent::PointerMove { .. })),
                "the lane sent the tap"
            );
        } else {
            match tapped {
                Err(Error::NotSupported {
                    capability: Some(ref capability),
                    ref message,
                    ..
                }) => {
                    assert_eq!(capability, "user_activity_guard");
                    assert!(message.contains("Xvfb"), "it names where to run: {message}");
                }
                other => panic!("{isolation:?}: {other:?}"),
            }
            assert!(
                log.sent().is_empty(),
                "nothing was sent on the shared desktop"
            );
            assert!(
                driver.take_announcements().is_empty(),
                "a refused run announces nothing"
            );
        }
    }
}

#[test]
fn a_lane_is_admitted_by_its_guard_and_where_its_input_lands_at_every_send() {
    let an_event = || OsEvent::Pause(Duration::from_millis(1));
    let rows = [
        (
            "in process, even guarded",
            Isolation::InProcess,
            true,
            Some("os_input"),
        ),
        (
            "shared, unguarded",
            Isolation::Shared,
            false,
            Some("user_activity_guard"),
        ),
        ("shared, guarded", Isolation::Shared, true, None),
        ("an emulator, unguarded", Isolation::Emulator, false, None),
        (
            "a device in someone's hands, unguarded",
            Isolation::Device,
            false,
            Some("user_activity_guard"),
        ),
        ("a device, guarded", Isolation::Device, true, None),
    ];
    for (row, isolation, guarded, refused) in rows {
        let (lane, log) = ScriptedLane::new(isolation);
        let mut lane = if guarded { lane.guarded() } else { lane };
        let sent = lane::send_guarded(&mut lane, &[an_event()]);
        match (refused, sent) {
            (None, Ok(())) => assert_eq!(log.sent().len(), 1, "{row}"),
            (
                Some(expected),
                Err(Error::NotSupported {
                    capability: Some(capability),
                    ..
                }),
            ) => {
                assert_eq!(capability, expected, "{row}");
                assert!(log.sent().is_empty(), "{row}: nothing was sent");
            }
            (expected, sent) => panic!("{row}: expected refusal {expected:?}, got {sent:?}"),
        }
    }
}

/// A run that ends early and then cannot release what it pressed answers with what stays held,
/// keeping why it ended as the first failure its verdict comes from: the person hears of the key
/// left down, and a run they interrupted stays cannot-verify.
#[test]
fn input_left_held_is_named_whatever_ended_the_run() {
    let an_event = || OsEvent::Pause(Duration::from_millis(1));
    let (lane, _log) = ScriptedLane::new(Isolation::Shared);
    let mut lane = lane
        .foreign_input_after(1, "a mouse move")
        .failing_releases(1);
    match lane::send_guarded(&mut lane, &[an_event(), an_event()]) {
        Err(error @ Error::ReleaseFailed { .. }) => {
            assert!(error.is_cannot_verify(), "{error}");
            let Error::ReleaseFailed { held, message, .. } = error else {
                unreachable!("matched above");
            };
            assert_eq!(held, vec!["Control".to_owned()]);
            assert!(message.contains("a mouse move"), "{message}");
        }
        other => panic!("expected what stays held, got {other:?}"),
    }
    // A run that ends early and releases everything still answers why it ended.
    let (lane, _log) = ScriptedLane::new(Isolation::Shared);
    let mut lane = lane.foreign_input_after(1, "a mouse move");
    assert!(matches!(
        lane::send_guarded(&mut lane, &[an_event(), an_event()]),
        Err(Error::InterruptedByUser { .. })
    ));
}

#[test]
fn an_rtl_windows_button_is_aimed_at_from_the_surfaces_physical_left() {
    use flui_foundation::TextDirection;
    use flui_widgets::{Directionality, RawButton, Row, Text};

    let dir = tempfile::tempdir().expect("a temporary directory");
    let (lane, log) = ScriptedLane::new(Isolation::Shared);
    let mut driver = Driver::launch(Launch::headless(|| {
        Directionality::new(
            TextDirection::Rtl,
            Row::new((
                RawButton::new(Text::new("First"))
                    .identifier("first")
                    .on_pressed(|_| {}),
                RawButton::new(Text::new("Second")).on_pressed(|_| {}),
            )),
        )
    }))
    .expect("the headless app starts")
    .with_os_input_lock(dir.path().join("os-input.lock"))
    .with_input_policy(InputPolicy::require(InputPath::Os))
    .with_lane(lane.guarded())
    .expect("the lane starts");
    let mut window = driver.main_window().expect("a window");
    let point = window
        .locate(Locator::identifier("first"))
        .resolve()
        .expect("the first button")
        .actionability()
        .expect("its facts")
        .point
        .expect("a point");
    assert!(
        point.x > 400.0,
        "an RTL row puts its first child on the right: {point:?}"
    );
    window
        .locate(Locator::identifier("first"))
        .tap()
        .expect("an OS tap");
    let first_move = log.sent().into_iter().find_map(|event| match event {
        OsEvent::PointerMove { to, .. } => Some(to),
        _ => None,
    });
    // The scripted lane's screen is the surface, at scale 1: the device pixel is the point's.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a point inside an 800 by 600 surface"
    )]
    let expected = (point.x.floor() as i32, point.y.floor() as i32);
    assert_eq!(
        first_move.map(|to| (to.x, to.y)),
        Some(expected),
        "the OS path maps the point the app published, with no mirroring"
    );
}

/// A sequence the lane says it cannot send is refused before its guard arms and before any event
/// goes out, as a lane refuses a key it has no code for or a shell hotkey.
#[test]
fn a_sequence_the_lane_cannot_send_is_refused_before_any_event() {
    let (lane, log) = ScriptedLane::new(Isolation::Shared);
    let mut lane = lane.guarded().refusing("os_shell_hotkey");
    let events = [
        OsEvent::KeyDown("Meta".into()),
        OsEvent::KeyUp("Meta".into()),
    ];
    let refused = lane::send_guarded(&mut lane, &events);
    assert!(
        matches!(
            &refused,
            Err(Error::NotSupported { capability: Some(capability), .. })
                if capability == "os_shell_hotkey"
        ),
        "{refused:?}"
    );
    assert!(log.sent().is_empty(), "nothing was sent");
    assert_eq!(log.arms(), 0, "the guard never armed");
    assert_eq!(log.releases(), 0, "nothing was pressed to release");
}

/// A scripted agent whose hello serves `element.tap`, answering it after `delay`, and the
/// methods it was asked.
fn serving_taps(delay: Duration) -> (ScriptedAgent, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::SemanticsLocate::NAME,
            method::ElementTap::NAME,
        ],
        Box::new(move |name, _| {
            log.lock().expect("the log").push(name.to_owned());
            match name {
                "windows.list" => Reply::Result(json!({ "windows": ["w1"] })),
                "element.tap" => {
                    std::thread::sleep(delay);
                    Reply::Result(json!({ "via": "synthetic", "effect": { "kind": "ran" } }))
                }
                _ => Reply::Error(ErrorObject::new(ErrorCode::NotFound, "nothing matches")),
            }
        }),
    )
    .expect("the stand-in binds its endpoint");
    (agent, asked)
}

fn tap_increment() -> TapParams {
    TapParams::new(Target::Locator(Locator::identifier("increment")))
}

/// A gesture the app's agent lists in its hello goes to the agent, unless the call or the
/// session asks for the `os` path, which the driver composes with its lane; the capabilities list
/// a served gesture once and the driver's compositions of the rest.
#[test]
fn a_gesture_the_agent_serves_goes_to_it_except_on_the_os_path() {
    let (agent, asked) = serving_taps(Duration::ZERO);
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");

    let capabilities = driver.capabilities();
    let listed = |name: &str| {
        capabilities
            .methods
            .iter()
            .filter(|method| method.as_str() == name)
            .count()
    };
    assert_eq!(listed(method::ElementTap::NAME), 1, "{capabilities:?}");
    assert_eq!(
        listed(method::ElementDoubleTap::NAME),
        1,
        "{capabilities:?}"
    );

    let tapped = driver
        .call::<method::ElementTap>(tap_increment())
        .expect("the agent answers the tap");
    assert_eq!(tapped.via, InputPath::Synthetic);
    let taps = |asked: &Mutex<Vec<String>>| {
        asked
            .lock()
            .expect("the log")
            .iter()
            .filter(|name| name.as_str() == method::ElementTap::NAME)
            .count()
    };
    assert_eq!(taps(&asked), 1, "the served tap went to the agent");

    let mut on_os = tap_increment();
    on_os.options.via = Some(InputPolicy::require(InputPath::Os));
    let composed = driver.call::<method::ElementTap>(on_os);
    assert!(composed.is_err(), "the session has no lane: {composed:?}");
    driver.set_input_policy(InputPolicy::require(InputPath::Os));
    let composed = driver.call::<method::ElementTap>(tap_increment());
    assert!(composed.is_err(), "the session has no lane: {composed:?}");
    assert_eq!(
        taps(&asked),
        1,
        "a tap on the `os` path, the call's or the session's, is the driver's"
    );
}

/// The driver waits for a served gesture as long as its act may hold, past its own reply bound:
/// an agent that answers a tap after the bound but within the tap's timeout is answered.
#[test]
fn a_served_gesture_is_waited_for_as_long_as_its_act_may_hold() {
    // The attached driver waits 15 s past what the app may hold a request for.
    let (agent, _asked) = serving_taps(Duration::from_secs(17));
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    let mut tap = tap_increment();
    tap.options.timeout_ms = Some(10_000);
    let tapped = driver
        .call::<method::ElementTap>(tap)
        .expect("the tap's act timeout extends the reply's deadline");
    assert_eq!(tapped.via, InputPath::Synthetic);
}

/// A gesture asked through the Rust handles, a locator's or a point's, goes to the app's agent
/// whole on the synthetic path when the agent serves it: the driver neither locates, checks
/// actionability nor injects.
#[test]
fn a_handles_gesture_on_the_synthetic_path_is_sent_to_the_agent_not_composed() {
    let (agent, asked) = serving_taps(Duration::ZERO);
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    let window = "w1".parse::<WindowId>().expect("a window handle");
    let tapped = driver
        .window(window)
        .locate(Locator::identifier("increment"))
        .via(InputPath::Synthetic)
        .tap()
        .expect("the agent answers the tap");
    assert_eq!(tapped.via, InputPath::Synthetic);
    driver
        .window(window)
        .tap_at(LogicalPoint::new(4.0, 4.0))
        .expect("the agent answers a tap at a point");
    let asked = asked.lock().expect("the log");
    assert_eq!(
        asked
            .iter()
            .filter(|name| name.as_str() == method::ElementTap::NAME)
            .count(),
        2,
        "{asked:?}"
    );
    assert!(
        asked.iter().all(|name| {
            name.as_str() != method::SemanticsLocate::NAME
                && name.as_str() != method::ElementActionability::NAME
                && name.as_str() != method::InputInject::NAME
        }),
        "nothing of the composition was asked: {asked:?}"
    );
}

/// A scripted agent whose hello serves every gesture, answering each that it ran, and the
/// methods it was asked.
fn serving_every_gesture() -> (ScriptedAgent, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::ElementTap::NAME,
            method::ElementDoubleTap::NAME,
            method::ElementLongPress::NAME,
            method::ElementDragTo::NAME,
            method::ElementSwipe::NAME,
            method::ElementFling::NAME,
            method::ElementPinch::NAME,
            method::ElementScrollIntoView::NAME,
            method::ElementTypeText::NAME,
            method::ElementPressKeys::NAME,
        ],
        Box::new(move |name, _| {
            log.lock().expect("the log").push(name.to_owned());
            match name {
                "windows.list" => Reply::Result(json!({ "windows": ["w1"] })),
                _ => Reply::Result(json!({ "via": "synthetic", "effect": { "kind": "ran" } })),
            }
        }),
    )
    .expect("the stand-in binds its endpoint");
    (agent, asked)
}

/// Each gesture of the driver's handles sends the agent that serves it its own method, whole: a
/// swipe is `element.swipe` and a fling `element.fling`, and nothing of a composition (a locate,
/// an actionability read, an injection) is asked.
#[test]
fn each_gesture_the_agent_serves_reaches_it_as_its_own_method() {
    use flui_driver::protocol::input::Direction;

    let (agent, asked) = serving_every_gesture();
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    let window = "w1".parse::<WindowId>().expect("a window handle");
    let mut window = driver.window(window);
    let mut target = window
        .locate(Locator::identifier("increment"))
        .via(InputPath::Synthetic);
    let calls: [(&str, Result<ActOutcome, Error>); 10] = [
        (method::ElementTap::NAME, target.tap()),
        (method::ElementDoubleTap::NAME, target.double_tap()),
        (method::ElementLongPress::NAME, target.long_press(None)),
        (
            method::ElementDragTo::NAME,
            target.drag_to(Target::Locator(Locator::identifier("increment"))),
        ),
        (method::ElementSwipe::NAME, target.swipe(Direction::Up)),
        (method::ElementFling::NAME, target.fling(Direction::Up)),
        (method::ElementPinch::NAME, target.pinch(2.0)),
        (
            method::ElementScrollIntoView::NAME,
            target.scroll_into_view(),
        ),
        (method::ElementTypeText::NAME, target.type_text("a")),
        (method::ElementPressKeys::NAME, target.press_keys(&["a"])),
    ];
    let asked = asked.lock().expect("the log");
    let gestures: Vec<&str> = asked
        .iter()
        .map(String::as_str)
        .filter(|name| *name != method::WindowsList::NAME)
        .collect::<Vec<&str>>();
    let sent: Vec<&str> = calls.iter().map(|(name, _)| *name).collect::<Vec<&str>>();
    assert_eq!(
        gestures, sent,
        "each entry sent its own method, and only it"
    );
    for (name, answer) in calls {
        let outcome = answer.unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(outcome.via, InputPath::Synthetic, "{name}");
    }
}

/// `element.scroll_into_view` is a semantics action: composed by the driver, a call that
/// requires another path is refused naming it, before the agent is asked anything.
#[test]
fn a_composed_scroll_into_view_refuses_a_path_it_has_no_form_on() {
    let (agent, asked) = serving_taps(Duration::ZERO);
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    for path in [InputPath::Synthetic, InputPath::Os] {
        let mut params: flui_driver::protocol::input::ScrollIntoViewParams =
            serde_json::from_value(json!({ "target": { "locator": { "identifier": "row" } } }))
                .expect("scroll into view reads");
        params.options.via = Some(InputPolicy::require(path));
        let refused = driver.call::<method::ElementScrollIntoView>(params);
        assert!(
            matches!(
                &refused,
                Err(Error::NotSupported { capability: Some(capability), .. })
                    if *capability == format!("{}_input", path.name())
            ),
            "{path:?}: {refused:?}"
        );
    }
    let asked = asked.lock().expect("the log");
    assert!(
        asked
            .iter()
            .all(|name| name.as_str() == method::WindowsList::NAME),
        "nothing was located or acted on: {asked:?}"
    );
}

/// One request the agent heard: its method and the `via` it carried.
type Asked = (String, Option<InputPolicy>);

/// A scripted agent whose hello serves `element.tap`, `element.long_press` and
/// `element.scroll_until`, answering each that it ran synthetically, and each method it was asked
/// with the `via` it was asked with.
fn recording_via() -> (ScriptedAgent, Arc<Mutex<Vec<Asked>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::ElementTap::NAME,
            method::ElementLongPress::NAME,
            method::ElementScrollUntil::NAME,
        ],
        Box::new(move |name, params| {
            let via = params
                .get("via")
                .map(|via| serde_json::from_value::<InputPolicy>(via.clone()).expect("a policy"));
            log.lock().expect("the log").push((name.to_owned(), via));
            match name {
                "windows.list" => Reply::Result(json!({ "windows": ["w1"] })),
                _ => Reply::Result(json!({ "via": "synthetic", "effect": { "kind": "ran" } })),
            }
        }),
    )
    .expect("the stand-in binds its endpoint");
    (agent, asked)
}

fn a_long_press() -> flui_driver::protocol::input::LongPressParams {
    serde_json::from_value(json!({ "target": { "locator": { "identifier": "row" } } }))
        .expect("a long press reads")
}

fn a_scroll_until() -> flui_driver::protocol::input::ScrollUntilParams {
    serde_json::from_value(json!({
        "target": { "locator": { "identifier": "list" } },
        "until": { "identifier": "row" },
    }))
    .expect("a scroll until reads")
}

/// The method the agent was asked last, and its `via`.
fn last_asked(asked: &Mutex<Vec<Asked>>) -> Option<Asked> {
    asked.lock().expect("the log").last().cloned()
}

/// The driver chooses a served gesture's path, and the agent, which knows no session, receives it
/// as the gesture's `via`: a session that requires the semantic path has it required in the app,
/// and a gesture with no semantic form is refused before the agent hears of it, as the agent
/// refuses it.
#[test]
fn a_session_that_requires_a_path_has_it_required_by_the_agent() {
    let (agent, asked) = recording_via();
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    let refused_semantic = |answer: &Result<ActOutcome, Error>| {
        matches!(
            answer,
            Err(Error::NotSupported { capability: Some(capability), .. })
                if capability == "semantic_input"
        )
    };

    driver.set_input_policy(InputPolicy::require(InputPath::Semantic));
    driver
        .call::<method::ElementTap>(tap_increment())
        .expect("the agent answers the tap");
    assert_eq!(
        last_asked(&asked),
        Some((
            method::ElementTap::NAME.to_owned(),
            Some(InputPolicy::require(InputPath::Semantic))
        )),
        "a tap has a semantic form: the session's requirement reaches the agent"
    );
    let before = asked.lock().expect("the log").len();
    let pressed = driver.call::<method::ElementLongPress>(a_long_press());
    assert!(refused_semantic(&pressed), "{pressed:?}");
    let scrolled = driver.call::<method::ElementScrollUntil>(a_scroll_until());
    assert!(refused_semantic(&scrolled), "{scrolled:?}");
    assert_eq!(
        asked.lock().expect("the log").len(),
        before,
        "a gesture with no semantic form is refused before the agent is asked"
    );
}

/// A session that prefers `os` with no lane falls back to synthetic, as `choose_path` says, and a
/// gesture the agent serves is then the agent's, which composes it with the window's own settings
/// rather than the driver's windowless defaults.
#[test]
fn a_session_that_prefers_os_with_no_lane_has_the_agent_play_its_gestures() {
    let (agent, asked) = recording_via();
    let mut driver = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    let synthetic = Some(InputPolicy::prefer(InputPath::Synthetic));

    driver.set_input_policy(InputPolicy::prefer(InputPath::Os));
    driver
        .call::<method::ElementLongPress>(a_long_press())
        .expect("the agent answers the long press");
    assert_eq!(
        last_asked(&asked),
        Some((method::ElementLongPress::NAME.to_owned(), synthetic)),
    );
    driver
        .call::<method::ElementScrollUntil>(a_scroll_until())
        .expect("the agent answers the scroll");
    assert_eq!(
        last_asked(&asked),
        Some((method::ElementScrollUntil::NAME.to_owned(), synthetic))
    );
}

/// A counter whose session sends every input by the OS path through a guarded scripted lane on
/// the shared desktop, its lock at `dir`'s `os-input.lock`.
fn on_the_os_path(probe: &Probe, dir: &tempfile::TempDir) -> (Driver, LaneLog) {
    let (lane, log) = ScriptedLane::new(Isolation::Shared);
    let driver = Driver::launch(counter(probe))
        .expect("the headless app starts")
        .with_os_input_lock(dir.path().join("os-input.lock"))
        .with_input_policy(InputPolicy::require(InputPath::Os))
        .with_lane(lane.guarded())
        .expect("the lane starts");
    (driver, log)
}

/// `count` moves of a mouse that name no duration.
fn moves(count: u32) -> ActionSequence {
    ActionSequence::new(vec![InputSource::Pointer {
        id: "p".to_owned(),
        device: PointerDevice::Mouse,
        actions: (0..count)
            .map(|step| PointerAction::Move {
                to: LogicalPoint::new(5.0 + f64::from(step % 10), 5.0),
                duration_ms: 0,
                pressure: None,
            })
            .collect(),
    }])
}

/// With no scenario to name its duration, the announcement states how long the events play: a
/// move that names no duration plays for a step, which the sequence's own span does not count.
#[test]
fn an_announcement_with_no_scenario_states_how_long_its_events_play() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let probe = Probe::default();
    let (mut driver, log) = on_the_os_path(&probe, &dir);
    let moves = moves(200);
    assert_eq!(moves.span(), Duration::ZERO);
    let mut window = driver.main_window().expect("a window");
    let performed = window.perform(moves).expect("the moves are sent");
    assert_eq!(performed.via, InputPath::Os);
    let announced = driver.take_announcements();
    let [line] = announced.as_slice() else {
        panic!("announced once: {announced:?}");
    };
    assert!(
        line.contains("mouse") && line.contains("about 3 s"),
        "200 steps of 16 ms play for 3.2 s: {line}"
    );
    assert_ne!(log.sent().len(), 0, "the moves were sent");
}

/// Input no OS lane sends is refused before the desktop is locked, the window located or the run
/// announced: the next session takes the lock at once.
#[test]
fn input_no_lane_sends_is_refused_before_the_desktop_is_locked() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let probe = Probe::default();
    let (mut driver, log) = on_the_os_path(&probe, &dir);
    let line_scroll = ActionSequence::new(vec![InputSource::Wheel {
        id: "w".to_owned(),
        actions: vec![WheelAction::Scroll {
            at: LogicalPoint::new(5.0, 5.0),
            delta_x: 0.0,
            delta_y: 3.0,
            unit: ScrollUnit::Line,
        }],
    }]);
    let mut window = driver.main_window().expect("a window");
    let refused = window.perform(line_scroll);
    assert!(
        matches!(&refused, Err(Error::NotSupported { capability: Some(capability), .. })
            if capability == "os_line_scroll"),
        "{refused:?}"
    );
    drop(
        flui_driver::os_input::OsInputLock::acquire(&dir.path().join("os-input.lock"))
            .expect("the refused call took no lock"),
    );
    assert!(
        driver.take_announcements().is_empty(),
        "nothing was announced"
    );
    assert!(log.sent().is_empty(), "nothing was sent");
}

/// The pointer devices a session takes are the ones its agent's hello says its injection
/// delivers, a virtual pointer among them; an agent that lists none (one older than protocol
/// 0.32) delivers a mouse, a touch contact and a pen.
#[test]
fn a_session_takes_the_pointers_its_agent_says_it_delivers() {
    let probe = Probe::default();
    let driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    assert_eq!(
        driver.capabilities().pointers,
        [
            PointerDevice::Mouse,
            PointerDevice::Touch,
            PointerDevice::Pen,
            PointerDevice::Virtual,
        ]
    );
    let agent = ScriptedAgent::start(
        &[method::WindowsList::NAME, method::InputInject::NAME],
        Box::new(|_, _| Reply::Result(json!({ "windows": ["w1"] }))),
    )
    .expect("the stand-in binds its endpoint");
    let older = Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches");
    assert_eq!(
        older.capabilities().pointers,
        [
            PointerDevice::Mouse,
            PointerDevice::Touch,
            PointerDevice::Pen
        ]
    );
}
