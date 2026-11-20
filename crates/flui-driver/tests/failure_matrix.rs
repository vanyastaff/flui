//! The driver's failure scenarios on the headless target, each run on every fixture app.

use std::time::Duration;

use flui_driver::protocol::act::EffectKind;
use flui_driver::protocol::error::{HandleKind, Retry};
use flui_driver::protocol::geometry::LogicalPoint;
use flui_driver::protocol::input::{
    ActionSequence, InputSource, PointerAction, PointerButton, PointerDevice,
};
use flui_driver::protocol::tree::ReadQuery;
use flui_driver::testing::ScriptedLane;
use flui_driver::{
    Driver, Error, InputPath, InputPolicy, Isolation, Launch, Locator, Report, TextMatch, Verdict,
    expect,
};

use crate::fixtures::{Counter, Gallery, Probe};

/// The fixture apps every row runs on.
#[derive(Debug, Clone, Copy)]
enum App {
    Counter,
    Gallery,
}

const APPS: [App; 2] = [App::Counter, App::Gallery];

fn launch(app: App, probe: &Probe) -> Launch {
    let probe = probe.clone();
    match app {
        App::Counter => Launch::headless(move || Counter { probe }).name("counter"),
        App::Gallery => Launch::headless(move || Gallery { probe }).name("gallery"),
    }
}

fn start(app: App, probe: &Probe) -> Driver {
    Driver::launch(launch(app, probe)).expect("the headless app starts")
}

fn count() -> Locator {
    Locator::default().name_matching(TextMatch::prefix("Count:"))
}

fn named(name: &str) -> Locator {
    Locator::named(name)
}

/// A lock file of the test's own, so tests that take the OS-input lock do not contend with
/// each other or with a run on the host.
fn lock_file(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("os-input.lock")
}

/// The press-down half of a tap at `at`: the contact stays held.
fn press_and_hold(at: LogicalPoint) -> ActionSequence {
    ActionSequence::new(vec![InputSource::Pointer {
        id: "held".into(),
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
        ],
    }])
}

