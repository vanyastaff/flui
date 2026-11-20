//! The X11 lane against a real app in a real window: the scenario app (`examples/scenario_app.rs`)
//! launched on an X virtual framebuffer of the test's own, with a session bus and the AT-SPI bus
//! beside it, read through AT-SPI as Orca reads it, captured through `GetImage`, and driven
//! through XTEST.
//!
//! Each test starts its own Xvfb (on a display no other server holds), `dbus-daemon` and
//! `at-spi-bus-launcher`, and turns AT-SPI on there as a screen reader does; the app and the lane
//! are pointed at them, and the person's desktop is never touched. A host without Xvfb,
//! `dbus-daemon` or `at-spi-bus-launcher` skips them, and fails them where `FLUI_REQUIRE_ATSPI`
//! is set, as CI's `test` job does.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use flui_driver::lane::x11::{STOPPED_WATCHING, X11Guard, X11Lane};
use flui_driver::lane::{ActivityGuard, Lane};
use flui_driver::os_input::OsDevice;
use flui_driver::protocol::input::{
    ActionSequence, DeviceClass, InputKind, InputSource, PointerAction, PointerButton,
    PointerDevice,
};
use flui_driver::protocol::wire::Role;
use flui_driver::steps::OsEvent;
use flui_driver::{
    Driver, Error, InputPath, InputPolicy, Isolation, Launch, Locator, Report, Scenario, Verdict,
    WindowSpec, expect,
};

use crate::scenarios;

/// Where distributions put `at-spi-bus-launcher`.
const LAUNCHERS: &[&str] = &[
    "/usr/libexec/at-spi-bus-launcher",
    "/usr/lib/at-spi2-core/at-spi-bus-launcher",
    "/usr/lib/at-spi-bus-launcher",
];

/// How long the buses take to come up, and a launched app to show its window with content.
const PATIENCE: Duration = Duration::from_secs(60);

/// The programs a desktop started, each leading a process group of its own, killed with what
/// they started when dropped, also when the desktop fails to start.
struct Children(Vec<Child>);

impl Drop for Children {
    fn drop(&mut self) {
        for child in self.0.iter_mut().rev() {
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{}", child.id())])
                .status();
            let _ = child.wait();
        }
    }
}

/// An X server, a session bus and the AT-SPI bus of the test's own.
struct Desktop {
    display: String,
    bus: String,
    _children: Children,
    runtime: tempfile::TempDir,
}

