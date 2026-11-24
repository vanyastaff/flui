//! The driver as a `Connection` (ADR-0400 §1), on the headless counter and a scripted agent: a
//! typed call, a wait, a performed sequence, its clock, the window the connection keeps, an answer
//! the agent left unanswered and a host tool that hung, and the end of a test.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_driver::protocol::act::ActionabilityParams;
use flui_driver::protocol::client::{CallError, Connection, EndChecks};
use flui_driver::protocol::error::{ErrorCode, ErrorObject};
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{
    ActionSequence, InputPath, InputSource, PointerAction, PointerButton, PointerDevice,
};
use flui_driver::protocol::locator::{Locator, Target, TextMatch};
use flui_driver::protocol::method::{
    self, ElementActionability, ElementTap, Method, SemanticsLocate, WindowPlacement,
};
use flui_driver::protocol::report::{CheckFailure, Outcome};
use flui_driver::protocol::session::WindowParams;
use flui_driver::protocol::wait::{Condition, Expectation, SettleScope, WaitEnd};
use flui_driver::protocol::{input::TapParams, locator::LocateParams};
use flui_driver::testing::{Reply, ScriptedAgent, ScriptedLane};
use flui_driver::{Driver, InputPolicy, Isolation, Launch};
use serde_json::json;

use crate::fixtures::{Counter, Probe};

fn counter(probe: &Probe) -> Launch {
    let probe = probe.clone();
    Launch::headless(move || Counter { probe })
}

fn increment() -> Target {
    Target::Locator(Locator::identifier("increment"))
}

#[test]
fn a_typed_call_answers_its_method_s_output() {
    let probe = Probe::default();
    let mut driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    let window = driver.windows().expect("the windows")[0];

    let placement = Connection::call::<WindowPlacement>(&mut driver, WindowParams::new(window))
        .expect("the window's placement");
    assert!(placement.scale > 0.0, "{placement:?}");

    let found = Connection::call::<SemanticsLocate>(
        &mut driver,
        LocateParams::new(Locator::identifier("increment").in_window(window)),
    )
    .expect("the button is located");
    assert_eq!(found.matches.len(), 1, "{found:?}");

    let tapped = Connection::call::<ElementTap>(&mut driver, TapParams::new(increment()))
        .expect("the tap is delivered");
    assert_eq!(tapped.via, InputPath::Synthetic);
    assert_eq!(probe.presses(), 1);
}

#[test]
fn a_wait_answers_on_the_connection_s_window() {
    let probe = Probe::default();
    let mut driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    Connection::call::<ElementTap>(&mut driver, TapParams::new(increment())).expect("a tap");
    let count = Condition::Matches {
        locator: Locator::default().name_matching(TextMatch::prefix("Count:")),
        expect: Expectation::Name(TextMatch::exact("Count: 1")),
    };
    let outcome = driver
        .wait(&count, Duration::from_secs(5))
        .expect("the wait is answered");
    assert_eq!(outcome.end, WaitEnd::Satisfied, "{outcome:?}");
    assert_eq!(outcome.scope, SettleScope::RuntimeOnly);
}

#[test]
fn a_performed_sequence_says_its_path_and_the_clock_is_virtual_in_process() {
    let probe = Probe::default();
    let mut driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    let window = driver.windows().expect("the windows")[0];
    let button = Connection::call::<SemanticsLocate>(
        &mut driver,
        LocateParams::new(Locator::identifier("increment").in_window(window)),
    )
    .expect("the button is located");
    let facts = Connection::call::<ElementActionability>(
        &mut driver,
        ActionabilityParams::new(button.matches[0].element),
    )
    .expect("the button's facts");
    let at: LogicalPoint = facts.point.expect("a point to press");
    let before = driver.now();
    assert!(before.virtual_clock, "{before:?}");
    assert_eq!(before.frame, None, "the driver holds no exact frame");

    let tap = ActionSequence::new(vec![InputSource::Pointer {
        id: "p".to_owned(),
        device: PointerDevice::Touch,
        actions: vec![
            PointerAction::Move {
                to: at,
                duration_ms: 0,
                pressure: None,
            },
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ],
    }]);
    let performed = driver.perform(&tap).expect("the sequence is performed");
    assert_eq!(performed.via, InputPath::Synthetic);
    assert_eq!(probe.presses(), 1);
    assert!(
        driver.now().time > before.time,
        "playing the sequence moved the virtual clock"
    );
}

