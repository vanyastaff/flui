//! The iOS Simulator lane on every host: its parsers against what `simctl` and `xcodebuild`
//! print, its choice of simulator, and the lane itself against a scripted Simulator
//! (`ScriptedXcrun`): every input refused before anything is sent, naming the Xcode and runtime,
//! the screen read as the window, and the app started with its launch values and ended with the
//! lane.

use std::time::Duration;

use flui_driver::Error;
use flui_driver::coordinates::{self, DevicePoint};
use flui_driver::lane::ios_simulator::device::{self, DeviceState, Simulator, Udid};
use flui_driver::lane::ios_simulator::screenshot;
use flui_driver::lane::ios_simulator::xcode::{self, HID_CAPABILITY};
use flui_driver::lane::ios_simulator::xcrun::{XcrunCommand, XcrunReply};
use flui_driver::lane::ios_simulator::{IosSimulatorLane, LANE, SimulatorApp, SimulatorLock};
use flui_driver::lane::{self, Lane as _, PositionFidelity};
use flui_driver::protocol::geometry::{LogicalPoint, LogicalSize};
use flui_driver::protocol::input::{PointerButton, PointerDevice};
use flui_driver::protocol::session::{Isolation, Placement};
use flui_driver::protocol::tree::WindowId;
use flui_driver::steps::OsEvent;
use flui_driver::testing::ScriptedXcrun;

use crate::android_lane::png_of;

pub(crate) const UDID: &str = "1F2E3D4C-5B6A-4798-8A7B-6C5D4E3F2A1B";
pub(crate) const BUNDLE: &str = "dev.flui.scenario";

/// What `simctl list devices --json` prints: the named simulator booted on iOS 18.0 beside
/// others, one entry missing its state, one with no UDID and a tvOS one.
pub(crate) fn listing(state: &str, available: bool) -> String {
    format!(
        r#"{{"devices": {{
  "com.apple.CoreSimulator.SimRuntime.iOS-18-0": [
    {{"udid": "{UDID}", "isAvailable": {available}, "state": "{state}", "name": "iPhone 16",
      "deviceTypeIdentifier": "com.apple.CoreSimulator.SimDeviceType.iPhone-16"}},
    {{"udid": "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", "isAvailable": true, "name": "iPad Air"}},
    {{"udid": "not-a-udid", "isAvailable": true, "state": "Booted", "name": "Broken"}}
  ],
  "com.apple.CoreSimulator.SimRuntime.tvOS-18-0": [
    {{"udid": "0A1B2C3D-4E5F-4061-8728-394A5B6C7D8E", "isAvailable": true, "state": "Booted",
      "name": "Apple TV"}}
  ]
}}}}"#
    )
}

/// `xcodebuild -version` as Xcode `version` (`build`) prints it.
fn xcodebuild(version: &str, build: &str) -> String {
    format!("Xcode {version}\nBuild version {build}\n")
}

/// A Simulator with the named simulator booted, under Xcode 27.1.
pub(crate) fn scripted() -> ScriptedXcrun {
    ScriptedXcrun::new()
        .reply(
            "simctl list devices",
            XcrunReply::new(0, listing("Booted", true)),
        )
        .reply(
            "xcodebuild -version",
            XcrunReply::new(0, xcodebuild("27.1", "27A9269")),
        )
}

fn udid() -> Udid {
    Udid::try_new(UDID).expect("a UDID")
}

/// A hold on the test's simulator, its lock file in a directory of its own, which goes after the
/// hold: the directory first in the pair, so a binding of both drops the hold first.
pub(crate) fn held() -> (tempfile::TempDir, SimulatorLock) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let lock = SimulatorLock::acquire_in(dir.path(), &udid()).expect("the simulator is free");
    (dir, lock)
}

/// The lane on the test's simulator, and the directory its hold lives in.
pub(crate) fn connect(xcrun: &ScriptedXcrun) -> (tempfile::TempDir, IosSimulatorLane) {
    let (dir, lock) = held();
    let lane =
        IosSimulatorLane::connect(xcrun.simulator(), lock, BUNDLE).expect("the lane connects");
    (dir, lane)
}

/// The lane on the test's simulator through `xcrun`, or why it did not connect.
fn try_connect(xcrun: &ScriptedXcrun, bundle: &str) -> Result<IosSimulatorLane, Error> {
    let (_dir, lock) = held();
    IosSimulatorLane::connect(xcrun.simulator(), lock, bundle)
}

fn window() -> WindowId {
    "w1".parse().expect("a window handle")
}

/// An iPhone's window: 393 by 852 points at 3x.
fn placement() -> Placement {
    Placement::new(window(), LogicalSize::new(393.0, 852.0), 3.0)
}

fn tap() -> Vec<OsEvent> {
    let pointer = || "finger".to_owned();
    vec![
        OsEvent::PointerMove {
            pointer: pointer(),
            device: PointerDevice::Touch,
            to: DevicePoint::new(600, 1200),
        },
        OsEvent::PointerDown {
            pointer: pointer(),
            device: PointerDevice::Touch,
            button: PointerButton::Primary,
        },
        OsEvent::PointerUp {
            pointer: pointer(),
            device: PointerDevice::Touch,
            button: PointerButton::Primary,
        },
    ]
}

