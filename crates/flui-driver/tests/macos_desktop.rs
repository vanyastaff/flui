//! The macOS lane against a real app in a real window: the scenario app (`examples/
//! scenario_app.rs`) launched in a process group of its own, read and pressed through the
//! accessibility API, captured through ScreenCaptureKit, and driven through the OS lane.
//!
//! The lane needs three privacy permissions for the process running the tests (the terminal, or
//! the test binary itself): Accessibility, Input Monitoring and Screen Recording. Without one, a
//! run that needs it reports `CANNOT_VERIFY` naming it, as a host that cannot run the lane;
//! `FLUI_REQUIRE_MACOS_PERMISSIONS=1` makes such a run fail instead. No automated run grants them
//! (CI type-checks this file on its macOS runner), so these runs are owed
//! (`crates/flui-native/platform-parity.toml`).

use std::process::Command;
use std::time::{Duration, Instant};

use flui_driver::lane::macos::safety::Permission;
use flui_driver::lane::macos::{MacosGuard, MacosLane, granted};
use flui_driver::lane::{ActivityGuard, Lane};
use flui_driver::os_input::OsDevice;
use flui_driver::protocol::input::InputKind;
use flui_driver::protocol::wire::Role;
use flui_driver::steps::OsEvent;
use flui_driver::{
    Driver, Error, InputPath, InputPolicy, Launch, Locator, Report, Scenario, Verdict, WindowSpec,
    expect,
};

use crate::scenarios;

/// The scenario app `app`, built from this crate's example in its debug profile, which carries
/// the agent.
fn scenario_app(app: &str) -> Launch {
    Launch::cargo_example("scenario_app")
        .package("flui-driver")
        .env("FLUI_DRIVER_SCENARIO_APP", app)
        .connect_wait(Duration::from_secs(120))
}

/// How long a launched app may take to put up its window with content.
const WINDOW_WITHIN: Duration = Duration::from_secs(60);

/// A session of the scenario app `app` with the macOS lane attached, its OS-input lock in `dir`,
/// once the app shows its window.
fn session(app: &str, dir: &tempfile::TempDir) -> Driver {
    let mut driver = Driver::launch(scenario_app(app)).expect("the scenario app starts");
    let deadline = Instant::now() + WINDOW_WITHIN;
    while driver.windows().expect("the app answers").is_empty() {
        assert!(
            Instant::now() < deadline,
            "the app showed no window with content within {WINDOW_WITHIN:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let pid = driver.app_pid().expect("a launched app has a process");
    driver
        .with_os_input_lock(dir.path().join("os-input.lock"))
        .with_lane(
            MacosLane::try_new(pid, flui_driver::Isolation::Shared)
                .expect("the lane runs on the shared desktop"),
        )
        .expect("the lane starts")
}

/// Waits for the app's window to show its controls.
fn shown(driver: &mut Driver) {
    let mut window = driver.main_window().expect("the app shows a window");
    expect(window.locate(Locator::identifier("like")))
        .timeout(Duration::from_secs(30))
        .to_have_count(1)
        .expect("the gallery shows its like button");
}

/// Whether the process holds `permissions`, which `what` needs; without them the run reports
/// `CANNOT_VERIFY`, or fails under `FLUI_REQUIRE_MACOS_PERMISSIONS=1`.
fn permitted_for(what: &str, permissions: &[Permission]) -> bool {
    let missing: Vec<Permission> = permissions
        .iter()
        .copied()
        .filter(|permission| !granted(*permission))
        .collect();
    if missing.is_empty() {
        return true;
    }
    assert!(
        std::env::var_os("FLUI_REQUIRE_MACOS_PERMISSIONS").is_none_or(|required| required != "1"),
        "{what} needs {missing:?} (FLUI_REQUIRE_MACOS_PERMISSIONS=1)"
    );
    eprintln!("CANNOT_VERIFY {what}: the process lacks {missing:?}");
    false
}

fn find<'a>(
    nodes: &'a [flui_driver::protocol::tree::Node],
    identifier: &str,
) -> Option<&'a flui_driver::protocol::tree::Node> {
    nodes.iter().find_map(|node| {
        if node.automation_id.as_deref() == Some(identifier) {
            Some(node)
        } else {
            find(&node.children, identifier)
        }
    })
}

