//! The Windows lane against a real app in a real window: the scenario app (`examples/
//! scenario_app.rs`) launched in its kill-on-close job, read through UI Automation, captured
//! through Windows Graphics Capture, and driven through the OS lane.
//!
//! nextest starts each test on a desktop of its own (`docs/testing.md`, "The Windows test
//! desktop"): windows, UI Automation, hooks and the compositor work there, and no injected input
//! does, since only the desktop that takes the user's input takes it. The lane reads that and
//! sends nothing there: a run through it is `cannot_verify`. Run by hand on the input desktop
//! (`FLUI_TEST_VISIBLE_DESKTOP=1`) the same tests send real input and must pass; no automated run
//! does that yet, and those runs are owed (`crates/flui-native/platform-parity.toml`).
//! `FLUI_REQUIRE_INPUT_DESKTOP=1` makes a run off the input desktop fail instead of reporting
//! `CANNOT_VERIFY`.
//!
//! A test that opens a window, installs the hooks, starts a job or sends input first asks
//! [`private_desktop::require`], which fails a run that is not on a desktop of its own without
//! `FLUI_TEST_VISIBLE_DESKTOP=1`: libtest (`cargo test`, cargo-gamma) starts a test on the
//! person's desktop, where these tests would open windows over their work. The `LiveDesktop` it
//! returns is a parameter of `session`, `live_guard`, `live_job` and `foreign_mouse_move`, so
//! those do not compile without the check; cargo-gamma skips this module (`gamma.toml`).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use flui_driver::lane::windows::job::KillOnCloseJob;
use flui_driver::lane::windows::{
    STOPPED_WATCHING, WindowsGuard, WindowsLane, desktop_takes_input,
};
use flui_driver::lane::{ActivityGuard, Lane};
use flui_driver::os_input::OsDevice;
use flui_driver::protocol::input::{DeviceClass, InputKind, PointerButton, PointerDevice};
use flui_driver::protocol::report::Report;
use flui_driver::protocol::verdict::Verdict;
use flui_driver::protocol::wire::Role;
use flui_driver::steps::OsEvent;
use flui_driver::{
    Driver, Error, InputPath, InputPolicy, Launch, Locator, Scenario, WindowSpec, expect,
};

use crate::private_desktop::{self, LiveDesktop};
use crate::scenarios;

/// The scenario app `app`, built from this crate's example in its debug profile, which carries
/// the agent.
fn scenario_app(app: &str) -> Launch {
    Launch::cargo_example("scenario_app")
        .package("flui-driver")
        .env("FLUI_DRIVER_SCENARIO_APP", app)
        .connect_wait(Duration::from_secs(60))
}

/// How long a launched app may take to put up its window with content.
const WINDOW_WITHIN: Duration = Duration::from_secs(60);

/// A session of the scenario app `app` with the Windows lane attached, its OS-input lock in
/// `dir`, once the app shows its window.
fn session(_live: &LiveDesktop, app: &str, dir: &tempfile::TempDir) -> Driver {
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
            WindowsLane::try_new(pid, flui_driver::Isolation::Shared)
                .expect("the lane runs on the shared desktop"),
        )
        .expect("the lane starts")
}

/// A guard, on a desktop these tests may use: its hooks and raw-input window go on the calling
/// thread's desktop.
fn live_guard(_live: &LiveDesktop) -> WindowsGuard {
    WindowsGuard::new()
}

/// A kill-on-close job, on a desktop these tests may use: what runs in it may show a window.
fn live_job(_live: &LiveDesktop) -> KillOnCloseJob {
    KillOnCloseJob::try_new().expect("a job")
}

/// Waits for the app's window to show its controls.
fn shown(driver: &mut Driver) {
    let mut window = driver.main_window().expect("the app shows a window");
    expect(window.locate(Locator::identifier("like")))
        .timeout(Duration::from_secs(30))
        .to_have_count(1)
        .expect("the gallery shows its like button");
}