#[test]
fn a_udid_is_a_uuid_and_is_held_in_upper_case() {
    let lower = Udid::try_new(UDID.to_ascii_lowercase()).expect("a lower-case UDID");
    assert_eq!(lower, udid(), "two spellings of one UDID are one simulator");
    assert_eq!(lower.as_str(), UDID);
    assert_eq!(lower.to_string(), UDID);
    for refused in [
        "",
        "booted",
        "-1F2E3D4C-5B6A-4798-8A7B-6C5D4E3F2A1B",
        "1F2E3D4C5B6A47988A7B6C5D4E3F2A1B",
        "1F2E3D4C-5B6A-4798-8A7B-6C5D4E3F2A1",
        "1F2E3D4C-5B6A-4798-8A7B-6C5D4E3F2A1G",
        "1F2E3D4C-5B6A-4798-8A7B-6C5D4E3F2A1B-0",
    ] {
        assert!(
            matches!(Udid::try_new(refused), Err(Error::InvalidArgument { .. })),
            "{refused:?}"
        );
    }
}

#[test]
fn a_bundle_identifier_is_checked_before_it_becomes_an_argument() {
    let longest = "a".repeat(255);
    for accepted in [
        "dev.flui.scenario",
        "com.example.My-App2",
        "a",
        longest.as_str(),
    ] {
        device::check_bundle_id(accepted).expect(accepted);
    }
    let long = "a".repeat(256);
    for refused in [
        "",
        "-dev.flui",
        ".dev.flui",
        "dev flui",
        "dev.flui;rm",
        "dev/flui",
        "dév.flui",
        long.as_str(),
    ] {
        assert!(
            matches!(
                device::check_bundle_id(refused),
                Err(Error::InvalidArgument { .. })
            ),
            "{refused:?}"
        );
        assert!(SimulatorApp::try_new(refused).is_err(), "{refused:?}");
    }
}

#[test]
fn an_apps_environment_takes_variable_names_alone() {
    let app = || SimulatorApp::try_new(BUNDLE).expect("a bundle");
    for accepted in ["FLUI_DRIVER_SCENARIO_APP", "_A", "a1"] {
        app().env(accepted, "x").expect(accepted);
    }
    for refused in ["", "1ST", "A-B", "A B", "A=B", "É"] {
        assert!(
            matches!(app().env(refused, "x"), Err(Error::InvalidArgument { .. })),
            "{refused:?}"
        );
    }
}

#[test]
fn the_listing_is_read_entry_by_entry() {
    let simulators = device::parse_devices(listing("Booted", true).as_bytes()).expect("a listing");
    let names: Vec<&str> = simulators
        .iter()
        .map(|simulator| simulator.name.as_str())
        .collect();
    // The iPad lacks its state and the third entry its UDID: each is left out alone.
    assert_eq!(names, ["iPhone 16", "Apple TV"]);
    assert_eq!(simulators[0].udid, udid());
    assert_eq!(simulators[0].state, DeviceState::Booted);
    assert!(simulators[0].available);
    assert_eq!(simulators[0].runtime_name(), "iOS 18.0");
    let other = device::parse_devices(listing("Booting", true).as_bytes()).expect("a listing");
    assert_eq!(other[0].state, DeviceState::Other("Booting".to_owned()));
    let shut = device::parse_devices(listing("Shutdown", true).as_bytes()).expect("a listing");
    assert_eq!(shut[0].state, DeviceState::Shutdown);
    assert_eq!(shut[0].state.to_string(), "Shutdown");
    for refused in ["", "[]", "{}", "{\"devices\": []}", "not json"] {
        assert!(
            device::parse_devices(refused.as_bytes()).is_err(),
            "{refused:?}"
        );
    }
}

#[test]
fn the_lane_drives_only_a_listed_available_booted_ios_simulator() {
    let read = |state: &str, available: bool| {
        device::parse_devices(listing(state, available).as_bytes()).expect("a listing")
    };
    let booted = read("Booted", true);
    assert_eq!(
        device::choose(&booted, &udid())
            .expect("the booted iPhone")
            .name,
        "iPhone 16"
    );
    let unavailable = |simulators: &[Simulator], udid: &Udid, says: &str| {
        let error = device::choose(simulators, udid).expect_err("refused");
        assert!(
            matches!(&error, Error::Lane { unavailable: true, message, .. } if message.contains(says)),
            "{error:?}"
        );
        assert!(error.is_cannot_verify(), "{error:?}");
    };
    let unknown = Udid::try_new("FFFFFFFF-FFFF-4FFF-8FFF-FFFFFFFFFFFF").expect("a UDID");
    unavailable(&booted, &unknown, "is listed");
    unavailable(&read("Shutdown", true), &udid(), "xcrun simctl boot");
    unavailable(&read("Booting", true), &udid(), "Booting, not booted");
    unavailable(&read("Booted", false), &udid(), "not installed");
    let television = Udid::try_new("0A1B2C3D-4E5F-4061-8728-394A5B6C7D8E").expect("a UDID");
    let error = device::choose(&booted, &television).expect_err("no iOS runtime");
    assert_eq!(
        error.missing_capability().as_deref(),
        Some("ios_simulator_runtime")
    );
}

