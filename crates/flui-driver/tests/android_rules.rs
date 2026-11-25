//! The Android lane's refusals at the system's edges, and what it leaves on the device: every
//! pointer's press at an edge of the display, every key and chord whose fallback leaves the app,
//! the dump each session writes, the backlog the guard forgets, and the first failure a release
//! after it keeps.

use std::time::Duration;

use flui_driver::Error;
use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::android::adb::{AdbCommand, AdbReply, Framing, HostAdb};
use flui_driver::lane::android::getevent;
use flui_driver::lane::android::guard::GeteventGuard;
use flui_driver::lane::android::held::HeldInput;
use flui_driver::lane::android::keys::{self, KeyName};
use flui_driver::lane::android::safety::{self, Refusal};
use flui_driver::lane::android::screencap::{self, Raw};
use flui_driver::lane::android::uiautomator::Bounds;
use flui_driver::lane::{self, ActivityGuard, Lane as _};
use flui_driver::protocol::input::PointerDevice;
use flui_driver::steps::OsEvent;
use flui_driver::testing::ScriptedAdb;

use crate::android_lane::{
    GETEVENT, abilities, connected, inputs, placement, raw_capture, scripted, tap,
};

fn press_at(device: PointerDevice, at: DevicePoint) -> Result<(), Refusal> {
    let mut held = HeldInput::new();
    let abilities = abilities();
    for event in &tap(device, at)[..2] {
        for command in held.plan(event, &abilities)? {
            held.took(&command);
        }
    }
    Ok(())
}

#[test]
fn a_press_of_every_pointer_at_an_edge_of_the_display_is_the_systems() {
    // 1080 by 2400 at 420 dpi: the edge is 126 pixels deep.
    let edges = [
        ("top", DevicePoint::new(540, 125)),
        ("bottom", DevicePoint::new(540, 2274)),
        ("left", DevicePoint::new(125, 1200)),
        ("right", DevicePoint::new(954, 1200)),
    ];
    for device in [
        PointerDevice::Touch,
        PointerDevice::Mouse,
        PointerDevice::Pen,
    ] {
        for (edge, at) in edges {
            assert!(
                matches!(press_at(device, at), Err(Refusal::SystemGesture { .. })),
                "a {} press at the {edge} edge",
                device.name()
            );
        }
        assert_eq!(
            press_at(device, DevicePoint::new(540, 1200)),
            Ok(()),
            "a {} press inside",
            device.name()
        );
    }
    // Through the lane, a mouse press on the bottom bar sends nothing.
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    let refused = lane::send_guarded(&mut lane, &tap(PointerDevice::Mouse, edges[1].1))
        .expect_err("Home's place");
    assert!(
        matches!(&refused, Error::NotSupported { capability: Some(capability), .. } if capability == "os_system_gesture"),
        "{refused}"
    );
    assert_eq!(inputs(&adb), Vec::<String>::new());
}

/// The chord of `modifiers` and `key`, pressed and released, as the plan takes it.
fn chord(modifiers: &[&str], key: &str) -> Result<(), Refusal> {
    let mut events: Vec<OsEvent> = modifiers
        .iter()
        .map(|name| OsEvent::KeyDown((*name).to_owned()))
        .collect();
    events.push(OsEvent::KeyDown(key.to_owned()));
    events.push(OsEvent::KeyUp(key.to_owned()));
    let abilities = abilities();
    let mut held = HeldInput::new();
    for event in &events {
        for command in held.plan(event, &abilities)? {
            held.took(&command);
        }
    }
    Ok(())
}

