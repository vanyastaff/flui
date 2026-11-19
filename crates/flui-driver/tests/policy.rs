//! The session's input policy, the scenario's window rule and the OS-input announcement
//! (ADR-0238 §5, §11), on the headless target with a scripted OS lane.

use std::time::Duration;

use flui_driver::os_input::OsDevice;
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{ActionSequence, InputSource, PointerAction, PointerDevice};
use flui_driver::steps::OsEvent;
use flui_driver::testing::{LaneLog, ScriptedLane};
use flui_driver::{
    Driver, Error, InputPath, InputPolicy, Isolation, Launch, Locator, Report, Scenario, Verdict,
    WindowSpec,
};

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
    report.run_scenario("increments", &mut driver, |driver, report| {
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
    assert_eq!(report.to_json()["lane"], "scripted");
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
        Err(Error::InvalidArgument { ref message }) => assert!(
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