#[test]
fn an_ios_runtime_reads_as_its_version() {
    for (identifier, name) in [
        (
            "com.apple.CoreSimulator.SimRuntime.iOS-18-0",
            Some("iOS 18.0"),
        ),
        (
            "com.apple.CoreSimulator.SimRuntime.iOS-26-4-1",
            Some("iOS 26.4.1"),
        ),
        ("com.apple.CoreSimulator.SimRuntime.iOS-27", Some("iOS 27")),
        ("com.apple.CoreSimulator.SimRuntime.tvOS-18-0", None),
        ("com.apple.CoreSimulator.SimRuntime.iOS-", None),
        ("com.apple.CoreSimulator.SimRuntime.iOS-18-x", None),
        ("com.apple.CoreSimulator.SimRuntime.iOS-18a-0", None),
        ("com.apple.CoreSimulator.SimRuntime.iOS-1-2-3-4", None),
    ] {
        assert_eq!(
            device::runtime_name(identifier).as_deref(),
            name,
            "{identifier}"
        );
    }
}

#[test]
fn the_launch_line_gives_the_apps_process() {
    for (printed, pid) in [
        ("dev.flui.scenario: 4321\n", Some(4321)),
        ("note\ndev.flui.scenario:  77 \n", Some(77)),
        ("dev.flui.scenario.other: 4321\n", None),
        ("dev.flui.scenario: 1\n", Some(1)),
        ("dev.flui.scenario: 0\n", None),
        ("dev.flui.scenario: -1\n", None),
        ("dev.flui.scenario 4321\n", None),
        ("", None),
    ] {
        assert_eq!(device::parse_launched(printed, BUNDLE), pid, "{printed:?}");
    }
}

#[test]
fn the_services_listing_gives_the_apps_running_processes() {
    let printed = "PID\tStatus\tLabel\n\
                   -\t0\tUIKitApplication:dev.flui.scenario[0a0b][rb-legacy]\n\
                   812\t0\tUIKitApplication:dev.flui.scenario[a1b2][rb-legacy]\n\
                   813\t0\tUIKitApplication:dev.flui.scenario.other[c3d4][rb-legacy]\n\
                   814\t0\tcom.apple.dev.flui.scenario\n\
                   0\t0\tUIKitApplication:dev.flui.scenario[e5f6]\n\
                   815 0 UIKitApplication:dev.flui.scenario[0001]\n\
                   816\n";
    assert_eq!(device::parse_running(printed, BUNDLE), [812, 815]);
    assert_eq!(device::parse_running("", BUNDLE), Vec::<u32>::new());
}

/// One session holds a simulator at a time: the second is refused as busy until the first lets
/// go, and another simulator is free meanwhile.
#[test]
fn a_second_session_on_a_simulator_is_refused_while_the_first_holds_it() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let first = SimulatorLock::acquire_in(dir.path(), &udid()).expect("a free simulator");
    let error = SimulatorLock::acquire_in(dir.path(), &udid()).expect_err("held");
    assert!(
        matches!(&error, Error::Busy { message, .. } if message.contains(UDID)),
        "{error:?}"
    );
    let other = Udid::try_new("0A1B2C3D-4E5F-4061-8728-394A5B6C7D8E").expect("a UDID");
    let _other = SimulatorLock::acquire_in(dir.path(), &other).expect("another simulator");
    assert_eq!(first.udid(), &udid());
    drop(first);
    let _again = SimulatorLock::acquire_in(dir.path(), &udid()).expect("free once let go");
}

/// A path goes to `xcrun` as it is, and a relative one that starts with `-` as `./` and it; a
/// command's debug form, like its text, names a launch value's key and never its value.
#[test]
fn a_commands_arguments_keep_its_paths_and_its_forms_keep_its_values_out() {
    use std::ffi::OsString;
    use std::path::PathBuf;

    let install = |app: &str| XcrunCommand::Install {
        udid: udid(),
        app: PathBuf::from(app),
    };
    let last = |command: XcrunCommand| command.args().last().cloned().expect("a path");
    assert_eq!(
        last(install("-Scenario.app")),
        OsString::from(format!(".{}-Scenario.app", std::path::MAIN_SEPARATOR))
    );
    assert_eq!(
        last(install("Scenario.app")),
        OsString::from("Scenario.app")
    );
    assert_eq!(
        last(install("/tmp/-Scenario.app")),
        OsString::from("/tmp/-Scenario.app")
    );
    let launch = XcrunCommand::Launch {
        udid: udid(),
        bundle: BUNDLE.to_owned(),
        env: vec![("FLUI_AGENT_TOKEN".to_owned(), "secret-token".to_owned())],
    };
    for shown in [launch.to_string(), format!("{launch:?}")] {
        assert!(
            shown.contains("FLUI_AGENT_TOKEN") && !shown.contains("secret-token"),
            "{shown}"
        );
    }
}

