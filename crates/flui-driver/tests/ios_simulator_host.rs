//! The host's `xcrun` door (`HostXcrun`) against a stand-in program: what a command printed, the
//! launch values in `simctl`'s environment and nowhere else, a screenshot read back from the file
//! `simctl` wrote and refused past its bound, the bound on output, the deadline, and a host
//! without Xcode. The stand-in is a shell script that answers by the words `xcrun` would pass on.

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use flui_driver::Error;
use flui_driver::lane::ios_simulator::device::Udid;
use flui_driver::lane::ios_simulator::xcrun::{HostXcrun, Xcrun, XcrunCommand};
use flui_driver::testing::host_xcrun_with;

use crate::android_lane::png_of;
use crate::ios_simulator_lane::{BUNDLE, UDID};

/// The UDID the stand-in knows no simulator by.
const GONE: &str = "FFFFFFFF-FFFF-4FFF-8FFF-FFFFFFFFFFFF";

/// The UDID whose screenshot is one byte past the bound.
const LONG: &str = "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA";

/// The UDID whose screenshot grows past the bound as it is read: a named pipe, whose length reads
/// as nothing, fed one byte past the bound once the stand-in has exited.
const GROWING: &str = "BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB";

/// A stand-in for `xcrun`: `simctl launch` prints the launch values it was handed and a stray
/// one, `simctl io` copies the PNG beside it to the path it is given (or writes the screenshots
/// `LONG` and `GROWING` name), `xcodebuild` floods, `simctl terminate` hangs, and the UDID `GONE`
/// names no simulator. The bound is 64 MiB, so one byte past it is 67108865.
const STAND_IN: &str = r#"#!/bin/sh
here="$(dirname "$0")"
case "$3" in
  FFFFFFFF-FFFF-4FFF-8FFF-FFFFFFFFFFFF) echo "Invalid device: $3" >&2; exit 164 ;;
  AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA) head -c 67108865 /dev/zero > "$6"; exit 0 ;;
  BBBBBBBB-BBBB-4BBB-8BBB-BBBBBBBBBBBB)
    mkfifo "$6"
    # The writer lets go of the stand-in's output before it blocks opening the pipe, which it
    # does until the lane reads the screenshot.
    ( exec >/dev/null 2>&1 </dev/null; exec head -c 67108865 /dev/zero > "$6" ) &
    exit 0 ;;
esac
case "$1 $2" in
  "simctl launch") printf '%s: 12\n%s\n%s\n%s\n' "$5" "$SIMCTL_CHILD_FLUI_AGENT_TOKEN" "${FLUI_AGENT_TOKEN:-unset}" "${SIMCTL_CHILD_STRAY:-unset}" ;;
  "simctl io") cp "$here/screen.png" "$6" ;;
  "simctl terminate") exec sleep 30 ;;
  "xcodebuild -version") head -c 200000 /dev/zero ;;
esac
"#;

/// The stand-in and the PNG it copies, written to a directory of their own, kept as long as the
/// directory.
fn stand_in(png: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("xcrun");
    std::fs::write(&path, STAND_IN).expect("the stand-in is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the stand-in runs");
    std::fs::write(dir.path().join("screen.png"), png).expect("the screen is written");
    (dir, path)
}

fn udid(udid: &str) -> Udid {
    Udid::try_new(udid).expect("a UDID")
}