#[test]
fn a_refused_call_carries_the_app_s_code() {
    let probe = Probe::default();
    let mut driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    let missing = Target::Locator(Locator::identifier("missing"));
    let mut tap = TapParams::new(missing);
    tap.options.timeout_ms = Some(50);
    let refused = Connection::call::<ElementTap>(&mut driver, tap);
    let Err(CallError::Failed(error)) = &refused else {
        panic!("a tap on nothing is refused: {refused:?}");
    };
    assert_eq!(error.code, ErrorCode::NotFound, "{error:?}");
    let refused = refused.expect_err("refused");
    assert!(!refused.is_cannot_verify());
}

#[test]
fn an_answer_that_did_not_arrive_releases_the_window_s_input() {
    let probe = Probe::default();
    // The agent answers `timeout` for whatever its window has not answered within a millisecond
    // of virtual time: a read answers in the frame it is asked, a tap takes frames to play.
    let launch = counter(&probe).reply_timeout(Duration::from_millis(1));
    let mut driver = Driver::launch(launch).expect("the headless app starts");
    let unanswered = Connection::call::<ElementTap>(&mut driver, TapParams::new(increment()));
    assert_eq!(
        unanswered,
        Err(CallError::Unanswered {
            may_have_run: true,
            released: true,
        }),
        "an accepted tap may have run, and the release is a read-sized answer"
    );
}

#[test]
fn a_device_s_end_names_what_it_cannot_check() {
    let probe = Probe::default();
    let mut driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    let failures = driver.finish(&EndChecks::default());
    let named: Vec<(&str, bool)> = failures
        .iter()
        .map(|failure| {
            let check = failure.check();
            (
                check.name.as_str(),
                matches!(check.outcome, Outcome::CannotVerify { .. }),
            )
        })
        .collect();
    assert_eq!(
        named,
        [
            ("settle", true),
            ("audit", true),
            ("drop ledger", true),
            ("nothing left behind", true),
        ],
        "the release passed, and every check a device cannot read is cannot-verify"
    );
    assert_eq!(driver.finish(&EndChecks::none()), []);
}