fn available(program: &str) -> bool {
    Command::new(program)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn dbus_send(bus: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("dbus-send")
        .env("DBUS_SESSION_BUS_ADDRESS", bus)
        .args(["--session", "--print-reply"])
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

impl Desktop {
    /// The desktop, or `None` where a program is missing, which fails where `FLUI_REQUIRE_ATSPI`
    /// is set.
    fn start() -> Option<Self> {
        let launcher = LAUNCHERS.iter().map(Path::new).find(|path| path.exists());
        let (Some(launcher), true, true, true) = (
            launcher,
            available("Xvfb"),
            available("dbus-daemon"),
            available("dbus-send"),
        ) else {
            assert!(
                std::env::var_os("FLUI_REQUIRE_ATSPI").is_none(),
                "FLUI_REQUIRE_ATSPI is set and Xvfb, dbus-daemon, dbus-send or \
                 at-spi-bus-launcher is missing"
            );
            eprintln!("skipped: no Xvfb, dbus-daemon, dbus-send or at-spi-bus-launcher");
            return None;
        };
        use std::os::unix::process::CommandExt as _;
        let runtime = tempfile::Builder::new()
            .prefix("flui-driver-x11-")
            .permissions(
                <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
            )
            .tempdir()
            .expect("a runtime directory");
        let mut children = Children(Vec::new());
        let (server, display) = flui_native::testing::xvfb::start(|command| {
            command
                .args(["-screen", "0", "1280x1024x24"])
                .process_group(0);
        })
        .expect("Xvfb starts");
        children.0.push(server);
        let bus = format!("unix:path={}", runtime.path().join("bus").display());
        let mut session = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .arg(format!("--address={bus}"))
            .env("DISPLAY", &display)
            .env("XDG_RUNTIME_DIR", runtime.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("dbus-daemon starts");
        let stdout = session.stdout.take().expect("piped");
        children.0.push(session);
        // The address is printed once the bus listens.
        std::io::BufRead::read_line(&mut std::io::BufReader::new(stdout), &mut String::new())
            .expect("dbus-daemon prints its address");
        children.0.push(
            Command::new(launcher)
                .arg("--launch-immediately")
                .env("DBUS_SESSION_BUS_ADDRESS", &bus)
                .env("DISPLAY", &display)
                .env("XDG_RUNTIME_DIR", runtime.path())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .expect("at-spi-bus-launcher starts"),
        );
        // Asked only once the launcher holds its name: asking the name before would have the bus
        // start a second launcher.
        let deadline = Instant::now() + PATIENCE;
        loop {
            let owned = dbus_send(
                &bus,
                &[
                    "--dest=org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus.NameHasOwner",
                    "string:org.a11y.Bus",
                ],
            );
            if owned.is_some_and(|reply| reply.contains("boolean true")) {
                break;
            }
            assert!(Instant::now() < deadline, "the AT-SPI bus did not come up");
            std::thread::sleep(Duration::from_millis(20));
        }
        dbus_send(
            &bus,
            &[
                "--dest=org.a11y.Bus",
                "/org/a11y/bus",
                "org.freedesktop.DBus.Properties.Set",
                "string:org.a11y.Status",
                "string:IsEnabled",
                "variant:boolean:true",
            ],
        )
        .expect("AT-SPI turns on");
        Some(Self {
            display,
            bus,
            _children: children,
            runtime,
        })
    }

    /// The scenario app `app`, built from this crate's example in its debug profile, on this
    /// desktop.
    fn app(&self, app: &str) -> Launch {
        Launch::cargo_example("scenario_app")
            .package("flui-driver")
            .env("FLUI_DRIVER_SCENARIO_APP", app)
            .env("DISPLAY", &self.display)
            .env("WAYLAND_DISPLAY", "")
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus)
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .connect_wait(PATIENCE)
    }

    fn lock(&self) -> PathBuf {
        self.runtime.path().join("os-input.lock")
    }

    /// The X11 lane for the app whose process is `pid`, on this desktop.
    fn lane(&self, pid: u32) -> X11Lane {
        X11Lane::try_new(pid, Some(self.display.clone()), Isolation::Xvfb)
            .expect("the lane runs on an X virtual framebuffer")
            .with_session_bus(self.bus.clone())
    }

    /// A session of the scenario app `app` with the X11 lane attached, once the app shows its
    /// window.
    fn session(&self, app: &str) -> Driver {
        let mut driver = Driver::launch(self.app(app)).expect("the scenario app starts");
        let deadline = Instant::now() + PATIENCE;
        while driver.windows().expect("the app answers").is_empty() {
            assert!(
                Instant::now() < deadline,
                "the app showed no window with content within {PATIENCE:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let pid = driver.app_pid().expect("a launched app has a process");
        driver
            .with_os_input_lock(self.lock())
            .with_lane(self.lane(pid))
            .expect("the lane starts")
    }

    /// One motion through XTEST from a client other than the lane's: what another program faking
    /// input sends.
    fn foreign_motion(&self) {
        foreign_motion_on(&self.display);
    }
}

/// One motion through XTEST on `display` from a client other than the lane's.
fn foreign_motion_on(display: &str) {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    use x11rb::protocol::xtest::ConnectionExt as _;
    let (connection, screen) = x11rb::rust_connection::RustConnection::connect(Some(display))
        .expect("the X server answers");
    let root = connection.setup().roots[screen].root;
    connection
        .xtest_fake_input(
            x11rb::protocol::xproto::MOTION_NOTIFY_EVENT,
            0,
            x11rb::CURRENT_TIME,
            root,
            7,
            7,
            0,
        )
        .expect("request");
    connection
        .get_input_focus()
        .expect("request")
        .reply()
        .expect("a round trip");
}

/// Waits for the app's window to show its controls.
fn shown(driver: &mut Driver) {
    let mut window = driver.main_window().expect("the app shows a window");
    expect(window.locate(Locator::identifier("like")))
        .timeout(Duration::from_secs(30))
        .to_have_count(1)
        .expect("the gallery shows its like button");
}

fn find<'a>(
    nodes: &'a [flui_driver::protocol::tree::Node],
    automation_id: &str,
) -> Option<&'a flui_driver::protocol::tree::Node> {
    nodes.iter().find_map(|node| {
        if node.automation_id.as_deref() == Some(automation_id) {
            Some(node)
        } else {
            find(&node.children, automation_id)
        }
    })
}

/// A certification run of `scenario` named `name`, taking `devices`, on `driver`.
fn certify(driver: &mut Driver, name: &str, devices: &[OsDevice]) {
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
}

fn failed(report: &Report) -> Vec<&flui_driver::report::Check> {
    report
        .checks()
        .iter()
        .filter(|check| check.outcome.verdict() != Verdict::Pass)
        .collect()
}

#[test]
fn at_spi_reads_and_invokes_the_app_as_a_screen_reader_does() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let mut driver = desktop.session("controls");
    shown(&mut driver);
    let mut window = driver.main_window().expect("a window");
    let tree = window.os_tree().expect("AT-SPI reads the window");
    let like = find(&tree.roots, "like").expect("AT-SPI shows like");
    assert_eq!(like.role, Role::Button, "{like:?}");
    assert_eq!(like.native_role, "button");
    assert_eq!(like.name.as_deref(), Some("Like"));
    assert!(like.rect.is_some(), "AT-SPI places it on the screen");
    let projection = window.compare_projection().expect("both trees read");
    assert!(
        projection.is_equal(),
        "AT-SPI shows what the app published: {:#?}\napp:\n{}\nos:\n{}",
        projection.differences(),
        projection.app,
        projection.os
    );
    window.os_invoke("like").expect("AT-SPI clicks like");
    expect(window.locate(Locator::identifier("likes")))
        .to_have_name("Likes: 1")
        .expect("the action reached the app");
    assert!(matches!(
        window.os_invoke("nothing-here"),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn every_role_reads_back_as_its_at_spi_role() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let mut driver = desktop.session("roles");
    let mut window = driver.main_window().expect("a window");
    expect(window.locate(Locator::named("button fixture")))
        .timeout(Duration::from_secs(30))
        .to_have_count(1)
        .expect("the roles page shows");
    let mut report = Report::new();
    report.run_scenario("roles", &mut driver, scenarios::roles);
    assert_eq!(report.verdict(), Verdict::Pass, "{:#?}", failed(&report));
}

#[test]
fn the_server_captures_the_apps_window_alone() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let mut driver = desktop.session("controls");
    shown(&mut driver);
    let mut window = driver.main_window().expect("a window");
    let placement = window.placement().expect("a placement");
    let capture = window.capture_os().expect("the server captures the window");
    let device = |logical: f64| (logical * placement.scale).round() as u32;
    assert_eq!(
        (capture.width, capture.height),
        (device(placement.size.width), device(placement.size.height)),
        "the capture is the window's surface, at its scale"
    );
    let first = capture.pixel(0, 0).expect("a first pixel");
    let differs = (0..capture.height)
        .step_by(4)
        .flat_map(|y| (0..capture.width).step_by(4).map(move |x| (x, y)))
        .any(|(x, y)| capture.pixel(x, y) != Some(first));
    assert!(differs, "the app drew its controls, not one flat colour");
}

/// The guard watches XInput 2's raw events: it starts and stops, and input another client fakes
/// through XTEST, which comes from the same devices as the lane's, is foreign, since the lane
/// sent none of it.
#[test]
fn the_guard_takes_input_another_client_fakes_for_the_persons() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let mut guard = X11Guard::new(Some(desktop.display.clone()));
    guard.arm().expect("the guard selects the raw events");
    assert_eq!(guard.foreign_input(), None, "nobody touched this server");
    desktop.foreign_motion();
    let deadline = Instant::now() + Duration::from_secs(5);
    let foreign = loop {
        if let Some(foreign) = guard.foreign_input() {
            break Some(foreign);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        foreign.is_some_and(|foreign| foreign.description != STOPPED_WATCHING),
        "the other client's motion is foreign input"
    );
    guard.disarm();
    guard.arm().expect("a guard arms again after it disarmed");
    assert_eq!(
        guard.foreign_input(),
        None,
        "a fresh arm forgets the last run"
    );
    guard.disarm();
}

/// The lane's own check, before a sequence's first event, refuses a device XTEST cannot fake and
/// a chord the desktop takes, and lets an app's chord through.
#[test]
fn the_lanes_check_refuses_what_xtest_cannot_send_or_the_desktop_takes() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let mut lane = desktop.lane(std::process::id());
    let touch = [OsEvent::PointerDown {
        pointer: "touch".into(),
        device: PointerDevice::Touch,
        button: PointerButton::Primary,
    }];
    let refused = lane.check(&touch);
    assert!(
        matches!(
            &refused,
            Err(Error::NotSupported { capability: Some(capability), .. })
                if capability == "os_input_device"
        ),
        "{refused:?}"
    );
    let switch_terminal = [
        OsEvent::KeyDown("Control".into()),
        OsEvent::KeyDown("Alt".into()),
        OsEvent::KeyDown("F2".into()),
    ];
    let refused = lane.check(&switch_terminal);
    assert!(
        matches!(
            &refused,
            Err(Error::NotSupported { capability: Some(capability), .. })
                if capability == "os_shell_hotkey"
        ),
        "{refused:?}"
    );
    let select_all = [
        OsEvent::KeyDown("Control".into()),
        OsEvent::KeyDown("a".into()),
        OsEvent::KeyUp("a".into()),
        OsEvent::KeyUp("Control".into()),
        OsEvent::Text("Grüße 日本".into()),
    ];
    lane.check(&select_all)
        .expect("an app's chord and any text pass");
}

/// The conformance row a platform's run reports, through the X11 lane: the scenario the headless
/// target runs in CI (`scenarios::the_conformance_scenario_passes_on_the_headless_target`).
#[test]
fn the_conformance_scenario_runs_through_the_os_lane() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let mut driver = desktop.session("controls");
    shown(&mut driver);
    certify(&mut driver, "conformance", &[OsDevice::Mouse]);
    let mut report = Report::new();
    report.run_scenario("conformance", &mut driver, scenarios::conformance);
    assert_eq!(report.lane(), Some("x11"));
    assert!(
        !report.announcements().is_empty(),
        "an OS run is announced before its first event"
    );
    assert_eq!(report.verdict(), Verdict::Pass, "{:#?}", failed(&report));
}