/// Whether this test runs where injected input goes; off it, the run reports `CANNOT_VERIFY`, or
/// fails under `FLUI_REQUIRE_INPUT_DESKTOP=1`.
fn input_desktop_for(what: &str) -> bool {
    if desktop_takes_input() {
        return true;
    }
    assert!(
        std::env::var_os("FLUI_REQUIRE_INPUT_DESKTOP").is_none_or(|required| required != "1"),
        "{what} needs the desktop that takes the user's input (FLUI_REQUIRE_INPUT_DESKTOP=1)"
    );
    eprintln!("CANNOT_VERIFY {what}: it needs the desktop that takes the user's input");
    false
}

#[test]
fn ui_automation_reads_and_invokes_the_app_as_a_screen_reader_does() {
    let live = private_desktop::require();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session(&live, "controls", &dir);
    shown(&mut driver);
    let mut window = driver.main_window().expect("a window");
    let tree = window.os_tree().expect("UI Automation reads the window");
    let like = scenarios_find(&tree.roots, "like").expect("UI Automation shows like");
    assert_eq!(like.role, Role::Button, "{like:?}");
    assert_eq!(like.name.as_deref(), Some("Like"));
    assert_eq!(
        like.automation_id.as_deref(),
        Some("like"),
        "the identifier is the AutomationId"
    );
    assert!(like.rect.is_some(), "UI Automation places it on screen");
    let projection = window.compare_projection().expect("both trees read");
    assert!(
        projection.is_equal(),
        "the OS shows what the app published: {:#?}\napp:\n{}\nos:\n{}",
        projection.differences(),
        projection.app,
        projection.os
    );
    window
        .os_invoke("like")
        .expect("UI Automation invokes like");
    expect(window.locate(Locator::identifier("likes")))
        .to_have_name("Likes: 1")
        .expect("the invoke reached the app");
    assert!(matches!(
        window.os_invoke("nothing-here"),
        Err(Error::NotFound { .. })
    ));
}

fn scenarios_find<'a>(
    nodes: &'a [flui_driver::protocol::tree::Node],
    automation_id: &str,
) -> Option<&'a flui_driver::protocol::tree::Node> {
    nodes.iter().find_map(|node| {
        if node.automation_id.as_deref() == Some(automation_id) {
            Some(node)
        } else {
            scenarios_find(&node.children, automation_id)
        }
    })
}