#[test]
fn the_xcode_version_is_read_from_xcodebuild() {
    let read = xcode::parse_version(&xcodebuild("27.1", "27A9269")).expect("a version");
    assert_eq!(read.version, "27.1");
    assert_eq!(read.build.as_deref(), Some("27A9269"));
    assert_eq!(read.to_string(), "Xcode 27.1 (27A9269)");
    let longest = format!("Xcode 27.1\nBuild version {}\n", "A".repeat(32));
    let long = format!("Xcode 27.1\nBuild version {}\n", "A".repeat(33));
    let longest_build = "A".repeat(32);
    let cases = [
        (
            "Xcode 26.4.1\nBuild version 26E101\n",
            Some(("26.4.1", Some("26E101"))),
        ),
        ("Xcode 27\n", Some(("27", None))),
        ("Xcode 27.1\nBuild version 27A 9269\n", Some(("27.1", None))),
        ("Xcode 27.1\nBuild version \n", Some(("27.1", None))),
        (
            longest.as_str(),
            Some(("27.1", Some(longest_build.as_str()))),
        ),
        (long.as_str(), Some(("27.1", None))),
        ("Xcode 27.1.0.1\n", None),
        ("Xcode 27.x\n", None),
        ("Xcode 2x\n", None),
        ("Xcode beta\n", None),
        (
            "xcode-select: error: tool 'xcodebuild' requires Xcode\n",
            None,
        ),
        ("", None),
    ];
    for (printed, expected) in cases {
        let read = xcode::parse_version(printed);
        assert_eq!(
            read.as_ref()
                .map(|read| (read.version.as_str(), read.build.as_deref())),
            expected,
            "{printed:?}"
        );
    }
}

/// Every input, under each Xcode the lane may meet, is refused before anything is sent, naming the
/// capability, the Xcode and the runtime; a sequence of pauses alone goes through. Xcode 27.1 is
/// one where the Simulator drops Indigo buttons and keys; Xcode 26.4 is one before that, where
/// `idb` records no loss and still the lane refuses, since the rule (the spec's decision D5) is no
/// OS input until a supported channel exists, not a split by Xcode version; the third gives no
/// version at all.
#[test]
fn every_input_is_refused_naming_the_xcode_and_the_runtime_before_anything_is_sent() {
    let versions = [
        (
            XcrunReply::new(0, xcodebuild("26.4", "26E5221a")),
            "Xcode 26.4 (26E5221a)",
        ),
        (
            XcrunReply::new(0, xcodebuild("27.1", "27A9269")),
            "Xcode 27.1 (27A9269)",
        ),
        (
            XcrunReply::new(1, "").stderr("xcode-select: error: no developer directory"),
            "whose version",
        ),
    ];
    let inputs = [
        tap(),
        vec![
            OsEvent::KeyDown("Enter".to_owned()),
            OsEvent::KeyUp("Enter".to_owned()),
        ],
        vec![OsEvent::Text("hello".to_owned())],
        vec![OsEvent::Scroll {
            at: DevicePoint::new(10, 10),
            delta_x: 0,
            delta_y: 30,
        }],
        vec![
            OsEvent::Pause(Duration::from_millis(1)),
            OsEvent::Text("a".to_owned()),
        ],
    ];
    for (reply, names) in versions {
        let xcrun = ScriptedXcrun::new()
            .reply(
                "simctl list devices",
                XcrunReply::new(0, listing("Booted", true)),
            )
            .reply("xcodebuild -version", reply);
        let (_dir, mut lane) = connect(&xcrun);
        let before = xcrun.log();
        for events in &inputs {
            let error = lane::send_guarded(&mut lane, events).expect_err("input is refused");
            assert_eq!(
                error.missing_capability().as_deref(),
                Some(HID_CAPABILITY),
                "{error:?}"
            );
            assert!(error.is_cannot_verify(), "{error:?}");
            let message = error.to_string();
            assert!(message.contains(names), "{names}: {message}");
            assert!(message.contains("iOS 18.0"), "{message}");
            let checked = lane.check(events).expect_err("refused before any event");
            assert_eq!(
                checked.missing_capability().as_deref(),
                Some(HID_CAPABILITY)
            );
            let direct = lane
                .send(&events[events.len() - 1])
                .expect_err("refused when sent");
            assert_eq!(direct.missing_capability().as_deref(), Some(HID_CAPABILITY));
        }
        assert_eq!(xcrun.log(), before, "nothing went to the Simulator");
        let started = std::time::Instant::now();
        lane::send_guarded(&mut lane, &[OsEvent::Pause(Duration::from_millis(60))])
            .expect("a pause alone is waited out");
        assert!(
            started.elapsed() >= Duration::from_millis(60),
            "the pause was waited out whole"
        );
        lane.release_all().expect("the lane holds nothing");
    }
}