#[test]
fn the_accessibility_api_reads_and_presses_the_app_as_voiceover_does() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session("controls", &dir);
    shown(&mut driver);
    let mut window = driver.main_window().expect("a window");
    let tree = window.os_tree();
    if !permitted_for(
        "the_accessibility_api_reads_and_presses_the_app_as_voiceover_does",
        &[Permission::Accessibility],
    ) {
        let error = tree.expect_err("an untrusted process reads no accessibility tree");
        assert_eq!(
            error.missing_capability().as_deref(),
            Some("macos_permission_accessibility")
        );
        return;
    }
    let tree = tree.expect("the accessibility API reads the window");
    let like = find(&tree.roots, "like").expect("the accessibility API shows like");
    assert_eq!(like.role, Role::Button, "{like:?}");
    assert_eq!(like.name.as_deref(), Some("Like"));
    assert_eq!(like.native_role, "AXButton");
    assert!(
        like.rect.is_some(),
        "the accessibility API places it on screen"
    );
    let projection = window.compare_projection().expect("both trees read");
    assert!(
        projection.is_equal(),
        "the OS shows what the app published: {:#?}\napp:\n{}\nos:\n{}",
        projection.differences(),
        projection.app,
        projection.os
    );
    window.os_invoke("like").expect("AXPress presses like");
    expect(window.locate(Locator::identifier("likes")))
        .to_have_name("Likes: 1")
        .expect("the press reached the app");
    assert!(matches!(
        window.os_invoke("nothing-here"),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn every_role_reads_back_as_its_ax_role_through_the_accessibility_api() {
    if !permitted_for(
        "every_role_reads_back_as_its_ax_role_through_the_accessibility_api",
        &[Permission::Accessibility],
    ) {
        return;
    }
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session("roles", &dir);
    let mut window = driver.main_window().expect("a window");
    expect(window.locate(Locator::named("button fixture")))
        .timeout(Duration::from_secs(30))
        .to_have_count(1)
        .expect("the roles page shows");
    let mut report = Report::new();
    report.run_scenario("macos-roles", &mut driver, scenarios::roles);
    assert_eq!(
        report.verdict(),
        Verdict::Pass,
        "{:#?}",
        report
            .checks()
            .iter()
            .filter(|check| check.outcome.verdict() != Verdict::Pass)
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_window_server_captures_the_apps_window_alone() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session("controls", &dir);
    shown(&mut driver);
    let mut window = driver.main_window().expect("a window");
    let placement = window.placement().expect("a placement");
    let captured = window
        .capture(flui_driver::pixels::CaptureSource::Os)
        .map(|capture| capture.image().clone());
    if !permitted_for(
        "the_window_server_captures_the_apps_window_alone",
        &[Permission::ScreenRecording],
    ) {
        let error = captured.expect_err("no capture without Screen Recording");
        assert!(error.is_cannot_verify(), "{error:?}");
        return;
    }
    let capture = captured.expect("ScreenCaptureKit captures the window");
    let device = |logical: f64| (logical * placement.scale).round() as u32;
    assert_eq!(
        (capture.width(), capture.height()),
        (device(placement.size.width), device(placement.size.height)),
        "the capture is the window's content, at its scale"
    );
    let first = capture.pixel(0, 0).expect("a first pixel");
    let differs = (0..capture.height())
        .step_by(4)
        .flat_map(|y| (0..capture.width()).step_by(4).map(move |x| (x, y)))
        .any(|(x, y)| capture.pixel(x, y) != Some(first));
    assert!(differs, "the app drew its controls, not one flat colour");
}

#[test]
fn the_guard_listens_and_stops() {
    let mut guard = MacosGuard::new();
    let armed = guard.arm();
    if !permitted_for(
        "the_guard_listens_and_stops",
        &[Permission::InputMonitoring],
    ) {
        let error = armed.expect_err("no tap without Input Monitoring");
        assert_eq!(
            error.missing_capability().as_deref(),
            Some("macos_permission_input_monitoring")
        );
        return;
    }
    armed.expect("the event tap installs");
    guard.disarm();
    guard.arm().expect("a guard arms again after it disarmed");
    guard.disarm();
}

/// The lane's own check, before a sequence's first event, refuses typed text whose Tab under a
/// held Command is the app switcher: it reads the plan the lane posts, text included.
#[test]
fn the_lanes_check_refuses_typed_text_that_completes_a_system_shortcut() {
    let mut lane =
        MacosLane::try_new(std::process::id(), flui_driver::Isolation::Shared).expect("a lane");
    let command_tab = [
        OsEvent::KeyDown("Meta".into()),
        OsEvent::Text("\t".into()),
        OsEvent::KeyUp("Meta".into()),
    ];
    let refused = lane.check(&command_tab);
    assert!(
        matches!(
            &refused,
            Err(Error::NotSupported { capability: Some(capability), .. })
                if capability == "os_shell_hotkey"
        ),
        "{refused:?}"
    );
    let plain = [
        OsEvent::KeyDown("Meta".into()),
        OsEvent::Text("plain".into()),
        OsEvent::KeyUp("Meta".into()),
    ];
    let checked = lane.check(&plain);
    if permitted_for(
        "the_lanes_check_refuses_typed_text_that_completes_a_system_shortcut",
        &[Permission::Accessibility],
    ) {
        assert!(checked.is_ok(), "{checked:?}");
    } else {
        assert_eq!(
            checked
                .expect_err("no events without Accessibility")
                .missing_capability()
                .as_deref(),
            Some("macos_permission_accessibility")
        );
    }
}

/// A launched app leads a process group of its own, and ends with its session.
#[test]
fn a_launched_app_ends_with_its_session() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let driver = session("controls", &dir);
    let pid = driver.app_pid().expect("a process");
    let group = Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    assert!(
        group.status.success(),
        "ps lists the running app: {group:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&group.stdout).trim(),
        pid.to_string(),
        "the app leads its own process group"
    );
    drop(driver);
    // `ps -p` fails when no process has the id: the app is gone (its group's grandchildren are
    // `launch::a_launched_apps_process_group_ends_with_its_session`'s).
    let gone = Command::new("ps")
        .args(["-o", "pid=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    assert!(
        !gone.status.success() && String::from_utf8_lossy(&gone.stdout).trim().is_empty(),
        "process {pid} outlived its session: {gone:?}"
    );
}

/// The scenario's certification run through the OS lane: announced, `cannot_verify` naming the
/// permission it lacks, and passing with them all.
fn certify(
    name: &str,
    app: &str,
    scenario: fn(&mut Driver, &mut Report) -> flui_driver::Result<()>,
    devices: &[OsDevice],
    sends_input: bool,
) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session(app, &dir);
    let scale = driver
        .main_window()
        .and_then(|mut window| window.placement())
        .expect("a placement")
        .scale;
    driver
        .declare(
            Scenario::new(name)
                .window(WindowSpec::new(
                    f64::from(scenarios::WINDOW.0),
                    f64::from(scenarios::WINDOW.1),
                    scale,
                ))
                .takes(devices.iter().copied(), Duration::from_secs(20))
                .certification(),
        )
        .expect("the window is the scenario's");
    assert_eq!(driver.input_policy(), InputPolicy::require(InputPath::Os));
    let mut report = Report::new();
    report.run_scenario(name, &mut driver, scenario);
    assert_eq!(report.lane(), Some("macos"), "{name}");
    let needed: &[Permission] = if sends_input {
        &[Permission::Accessibility, Permission::InputMonitoring]
    } else {
        &[Permission::Accessibility]
    };
    if !permitted_for(name, needed) {
        assert_eq!(
            report.verdict(),
            Verdict::CannotVerify,
            "{name}: {report:#?}"
        );
        return;
    }
    if sends_input {
        assert!(
            !report.announcements().is_empty(),
            "{name}: an OS run is announced before its first event"
        );
    }
    assert_eq!(report.verdict(), Verdict::Pass, "{name}: {report:#?}");
}

/// The conformance row: the scenario the headless target runs in CI
/// (`scenarios::the_conformance_scenario_passes_on_the_headless_target`), here through the OS
/// lane, as its certification run.
#[test]
fn the_conformance_scenario_runs_through_the_os_lane() {
    certify(
        "conformance",
        "controls",
        scenarios::conformance,
        &[OsDevice::Mouse],
        true,
    );
}

/// The certification runs of the macOS device checks, through the OS lane.
#[test]
fn the_device_check_scenarios_run_through_the_os_lane() {
    type ScenarioFn = fn(&mut Driver, &mut Report) -> flui_driver::Result<()>;
    // A row: the scenario, its app, what it takes, and whether it sends OS input at all (the
    // accessibility scenario acts through the accessibility API and the agent alone).
    let rows: [(&str, &str, ScenarioFn, &[OsDevice], bool); 4] = [
        (
            "macos-gallery",
            "controls",
            scenarios::gallery,
            &[OsDevice::Mouse, OsDevice::Keyboard],
            true,
        ),
        (
            "macos-a11y",
            "controls",
            scenarios::accessibility,
            &[],
            false,
        ),
        (
            "macos-input",
            "controls",
            scenarios::macos_input,
            &[OsDevice::Mouse, OsDevice::Keyboard],
            true,
        ),
        (
            "macos-notes",
            "notes",
            scenarios::macos_notes,
            &[OsDevice::Keyboard, OsDevice::Mouse],
            true,
        ),
    ];
    for (name, app, scenario, devices, sends_input) in rows {
        certify(name, app, scenario, devices, sends_input);
    }
}

/// Moving the mouse during an OS run ends it `cannot_verify` with what the lane pressed
/// released. The foreign move is a Quartz event posted at the HID level with no lane tag, which
/// the guard cannot tell from a person's hand.
#[test]
fn a_foreign_mouse_move_mid_run_ends_it_and_releases_its_input() {
    if !permitted_for(
        "a_foreign_mouse_move_mid_run_ends_it_and_releases_its_input",
        &[Permission::Accessibility, Permission::InputMonitoring],
    ) {
        return;
    }
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver =
        session("controls", &dir).with_input_policy(InputPolicy::require(InputPath::Os));
    shown(&mut driver);
    let intruder = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(300));
        foreign_mouse_move();
    });
    let mut window = driver.main_window().expect("a window");
    let held = window
        .locate(Locator::identifier("pad"))
        .long_press(Some(Duration::from_secs(2)));
    intruder.join().expect("the intruder moved the mouse");
    assert!(
        matches!(held, Err(Error::InterruptedByUser { .. })),
        "the run ended on the foreign move: {held:?}"
    );
    // The press the lane made was released: the app's journal ends it.
    let journal = window.journal().expect("the journal");
    let kinds: Vec<InputKind> = journal.entries.iter().map(|entry| entry.kind).collect();
    let pressed = kinds
        .iter()
        .rposition(|kind| *kind == InputKind::PointerDown)
        .expect("the lane pressed the pad before the move came");
    assert!(
        kinds[pressed..]
            .iter()
            .any(|kind| matches!(kind, InputKind::PointerUp | InputKind::PointerCancel)),
        "the press was released: {kinds:?}"
    );
}

/// One mouse move posted at the HID level from no event source of the lane's.
fn foreign_mouse_move() {
    use objc2_core_foundation::CGPoint;
    use objc2_core_graphics::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
    let event = CGEvent::new_mouse_event(
        None,
        CGEventType::MouseMoved,
        CGPoint::new(5.0, 5.0),
        CGMouseButton::Left,
    )
    .expect("a mouse move");
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&*event));
}
