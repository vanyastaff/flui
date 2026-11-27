//! The driver as a `Connection` (ADR-0400 §1), on the headless counter and a scripted agent: a
//! typed call, a wait, a performed sequence, its clock, the window the connection keeps, an answer
//! the agent left unanswered and a host tool that hung, and the end of a test.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_driver::protocol::act::{
    ActionabilityCheck, ActionabilityParams, Effect, EffectKind, EffectUnit,
};
use flui_driver::protocol::client::{CallError, Connection, EndChecks};
use flui_driver::protocol::error::{ErrorCode, ErrorObject, HandleKind, Retry};
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{
    ActionSequence, InputKind, InputParams, InputPath, InputSource, JournalParams, LongPressParams,
    PointerAction, PointerButton, PointerDevice, gestures,
};
use flui_driver::protocol::locator::{Locator, Target, TextMatch};
use flui_driver::protocol::method::{
    self, ElementActionability, ElementTap, Method, SemanticsLocate, WindowPlacement,
};
use flui_driver::protocol::report::{CheckFailure, Outcome};
use flui_driver::protocol::session::WindowParams;
use flui_driver::protocol::version::ProtocolVersion;
use flui_driver::protocol::wait::{Condition, Expectation, SettleScope, WaitEnd};
use flui_driver::protocol::{input::TapParams, locator::LocateParams};
use flui_driver::testing::{Reply, ScriptedAgent, ScriptedLane};
use flui_driver::{Driver, Error, InputPolicy, Isolation, Launch};
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
    assert_eq!(
        outcome.scope,
        SettleScope::Complete,
        "the headless host composes its half of the list"
    );
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
    // of virtual time past what the request plays at 60 Hz: a window presenting at 30 Hz plays a
    // tap's ticks a frame each, twice as long. The tap is injected input: a served `element.tap`
    // may hold for its act timeout, which leaves a 30 Hz window time to play it.
    let launch = counter(&probe)
        .reply_timeout(Duration::from_millis(1))
        .frame_period(Duration::from_millis(33))
        .expect("a headless launch");
    let mut driver = Driver::launch(launch).expect("the headless app starts");
    let window = driver.windows().expect("the windows")[0];
    let at = driver
        .window(window)
        .locate(Locator::identifier("increment"))
        .resolve()
        .expect("the button")
        .actionability()
        .expect("its facts")
        .point
        .expect("a point to tap");
    let unanswered = Connection::call::<method::InputInject>(
        &mut driver,
        InputParams::new(
            window,
            gestures::tap(at, PointerButton::Primary, &gestures::Defaults::WINDOWLESS),
        ),
    );
    assert_eq!(
        unanswered,
        Err(CallError::Unanswered {
            may_have_run: true,
            released: true,
        }),
        "an accepted tap may have run, and the release is a read-sized answer"
    );
}