#[test]
fn the_lane_is_the_simulator_and_has_no_guard() {
    let (_dir, mut lane) = connect(&scripted());
    assert_eq!(lane.name(), LANE);
    let capabilities = lane.capabilities();
    assert_eq!(capabilities.isolation, Isolation::Simulator);
    assert!(capabilities.pointers.is_empty(), "{capabilities:?}");
    assert!(!capabilities.keys);
    assert!(!capabilities.compositions);
    assert_eq!(capabilities.accessibility, None);
    assert_eq!(capabilities.fidelity, PositionFidelity::OrderOnly);
    assert!(lane.guard().is_none());
    lane::admit(&mut lane).expect("a simulator is isolated without a guard");
    assert_eq!(lane.simulator().udid, udid());
    assert_eq!(
        lane.xcode().map(ToString::to_string).as_deref(),
        Some("Xcode 27.1 (27A9269)")
    );
    let error = lane.read_tree(&placement()).expect_err("no OS tree");
    assert_eq!(
        error.missing_capability().as_deref(),
        Some("os_accessibility_tree")
    );
}

#[test]
fn the_lane_refuses_a_simulator_it_cannot_drive() {
    let shut = ScriptedXcrun::new().reply(
        "simctl list devices",
        XcrunReply::new(0, listing("Shutdown", true)),
    );
    let error = try_connect(&shut, BUNDLE).expect_err("a shut-down simulator");
    assert!(error.is_cannot_verify(), "{error:?}");
    assert_eq!(shut.log(), ["simctl list devices --json"]);
    let no_xcode = ScriptedXcrun::new().reply(
        "simctl list devices",
        XcrunReply::new(72, "").stderr("xcrun: error: unable to find utility \"simctl\""),
    );
    let error = try_connect(&no_xcode, BUNDLE).expect_err("no simctl");
    assert!(
        matches!(
            &error,
            Error::Lane {
                unavailable: true,
                ..
            }
        ),
        "{error:?}"
    );
    let error = try_connect(&scripted(), "-bad").expect_err("no bundle identifier");
    assert!(matches!(error, Error::InvalidArgument { .. }), "{error:?}");
}

/// What `simctl` says when the host cannot run the lane (no Xcode, no such simulator, a
/// simulator that shut down, a CoreSimulator service gone) makes the run cannot-verify; any other
/// failure fails it, with what `simctl` said cut to a message's length.
#[test]
fn a_failure_the_host_causes_is_cannot_verify_and_any_other_fails_the_run() {
    let connect_with = |reply: XcrunReply| {
        let xcrun = ScriptedXcrun::new().reply("simctl list devices", reply);
        try_connect(&xcrun, BUNDLE).expect_err("refused")
    };
    for said in [
        "xcrun: error: invalid active developer path",
        "Invalid device: 1F2E3D4C-5B6A-4798-8A7B-6C5D4E3F2A1B",
        "Unable to lookup in current state: Shutdown",
        "No devices are booted.",
        "Failed to initialize simulator device set: CoreSimulatorService connection interrupted",
    ] {
        let error = connect_with(XcrunReply::new(1, "").stderr(said));
        assert!(
            matches!(&error, Error::Lane { unavailable: true, message, .. } if message.contains(said)),
            "{said}: {error:?}"
        );
    }
    let long = "x".repeat(400);
    let error = connect_with(XcrunReply::new(2, "").stderr(long.as_str()));
    let Error::Lane {
        unavailable,
        message,
        ..
    } = &error
    else {
        panic!("a lane failure: {error:?}");
    };
    assert!(!unavailable, "{error:?}");
    assert!(!error.is_cannot_verify(), "{error:?}");
    assert!(
        message.ends_with(&format!("it exited with 2: {}…", "x".repeat(300))),
        "{message}"
    );
    let failure = |reply: XcrunReply| reply.failure();
    assert_eq!(failure(XcrunReply::new(0, "out")), None);
    assert_eq!(
        failure(XcrunReply::new(3, "")).as_deref(),
        Some("it exited with 3")
    );
    assert_eq!(
        failure(XcrunReply::new(3, "").stderr("  nope \n")).as_deref(),
        Some("it exited with 3: nope")
    );
    let mut ended = XcrunReply::new(0, "");
    ended.exit = None;
    assert_eq!(
        failure(ended).as_deref(),
        Some("it ended without an exit code")
    );
}

