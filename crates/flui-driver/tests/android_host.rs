//! The host's `adb` door (`HostAdb`) against a stand-in program: what a command printed and how
//! it exited, the bound on its output, its deadline, a program that is missing, and streams cut
//! into records. The stand-in is a shell script that answers by the words `adb` would pass on.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use flui_driver::lane::android::adb::{self, Adb, AdbCommand, Framing, HostAdb, RunFailure};
use flui_driver::{Awaited, Error};

/// A stand-in for `adb`, answering `devices` and a few shell lines the tests name.
const STAND_IN: &str = r#"#!/bin/sh
serial=""
if [ "$1" = "-s" ]; then serial="$2"; shift 2; fi
case "$1" in
  devices) printf 'List of devices attached\nemulator-5554\tdevice\n%s' "$serial" ;;
  shell|exec-out)
    shift
    if [ "$1" = "-tt" ]; then shift; fi
    case "$1" in
      serial) printf '%s\n' "$serial" ;;
      fail) echo "Error: nope" >&2; exit 3 ;;
      flood) head -c 200000 /dev/zero ;;
      sleep) exec sleep 30 ;;
      lines) printf 'one\r\ntwo\n'; exec sleep 30 ;;
      echo) while read -r line; do printf '{"got":"%s"}' "$line"; done ;;
      long) head -c 5000 /dev/zero | tr '\0' x; echo ;;
      quit) printf 'last\n' ;;
    esac ;;
esac
"#;

/// The stand-in written to a directory of its own, kept as long as the directory.
fn stand_in() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("adb");
    std::fs::write(&path, STAND_IN).expect("the stand-in is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("the stand-in runs");
    (dir, path)
}

fn shell(line: &str) -> AdbCommand {
    AdbCommand::Shell(line.to_owned())
}

#[test]
fn a_command_answers_what_it_printed_and_how_it_exited() {
    let (_dir, path) = stand_in();
    let mut host = HostAdb::try_new(&path, "emulator-5554").expect("a named device");
    let listing = host
        .run(&AdbCommand::Devices, 4096, Duration::from_secs(10))
        .expect("the devices");
    assert_eq!(listing.exit, Some(0));
    assert_eq!(
        listing.text(),
        "List of devices attached\nemulator-5554\tdevice\n",
        "the listing names no device"
    );
    assert!(
        HostAdb::try_new(&path, " ").is_err(),
        "a serial names a device"
    );
    let mut device = HostAdb::try_new(&path, "emulator-5554").expect("a named device");
    let serial = device
        .run(&shell("serial"), 4096, Duration::from_secs(10))
        .expect("the serial");
    assert_eq!(serial.text(), "emulator-5554\n", "the device is named");
    let failed = device
        .run(&shell("fail"), 4096, Duration::from_secs(10))
        .expect("a failure is an answer");
    assert_eq!(failed.exit, Some(3));
    assert_eq!(failed.stderr.trim(), "Error: nope");
}

#[test]
fn output_past_its_bound_and_a_command_past_its_deadline_are_ended() {
    let (_dir, path) = stand_in();
    let mut host = HostAdb::try_new(&path, "x").expect("a named device");
    // Both were started and their answers lost: the device may have done them.
    match host.run(&shell("flood"), 1000, Duration::from_secs(10)) {
        Err(RunFailure::Unanswered(Error::Lane { message, .. })) => {
            assert!(message.contains("1000 bytes"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let started = Instant::now();
    let timed_out = match host.run(&shell("sleep"), 4096, Duration::from_millis(300)) {
        Err(RunFailure::Unanswered(timed_out)) => timed_out,
        other => panic!("past its deadline: {other:?}"),
    };
    assert!(timed_out.may_have_run(), "{timed_out}");
    assert!(matches!(
        timed_out,
        Error::Timeout {
            awaited: Awaited::Host,
            ..
        }
    ));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "ended, not waited out"
    );
    let mut missing = HostAdb::try_new("/nonexistent/adb", "x").expect("a named device");
    // An `adb` that does not start sends nothing to the device.
    match missing.run(&AdbCommand::Devices, 4096, Duration::from_secs(1)) {
        Err(RunFailure::NotStarted(Error::Lane {
            unavailable: true, ..
        })) => {}
        other => panic!("{other:?}"),
    }
    assert!(missing.open(&shell("lines"), Framing::Lines).is_err());
}

#[test]
fn a_stream_hands_over_its_records_and_says_when_it_ended() {
    let (_dir, path) = stand_in();
    let mut host = HostAdb::try_new(&path, "x").expect("a named device");
    let mut lines = host
        .open(
            &AdbCommand::ShellTerminal("lines".to_owned()),
            Framing::Lines,
        )
        .expect("a stream");
    let wait = Duration::from_secs(10);
    assert_eq!(
        lines.next_record(wait).expect("a line").as_deref(),
        Some("one")
    );
    assert_eq!(
        lines.next_record(wait).expect("a line").as_deref(),
        Some("two")
    );
    assert_eq!(
        lines
            .next_record(Duration::from_millis(100))
            .expect("still running"),
        None
    );
    assert_eq!(
        lines.next_record(Duration::ZERO).expect("still running"),
        None
    );
    drop(lines);
    let mut echo = host
        .open(&shell("echo"), Framing::JsonObjects)
        .expect("a stream");
    echo.send_line("hi").expect("it reads its input");
    assert_eq!(
        echo.next_record(wait).expect("an answer").as_deref(),
        Some(r#"{"got":"hi"}"#)
    );
    let mut quit = host.open(&shell("quit"), Framing::Lines).expect("a stream");
    assert_eq!(
        quit.next_record(wait).expect("its line").as_deref(),
        Some("last")
    );
    assert!(quit.next_record(wait).is_err(), "it ended");
    assert!(quit.next_record(Duration::ZERO).is_err(), "and stays ended");
    let mut long = host.open(&shell("long"), Framing::Lines).expect("a stream");
    assert!(
        long.next_record(wait).is_err(),
        "a record past {} bytes",
        adb::MAX_RECORD
    );
}