/// A gesture the app's agent serves, accepted and playing when its reply deadline passes, is
/// unanswered the same way: it may have run, its release is answered, and nothing it pressed
/// stays down. A long press on the hold surface of a window presenting at 30 Hz plays its ticks
/// a frame each, past the bound its parameters give at 60 Hz; `force` takes it at its first
/// attempt, so its timeout adds nothing.
#[test]
fn a_served_gesture_that_outlives_its_reply_deadline_releases_what_it_pressed() {
    let probe = Probe::default();
    let launch = counter(&probe)
        .reply_timeout(Duration::from_millis(1))
        .frame_period(Duration::from_millis(33))
        .expect("a headless launch");
    let mut driver = Driver::launch(launch).expect("the headless app starts");
    let press: LongPressParams = serde_json::from_value(json!({
        "target": { "locator": { "identifier": "hold" } },
        "duration_ms": 800,
        "timeout_ms": 1,
        "force": true,
    }))
    .expect("a long press reads");
    let unanswered = Connection::call::<method::ElementLongPress>(&mut driver, press);
    assert_eq!(
        unanswered,
        Err(CallError::Unanswered {
            may_have_run: true,
            released: true,
        }),
        "an accepted press may have run, and the release is answered"
    );
    assert_eq!(probe.downs(), 1, "the press went down");
    let window = driver.windows().expect("the windows")[0];
    let journal = Connection::call::<method::InputJournal>(&mut driver, JournalParams::new(window))
        .expect("the journal is read");
    let last = journal
        .entries
        .iter()
        .rev()
        .find(|entry| {
            matches!(
                entry.kind,
                InputKind::PointerDown | InputKind::PointerUp | InputKind::PointerCancel
            )
        })
        .map(|entry| entry.kind);
    assert!(
        matches!(last, Some(InputKind::PointerUp | InputKind::PointerCancel)),
        "nothing the press pressed stays down: {last:?}"
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

/// A scripted agent that lists `w1`, `w2`, `w3` the first time it is asked and `w3`, `w2` after,
/// as though `w1` closed and `w3` came forward, answers `semantics.locate` with `locate` and
/// `input.release` with `release`.
fn scripted_windows(locate: Reply, release: Reply) -> (ScriptedAgent, Asked) {
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
                    let windows: &[&str] = if listed == 1 {
                        &["w1", "w2", "w3"]
                    } else {
                        &["w3", "w2"]
                    };
                    Reply::Result(json!({ "windows": windows }))
                }
                "semantics.locate" => locate.clone(),
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

/// The windows each request of `method` was asked for, `-` for one that names none.
fn asked_of(asked: &Asked, method: &str) -> Vec<String> {
    asked
        .lock()
        .expect("the log")
        .iter()
        .filter(|(name, _)| name == method)
        .map(|(_, window)| window.clone().unwrap_or_else(|| "-".to_owned()))
        .collect()
}

fn released() -> Reply {
    Reply::Result(json!({ "pointers": 0, "keys": 0 }))
}

fn unanswered() -> Reply {
    Reply::Error(ErrorObject::new(
        ErrorCode::Timeout,
        "the window did not answer in time",
    ))
}

fn closed() -> Reply {
    Reply::Error(
        ErrorObject::new(ErrorCode::Gone, "the window has closed")
            .retry(Retry::Never)
            .handle(HandleKind::Window, "w1"),
    )
}

#[test]
fn the_connection_keeps_the_window_it_was_first_used_on() {
    let (agent, asked) = scripted_windows(unanswered(), released());
    let mut driver = attach(&agent);
    for _ in 0..2 {
        let failures = driver.finish(&EndChecks::none());
        assert_eq!(failures, [], "the release passed");
    }
    assert_eq!(
        asked_of(&asked, method::InputRelease::NAME),
        ["w1", "w1"],
        "the app listing `w3` first later does not move the connection: {asked:?}"
    );
    assert_eq!(asked_of(&asked, method::WindowsList::NAME).len(), 1);
}

#[test]
fn an_unanswered_call_releases_the_connection_s_window() {
    let refused = Reply::Error(ErrorObject::new(ErrorCode::Platform, "the release failed"));
    for (release, answered) in [(released(), true), (refused, false)] {
        let (agent, asked) = scripted_windows(unanswered(), release);
        let mut driver = attach(&agent);
        // The end resolves the connection on the first listing, `w1` first; the app lists `w3`
        // first afterwards.
        assert_eq!(
            driver.finish(&EndChecks::none()).len(),
            usize::from(!answered)
        );
        // A locator that names no window, which the agent alone would refuse in an app of
        // several windows.
        let unanswered = Connection::call::<SemanticsLocate>(
            &mut driver,
            LocateParams::new(Locator::identifier("increment")),
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
            asked_of(&asked, method::SemanticsLocate::NAME),
            ["w1"],
            "a call that names no window is sent to the connection's"
        );
        assert_eq!(
            asked_of(&asked, method::InputRelease::NAME),
            ["w1", "w1"],
            "the end and the unanswered call release the connection's window"
        );
    }
}

#[test]
fn a_call_that_names_another_window_is_refused_before_it_is_sent() {
    let (agent, asked) = scripted_windows(unanswered(), released());
    let mut driver = attach(&agent);
    assert_eq!(driver.finish(&EndChecks::none()), []);
    let w2 = driver.windows().expect("the windows")[1];
    let refused = Connection::call::<SemanticsLocate>(
        &mut driver,
        LocateParams::new(Locator::identifier("increment").in_window(w2)),
    );
    let Err(CallError::Failed(error)) = &refused else {
        panic!("a call to another window is refused: {refused:?}");
    };
    assert_eq!(error.code, ErrorCode::InvalidArgument, "{error:?}");
    assert!(
        error.message.contains("w1") && error.message.contains("w2"),
        "{error:?}"
    );
    assert_eq!(
        asked_of(&asked, method::SemanticsLocate::NAME),
        Vec::<String>::new(),
        "nothing reached the agent"
    );
    assert_eq!(
        asked_of(&asked, method::InputRelease::NAME),
        ["w1"],
        "only the end released"
    );
}

#[test]
fn a_closed_connection_window_is_gone_and_never_replaced() {
    let (agent, asked) = scripted_windows(closed(), closed());
    let mut driver = attach(&agent);
    // The end resolves the connection on `w1`; the app then answers that `w1` closed, and lists
    // `w3` first.
    assert_eq!(
        driver.finish(&EndChecks::none()),
        [],
        "a closed window holds no input: its release is a release"
    );
    let gone = Connection::call::<SemanticsLocate>(
        &mut driver,
        LocateParams::new(Locator::identifier("increment")),
    );
    let Err(CallError::Failed(error)) = &gone else {
        panic!("a call to the closed window is `gone`: {gone:?}");
    };
    assert_eq!(
        (error.code, error.kind),
        (ErrorCode::Gone, Some(HandleKind::Window)),
        "{error:?}"
    );
    assert_eq!(driver.finish(&EndChecks::none()), []);
    assert_eq!(asked_of(&asked, method::SemanticsLocate::NAME), ["w1"]);
    assert_eq!(asked_of(&asked, method::InputRelease::NAME), ["w1", "w1"]);
    assert_eq!(
        asked_of(&asked, method::WindowsList::NAME).len(),
        1,
        "the connection is not moved to another window"
    );
}

#[test]
fn a_failed_release_at_the_end_is_a_failed_check() {
    let refused = Reply::Error(ErrorObject::new(ErrorCode::Platform, "the release failed"));
    let (agent, _asked) = scripted_windows(unanswered(), refused);
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

/// A condition an idle application cannot make true ends idle as soon as the headless host,
/// which composes its half of the list, sees nothing but the test can change it (ADR-0400 §3).
#[test]
fn a_wait_that_never_holds_on_an_idle_app_ends_idle() {
    let probe = Probe::default();
    let mut driver = Driver::launch(counter(&probe)).expect("the headless app starts");
    let never = Condition::Matches {
        locator: Locator::default().name_matching(TextMatch::prefix("Count:")),
        expect: Expectation::Name(TextMatch::exact("Count: 5")),
    };
    let outcome = driver
        .wait(&never, Duration::from_millis(200))
        .expect("the wait is answered");
    assert_eq!(outcome.end, WaitEnd::Idle, "{outcome:?}");
    assert!(outcome.unsettled.is_empty(), "{outcome:?}");
    assert_eq!(probe.presses(), 0);
}

/// Each wire answer reads into its variant and is stated again as the app sent it, every field
/// its code carries kept; a failure only the driver sees takes the code a caller branches on.
#[test]
fn an_error_is_stated_on_the_wire_as_it_was_read() {
    let mut gone = ErrorObject::new(ErrorCode::Gone, "the app exited")
        .handle(HandleKind::Window, "w2")
        .effect(Effect::new(EffectKind::MayHaveRun).detail("the tap was sent"));
    gone.exit = Some(3);
    // `stale` names the handle that went, and no kind.
    let mut stale = ErrorObject::new(ErrorCode::Stale, "the element went");
    stale.handle = Some("w1/e4".to_owned());
    let answers = [
        ErrorObject::new(ErrorCode::NotFound, "no button").retry(Retry::Never),
        ErrorObject::ambiguous("two buttons", vec!["w1/e1".to_owned(), "w1/e2".to_owned()]),
        stale,
        gone,
        ErrorObject::not_actionable(
            "covered",
            ActionabilityCheck::Receives,
            Some("w1/e9".to_owned()),
        ),
        ErrorObject::version(ProtocolVersion::new(0, 31), ProtocolVersion::new(0, 32)),
        ErrorObject::new(ErrorCode::Timeout, "no answer")
            .effect(Effect::new(EffectKind::MayHaveRun))
            .retry(Retry::Soon),
        ErrorObject::new(ErrorCode::Platform, "the OS refused")
            .effect(Effect::new(EffectKind::Partial)),
        // Part of an input played before the failure, as the app counts it: on a busy, which
        // holds an effect, with its own `soon`.
        ErrorObject::new(ErrorCode::Busy, "the app was busy")
            .effect(Effect::partial(2, 5, EffectUnit::Tick))
            .retry(Retry::Soon),
        // On a code whose variant holds none, with `never` and with the app's own `soon`.
        ErrorObject::new(ErrorCode::InvalidArgument, "no such key")
            .effect(Effect::partial(2, 5, EffectUnit::Tick))
            .retry(Retry::Never),
        ErrorObject::new(ErrorCode::NotForeground, "the shade is in front")
            .effect(Effect::partial(1, 3, EffectUnit::Event))
            .retry(Retry::Soon),
    ];
    for answer in answers {
        let read = Error::from_wire(answer.clone());
        assert_eq!(read.to_wire(), answer, "{read:?}");
    }
    // The other codes whose variant holds an effect keep it on the variant, not partly sent.
    let held = |code: ErrorCode, retry: Retry| {
        Error::from_wire(
            ErrorObject::new(code, "part of an input played")
                .effect(Effect::partial(1, 4, EffectUnit::Tick))
                .retry(retry),
        )
    };
    let not_found = held(ErrorCode::NotFound, Retry::WhenAppears);
    assert!(
        matches!(
            not_found,
            Error::NotFound {
                effect: Some(_),
                ..
            }
        ),
        "{not_found:?}"
    );
    let input_held = held(ErrorCode::InputHeld, Retry::Soon);
    assert!(
        matches!(
            input_held,
            Error::InputHeld {
                effect: Some(_),
                ..
            }
        ),
        "{input_held:?}"
    );
    let not_actionable = held(ErrorCode::NotActionable, Retry::WhenAppears);
    assert!(
        matches!(
            not_actionable,
            Error::NotActionable {
                effect: Some(_),
                ..
            }
        ),
        "{not_actionable:?}"
    );
    for read in [not_found, input_held, not_actionable] {
        assert!(read.may_have_run(), "{read:?}");
    }
    // A busy that holds the effect stays a busy; a code whose variant holds none is partly sent,
    // its code and retry as the app said.
    let busy = Error::from_wire(
        ErrorObject::new(ErrorCode::Busy, "the app was busy")
            .effect(Effect::partial(2, 5, EffectUnit::Tick))
            .retry(Retry::Soon),
    );
    assert!(matches!(busy, Error::Busy { .. }), "{busy:?}");
    assert!(busy.may_have_run());
    let soon = Error::from_wire(
        ErrorObject::new(ErrorCode::NotForeground, "the shade is in front")
            .effect(Effect::partial(1, 3, EffectUnit::Event))
            .retry(Retry::Soon),
    );
    assert!(matches!(soon, Error::PartlySent { .. }), "{soon:?}");
    assert_eq!(
        (soon.code(), soon.retry()),
        (Some(ErrorCode::NotForeground), Retry::Soon)
    );
    let mut unsupported = ErrorObject::new(ErrorCode::NotSupported, "no recorder");
    unsupported.capability = Some("record".to_owned());
    assert_eq!(Error::from_wire(unsupported.clone()).to_wire(), unsupported);

    let driver_only: [(Error, ErrorCode, Option<&str>); 3] = [
        (
            Error::InterruptedByUser {
                lane: "windows".to_owned(),
                event: "a key".to_owned(),
            },
            ErrorCode::NotSupported,
            Some("uninterrupted_os_input"),
        ),
        (
            Error::Lane {
                lane: "adb".to_owned(),
                unavailable: true,
                message: "no device".to_owned(),
            },
            ErrorCode::NotSupported,
            Some("adb_lane"),
        ),
        (
            Error::Protocol {
                message: "a line that is not one".to_owned(),
            },
            ErrorCode::Platform,
            None,
        ),
    ];
    for (error, code, capability) in driver_only {
        let wire = error.to_wire();
        assert_eq!(
            (wire.code, wire.capability.as_deref(), wire.retry),
            (code, capability, Retry::Never),
            "{error:?}"
        );
        assert_eq!(wire.message, error.to_string());
        assert_eq!(
            CallError::failed(wire).is_cannot_verify(),
            code == ErrorCode::NotSupported
        );
    }
}

/// A run of ticks that name no duration plays one frame each: the reply's bound counts them, so
/// an agent bounding its reply at 200 ms past what the input takes answers the whole run.
#[test]
fn a_long_run_of_ticks_that_name_no_duration_is_answered_when_it_has_played() {
    let probe = Probe::default();
    let launch = counter(&probe).reply_timeout(Duration::from_millis(200));
    let mut driver = Driver::launch(launch).expect("the headless app starts");
    let actions = (0..300)
        .map(|step| PointerAction::Move {
            to: LogicalPoint::new(5.0 + f64::from(step % 10), 5.0),
            duration_ms: 0,
            pressure: None,
        })
        .collect();
    let moves = ActionSequence::new(vec![InputSource::Pointer {
        id: "p".to_owned(),
        device: PointerDevice::Mouse,
        actions,
    }]);
    assert_eq!(moves.span(), Duration::ZERO, "no tick names a duration");
    let before = Connection::now(&driver).time;
    let performed = Connection::perform(&mut driver, &moves).expect("the moves are answered");
    assert_eq!(performed.via, InputPath::Synthetic);
    assert!(
        Connection::now(&driver).time.saturating_sub(before) >= Duration::from_millis(16 * 299),
        "the agent played a tick a frame"
    );
}

/// A call the connection itself had no answer to within its budget, the agent silent past it, is
/// unanswered as the agent's `timeout` is, and the window's input is released.
#[test]
fn a_call_the_agent_answers_past_the_connection_s_budget_is_unanswered() {
    let asked = Asked::default();
    let log = Arc::clone(&asked);
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
                "windows.list" => Reply::Result(json!({ "windows": ["w1"] })),
                "semantics.locate" => {
                    // Past the 15 s an attached driver waits for an agent's answer.
                    std::thread::sleep(Duration::from_millis(15_500));
                    Reply::Result(json!({ "elements": [] }))
                }
                _ => released(),
            }
        }),
    )
    .expect("the stand-in binds its endpoint");
    let mut driver = attach(&agent);
    let unanswered = Connection::call::<SemanticsLocate>(
        &mut driver,
        LocateParams::new(Locator::identifier("increment")),
    );
    assert_eq!(
        unanswered,
        Err(CallError::Unanswered {
            may_have_run: false,
            released: true,
        }),
        "the connection's own deadline is the agent's silence, not a host tool's"
    );
    assert_eq!(asked_of(&asked, method::InputRelease::NAME), ["w1"]);
}