/// The launch values reach `simctl` in its children's spelling alone, and a `SIMCTL_CHILD_`
/// variable the driver's own environment carries does not reach it at all.
#[test]
fn the_launch_values_reach_simctl_as_its_childrens_and_nowhere_else() {
    let (_dir, program) = stand_in(&[]);
    let inherited = std::env::vars_os().chain([(
        OsString::from("SIMCTL_CHILD_STRAY"),
        OsString::from("a value the lane did not set"),
    )]);
    let mut xcrun = host_xcrun_with(program, inherited);
    let shown = format!("{xcrun:?}");
    assert!(
        shown.contains("SIMCTL_CHILD_STRAY") && !shown.contains("a value the lane did not set"),
        "the debug form names keys, never values: {shown}"
    );
    let launch = XcrunCommand::Launch {
        udid: udid(UDID),
        bundle: BUNDLE.to_owned(),
        env: vec![("FLUI_AGENT_TOKEN".to_owned(), "secret-token".to_owned())],
    };
    let reply = xcrun
        .run(&launch, 4096, Duration::from_secs(10))
        .expect("the launch answers");
    assert_eq!(reply.failure(), None, "{reply:?}");
    assert_eq!(
        reply.text(),
        format!("{BUNDLE}: 12\nsecret-token\nunset\nunset\n"),
        "the value goes in `SIMCTL_CHILD_`'s spelling alone, and no stray one goes"
    );
    assert!(!launch.to_string().contains("secret-token"), "{launch}");
}

/// A screenshot file past the bound is refused by its length before it is read, and one that
/// grows past the bound while it is read is refused once it passed it.
#[test]
fn a_screenshot_past_its_bound_is_refused_however_it_got_there() {
    let (_dir, program) = stand_in(&[]);
    let mut xcrun = HostXcrun::at(program);
    let error = xcrun
        .screenshot(&udid(LONG), Duration::from_secs(30))
        .expect_err("past its bound");
    assert!(
        matches!(&error, Error::Lane { message, .. } if message.contains("67108865 bytes")),
        "{error:?}"
    );
    let error = xcrun
        .screenshot(&udid(GROWING), Duration::from_secs(30))
        .expect_err("grew past its bound");
    assert!(
        matches!(&error, Error::Lane { message, .. } if message.contains("grew past the bound")),
        "{error:?}"
    );
}

#[test]
fn a_screenshot_is_read_back_from_the_file_simctl_wrote() {
    let png = png_of(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255]);
    let (_dir, program) = stand_in(&png);
    let mut xcrun = HostXcrun::at(program);
    let read = xcrun
        .screenshot(&udid(UDID), Duration::from_secs(10))
        .expect("the screenshot");
    assert_eq!(read, png);
    let error = xcrun
        .screenshot(&udid(GONE), Duration::from_secs(10))
        .expect_err("no such simulator");
    assert!(
        matches!(&error, Error::Lane { unavailable: true, message, .. } if message.contains("Invalid device")),
        "{error:?}"
    );
}

#[test]
fn output_past_its_bound_and_a_command_past_its_deadline_are_ended() {
    let (_dir, program) = stand_in(&[]);
    let mut xcrun = HostXcrun::at(program);
    let error = xcrun
        .run(&XcrunCommand::XcodeVersion, 1000, Duration::from_secs(10))
        .expect_err("past its bound");
    assert!(
        matches!(&error, Error::Lane { unavailable: false, message, .. } if message.contains("more than 1000 bytes")),
        "{error:?}"
    );
    let started = Instant::now();
    let terminate = XcrunCommand::Terminate {
        udid: udid(UDID),
        bundle: BUNDLE.to_owned(),
    };
    let error = xcrun
        .run(&terminate, 1000, Duration::from_millis(200))
        .expect_err("past its deadline");
    assert!(matches!(error, Error::Timeout { .. }), "{error:?}");
    assert_eq!(
        error.effect().map(|effect| effect.kind),
        Some(flui_driver::protocol::act::EffectKind::MayHaveRun),
        "the Simulator may have done it"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the command was ended at its deadline"
    );
}

#[test]
fn a_host_without_xcrun_cannot_run_the_lane() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut xcrun = HostXcrun::at(dir.path().join("xcrun"));
    let error = xcrun
        .run(&XcrunCommand::ListDevices, 1000, Duration::from_secs(10))
        .expect_err("no xcrun");
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
    assert!(error.is_cannot_verify(), "{error:?}");
}