/// The listing is read within its bound and the version within its own, each to the byte: a
/// listing past it refuses the lane, and a version past it leaves the Xcode unnamed.
#[test]
fn what_simctl_and_xcodebuild_print_is_read_within_its_bound() {
    // JSON and `xcodebuild`'s lines both take trailing spaces.
    let padded = |mut text: String, length: usize| {
        let pad = length - text.len();
        text.extend(std::iter::repeat_n(' ', pad));
        text
    };
    let at = |listing_length: usize, version_length: usize| {
        ScriptedXcrun::new()
            .reply(
                "simctl list devices",
                XcrunReply::new(0, padded(listing("Booted", true), listing_length)),
            )
            .reply(
                "xcodebuild -version",
                XcrunReply::new(0, padded(xcodebuild("27.1", "27A9269"), version_length)),
            )
    };
    let small = 64 * 1024;
    let (_dir, lane) = connect(&at(device::MAX_LISTING, small));
    assert_eq!(
        lane.xcode().map(|xcode| xcode.version.as_str()),
        Some("27.1"),
        "a listing and a version at their bounds"
    );
    drop(lane);
    let (_dir, lane) = connect(&at(device::MAX_LISTING, small + 1));
    assert_eq!(
        lane.xcode(),
        None,
        "a version past its bound leaves the Xcode unnamed"
    );
    drop(lane);
    let error = try_connect(&at(device::MAX_LISTING + 1, small), BUNDLE)
        .expect_err("a listing past its bound");
    assert!(matches!(error, Error::Lane { .. }), "{error:?}");
}

#[test]
fn a_capture_is_the_screen_when_the_window_fills_it() {
    let (width, height) = (1179, 2556);
    let mut rgba = vec![255_u8; (width * height * 4) as usize];
    rgba[..4].copy_from_slice(&[10, 20, 30, 255]);
    // The screen turned, then upright: each capture reads the screen afresh.
    let turned = png_of(height, width, &rgba);
    let xcrun = scripted()
        .screenshot(turned)
        .screenshot(png_of(width, height, &rgba));
    let (_dir, mut lane) = connect(&xcrun);
    let error = lane.capture_window(&placement()).expect_err("turned");
    assert_eq!(
        error.missing_capability().as_deref(),
        Some("window_position")
    );
    let image = lane.capture_window(&placement()).expect("the screen");
    assert_eq!((image.width(), image.height()), (width, height));
    assert_eq!(image.pixel(0, 0), Some([10, 20, 30, 255]));
    let geometry = lane
        .locate_window(&placement())
        .expect("the window is the screen");
    let corner = |x: f64, y: f64| {
        coordinates::to_device(&geometry, LogicalPoint::new(x, y)).expect("on the screen")
    };
    assert_eq!(corner(0.0, 0.0), DevicePoint::new(0, 0));
    assert_eq!(corner(392.9, 851.9), DevicePoint::new(1178, 2555));
    assert!(
        xcrun
            .commands()
            .iter()
            .any(|command| matches!(command, XcrunCommand::Screenshot { .. })),
        "{:?}",
        xcrun.log()
    );
}

#[test]
fn a_screen_of_another_size_than_the_window_is_refused_before_it_is_decoded() {
    // A landscape screen under a portrait window, and a window that does not fill its screen.
    for (width, height) in [(2556, 1179), (1179, 1200), (1182, 2556)] {
        let xcrun = scripted().screenshot(png_of(
            width,
            height,
            &vec![0; (width * height * 4) as usize],
        ));
        let (_dir, mut lane) = connect(&xcrun);
        let error = lane
            .capture_window(&placement())
            .expect_err("not the window");
        assert_eq!(
            error.missing_capability().as_deref(),
            Some("window_position"),
            "{width} x {height}: {error:?}"
        );
        let error = lane
            .locate_window(&placement())
            .expect_err("not the window");
        assert_eq!(
            error.missing_capability().as_deref(),
            Some("window_position")
        );
    }
    // Two pixels off on a side is the window laid out in whole pixels; three is another screen.
    for (width, height) in [(1181, 2554), (1177, 2558)] {
        screenshot::check_fills(&placement(), width, height).expect("within the tolerance");
    }
    for (width, height) in [(1176, 2556), (1179, 2559)] {
        assert!(
            screenshot::check_fills(&placement(), width, height).is_err(),
            "{width} x {height}"
        );
    }
    // A header that declares a screen past the bound, or no PNG at all, is never decoded.
    let mut huge = png_of(1, 1, &[0, 0, 0, 255]);
    huge[16..20].copy_from_slice(&9000_u32.to_be_bytes());
    let declared = |width: u32, height: u32| {
        let mut png = png_of(1, 1, &[0, 0, 0, 255]);
        png[16..20].copy_from_slice(&width.to_be_bytes());
        png[20..24].copy_from_slice(&height.to_be_bytes());
        png
    };
    for refused in [
        huge,
        declared(1, 8193),
        declared(8193, 1),
        declared(0, 1),
        declared(1, 0),
        b"GIF89a".to_vec(),
        Vec::new(),
    ] {
        let error = screenshot::window_image(&refused, &placement()).expect_err("refused");
        assert!(matches!(error, Error::Lane { .. }), "{error:?}");
    }
    let error = screenshot::window_image(b"GIF89a", &placement()).expect_err("no PNG");
    assert!(error.to_string().contains("no PNG"), "{error}");
    // A screen of the bound itself is read.
    let column = Placement::new(window(), LogicalSize::new(1.0, 8192.0), 1.0);
    let image = screenshot::window_image(&png_of(1, 8192, &vec![0; 8192 * 4]), &column)
        .expect("a side of the bound");
    assert_eq!((image.width(), image.height()), (1, 8192));
    let row = Placement::new(window(), LogicalSize::new(8192.0, 1.0), 1.0);
    let image = screenshot::window_image(&png_of(8192, 1, &vec![0; 8192 * 4]), &row)
        .expect("a side of the bound, the other of one pixel");
    assert_eq!((image.width(), image.height()), (8192, 1));
    let bad_scale =
        |width: f64, scale: f64| Placement::new(window(), LogicalSize::new(width, 852.0), scale);
    for placement in [
        bad_scale(f64::NAN, 3.0),
        bad_scale(f64::INFINITY, 3.0),
        bad_scale(393.0, 0.0),
        bad_scale(393.0, -3.0),
    ] {
        assert!(
            matches!(
                screenshot::check_fills(&placement, 1179, 2556),
                Err(Error::InvalidArgument { .. })
            ),
            "{placement:?}"
        );
    }
}