/// The certification runs of the device checks, through the X11 lane: the gallery, the
/// accessibility checks and the notes pass; the input checks cannot be verified for touch and pen,
/// which XTEST does not fake, and pass for the mouse, the wheel and keys.
#[test]
fn the_device_check_scenarios_run_through_the_os_lane() {
    type ScenarioFn = fn(&mut Driver, &mut Report) -> flui_driver::Result<()>;
    let rows: [(&str, &str, ScenarioFn, &[OsDevice], Verdict); 4] = [
        (
            "x11-gallery",
            "controls",
            scenarios::gallery,
            &[OsDevice::Mouse, OsDevice::Keyboard],
            Verdict::Pass,
        ),
        (
            "x11-a11y",
            "controls",
            scenarios::accessibility,
            &[],
            Verdict::Pass,
        ),
        (
            "x11-input",
            "controls",
            scenarios::input,
            &[
                OsDevice::Mouse,
                OsDevice::Touch,
                OsDevice::Pen,
                OsDevice::Keyboard,
            ],
            Verdict::CannotVerify,
        ),
        (
            "x11-notes",
            "notes",
            scenarios::notes,
            &[OsDevice::Keyboard, OsDevice::Mouse],
            Verdict::Pass,
        ),
    ];
    let Some(desktop) = Desktop::start() else {
        return;
    };
    for (name, app, scenario, devices, expected) in rows {
        let mut driver = desktop.session(app);
        certify(&mut driver, name, devices);
        let mut report = Report::new();
        report.run_scenario(name, &mut driver, scenario);
        assert_eq!(report.verdict(), expected, "{name}: {:#?}", failed(&report));
        if expected == Verdict::CannotVerify {
            let refused: Vec<&flui_driver::report::Check> = failed(&report);
            assert!(
                refused
                    .iter()
                    .all(|check| check.name.contains("touch") || check.name.contains("pen")),
                "{name}: only the touch and pen checks cannot be verified: {refused:#?}"
            );
            assert!(
                report
                    .cannot_verify()
                    .contains(&"os_input_device".to_owned()),
                "{:?}",
                report.cannot_verify()
            );
        }
    }
}