/// What a scripted agent was asked: each request's method and the window its parameters name.
type Asked = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// A scripted agent that lists `w1`, `w2`, `w3` the first time it is asked and the reverse after,
/// answers `semantics.locate` with `timeout`, and answers `input.release` with `release`.
fn reordering_agent(release: Reply) -> (ScriptedAgent, Asked) {
    let asked = Asked::default();
    let log = Arc::clone(&asked);
    let mut listed = 0_u32;
    let agent = ScriptedAgent::start(
        &[
            method::WindowsList::NAME,
            method::SemanticsLocate::NAME,
            method::InputRelease::NAME,
        ],
        Box::new(move |name, params| {
            let window = params
                .get("window")
                .or_else(|| params.pointer("/locator/window"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            log.lock().expect("the log").push((name.to_owned(), window));
            match name {
                "windows.list" => {
                    listed += 1;
                    let windows = if listed == 1 {
                        ["w1", "w2", "w3"]
                    } else {
                        ["w3", "w2", "w1"]
                    };
                    Reply::Result(json!({ "windows": windows }))
                }
                "semantics.locate" => Reply::Error(ErrorObject::new(
                    ErrorCode::Timeout,
                    "the window did not answer in time",
                )),
                _ => release.clone(),
            }
        }),
    )
    .expect("the stand-in binds its endpoint");
    (agent, asked)
}

fn attach(agent: &ScriptedAgent) -> Driver {
    Driver::attach(&agent.endpoint(), agent.token(), Duration::from_secs(10))
        .expect("the driver attaches")
}

/// The windows each `input.release` was asked for.
fn releases(asked: &Asked) -> Vec<Option<String>> {
    asked
        .lock()
        .expect("the log")
        .iter()
        .filter(|(name, _)| name == method::InputRelease::NAME)
        .map(|(_, window)| window.clone())
        .collect()
}

fn released() -> Reply {
    Reply::Result(json!({ "pointers": 0, "keys": 0 }))
}

#[test]
fn the_connection_keeps_the_window_it_was_first_used_on() {
    let (agent, asked) = reordering_agent(released());
    let mut driver = attach(&agent);
    for _ in 0..2 {
        let failures = driver.finish(&EndChecks::none());
        assert_eq!(failures, [], "the release passed");
    }
    assert_eq!(
        releases(&asked),
        [Some("w1".to_owned()), Some("w1".to_owned())],
        "the app listing `w3` first later does not move the connection: {asked:?}"
    );
}

#[test]
fn an_unanswered_call_releases_the_window_it_addressed() {
    let refused = Reply::Error(ErrorObject::new(ErrorCode::Platform, "the release failed"));
    for (release, answered) in [(released(), true), (refused, false)] {
        let (agent, asked) = reordering_agent(release);
        let mut driver = attach(&agent);
        // The end resolves the connection on the first listing, `w1` first; the app lists `w3`
        // first afterwards.
        assert_eq!(
            driver.finish(&EndChecks::none()).len(),
            usize::from(!answered)
        );
        let w2 = driver.windows().expect("the windows")[1];

        let unanswered = Connection::call::<SemanticsLocate>(
            &mut driver,
            LocateParams::new(Locator::identifier("increment").in_window(w2)),
        );
        assert_eq!(
            unanswered,
            Err(CallError::Unanswered {
                may_have_run: false,
                released: answered,
            }),
            "a read the agent left unanswered ran nothing"
        );
        assert_eq!(
            releases(&asked),
            [Some("w1".to_owned()), Some("w2".to_owned())],
            "the end releases the connection's window, the unanswered call the one it named"
        );
    }
}

#[test]
fn a_failed_release_at_the_end_is_a_failed_check() {
    let refused = Reply::Error(ErrorObject::new(ErrorCode::Platform, "the release failed"));
    let (agent, _asked) = reordering_agent(refused);
    let mut driver = attach(&agent);
    let failures = driver.finish(&EndChecks::none());
    let [failure] = failures.as_slice() else {
        panic!("only the release is checked: {failures:?}");
    };
    let check = failure.check();
    assert_eq!(check.name, "release held input");
    assert!(
        matches!(&check.outcome, Outcome::Failed { reason } if reason.contains("the release failed")),
        "{check:?}"
    );
}

#[test]
fn a_host_tool_that_hangs_is_a_failed_call_with_its_message() {
    let probe = Probe::default();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let (lane, log) = ScriptedLane::new(Isolation::Xvfb);
    let mut driver = Driver::launch(counter(&probe))
        .expect("the headless app starts")
        .with_os_input_lock(dir.path().join("os-input.lock"))
        .with_input_policy(InputPolicy::require(InputPath::Os))
        .with_lane(lane.hanging_sends())
        .expect("the lane starts");
    let tap = ActionSequence::new(vec![InputSource::Pointer {
        id: "p".to_owned(),
        device: PointerDevice::Mouse,
        actions: vec![
            PointerAction::Move {
                to: LogicalPoint::new(1.0, 1.0),
                duration_ms: 0,
                pressure: None,
            },
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ],
    }]);
    let failed = driver.perform(&tap);
    let Err(CallError::Failed(error)) = &failed else {
        panic!("a lane's hung tool is the lane's failure, not an unanswered agent: {failed:?}");
    };
    assert_eq!(error.code, ErrorCode::Timeout, "{error:?}");
    assert!(
        error
            .message
            .contains("the scripted OS did not answer the send"),
        "{error:?}"
    );
    assert!(log.releases() > 0, "the lane released what it pressed");
}

#[test]
fn a_wait_that_never_holds_ends_unsatisfied() {
    let probe = Probe::default();
    let mut driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    let never = Condition::Matches {
        locator: Locator::default().name_matching(TextMatch::prefix("Count:")),
        expect: Expectation::Name(TextMatch::exact("Count: 5")),
    };
    let outcome = driver
        .wait(&never, Duration::from_millis(200))
        .expect("the wait is answered");
    assert_eq!(outcome.end, WaitEnd::Unsatisfied, "{outcome:?}");
    assert_eq!(probe.presses(), 0);
}