/// A PNG's size is read from its header's first chunk, which must be `IHDR` of 13 bytes, each
/// side a big-endian word.
#[test]
fn a_pngs_size_is_read_from_its_header() {
    let header = |length: u32, chunk: &[u8; 4], width: u32, height: u32| {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    };
    assert_eq!(
        screenshot::png_size(&header(13, b"IHDR", 0x0102_0304, 0x0506_0708)),
        Some((0x0102_0304, 0x0506_0708))
    );
    assert_eq!(screenshot::png_size(&png_of(3, 2, &[0; 24])), Some((3, 2)));
    let mut not_png = header(13, b"IHDR", 3, 2);
    not_png[1] = b'Q';
    for refused in [
        header(12, b"IHDR", 3, 2),
        header(13, b"IDAT", 3, 2),
        not_png,
        header(13, b"IHDR", 3, 2)[..23].to_vec(),
    ] {
        assert_eq!(screenshot::png_size(&refused), None, "{refused:?}");
    }
}

#[cfg(unix)]
mod start {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;

    use flui_driver::Endpoint;
    use flui_driver::lane::ios_simulator::SimulatorApp;
    use flui_driver::lane::ios_simulator::xcrun::{XcrunCommand, XcrunReply};
    use flui_driver::protocol::launch::{ENDPOINT_ENV, TOKEN_ENV};

    use super::{BUNDLE, connect, scripted, udid};

    /// What `launchctl list` prints inside a simulator where the scenario app runs as `pid`.
    fn running(pid: u32) -> String {
        format!(
            "PID\tStatus\tLabel\n-\t0\tcom.apple.springboard.idle\n\
             {pid}\t0\tUIKitApplication:{BUNDLE}[a1b2][rb-legacy]\n"
        )
    }

    fn launched(xcrun: &flui_driver::testing::ScriptedXcrun) -> Vec<XcrunCommand> {
        xcrun
            .commands()
            .into_iter()
            .filter(|command| matches!(command, XcrunCommand::Launch { .. }))
            .collect()
    }