/// Another client moving the pointer during an OS run ends it `cannot_verify`, with the button
/// the lane held released: the app's journal ends the press.
#[test]
fn a_foreign_motion_mid_run_ends_it_and_releases_its_button() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let mut driver = desktop
        .session("controls")
        .with_input_policy(InputPolicy::require(InputPath::Os));
    shown(&mut driver);
    let pad = scenarios::point_of(&mut driver, "pad").expect("the pad shows");
    let held = ActionSequence::new(vec![InputSource::Pointer {
        id: "mouse".to_owned(),
        device: PointerDevice::Mouse,
        actions: vec![
            PointerAction::Move {
                to: pad,
                duration_ms: 0,
                pressure: None,
            },
            PointerAction::Down {
                button: PointerButton::Primary,
                pressure: None,
            },
            PointerAction::Pause { duration_ms: 2_000 },
            PointerAction::Up {
                button: PointerButton::Primary,
            },
        ],
    }]);
    // The rest of the desktop lives to the end of the test.
    let display = desktop.display;
    let intruder = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        foreign_motion_on(&display);
    });
    let mut window = driver.main_window().expect("a window");
    let outcome = window.perform(held);
    intruder.join().expect("the intruder moved the pointer");
    assert!(
        matches!(outcome, Err(Error::InterruptedByUser { .. })),
        "the run ended on the foreign motion: {outcome:?}"
    );
    let journal = window.journal().expect("the journal");
    let mouse: Vec<InputKind> = journal
        .entries
        .iter()
        .filter(|entry| entry.device == DeviceClass::Mouse)
        .map(|entry| entry.kind)
        .filter(|kind| {
            matches!(
                kind,
                InputKind::PointerDown | InputKind::PointerUp | InputKind::PointerCancel
            )
        })
        .collect();
    assert_eq!(mouse.first(), Some(&InputKind::PointerDown), "{mouse:?}");
    assert!(
        matches!(
            mouse.last(),
            Some(InputKind::PointerUp | InputKind::PointerCancel)
        ),
        "the button was released: {mouse:?}"
    );
}

/// A launched app ends with its session, and lives as long as the session even when the thread
/// that launched it ended first: the parent-death signal that ends it with the driver is tied to
/// a thread the session keeps, not to the caller's.
#[test]
fn a_launched_app_lives_as_long_as_its_session_whatever_thread_launched_it() {
    let Some(desktop) = Desktop::start() else {
        return;
    };
    let launch = desktop.app("controls");
    let driver = std::thread::spawn(move || Driver::launch(launch).expect("the app starts"))
        .join()
        .expect("the launching thread ended");
    let pid = driver.app_pid().expect("a process");
    std::thread::sleep(Duration::from_millis(500));
    let alive = |pid: u32| Path::new(&format!("/proc/{pid}")).exists() && !zombie(pid);
    assert!(
        alive(pid),
        "process {pid} died with the thread that launched it"
    );
    drop(driver);
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(
            Instant::now() < deadline,
            "process {pid} outlived its session"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Whether `pid` is a zombie, ended and not yet reaped.
fn zombie(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next().map(|state| state == "Z"))
        })
        .unwrap_or(true)
}