#[test]
fn every_fallback_that_leaves_the_app_is_refused() {
    // One row per fallback of `Virtual.kcm` and `Generic.kcm` that leaves the app or changes the
    // device, and what the lane answers for it.
    let rows: &[(&[&str], &str, &str)] = &[
        (&["Alt"], " ", "SEARCH"),
        (&["Meta"], " ", "SEARCH"),
        (&["Control"], " ", "LANGUAGE_SWITCH"),
        (&["Alt"], "Escape", "HOME"),
        (&["Meta"], "Escape", "HOME"),
        (&["Control", "Alt"], "Backspace", "BACK"),
    ];
    for &(modifiers, key, becomes) in rows {
        match chord(modifiers, key) {
            Err(Refusal::SystemChord { effect, .. }) => {
                assert!(effect.contains(becomes), "{modifiers:?}+{key:?}: {effect}");
            }
            other => panic!("{modifiers:?}+{key:?}: {other:?}"),
        }
    }
    // Each entry of the table has its row.
    assert_eq!(safety::FALLBACKS.len(), 6);
    for fallback in safety::FALLBACKS {
        assert_eq!(
            safety::system_chord(fallback.held, fallback.key),
            Some(fallback.becomes)
        );
    }
    // The fallbacks that stay in the app, and chords that are no fallback, are sent: Escape alone
    // reaches a `NativeActivity`, and Control and Escape's fallback is the app's own MENU.
    for (modifiers, key) in [
        (&[][..], "Escape"),
        (&["Control"][..], "Escape"),
        (&["Shift"][..], "Escape"),
        (&["Control"][..], "Backspace"),
        (&["Alt"][..], "Backspace"),
        (&[][..], " "),
        (&["Shift"][..], " "),
        (&["Control"][..], "a"),
    ] {
        assert_eq!(chord(modifiers, key), Ok(()), "{modifiers:?}+{key:?}");
    }
    // The gamepad buttons whose fallback is Back, Menu or Home have no name the lane takes.
    for name in [
        "GamepadB",
        "GamepadY",
        "GamepadSelect",
        "GamepadMode",
        "ButtonB",
    ] {
        assert_eq!(keys::resolve(name), KeyName::Unknown, "{name}");
    }
}

#[test]
fn a_device_is_driven_only_by_its_serial() {
    assert!(HostAdb::try_new("adb", "").is_err());
    assert!(HostAdb::try_new("adb", "  ").is_err());
    assert!(HostAdb::try_new("adb", "emulator-5554").is_ok());
}

#[test]
fn arming_forgets_every_line_that_came_while_the_guard_was_disarmed() {
    let adb = ScriptedAdb::new().stream(
        "shell -tt getevent",
        GETEVENT.lines().map(str::to_owned).collect(),
        None,
    );
    let stream = adb
        .device()
        .open(
            &AdbCommand::ShellTerminal(getevent::WATCH.to_owned()),
            Framing::Lines,
        )
        .expect("a stream");
    let mut guard = GeteventGuard::new(stream);
    guard.arm().expect("watching");
    guard.disarm();
    // The person used the device between two runs: far more lines than one look reads.
    for index in 0..3000 {
        adb.push(
            "shell -tt getevent",
            &format!("/dev/input/event5: EV_ABS       ABS_MT_POSITION_X    {index:08x}"),
        );
    }
    guard.arm().expect("armed again");
    assert!(
        guard.foreign_input().is_none(),
        "what came before the run is not the run's"
    );
    adb.push(
        "shell -tt getevent",
        "/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000001",
    );
    assert!(guard.foreign_input().is_some());
}

#[test]
fn arming_waits_out_a_backlog_the_reader_is_still_refilling() {
    let adb = ScriptedAdb::new().stream(
        "shell -tt getevent",
        GETEVENT.lines().map(str::to_owned).collect(),
        None,
    );
    let stream = adb
        .device()
        .open(
            &AdbCommand::ShellTerminal(getevent::WATCH.to_owned()),
            Framing::Lines,
        )
        .expect("a stream");
    let mut guard = GeteventGuard::new(stream);
    guard.arm().expect("watching");
    guard.disarm();
    // Touches from between two runs: some waiting, and more the reader hands over a moment after
    // the channel ran empty.
    adb.push(
        "shell -tt getevent",
        "/dev/input/event5: EV_ABS       ABS_MT_POSITION_X    00000001",
    );
    let refilled: Vec<String> = (0..50)
        .map(|index| format!("/dev/input/event5: EV_ABS       ABS_MT_POSITION_Y    {index:08x}"))
        .collect();
    adb.push_after("shell -tt getevent", &refilled, Duration::from_millis(30));
    guard.arm().expect("armed again");
    std::thread::sleep(Duration::from_millis(60));
    assert!(
        guard.foreign_input().is_none(),
        "what the reader refilled from before the run is not the run's"
    );
}