type Row = (&'static str, fn(App));

/// The failure scenarios of the driver's design, in its order.
const ROWS: &[Row] = &[
    ("the app exits mid-command", the_app_exits_mid_command),
    (
        "the driver goes with a contact held",
        driver_goes_with_a_contact_held,
    ),
    ("an accepted action times out", an_accepted_action_times_out),
    (
        "app code panics during a request",
        app_code_panics_during_a_request,
    ),
    (
        "a callback replaces what was tapped",
        a_callback_replaces_what_was_tapped,
    ),
    (
        "a handle outlives its element",
        a_handle_outlives_its_element,
    ),
    (
        "two OS-input sessions on one desktop",
        two_os_input_sessions_on_one_desktop,
    ),
    ("a path the target lacks", a_path_the_target_lacks),
    ("a point outside the window", a_point_outside_the_window),
    ("a read past its budget", a_read_past_its_budget),
    (
        "the user interrupts an OS run",
        the_user_interrupts_an_os_run,
    ),
];

#[test]
fn every_failure_row_holds_on_every_fixture() {
    for app in APPS {
        for (row, case) in ROWS {
            eprintln!("{row} on the {app:?}");
            case(app);
        }
    }
}

fn the_app_exits_mid_command(app: App) {
    let probe = Probe::default();
    let mut driver = start(app, &probe);
    let mut window = driver.main_window().expect("a window");
    // The handler closes the app's only window, and the app exits while the tap is answered.
    let ended = window.locate(named("Quit")).tap();
    eprintln!("quit answered {ended:?}");
    match ended {
        Err(Error::Gone {
            kind: Some(HandleKind::Process),
            ref message,
            ..
        }) => assert!(message.contains("exited"), "{message}"),
        other => {
            panic!("{app:?}: an app that exits mid-command answers the process gone: {other:?}")
        }
    }
    // Every later call fails the same way, at once.
    let later = driver.windows();
    assert!(
        matches!(
            later,
            Err(Error::Gone {
                kind: Some(HandleKind::Process),
                ..
            })
        ),
        "{app:?}: {later:?}"
    );
}

fn driver_goes_with_a_contact_held(app: App) {
    let probe = Probe::default();
    let mut driver = start(app, &probe);
    let mut window = driver.main_window().expect("a window");
    let hold = window
        .locate(Locator::identifier("hold"))
        .resolve()
        .expect("the hold surface")
        .actionability()
        .expect("its facts");
    let at = hold.point.expect("a point to press");
    window
        .perform(press_and_hold(at))
        .expect("the press is delivered");
    assert_eq!(probe.downs(), 1, "{app:?}: the contact went down");
    assert_eq!(probe.cancels(), 0);
    drop(driver);
    assert_eq!(
        probe.cancels(),
        1,
        "{app:?}: the driver's going cancels the contact it held"
    );
}

fn an_accepted_action_times_out(app: App) {
    let probe = Probe::default();
    // The agent answers `timeout` for whatever its window has not answered within a millisecond
    // of virtual time: a read answers in the frame it is asked, a tap takes frames to play.
    let launch = launch(app, &probe).reply_timeout(Duration::from_millis(1));
    let mut driver = Driver::launch(launch).expect("the headless app starts");
    let mut window = driver.main_window().expect("a window");
    let tapped = window.locate(Locator::identifier("increment")).tap();
    let Err(error) = tapped else {
        panic!("{app:?}: the tap answered in time: {tapped:?}");
    };
    assert!(matches!(error, Error::Timeout { .. }), "{app:?}: {error:?}");
    assert_eq!(
        error.effect().map(|effect| effect.kind),
        Some(EffectKind::MayHaveRun),
        "{app:?}: {error:?}"
    );
    assert_eq!(
        error.retry(),
        Retry::Never,
        "an acting call is never retried as it stands"
    );
    // The action stays queued and runs: the next read shows it, and it ran once.
    expect(window.locate(count()))
        .to_have_name("Count: 1")
        .expect("the queued tap ran");
    window
        .wait_until(
            flui_driver::protocol::wait::Condition::Frames(10),
            Duration::from_secs(1),
        )
        .expect("frames pass");
    assert_eq!(probe.presses(), 1, "{app:?}: the tap was not resent");
}

fn app_code_panics_during_a_request(app: App) {
    let probe = Probe::default();
    let mut driver = start(app, &probe);
    let mut window = driver.main_window().expect("a window");
    // On the semantic path the handler runs in the agent's job, which contains it.
    let failed = window
        .locate(named("Explode"))
        .require(InputPath::Semantic)
        .tap();
    assert!(
        matches!(failed, Err(Error::Platform { .. })),
        "{app:?}: {failed:?}"
    );
    assert_eq!(probe.explosions(), 1);
    // On the synthetic path it panics in the frame that dispatches the tap, which contains it.
    let _ = window.locate(named("Explode")).tap();
    assert_eq!(probe.explosions(), 2);
    // The agent stays serving.
    window
        .locate(Locator::identifier("increment"))
        .tap()
        .expect("the next request is answered");
    expect(window.locate(count()))
        .to_have_name("Count: 1")
        .expect("and acts");
}

fn a_callback_replaces_what_was_tapped(app: App) {
    let probe = Probe::default();
    let mut driver = start(app, &probe);
    let mut window = driver.main_window().expect("a window");
    let outcome = window
        .locate(named("Replace me"))
        .tap()
        .expect("the tap whose callback replaces its target");
    assert_eq!(outcome.effect.kind, EffectKind::Ran, "{app:?}");
    expect(window.locate(named("Replaced")))
        .to_have_count(1)
        .expect("the replacement is read");
    expect(window.locate(named("Replace me")))
        .to_be_gone()
        .expect("what was tapped is gone");
}

fn a_handle_outlives_its_element(app: App) {
    let probe = Probe::default();
    let mut driver = start(app, &probe);
    let mut window = driver.main_window().expect("a window");
    let old = window
        .locate(named("Replace me"))
        .resolve()
        .expect("the button")
        .reference();
    window
        .element(old)
        .tap()
        .expect("the button replaces itself");
    expect(window.locate(named("Replaced")))
        .to_have_count(1)
        .expect("a new element stands where it was");
    let stale = window.element(old).tap();
    assert!(
        matches!(
            stale,
            Err(Error::Gone {
                kind: Some(HandleKind::Element) | None,
                ..
            })
        ),
        "{app:?}: the old handle is never rebound: {stale:?}"
    );
}

fn two_os_input_sessions_on_one_desktop(app: App) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let probe = Probe::default();
    let session = |probe: &Probe| {
        let (lane, _log) = ScriptedLane::new(Isolation::Shared);
        start(app, probe)
            .with_os_input_lock(lock_file(&dir))
            .with_input_policy(InputPolicy::require(InputPath::Os))
            .with_lane(lane.guarded())
            .expect("the lane starts")
    };
    let mut first = session(&probe);
    let mut second = session(&probe);
    first
        .main_window()
        .expect("a window")
        .locate(Locator::identifier("increment"))
        .tap()
        .expect("the first session sends OS input");
    let refused = second
        .main_window()
        .expect("a window")
        .locate(Locator::identifier("increment"))
        .tap();
    assert!(
        matches!(refused, Err(Error::Busy { .. })),
        "{app:?}: the second session waits its turn: {refused:?}"
    );
    drop(first);
    second
        .main_window()
        .expect("a window")
        .locate(Locator::identifier("increment"))
        .tap()
        .expect("once the first session ends, the second takes the desktop");
}