#[test]
fn every_role_reads_back_as_its_control_type_through_ui_automation() {
    let live = private_desktop::require();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session(&live, "roles", &dir);
    let mut window = driver.main_window().expect("a window");
    expect(window.locate(Locator::named("button fixture")))
        .timeout(Duration::from_secs(30))
        .to_have_count(1)
        .expect("the roles page shows");
    let mut report = Report::new();
    driver.run_scenario(&mut report, "roles", scenarios::roles);
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
fn the_compositor_captures_the_apps_window_alone() {
    let live = private_desktop::require();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session(&live, "controls", &dir);
    shown(&mut driver);
    let mut window = driver.main_window().expect("a window");
    let placement = window.placement().expect("a placement");
    let captured = window
        .capture(flui_driver::pixels::CaptureSource::Os)
        .map(|capture| capture.image().clone());
    // The compositor gives a window on a desktop it does not show no area: such a host cannot
    // make the capture, and says so.
    if !input_desktop_for("the_compositor_captures_the_apps_window_alone") {
        let error = captured.expect_err("a window the compositor does not show has no capture");
        assert!(error.is_cannot_verify(), "{error:?}");
        return;
    }
    let capture = captured.expect("the compositor captures the window");
    let device = |logical: f64| (logical * placement.scale).round() as u32;
    assert_eq!(
        (capture.width(), capture.height()),
        (device(placement.size.width), device(placement.size.height)),
        "the capture is the window's surface, at its scale"
    );
    let first = capture.pixel(0, 0).expect("a first pixel");
    let differs = (0..capture.height())
        .step_by(4)
        .flat_map(|y| (0..capture.width()).step_by(4).map(move |x| (x, y)))
        .any(|(x, y)| capture.pixel(x, y) != Some(first));
    assert!(differs, "the app drew its controls, not one flat colour");
}

#[test]
fn the_guard_watches_and_stops_on_a_desktop_of_its_own() {
    let live = private_desktop::require();
    let mut guard = live_guard(&live);
    guard
        .arm()
        .expect("the hooks and the raw-input window install");
    assert_eq!(guard.foreign_input(), None, "nobody touched this desktop");
    guard.disarm();
    guard.arm().expect("a guard arms again after it disarmed");
    guard.disarm();
}

/// The lane's own check, before a sequence's first event, refuses typed text whose Tab under a
/// held Alt is the task switcher: it reads the plan the lane sends, text included.
#[test]
fn the_lanes_check_refuses_typed_text_that_completes_a_shell_hotkey() {
    let mut lane = WindowsLane::for_process(std::process::id());
    let alt_tab = [
        OsEvent::KeyDown("Alt".into()),
        OsEvent::Text("\t".into()),
        OsEvent::KeyUp("Alt".into()),
    ];
    let refused = lane.check(&alt_tab);
    assert!(
        matches!(
            &refused,
            Err(Error::NotSupported { capability: Some(capability), .. })
                if capability == "os_shell_hotkey"
        ),
        "{refused:?}"
    );
    let plain = [
        OsEvent::KeyDown("Alt".into()),
        OsEvent::Text("plain".into()),
        OsEvent::KeyUp("Alt".into()),
    ];
    assert!(lane.check(&plain).is_ok());
}

/// The lane's check lets the app have the chords the shell would take only on a release, which
/// the lane masks: five Shift-clicks, Control+Shift+click, a sequence that ends with Control+Shift
/// held; the presses the shell takes are still refused.
#[test]
fn the_lanes_check_allows_chords_whose_release_is_masked() {
    let mut lane = WindowsLane::for_process(std::process::id());
    let click = || {
        let button = |pressed: bool| {
            let (pointer, device, button) = (
                "mouse".to_owned(),
                PointerDevice::Mouse,
                PointerButton::Primary,
            );
            if pressed {
                OsEvent::PointerDown {
                    pointer,
                    device,
                    button,
                }
            } else {
                OsEvent::PointerUp {
                    pointer,
                    device,
                    button,
                }
            }
        };
        [button(true), button(false)]
    };
    let key = |name: &str, pressed: bool| {
        if pressed {
            OsEvent::KeyDown(name.into())
        } else {
            OsEvent::KeyUp(name.into())
        }
    };
    let shift_click = || {
        let mut events = vec![key("Shift", true)];
        events.extend(click());
        events.push(key("Shift", false));
        events
    };
    let five_shift_clicks: Vec<OsEvent> = (0..5).flat_map(|_| shift_click()).collect();
    let mut control_shift_click = vec![key("Control", true), key("Shift", true)];
    control_shift_click.extend(click());
    control_shift_click.extend([key("Shift", false), key("Control", false)]);
    let left_held = [key("Control", true), key("Shift", true)];
    for (case, events) in [
        ("five Shift-clicks", five_shift_clicks.as_slice()),
        ("Control+Shift+click", control_shift_click.as_slice()),
        ("Control+Shift left held", left_held.as_slice()),
    ] {
        let checked = lane.check(events);
        assert!(checked.is_ok(), "{case}: {checked:?}");
    }
    for refused in [
        vec![key("Alt", true), key("Tab", true)],
        vec![key("Meta", true)],
        vec![
            key("Control", true),
            key("Shift", true),
            key("Escape", true),
        ],
    ] {
        let checked = lane.check(&refused);
        assert!(
            matches!(
                &checked,
                Err(Error::NotSupported { capability: Some(capability), .. })
                    if capability == "os_shell_hotkey"
            ),
            "{refused:?}: {checked:?}"
        );
    }
}

/// A guard whose watcher is gone while armed can no longer tell the person's input from the
/// lane's: it says so, which ends the run as the person's input does, never as unguarded.
#[test]
fn a_guard_whose_watcher_died_ends_the_run() {
    let live = private_desktop::require();
    let mut guard = live_guard(&live);
    guard.arm().expect("the guard watches");
    assert!(
        flui_driver::testing::end_guard_watcher(&guard),
        "the watcher was there to end"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let reported = loop {
        if let Some(foreign) = guard.foreign_input() {
            break Some(foreign);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        reported.map(|foreign| foreign.description),
        Some(STOPPED_WATCHING.to_owned())
    );
    guard.disarm();
}

/// A launched process joins its job before its first instruction: it starts suspended, runs
/// nothing until resumed, and then runs.
#[test]
fn a_process_joins_its_job_before_it_runs() {
    let live = private_desktop::require();
    let job = live_job(&live);
    let mut command = Command::new("cmd");
    command
        .args(["/c", "exit", "7"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut suspended = job
        .spawn_suspended(&mut command)
        .expect("started suspended");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        suspended.try_wait().expect("a status").is_none(),
        "a suspended process has not run"
    );
    let mut child = suspended.resume().expect("resumed");
    let status = child.wait().expect("it ran");
    assert_eq!(status.code(), Some(7));
}

#[test]
fn a_job_ends_its_processes_when_it_closes() {
    let live = private_desktop::require();
    let job = live_job(&live);
    let mut command = Command::new("cmd");
    command
        .args(["/c", "ping", "-n", "60", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = job
        .spawn_suspended(&mut command)
        .expect("a child that would run a minute, in the job")
        .resume()
        .expect("resumed");
    assert!(child.try_wait().expect("a status").is_none(), "it runs");
    drop(job);
    let deadline = Instant::now() + Duration::from_secs(10);
    let ended = loop {
        if let Some(status) = child.try_wait().expect("a status") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(ended.is_some(), "closing the job ended the child");
}

#[test]
fn a_launched_app_ends_with_its_session() {
    let live = private_desktop::require();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let driver = session(&live, "controls", &dir);
    let pid = driver.app_pid().expect("a process");
    drop(driver);
    let gone = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .expect("tasklist runs");
    let listing = String::from_utf8_lossy(&gone.stdout);
    assert!(
        !listing.contains(&pid.to_string()),
        "process {pid} outlived its session: {listing}"
    );
}

/// The LY99 conformance row: the scenario the headless target runs in CI
/// (`scenarios::the_conformance_scenario_passes_on_the_headless_target`), here through the OS
/// lane, as its certification run.
#[test]
fn the_conformance_scenario_runs_through_the_os_lane() {
    let live = private_desktop::require();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver = session(&live, "controls", &dir);
    shown(&mut driver);
    let scale = driver
        .main_window()
        .and_then(|mut window| window.placement())
        .expect("a placement")
        .scale;
    driver
        .declare(
            Scenario::new("conformance")
                .window(WindowSpec::new(
                    f64::from(scenarios::WINDOW.0),
                    f64::from(scenarios::WINDOW.1),
                    scale,
                ))
                .takes([OsDevice::Mouse], Duration::from_secs(5))
                .certification(),
        )
        .expect("the window is the scenario's");
    assert_eq!(driver.input_policy(), InputPolicy::require(InputPath::Os));
    let mut report = Report::new();
    driver.run_scenario(&mut report, "conformance", scenarios::conformance);
    assert_eq!(report.lane(), Some("windows"));
    assert!(
        !report.announcements().is_empty(),
        "an OS run is announced before its first event"
    );
    if !input_desktop_for("the_conformance_scenario_runs_through_the_os_lane") {
        assert_eq!(report.verdict(), Verdict::CannotVerify, "{report:#?}");
        assert!(
            report.cannot_verify().contains(&"windows_lane".to_owned()),
            "off the input desktop the lane is one the host cannot run: {:?}",
            report.cannot_verify()
        );
        return;
    }
    assert_eq!(report.verdict(), Verdict::Pass, "{report:#?}");
}

/// The certification runs of the device checks, through the OS lane: `cannot_verify` off the
/// input desktop, and passing on it.
#[test]
fn the_device_check_scenarios_run_through_the_os_lane() {
    let live = private_desktop::require();
    type ScenarioFn = fn(&mut Driver, &mut Report) -> flui_driver::Result<()>;
    // A row: the scenario, its app, what it takes, and whether it sends OS input at all (the
    // accessibility scenario acts through UI Automation and the agent alone, so it passes off the
    // input desktop too).
    let rows: [(&str, &str, ScenarioFn, &[OsDevice], bool); 4] = [
        (
            "windows-gallery",
            "controls",
            scenarios::gallery,
            &[OsDevice::Mouse, OsDevice::Keyboard],
            true,
        ),
        (
            "windows-a11y",
            "controls",
            scenarios::accessibility,
            &[],
            false,
        ),
        (
            "windows-input",
            "controls",
            scenarios::input,
            &[
                OsDevice::Mouse,
                OsDevice::Touch,
                OsDevice::Pen,
                OsDevice::Keyboard,
            ],
            true,
        ),
        (
            "windows-notes",
            "notes",
            scenarios::notes,
            &[OsDevice::Keyboard, OsDevice::Mouse],
            true,
        ),
    ];
    let on_input_desktop = input_desktop_for("the_device_check_scenarios_run_through_the_os_lane");
    for (name, app, scenario, devices, sends_input) in rows {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut driver = session(&live, app, &dir);
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
        let mut report = Report::new();
        driver.run_scenario(&mut report, name, scenario);
        let expected = if on_input_desktop || !sends_input {
            Verdict::Pass
        } else {
            Verdict::CannotVerify
        };
        assert_eq!(report.verdict(), expected, "{name}: {report:#?}");
    }
}

/// Moving the mouse during an OS run ends it `cannot_verify` with what the lane pressed
/// released. The foreign move comes from a second `SendInput` caller, which the guard cannot tell
/// from a person's hand: it carries no lane tag.
#[test]
fn a_foreign_mouse_move_mid_run_ends_it_and_releases_its_input() {
    if !input_desktop_for("a_foreign_mouse_move_mid_run_ends_it_and_releases_its_input") {
        return;
    }
    let live = private_desktop::require();
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut driver =
        session(&live, "controls", &dir).with_input_policy(InputPolicy::require(InputPath::Os));
    shown(&mut driver);
    let intruder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        foreign_mouse_move(&live);
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
    // The contact the lane put down was lifted: the app's journal ends it.
    let journal = window.journal().expect("the journal");
    let touch: Vec<InputKind> = journal
        .entries
        .iter()
        .filter(|entry| entry.device == DeviceClass::Touch)
        .map(|entry| entry.kind)
        .collect();
    assert_eq!(touch.first(), Some(&InputKind::PointerDown), "{touch:?}");
    assert!(
        matches!(
            touch.last(),
            Some(InputKind::PointerUp | InputKind::PointerCancel)
        ),
        "the contact was released: {touch:?}"
    );
}

/// One relative mouse move through `SendInput`, with no lane tag.
fn foreign_mouse_move(_live: &LiveDesktop) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 1,
                dy: 0,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    #[expect(
        unsafe_code,
        reason = "one SendInput call, the foreign input under test"
    )]
    // SAFETY: one fully initialized INPUT and its size.
    let sent = unsafe { SendInput(&[input], i32::try_from(size_of::<INPUT>()).expect("small")) };
    assert_eq!(sent, 1, "the foreign move went out");
}
