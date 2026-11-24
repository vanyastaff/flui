//! The driver as a `Connection` (ADR-0400 §1), on the headless counter: a typed call, a wait, a
//! performed sequence, its clock, an answer that did not arrive, and the end of a test.

use std::time::Duration;

use flui_driver::protocol::act::ActionabilityParams;
use flui_driver::protocol::client::{CallError, Connection, EndChecks};
use flui_driver::protocol::error::ErrorCode;
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{
    ActionSequence, InputPath, InputSource, PointerAction, PointerButton, PointerDevice,
};
use flui_driver::protocol::locator::{Locator, Target, TextMatch};
use flui_driver::protocol::method::{
    ElementActionability, ElementTap, SemanticsLocate, WindowPlacement,
};
use flui_driver::protocol::report::{CheckFailure, Outcome};
use flui_driver::protocol::session::WindowParams;
use flui_driver::protocol::wait::{Condition, Expectation, SettleScope, WaitEnd};
use flui_driver::protocol::{input::TapParams, locator::LocateParams};
use flui_driver::{Driver, Launch};

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