fn a_path_the_target_lacks(app: App) {
    let probe = Probe::default();
    let mut driver = start(app, &probe);
    driver.set_input_policy(InputPolicy::require(InputPath::Os));
    let mut report = Report::new();
    let tapped = report.check("tap through the OS", || {
        driver
            .main_window()?
            .locate(Locator::identifier("increment"))
            .tap()
    });
    match tapped {
        Err(Error::NotSupported {
            capability: Some(ref capability),
            ..
        }) => assert_eq!(capability, "os_input"),
        other => panic!("{app:?}: a missing path is named, not fallen back from: {other:?}"),
    }
    assert_eq!(report.verdict(), Verdict::CannotVerify);
    assert_eq!(report.cannot_verify(), ["os_input"]);
    assert_eq!(probe.presses(), 0, "nothing was delivered by another path");
}

fn a_point_outside_the_window(app: App) {
    let probe = Probe::default();
    let (lane, log) = ScriptedLane::new(Isolation::Shared);
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = start(app, &probe)
        .with_os_input_lock(lock_file(&dir))
        .with_lane(lane.guarded())
        .expect("the lane starts");
    let mut window = driver.main_window().expect("a window");
    let outside = press_and_hold(LogicalPoint::new(800.0, 10.0));
    let refused = window.perform_via(outside, InputPolicy::require(InputPath::Os));
    assert!(
        matches!(refused, Err(Error::OutsideTarget { .. })),
        "{app:?}: the right edge is outside, never clamped in: {refused:?}"
    );
    assert!(log.sent().is_empty(), "nothing was sent");
}

fn a_read_past_its_budget(app: App) {
    let probe = Probe::default();
    let mut driver = start(app, &probe);
    let mut window = driver.main_window().expect("a window");
    let tree = window
        .read_with(ReadQuery::new().max_nodes(2))
        .expect("a budgeted read");
    assert!(tree.truncated, "{app:?}: a read past its budget says so");
    let whole = window.read().expect("a whole read");
    assert!(!whole.truncated, "{app:?}");
}

fn the_user_interrupts_an_os_run(app: App) {
    let probe = Probe::default();
    let (lane, log) = ScriptedLane::new(Isolation::Shared);
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = start(app, &probe)
        .with_os_input_lock(lock_file(&dir))
        .with_input_policy(InputPolicy::require(InputPath::Os))
        .with_lane(lane.foreign_input_after(1, "a mouse move"))
        .expect("the lane starts");
    let mut report = Report::new();
    let interrupted = report.check("tap through the OS", || {
        driver
            .main_window()?
            .locate(Locator::identifier("increment"))
            .tap()
    });
    match interrupted {
        Err(Error::InterruptedByUser { ref event, .. }) => assert_eq!(event, "a mouse move"),
        other => panic!("{app:?}: {other:?}"),
    }
    assert_eq!(
        log.sent().len(),
        1,
        "{app:?}: the lane stopped before its next event"
    );
    assert!(log.releases() >= 1, "{app:?}: and released what it held");
    assert_eq!(
        report.verdict(),
        Verdict::CannotVerify,
        "never a failure, never a pass"
    );
    assert_eq!(report.cannot_verify(), ["uninterrupted_os_input"]);
}