#[test]
fn a_capture_is_read_and_cut_in_the_buffer_it_came_in() {
    let bytes = raw_capture(4, 3, 1, 16);
    let address = bytes.as_ptr();
    let Ok(Raw::Rgba {
        width,
        height,
        rgba,
    }) = screencap::parse_raw(bytes)
    else {
        panic!("a capture");
    };
    assert_eq!(rgba.as_ptr(), address, "the pixels were not copied out");
    let window = Bounds {
        left: 1,
        top: 1,
        right: 3,
        bottom: 3,
    };
    let expected: Vec<u8> = [5_u8, 6, 9, 10]
        .iter()
        .flat_map(|&index| [index, 1, 2, 3])
        .collect();
    let cut = screencap::crop(width, height, rgba, window).expect("inside");
    assert_eq!(cut.rgba().as_ptr(), address, "the window was cut in place");
    assert_eq!(cut.rgba(), expected.as_slice());
}

#[test]
fn each_session_dumps_to_a_file_of_its_own() {
    let dump_path = |adb: &ScriptedAdb| {
        adb.commands("exec-out uiautomator dump")
            .first()
            .and_then(|line| line.split_whitespace().nth(3).map(str::to_owned))
            .expect("a dump")
    };
    // Only a read of the tree dumps; it fails to match the empty app tree once it has.
    let read = |adb: &ScriptedAdb| {
        let mut lane = connected(adb);
        let empty = flui_driver::protocol::tree::Tree::new(Vec::new(), false);
        let _ = lane.read_tree(&placement(), &empty);
    };
    let first = scripted(true, true);
    read(&first);
    let second = scripted(true, true);
    read(&second);
    let (one, two) = (dump_path(&first), dump_path(&second));
    assert_ne!(one, two, "two sessions on one device dump apart");
    assert!(
        one.starts_with("/data/local/tmp/flui-driver-")
            && std::path::Path::new(&one)
                .extension()
                .is_some_and(|extension| extension == "xml"),
        "{one}"
    );
    let line = &first.commands("exec-out uiautomator dump")[0];
    assert!(line.contains(&format!("rm -f {one}")), "{line}");
}

#[test]
fn a_release_that_fails_after_the_run_did_keeps_the_runs_effect_and_verdict() {
    // A press that timed out, then a cancel the device refused: the press may have run.
    let adb = scripted(true, true)
        .timeout_once("shell input touchscreen motionevent DOWN")
        .reply_once(
            "shell input touchscreen motionevent CANCEL",
            AdbReply::new(1, "").stderr("Error: refused"),
        );
    let mut lane = connected(&adb);
    let failed = lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect_err("held after a timeout");
    assert!(matches!(failed, Error::ReleaseFailed { .. }), "{failed}");
    assert!(failed.may_have_run(), "{failed}");
    assert!(!failed.is_cannot_verify());
    // The person's touch, then a cancel the device refused: the run still could not be verified.
    let adb = scripted(false, true)
        .on_command(
            "shell input touchscreen motionevent DOWN",
            "shell -tt getevent",
            "/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000045",
        )
        .reply_once(
            "shell input touchscreen motionevent CANCEL",
            AdbReply::new(1, "").stderr("Error: refused"),
        );
    let mut lane = connected(&adb);
    let interrupted = lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect_err("interrupted, then held");
    assert!(
        matches!(interrupted, Error::ReleaseFailed { .. }),
        "{interrupted}"
    );
    assert!(interrupted.is_cannot_verify(), "{interrupted}");
    assert_eq!(
        interrupted.missing_capability().as_deref(),
        Some("uninterrupted_os_input")
    );
    // The press went down before the touch was seen, so the run may have run.
    assert!(interrupted.may_have_run(), "{interrupted}");
    // A release that fails on its own carries no first failure.
    let adb = scripted(true, true).reply_once(
        "shell input touchscreen motionevent CANCEL",
        AdbReply::new(1, "").stderr("Error: refused"),
    );
    let mut lane = connected(&adb);
    let events = tap(PointerDevice::Touch, DevicePoint::new(300, 600));
    lane.send(&events[0]).expect("placed");
    lane.send(&events[1]).expect("pressed");
    let alone = lane.release_all().expect_err("refused");
    assert!(
        matches!(&alone, Error::ReleaseFailed { first: None, .. }),
        "{alone}"
    );
    assert!(!alone.is_cannot_verify() && !alone.may_have_run());
}