    #[test]
    fn starting_the_app_hands_it_its_endpoint_and_token_as_launch_values() {
        let xcrun = scripted()
            .reply("simctl install", XcrunReply::new(0, ""))
            .reply(
                "simctl launch",
                XcrunReply::new(0, format!("{BUNDLE}: 4321\n")),
            )
            .reply("simctl spawn", XcrunReply::new(0, running(4321)))
            .reply("simctl terminate", XcrunReply::new(0, ""));
        let (_dir, mut lane) = connect(&xcrun);
        let app = SimulatorApp::try_new(BUNDLE)
            .expect("a bundle")
            .app_bundle("/tmp/Scenario.app")
            .env("FLUI_DRIVER_SCENARIO_APP", "insets")
            .and_then(|app| app.env(TOKEN_ENV, "a token of the app's own"))
            .and_then(|app| app.env(ENDPOINT_ENV, "/a/socket/of/the/apps/own"))
            .expect("environment variables");
        let started = lane.start(&app).expect("the app starts");
        assert_eq!(started.pid, Some(4321));
        let shown = format!("{started:?}");
        assert!(
            !shown.contains(&started.token) && shown.contains("4321"),
            "the token stays out of the debug form: {shown}"
        );
        assert_eq!(
            started.token.len(),
            64,
            "twice the shortest token an app takes"
        );
        let Endpoint::Local(address) = &started.endpoint else {
            panic!("a Unix socket on the host: {:?}", started.endpoint);
        };
        let directory = PathBuf::from(address)
            .parent()
            .expect("the socket's directory")
            .to_owned();
        let mode = std::fs::metadata(&directory)
            .expect("the directory is there while the app runs")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "only this user enters it");
        let install = XcrunCommand::Install {
            udid: udid(),
            app: PathBuf::from("/tmp/Scenario.app"),
        };
        assert!(xcrun.commands().contains(&install), "{:?}", xcrun.log());
        let [launch] = launched(&xcrun).try_into().expect("one launch");
        assert_eq!(
            launch.args(),
            [
                "simctl",
                "launch",
                "--terminate-running-process",
                super::UDID,
                BUNDLE
            ]
        );
        let child = |key: &str| format!("SIMCTL_CHILD_{key}");
        assert_eq!(
            launch.env(),
            [
                (child("FLUI_DRIVER_SCENARIO_APP"), "insets".to_owned()),
                (child(ENDPOINT_ENV), address.clone()),
                (child(TOKEN_ENV), started.token.clone()),
            ],
            "the app's own values, and the agent's the lane's alone"
        );
        assert!(
            xcrun
                .log()
                .iter()
                .all(|line| !line.contains(&started.token)),
            "the token never reaches a command's text: {:?}",
            xcrun.log()
        );
        assert!(
            !format!("{launch:?}").contains(&started.token),
            "nor its debug form: {launch:?}"
        );
        drop(lane);
        let terminate = XcrunCommand::Terminate {
            udid: udid(),
            bundle: BUNDLE.to_owned(),
        };
        let commands = xcrun.commands();
        assert_eq!(
            commands[commands.len() - 2..],
            [XcrunCommand::ListServices { udid: udid() }, terminate],
            "the lane ends the app it finds still running"
        );
        assert!(
            !directory.exists(),
            "the socket's directory goes with the lane"
        );
    }

    #[test]
    fn a_launch_that_timed_out_still_ends_the_app() {
        let xcrun = scripted()
            .timeout_once("simctl launch")
            .reply("simctl terminate", XcrunReply::new(0, ""));
        let (_dir, mut lane) = connect(&xcrun);
        let error = lane
            .start(&SimulatorApp::try_new(BUNDLE).expect("a bundle"))
            .expect_err("the launch timed out");
        assert!(
            matches!(error, flui_driver::Error::Timeout { .. }),
            "{error:?}"
        );
        assert!(
            !xcrun
                .commands()
                .iter()
                .any(|command| matches!(command, XcrunCommand::Install { .. })),
            "an app named by its bundle alone is not installed"
        );
        drop(lane);
        assert!(
            matches!(
                xcrun.commands().last(),
                Some(XcrunCommand::Terminate { .. })
            ),
            "{:?}",
            xcrun.log()
        );
    }

    /// When the lane ends, the app it started is ended only while its own process runs: one that
    /// exited is neither ended nor reported, and neither is a copy someone else started since;
    /// one the lane could not end is reported as still running.
    #[test]
    fn only_the_apps_own_running_process_is_ended_and_an_exited_one_is_no_failure() {
        let ended = |services: &str, terminate: XcrunReply| {
            let xcrun = scripted()
                .reply(
                    "simctl launch",
                    XcrunReply::new(0, format!("{BUNDLE}: 4321\n")),
                )
                .reply("simctl spawn", XcrunReply::new(0, services))
                .reply("simctl terminate", terminate);
            let (_dir, mut lane) = connect(&xcrun);
            lane.start(&SimulatorApp::try_new(BUNDLE).expect("a bundle"))
                .expect("the app starts");
            let ((), log) = flui_testing::log_capture::capture(|| drop(lane));
            let terminated = xcrun
                .commands()
                .iter()
                .any(|command| matches!(command, XcrunCommand::Terminate { .. }));
            (terminated, log.contains("still runs"))
        };
        let ok = || XcrunReply::new(0, "");
        assert_eq!(
            ended(&running(4321), ok()),
            (true, false),
            "its own process"
        );
        assert_eq!(
            ended("PID\tStatus\tLabel\n", ok()),
            (false, false),
            "it exited"
        );
        assert_eq!(
            ended(&running(99), ok()),
            (false, false),
            "someone else's copy"
        );
        assert_eq!(
            ended(
                &running(4321),
                XcrunReply::new(3, "").stderr("terminate failed")
            ),
            (true, true),
            "a copy the lane could not end"
        );
    }

    /// A path that is not UTF-8 reaches `xcrun` byte for byte.
    #[test]
    fn a_path_that_is_not_utf8_is_passed_as_it_is() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let app = PathBuf::from(OsStr::from_bytes(b"/tmp/Sc\xffenario.app"));
        let install = XcrunCommand::Install {
            udid: udid(),
            app: app.clone(),
        };
        assert_eq!(
            install.args().last().map(std::ffi::OsString::as_os_str),
            Some(app.as_os_str())
        );
    }

    #[test]
    fn a_lane_that_started_nothing_ends_nothing() {
        let xcrun = scripted();
        drop(connect(&xcrun));
        assert!(
            !xcrun
                .commands()
                .iter()
                .any(|command| matches!(command, XcrunCommand::Terminate { .. })),
            "{:?}",
            xcrun.log()
        );
        let (_dir, mut lane) = connect(&xcrun);
        let other = SimulatorApp::try_new("dev.flui.other").expect("a bundle");
        assert!(matches!(
            lane.start(&other),
            Err(flui_driver::Error::InvalidArgument { .. })
        ));
    }
}
