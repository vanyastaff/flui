//! The Android lane at its edges: the bounds of what it reads from the device, the exact lines it
//! writes to it, and the orders of events its plan and its release must survive.

use std::time::{Duration, Instant};

use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::android::adb::{AdbCommand, AdbReply, Framing, Printed, Splitter};
use flui_driver::lane::android::device;
use flui_driver::lane::android::getevent::{self, Line, Watch};
use flui_driver::lane::android::guard::GeteventGuard;
use flui_driver::lane::android::held::{
    Abilities, Command, HeldInput, MotionAction, MotionSource, Part, PenChange,
};
use flui_driver::lane::android::keys;
use flui_driver::lane::android::safety::{self, Display, Refusal};
use flui_driver::lane::android::screencap::{self, Raw};
use flui_driver::lane::android::uiautomator::{self, Bounds, UiDump};
use flui_driver::lane::android::uinput;
use flui_driver::lane::android::{AndroidApp, AndroidLane};
use flui_driver::lane::{self, ActivityGuard, Lane as _};
use flui_driver::protocol::input::{PointerButton, PointerDevice};
use flui_driver::steps::OsEvent;
use flui_driver::testing::ScriptedAdb;
use flui_driver::{Awaited, Error, Isolation};

use crate::android_lane::{
    FOCUSED, GETEVENT, PACKAGE, PROBED, USAGE, WINDOWS, abilities, answer_syncs, capability,
    connected, focus_of, inputs, png_of, raw_capture, scripted, scripted_with, tap,
};

const AT: DevicePoint = DevicePoint::new(300, 600);

fn run(
    held: &mut HeldInput,
    events: &[OsEvent],
    abilities: &Abilities,
) -> Result<Vec<Command>, Refusal> {
    let mut commands = Vec::new();
    for event in events {
        let planned = held.plan(event, abilities)?;
        for command in &planned {
            held.took(command);
        }
        commands.extend(planned);
    }
    Ok(commands)
}

#[test]
fn the_pen_and_the_probe_register_exactly_these_devices() {
    let axes = uinput::stylus_axes(1080, 2400);
    assert_eq!(
        uinput::stylus(&axes).command(),
        concat!(
            r#"{"id":2,"command":"register","name":"FLUI Driver Stylus","vid":6353,"pid":43981,"bus":"usb","#,
            r#""configuration":[{"type":"UI_SET_EVBIT","data":["EV_KEY","EV_ABS"]},"#,
            r#"{"type":"UI_SET_KEYBIT","data":["BTN_TOOL_PEN","BTN_TOUCH","BTN_STYLUS"]},"#,
            r#"{"type":"UI_SET_ABSBIT","data":["ABS_X","ABS_Y","ABS_PRESSURE","ABS_TILT_X","ABS_TILT_Y"]},"#,
            r#"{"type":"UI_SET_PROPBIT","data":["INPUT_PROP_DIRECT"]}],"#,
            r#""abs_info":["#,
            r#"{"code":"ABS_X","info":{"value":0,"minimum":0,"maximum":1079,"fuzz":0,"flat":0,"resolution":0}},"#,
            r#"{"code":"ABS_Y","info":{"value":0,"minimum":0,"maximum":2399,"fuzz":0,"flat":0,"resolution":0}},"#,
            r#"{"code":"ABS_PRESSURE","info":{"value":0,"minimum":0,"maximum":4095,"fuzz":0,"flat":0,"resolution":0}},"#,
            r#"{"code":"ABS_TILT_X","info":{"value":0,"minimum":-90,"maximum":90,"fuzz":0,"flat":0,"resolution":0}},"#,
            r#"{"code":"ABS_TILT_Y","info":{"value":0,"minimum":-90,"maximum":90,"fuzz":0,"flat":0,"resolution":0}}]}"#,
        )
    );
    assert_eq!(
        uinput::probe().command(),
        concat!(
            r#"{"id":1,"command":"register","name":"FLUI Driver Probe","vid":6353,"pid":43980,"bus":"usb","#,
            r#""configuration":[{"type":"UI_SET_EVBIT","data":["EV_KEY"]},"#,
            r#"{"type":"UI_SET_KEYBIT","data":["BTN_TOUCH"]}],"abs_info":[]}"#,
        )
    );
    assert_eq!(
        uinput::sync(2, 7),
        r#"{"id":2,"command":"sync","syncToken":"7"}"#
    );
    // The probe is found among several devices, wherever it is listed.
    let list =
        format!("N: Name=\"sec_touchscreen\"\nP: Phys=\n\n{PROBED}\nN: Name=\"gpio_keys\"\n");
    assert!(uinput::lists_device(&list, uinput::PROBE_NAME));
    assert!(!uinput::lists_device(
        "N: Name=\"FLUI Driver Probe 2\"\n",
        uinput::PROBE_NAME
    ));
}

#[test]
fn a_point_off_the_turned_display_lands_nowhere() {
    let natural = (1080, 2400);
    for (x, y, rotation) in [
        (0, 2400, 0),
        (0, -1, 0),
        (-1, 0, 0),
        (2400, 0, 1),
        (0, 1080, 1),
        (0, 0, 4),
    ] {
        assert_eq!(
            uinput::natural_point(DevicePoint::new(x, y), rotation, natural, natural),
            None,
            "({x}, {y}) at rotation {rotation}"
        );
    }
    assert_eq!(uinput::turned((1080, 2400), 3), (2400, 1080));
    assert_eq!(uinput::turned((1080, 2400), 2), (1080, 2400));
}

#[test]
fn a_raw_header_is_read_word_by_word_and_a_side_of_exactly_the_bound_is_taken() {
    // A header alone, in a format the lane does not read raw: every word is read.
    let header = [
        8_u32.to_le_bytes(),
        2_u32.to_le_bytes(),
        5_u32.to_le_bytes(),
    ]
    .concat();
    assert_eq!(screencap::parse_raw(header), Ok(Raw::OtherFormat(5)));
    // A width whose high byte is set is past the bound, whatever its low bytes.
    let wide = [[1, 0, 0, 1], 1_u32.to_le_bytes(), 1_u32.to_le_bytes()].concat();
    assert!(screencap::parse_raw(wide).is_err());
    let Ok(Raw::Rgba { width, height, .. }) =
        screencap::parse_raw(raw_capture(screencap::MAX_SIDE, 1, 1, 12))
    else {
        panic!("a side of exactly the bound is read");
    };
    assert_eq!((width, height), (screencap::MAX_SIDE, 1));
    let Ok(Raw::Rgba { rgba, .. }) =
        screencap::parse_raw(raw_capture(1, screencap::MAX_SIDE, 1, 16))
    else {
        panic!("a tall side of exactly the bound is read");
    };
    assert_eq!(rgba.len(), screencap::MAX_SIDE as usize * 4);
    // Every pixel of an RGBX capture is opaque, and no colour byte is touched.
    let Ok(Raw::Rgba { rgba, .. }) = screencap::parse_raw(raw_capture(3, 1, 2, 16)) else {
        panic!("RGBX reads");
    };
    assert_eq!(rgba, vec![0, 1, 2, 255, 1, 1, 2, 255, 2, 1, 2, 255]);
}

#[test]
fn a_png_past_the_bound_is_refused() {
    let line = |length: u32| vec![0_u8; length as usize * 4];
    let at_bound = png_of(screencap::MAX_SIDE, 1, &line(screencap::MAX_SIDE));
    assert!(
        screencap::parse_png(&at_bound).is_ok(),
        "a side of exactly the bound"
    );
    let tall_at_bound = png_of(1, screencap::MAX_SIDE, &line(screencap::MAX_SIDE));
    assert!(
        screencap::parse_png(&tall_at_bound).is_ok(),
        "a height of exactly the bound"
    );
    // A format word read whole: its high byte makes it another format.
    let header = [1_u32.to_le_bytes(), 1_u32.to_le_bytes(), [1, 0, 0, 1]].concat();
    assert_eq!(
        screencap::parse_raw(header),
        Ok(Raw::OtherFormat(0x0100_0001))
    );
    let header = [1_u32.to_le_bytes(), 1_u32.to_le_bytes(), [1, 0, 1, 0]].concat();
    assert_eq!(
        screencap::parse_raw(header),
        Ok(Raw::OtherFormat(0x0001_0001))
    );
    let wide = png_of(screencap::MAX_SIDE + 1, 1, &line(screencap::MAX_SIDE + 1));
    assert!(screencap::parse_png(&wide).is_err());
    let tall = png_of(1, screencap::MAX_SIDE + 1, &line(screencap::MAX_SIDE + 1));
    assert!(screencap::parse_png(&tall).is_err());
}

#[test]
fn a_window_is_cut_only_when_it_lies_inside_the_capture() {
    let (width, height) = (4, 3);
    let rgba: Vec<u8> = (0..width * height * 4)
        .map(|byte| u8::try_from(byte % 256).expect("small"))
        .collect();
    let window = |left, top, right, bottom| Bounds {
        left,
        top,
        right,
        bottom,
    };
    let whole =
        screencap::crop(width, height, rgba.clone(), window(0, 0, 4, 3)).expect("the whole");
    assert_eq!(
        (whole.width(), whole.height(), whole.rgba().len()),
        (4, 3, 48)
    );
    let inner = screencap::crop(width, height, rgba.clone(), window(0, 0, 2, 2)).expect("inside");
    assert_eq!((inner.width(), inner.height()), (2, 2));
    let corner =
        screencap::crop(width, height, rgba.clone(), window(3, 2, 4, 3)).expect("a corner");
    assert_eq!(corner.rgba(), &rgba[44..48]);
    for refused in [
        window(-1, 0, 2, 2),
        window(0, -1, 2, 2),
        window(1, 1, 1, 2),
        window(1, 1, 2, 1),
        window(0, 0, 5, 3),
        window(0, 0, 4, 4),
        window(2, 1, 1, 3),
    ] {
        assert!(
            screencap::crop(width, height, rgba.clone(), refused).is_err(),
            "{refused:?}"
        );
    }
    // A buffer shorter than its size says is refused, not read past.
    assert!(screencap::crop(width, height, rgba[..40].to_vec(), window(0, 0, 4, 3)).is_err());
}

#[test]
fn the_dump_is_bounded_in_size_depth_and_rotation_and_reads_no_dtd() {
    let nested = |depth: usize| {
        format!(
            "<hierarchy rotation=\"3\">{}{}</hierarchy>",
            "<node package=\"p\">".repeat(depth),
            "</node>".repeat(depth)
        )
    };
    let deepest = UiDump::parse(&nested(uiautomator::MAX_DEPTH)).expect("as deep as the bound");
    assert_eq!(deepest.rotation, 3);
    assert!(UiDump::parse(&nested(uiautomator::MAX_DEPTH + 1)).is_err());
    let rotations: Vec<bool> = (0..6)
        .map(|rotation| UiDump::parse(&format!("<hierarchy rotation=\"{rotation}\"/>")).is_ok())
        .collect();
    assert_eq!(rotations, [true, true, true, true, false, false]);
    let doctype = "<!DOCTYPE hierarchy [<!ENTITY a \"x\">]><hierarchy rotation=\"0\"/>";
    assert!(UiDump::parse(doctype).is_err(), "a DTD is refused");
    let padded = {
        let body = "<hierarchy rotation=\"0\"/>";
        format!("{body}{}", " ".repeat(uiautomator::MAX_DUMP - body.len()))
    };
    assert_eq!(padded.len(), uiautomator::MAX_DUMP);
    assert!(
        UiDump::parse(&padded).is_ok(),
        "a dump of exactly the bound is read"
    );
    assert!(
        UiDump::parse(&format!("{padded} ")).is_err(),
        "a well-formed dump past the bound is refused"
    );
    assert_eq!(
        Bounds::parse("[10,20][5,8]").map(|bounds| (bounds.width(), bounds.height())),
        Some((0, 0))
    );
}

#[test]
fn the_window_is_matched_within_its_tolerance() {
    let adb = scripted(true, true);
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let window: flui_driver::protocol::tree::WindowId = "w1".parse().expect("a window");
    // The fixture's window is 1080 by 2340 pixels.
    for (width, height, found) in [
        (1080.0, 2340.0, true),
        (1082.0, 2340.0, true),
        (1080.0, 2338.0, true),
        (1082.5, 2340.0, false),
        (1080.0, 2342.5, false),
    ] {
        let placement = flui_driver::protocol::session::Placement::new(
            window,
            flui_driver::protocol::geometry::LogicalSize::new(width, height),
            1.0,
        );
        assert_eq!(
            lane.locate_window(&placement).is_ok(),
            found,
            "{width} x {height}"
        );
    }
}

#[test]
fn getevent_input_counts_only_while_armed_and_the_first_is_kept() {
    let fresh = || {
        let mut watch = Watch::new();
        for line in GETEVENT.lines() {
            watch.read(line);
        }
        watch
    };
    let touch = "/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000045";
    let key = "/dev/input/event2: EV_KEY       KEY_VOLUMEDOWN       DOWN";
    let mut watch = fresh();
    watch.read(touch);
    assert_eq!(watch.foreign(), None, "not armed");
    watch.arm();
    watch.read(key);
    watch.read(touch);
    assert_eq!(
        watch.foreign(),
        Some("EV_KEY KEY_VOLUMEDOWN DOWN from event2"),
        "the first"
    );
    watch.arm();
    assert_eq!(watch.foreign(), None, "arming forgets what came before");
    watch.disarm();
    watch.read(touch);
    assert_eq!(watch.foreign(), None, "disarmed");
    // A device removed and not added again is no one's the watch knows: its input counts.
    let mut watch = fresh();
    watch.read("remove device 4: /dev/input/event9");
    watch.arm();
    watch.read("/dev/input/event9: EV_KEY       BTN_TOOL_PEN         DOWN");
    assert!(watch.foreign().is_some());
    // A device added again under another name is read by its new name.
    let mut watch = fresh();
    watch.read("add device 5: /dev/input/event9");
    watch.read("  name:     \"usb_mouse\"");
    watch.arm();
    watch.read("/dev/input/event9: EV_REL       REL_X                00000001");
    assert!(watch.foreign().is_some());
    // A line with no device path is no event.
    let mut watch = fresh();
    watch.arm();
    watch.read("warning: EV_KEY KEY_A DOWN");
    assert_eq!(watch.foreign(), None);
    assert_eq!(
        getevent::parse_line("remove device 4: /dev/input/event9"),
        Line::Removed("/dev/input/event9".to_owned())
    );
    assert!(!Watch::new().started());
}

#[test]
fn past_its_bound_a_new_device_is_taken_for_someone_elses() {
    let mut watch = Watch::new();
    for index in 0..getevent::MAX_DEVICES - 1 {
        watch.read(&format!("add device {index}: /dev/input/event{index}"));
        watch.read("  name:     \"sensor\"");
    }
    // The last device the bound holds.
    watch.read("add device 99: /dev/input/event99");
    watch.read(&format!("  name:     \"{}\"", uinput::STYLUS_NAME));
    // Past the bound.
    watch.read("add device 100: /dev/input/event100");
    watch.read(&format!("  name:     \"{}\"", uinput::STYLUS_NAME));
    watch.arm();
    watch.read("/dev/input/event99: EV_KEY       BTN_TOUCH            DOWN");
    assert_eq!(
        watch.foreign(),
        None,
        "the last remembered device is the lane's"
    );
    watch.read("/dev/input/event100: EV_KEY       BTN_TOUCH            DOWN");
    assert!(watch.foreign().is_some(), "a device past the bound counts");
}

#[test]
fn the_guard_waits_for_getevent_to_name_a_device_and_stops_with_it() {
    let late: Vec<String> = GETEVENT.lines().map(str::to_owned).collect();
    let adb = ScriptedAdb::new()
        .stream("shell -tt getevent", Vec::new(), None)
        .late_records("shell -tt getevent", late);
    let mut device = adb.device();
    let stream = device
        .open(
            &AdbCommand::ShellTerminal(getevent::WATCH.to_owned()),
            Framing::Lines,
        )
        .expect("a stream");
    let mut guard = GeteventGuard::new(stream);
    guard.arm().expect("the devices came while it waited");
    adb.push(
        "shell -tt getevent",
        "/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000001",
    );
    assert!(guard.foreign_input().is_some());
    guard.disarm();
    guard.arm().expect("armed again");
    assert!(
        guard.foreign_input().is_none(),
        "arming forgets what came before"
    );
    guard.disarm();
    adb.push(
        "shell -tt getevent",
        "/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000002",
    );
    assert!(guard.foreign_input().is_none(), "disarmed");
    assert!(format!("{guard:?}").contains("GeteventGuard"));
    // A watch that ended before it named a device cannot arm.
    let ended = ScriptedAdb::new().ending_stream("shell -tt getevent", Vec::new());
    let stream = ended
        .device()
        .open(
            &AdbCommand::ShellTerminal(getevent::WATCH.to_owned()),
            Framing::Lines,
        )
        .expect("a stream");
    match GeteventGuard::new(stream).arm() {
        Err(Error::Lane { message, .. }) => {
            assert!(message.starts_with(flui_driver::lane::android::guard::STOPPED_WATCHING));
        }
        other => panic!("{other:?}"),
    }
}

/// A slow scripted command answers once it has run, within its deadline; past it, it is ended at
/// the deadline and times out with a `may_have_run` effect, as the host's `adb` does.
#[test]
fn a_slow_scripted_command_answers_within_its_deadline_and_times_out_past_it() {
    let adb = ScriptedAdb::new()
        .reply_once_after(
            "shell slow",
            AdbReply::new(0, "done"),
            Duration::from_secs(5),
        )
        .reply_once_after(
            "shell quick",
            AdbReply::new(0, "done"),
            Duration::from_millis(20),
        );
    let mut device = adb.device();
    let started = Instant::now();
    let refused = device
        .run(
            &AdbCommand::Shell("slow".to_owned()),
            64,
            Duration::from_millis(50),
        )
        .expect_err("the command outlives its deadline");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the deadline ends the command, not the device"
    );
    match &refused {
        Error::Timeout { effect, .. } => assert_eq!(
            effect.as_ref().map(|effect| effect.kind),
            Some(flui_driver::protocol::act::EffectKind::MayHaveRun)
        ),
        other => panic!("{other:?}"),
    }
    let reply = device
        .run(
            &AdbCommand::Shell("quick".to_owned()),
            64,
            Duration::from_secs(5),
        )
        .expect("the command answers within its deadline");
    assert_eq!(reply.text(), "done");
}

#[test]
fn a_guard_that_hears_no_device_within_its_wait_cannot_arm() {
    let adb = ScriptedAdb::new().stream("shell -tt getevent", Vec::new(), None);
    let stream = adb
        .device()
        .open(
            &AdbCommand::ShellTerminal(getevent::WATCH.to_owned()),
            Framing::Lines,
        )
        .expect("a stream");
    let started = Instant::now();
    let refused = GeteventGuard::new(stream).arm().expect_err("nothing named");
    assert!(matches!(refused, Error::Lane { .. }), "{refused}");
    assert!(started.elapsed() >= flui_driver::lane::android::guard::ARM_WITHIN);
}

#[test]
fn device_output_at_its_edges() {
    let listing = "* daemon not running; starting now at tcp:5037\n\
                   * daemon started successfully\n\
                   List of devices attached\n\
                   emulator-5554\tdevice\n";
    let entries = device::parse_devices(listing);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].serial, "emulator-5554");
    let unauthorized = device::parse_devices("R5\tunauthorized\n");
    match device::choose(&unauthorized, Some("R5")) {
        Err(Error::Lane {
            unavailable: true,
            message,
            ..
        }) => assert!(message.contains("accept the prompt"), "{message}"),
        other => panic!("{other:?}"),
    }
    // Only the two lines after the API level say whether it is an emulator.
    assert_eq!(device::parse_properties("34\n\n\n1\n").1, Isolation::Device);
    assert_eq!(
        device::parse_display("Physical size: 1080x0\nPhysical density: 420\n"),
        None
    );
    assert_eq!(
        device::parse_display("Physical size: 1080x2400\nPhysical density: 0\n"),
        None
    );
    let one = device::parse_display("Physical size: 1x1\nPhysical density: 1\n").expect("tiny");
    assert_eq!((one.physical, one.density), ((1, 1), 1));
    let name = |length: usize| "a".repeat(length);
    assert!(device::check_component(&name(255), "id").is_ok());
    assert!(device::check_component(&name(256), "id").is_err());
    assert_eq!(device::INPUT_USAGE, "input help 2>&1");
    assert_eq!(focus_of("  mCurrentFocus=Window{1 u0 }\n"), None);
    assert_eq!(
        focus_of("  mCurrentFocus=Window{9e u0 Application Error: x}\n")
            .map(|focus| focus.title)
            .as_deref(),
        Some("Application Error: x"),
        "a title with spaces is read whole"
    );
    assert_eq!(
        focus_of("  mCurrentFocus=Window{4f u0 a/.B} 0x1\n")
            .map(|focus| (focus.hash.to_string(), focus.title)),
        Some(("4f".to_owned(), "a/.B".to_owned())),
        "what follows the window's name is not its title"
    );
    assert_eq!(
        focus_of(FOCUSED).map(|focus| focus.title).as_deref(),
        Some("dev.flui.scenario/android.app.NativeActivity")
    );
}

#[test]
fn a_failure_is_read_from_its_first_words_and_a_vanished_device_is_unavailable() {
    assert!(
        AdbReply::new(0, "")
            .stderr("error: closed")
            .failure(Printed::Data)
            .is_some()
    );
    assert_eq!(
        AdbReply::new(0, "")
            .stderr("the last word is error:")
            .failure(Printed::Data),
        None
    );
    // A press the device refused: unavailable when the device went, a lane failure otherwise.
    let pressed = |said: &str| {
        let adb = scripted(true, true).reply_once(
            "shell input touchscreen motionevent DOWN",
            AdbReply::new(1, "").stderr(said),
        );
        let mut lane = connected(&adb);
        lane::send_guarded(&mut lane, &tap(PointerDevice::Touch, AT)).expect_err("refused")
    };
    for said in [
        "error: device offline",
        "error: device 'x' not found",
        "error: no devices/emulators found",
        "error: device unauthorized.",
        "error: device still authorizing",
    ] {
        assert!(
            matches!(
                pressed(said),
                Error::Lane {
                    unavailable: true,
                    ..
                }
            ),
            "{said}"
        );
    }
    assert!(matches!(
        pressed("Error: something else"),
        Error::Lane {
            unavailable: false,
            ..
        }
    ));
    let long = "z".repeat(400);
    let failure = AdbReply::new(3, "")
        .stderr(long)
        .failure(Printed::Data)
        .expect("a failure");
    assert_eq!(
        failure.matches('z').count(),
        300,
        "a message is cut at 300 characters"
    );
    assert!(failure.ends_with('…'));
    let exact = AdbReply::new(3, "")
        .stderr("y".repeat(300))
        .failure(Printed::Data)
        .expect("a failure");
    assert!(!exact.ends_with('…'), "300 characters are kept whole");
    assert_eq!(
        AdbReply::new(2, "").failure(Printed::Data).as_deref(),
        Some("it exited with 2")
    );
    assert!(AdbCommand::Shell(String::new()).names_device());
    assert!(!AdbCommand::Devices.names_device());
    assert_eq!(
        AdbCommand::ExecOut("screencap".to_owned()).to_string(),
        "exec-out screencap"
    );
    let mut splitter = Splitter::new(Framing::JsonObjects);
    let records: Vec<String> = br#"{"a":1}{"b":"\\"}"#
        .iter()
        .filter_map(|&byte| splitter.push(byte).expect("in bounds"))
        .collect();
    assert_eq!(
        records,
        vec![r#"{"a":1}"#.to_owned(), r#"{"b":"\\"}"#.to_owned()]
    );
    // A string that opens with an escaped quote ends at its own closing quote.
    let mut splitter = Splitter::new(Framing::JsonObjects);
    let records: Vec<String> = br#"{"\"x":"}"}"#
        .iter()
        .filter_map(|&byte| splitter.push(byte).expect("in bounds"))
        .collect();
    assert_eq!(records, vec![r#"{"\"x":"}"}"#.to_owned()]);
}

#[test]
fn a_plan_keeps_each_pointer_and_its_contact_apart() {
    let abilities = abilities();
    let mut held = HeldInput::new();
    let mut events = tap(PointerDevice::Touch, AT);
    events.truncate(2);
    run(&mut held, &events, &abilities).expect("a contact");
    // Another pointer's move while the contact is down only places that pointer.
    let other = OsEvent::PointerMove {
        pointer: "other".to_owned(),
        device: PointerDevice::Mouse,
        to: DevicePoint::new(400, 700),
    };
    assert_eq!(held.plan(&other, &abilities), Ok(Vec::new()));
    // The contact's own move drags it, and the release lifts it where it went.
    let drag = OsEvent::PointerMove {
        pointer: "scenario.touch".to_owned(),
        device: PointerDevice::Touch,
        to: DevicePoint::new(350, 650),
    };
    let moved = run(&mut held, &[drag], &abilities).expect("a drag");
    assert_eq!(
        moved,
        vec![Command::Motion {
            pointer: "scenario.touch".to_owned(),
            source: MotionSource::Touchscreen,
            action: MotionAction::Move,
            at: DevicePoint::new(350, 650),
        }]
    );
    assert_eq!(
        moved[0].line().as_deref(),
        Some("input touchscreen motionevent MOVE 350 650")
    );
    // Another pointer's release lifts nothing.
    let other_up = OsEvent::PointerUp {
        pointer: "other".to_owned(),
        device: PointerDevice::Mouse,
        button: PointerButton::Primary,
    };
    assert_eq!(held.plan(&other_up, &abilities), Ok(Vec::new()));
    let up = OsEvent::PointerUp {
        pointer: "scenario.touch".to_owned(),
        device: PointerDevice::Touch,
        button: PointerButton::Primary,
    };
    assert_eq!(
        held.plan(&up, &abilities),
        Ok(vec![Command::Motion {
            pointer: "scenario.touch".to_owned(),
            source: MotionSource::Touchscreen,
            action: MotionAction::Up,
            at: DevicePoint::new(350, 650),
        }])
    );
    // A pen's release while it only hovers lifts nothing.
    let mut pen = HeldInput::new();
    run(&mut pen, &tap(PointerDevice::Pen, AT)[..1], &abilities).expect("a hover");
    let lift = OsEvent::PointerUp {
        pointer: "scenario.pen".to_owned(),
        device: PointerDevice::Pen,
        button: PointerButton::Primary,
    };
    assert_eq!(pen.plan(&lift, &abilities), Ok(Vec::new()));
    // A mouse press goes through the mouse.
    let mut mouse = HeldInput::new();
    let pressed =
        run(&mut mouse, &tap(PointerDevice::Mouse, AT)[..2], &abilities).expect("a click");
    assert_eq!(
        pressed[0].line().as_deref(),
        Some("input mouse motionevent DOWN 300 600")
    );
    assert_eq!(
        mouse.held_by(Part::Contact),
        vec!["the mouse button of scenario.mouse"]
    );
}

#[test]
fn a_plan_refuses_what_input_cannot_carry_whatever_the_order() {
    let abilities = abilities();
    let refused = |events: &[OsEvent], abilities: &Abilities| {
        run(&mut HeldInput::new(), events, abilities).expect_err("refused")
    };
    // A pen pressed under an id a touch placed, on a device without `uinput`.
    let mut without = abilities;
    without.uinput = false;
    let same_id = [
        OsEvent::PointerMove {
            pointer: "p".to_owned(),
            device: PointerDevice::Touch,
            to: AT,
        },
        OsEvent::PointerDown {
            pointer: "p".to_owned(),
            device: PointerDevice::Pen,
            button: PointerButton::Primary,
        },
    ];
    assert!(matches!(
        refused(&same_id, &without),
        Refusal::NeedsUinput { .. }
    ));
    // A press before its pointer went anywhere, or before the window was located.
    let press = OsEvent::PointerDown {
        pointer: "q".to_owned(),
        device: PointerDevice::Touch,
        button: PointerButton::Primary,
    };
    assert!(matches!(
        refused(&[press], &abilities),
        Refusal::UnsentInput { .. }
    ));
    let mut unlocated = abilities;
    unlocated.display = None;
    assert!(matches!(
        refused(&tap(PointerDevice::Touch, AT)[..2], &unlocated),
        Refusal::WindowGone
    ));
    // A scroll with a key held, or without `input scroll`.
    let scroll = OsEvent::Scroll {
        at: AT,
        delta_x: 80,
        delta_y: -20,
    };
    assert!(matches!(
        refused(
            &[OsEvent::KeyDown("Shift".to_owned()), scroll.clone()],
            &abilities
        ),
        Refusal::UnsentInput { .. }
    ));
    let mut no_scroll = abilities;
    no_scroll.input.pointer_scroll = false;
    assert!(matches!(
        refused(std::slice::from_ref(&scroll), &no_scroll),
        Refusal::NeedsCommand {
            command: "scroll",
            ..
        }
    ));
    assert_eq!(
        run(&mut HeldInput::new(), &[scroll], &abilities),
        Ok(vec![Command::Scroll {
            at: AT,
            vertical: 0.5,
            horizontal: 2.0,
        }])
    );
    // A key pressed twice before its release is one key.
    assert_eq!(
        run(
            &mut HeldInput::new(),
            &[
                OsEvent::KeyDown("a".to_owned()),
                OsEvent::KeyDown("a".to_owned()),
                OsEvent::KeyUp("a".to_owned()),
            ],
            &abilities
        )
        .map(|commands| commands.len()),
        Ok(1)
    );
    // A key a name has no code for, on its release too.
    assert!(matches!(
        refused(&[OsEvent::KeyUp("Hyper".to_owned())], &abilities),
        Refusal::UnknownKey { .. }
    ));
    // More pointers than a sequence may name.
    let many: Vec<OsEvent> = (0..=flui_driver::lane::android::held::MAX_POINTERS)
        .map(|index| OsEvent::PointerMove {
            pointer: format!("p{index}"),
            device: PointerDevice::Touch,
            to: AT,
        })
        .collect();
    assert!(matches!(
        refused(&many, &abilities),
        Refusal::UnsentInput { .. }
    ));
    let enough = &many[..flui_driver::lane::android::held::MAX_POINTERS];
    assert!(run(&mut HeldInput::new(), enough, &abilities).is_ok());
    // A pointer moved twice is one pointer, at where it went last.
    let mut held = HeldInput::new();
    let twice = [
        OsEvent::PointerMove {
            pointer: "t".to_owned(),
            device: PointerDevice::Touch,
            to: DevicePoint::new(1, 1),
        },
        OsEvent::PointerMove {
            pointer: "t".to_owned(),
            device: PointerDevice::Touch,
            to: AT,
        },
        OsEvent::PointerDown {
            pointer: "t".to_owned(),
            device: PointerDevice::Touch,
            button: PointerButton::Primary,
        },
    ];
    let commands = run(&mut held, &twice, &abilities).expect("a press");
    assert!(matches!(commands.last(), Some(Command::Motion { at, .. }) if *at == AT));
}

#[test]
fn modifiers_released_after_a_chord_send_nothing_and_the_next_tap_is_alone() {
    let abilities = abilities();
    let down = |name: &str| OsEvent::KeyDown(name.to_owned());
    let up = |name: &str| OsEvent::KeyUp(name.to_owned());
    let mut held = HeldInput::new();
    let first = run(
        &mut held,
        &[
            down("Control"),
            down("Shift"),
            down("a"),
            up("a"),
            up("Shift"),
            up("Control"),
        ],
        &abilities,
    )
    .expect("a chord");
    assert_eq!(first.len(), 1, "{first:?}");
    // A modifier tapped after the chord is a tap of its own.
    let second = run(&mut held, &[down("Shift"), up("Shift")], &abilities).expect("a tap");
    assert_eq!(second, vec![Command::Key(keys::SHIFT)]);
    // A modifier released while another is held sends nothing.
    let mut held = HeldInput::new();
    let both = run(
        &mut held,
        &[down("Control"), down("Shift"), up("Shift")],
        &abilities,
    )
    .expect("held");
    assert_eq!(both, Vec::new());
}

#[test]
fn what_each_part_holds_is_named_and_released_on_its_own() {
    let abilities = abilities();
    let mut held = HeldInput::new();
    run(&mut held, &tap(PointerDevice::Pen, AT)[..2], &abilities).expect("a pen down");
    assert_eq!(held.held_by(Part::Pen), vec!["the pen, touching"]);
    assert_eq!(held.held_by(Part::Contact), Vec::<String>::new());
    assert!(held.pen_in_range());
    assert_eq!(held.release_plan_of(Part::Contact), Vec::new());
    assert_eq!(
        held.release_plan_of(Part::Pen),
        vec![
            Command::Pen {
                change: PenChange::Lift,
                at: AT
            },
            Command::Pen {
                change: PenChange::Leave,
                at: AT
            },
        ]
    );
    // A move of the pen keeps it touching; a lift keeps it in range.
    held.took(&Command::Pen {
        change: PenChange::Move,
        at: DevicePoint::new(310, 610),
    });
    assert_eq!(held.held_by(Part::Pen), vec!["the pen, touching"]);
    held.took(&Command::Pen {
        change: PenChange::Lift,
        at: DevicePoint::new(310, 610),
    });
    assert_eq!(held.held_by(Part::Pen), vec!["the pen, in range"]);
    held.pen_device_gone();
    assert_eq!(held.held_by(Part::Pen), Vec::<String>::new());
    // The end of a sequence forgets where each pointer is.
    let mut placed = HeldInput::new();
    run(&mut placed, &tap(PointerDevice::Touch, AT)[..1], &abilities).expect("placed");
    placed.forget_plans();
    assert!(
        run(
            &mut placed,
            &tap(PointerDevice::Touch, AT)[1..2],
            &abilities
        )
        .is_err(),
        "the pointer is forgotten"
    );
    // It forgets a modifier held, so a press is no longer refused for it.
    let mut keys_held = HeldInput::new();
    run(
        &mut keys_held,
        &[OsEvent::KeyDown("Shift".to_owned())],
        &abilities,
    )
    .expect("held here");
    keys_held.forget_plans();
    assert!(
        run(&mut keys_held, &tap(PointerDevice::Touch, AT), &abilities).is_ok(),
        "no key is held"
    );
    // It forgets a key held, so another key is no second key at once.
    let mut pending = HeldInput::new();
    run(
        &mut pending,
        &[OsEvent::KeyDown("a".to_owned())],
        &abilities,
    )
    .expect("held here");
    pending.forget_plans();
    assert!(
        run(
            &mut pending,
            &[OsEvent::KeyDown("b".to_owned())],
            &abilities
        )
        .is_ok()
    );
    // It forgets that a chord used its modifiers, so a modifier tapped next is a tap.
    let mut used = HeldInput::new();
    run(
        &mut used,
        &[
            OsEvent::KeyDown("Control".to_owned()),
            OsEvent::KeyDown("a".to_owned()),
            OsEvent::KeyUp("a".to_owned()),
        ],
        &abilities,
    )
    .expect("a chord");
    used.forget_plans();
    assert_eq!(
        run(
            &mut used,
            &[
                OsEvent::KeyDown("Shift".to_owned()),
                OsEvent::KeyUp("Shift".to_owned())
            ],
            &abilities
        ),
        Ok(vec![Command::Key(keys::SHIFT)])
    );
    // A modifier pressed twice is held once.
    let a = match keys::resolve("a") {
        keys::KeyName::Key(code) => code,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        run(
            &mut HeldInput::new(),
            &[
                OsEvent::KeyDown("Control".to_owned()),
                OsEvent::KeyDown("Control".to_owned()),
                OsEvent::KeyDown("a".to_owned()),
                OsEvent::KeyUp("a".to_owned()),
            ],
            &abilities
        ),
        Ok(vec![Command::Chord(vec![keys::CONTROL, a])])
    );
    // Only a press, a key, text or a scroll asks for the focus.
    let needs: Vec<bool> = [
        Command::Pen {
            change: PenChange::Press,
            at: AT,
        },
        Command::Pen {
            change: PenChange::Enter,
            at: AT,
        },
        Command::Motion {
            pointer: "p".to_owned(),
            source: MotionSource::Mouse,
            action: MotionAction::Move,
            at: AT,
        },
        Command::Scroll {
            at: AT,
            vertical: 1.0,
            horizontal: 0.0,
        },
    ]
    .iter()
    .map(Command::needs_focus)
    .collect();
    assert_eq!(needs, [true, false, false, true]);
    assert_eq!(
        Command::Pen {
            change: PenChange::Press,
            at: AT
        }
        .line(),
        None
    );
}

#[test]
fn the_rules_name_what_takes_each_refusal() {
    let display = Display {
        width: 1080,
        height: 2400,
        density: 427,
    };
    // 48 dp at 427 dpi is 128.1 pixels, taken as 129.
    assert_eq!(display.gesture_edge(), 129);
    assert_eq!(
        Display {
            density: 640,
            ..display
        }
        .gesture_edge(),
        192
    );
    let edge = |x, y| safety::gesture_edge(DevicePoint::new(x, y), &display);
    assert_eq!(
        edge(500, 0),
        Some("the swipe that opens the notification shade and quick settings")
    );
    assert_eq!(
        edge(500, 2399),
        Some("the swipe to home and the recent apps")
    );
    assert_eq!(edge(0, 1200), Some("the back gesture"));
    assert_eq!(edge(1079, 1200), Some("the back gesture"));
    let a = match keys::resolve("a") {
        keys::KeyName::Key(code) => code,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        safety::system_chord(&[keys::META], a),
        Some("Android's window manager, whose system shortcuts all hold Meta,")
    );
    assert_eq!(
        safety::system_chord(&[keys::ALT], keys::TAB),
        Some("the app switcher")
    );
    assert_eq!(safety::system_chord(&[keys::ALT], a), None);
    assert_eq!(safety::system_chord(&[keys::CONTROL], keys::TAB), None);
    assert_eq!(
        safety::system_chord(&[keys::CONTROL], keys::SPACE),
        Some("LANGUAGE_SWITCH, which switches the keyboard layout (Control and Space)")
    );
    assert_eq!(
        safety::chord_name(&[keys::CONTROL, keys::SHIFT], a),
        "CTRL_LEFT+SHIFT_LEFT+A"
    );
    assert!(matches!(
        Refusal::WindowGone.into_error(),
        Error::Gone {
            kind: Some(flui_driver::protocol::error::HandleKind::Window),
            handle: None,
            exit: None,
            ..
        }
    ));
    assert!(matches!(
        Refusal::ForeignIsolation {
            isolation: Isolation::Xvfb
        }
        .into_error(),
        Error::InvalidArgument { .. }
    ));
}

#[test]
fn the_lane_answers_its_capabilities_from_the_device() {
    let older = USAGE
        .lines()
        .filter(|line| !line.contains("motionevent"))
        .collect::<Vec<_>>()
        .join("\n");
    let adb = scripted(true, false).reply_once("shell input help", AdbReply::new(0, older));
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let capabilities = lane.capabilities();
    assert_eq!(capabilities.pointers, Vec::<PointerDevice>::new());
    assert!(capabilities.keys);
    assert!(format!("{lane:?}").contains(PACKAGE));
    lane.locate_window(&crate::android_lane::placement())
        .expect("the window is found");
    let refused =
        lane::send_guarded(&mut lane, &tap(PointerDevice::Touch, AT)).expect_err("no motionevent");
    assert_eq!(capability(&refused), Some("android_input_motionevent"));
}

#[test]
fn a_sequence_refused_late_sends_nothing_at_all() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    let mut events = tap(PointerDevice::Touch, AT);
    events.push(OsEvent::KeyDown("GoHome".to_owned()));
    assert!(matches!(
        lane.check(&events),
        Err(Error::NotSupported { .. })
    ));
    let refused = lane::send_guarded(&mut lane, &events).expect_err("home is refused");
    assert_eq!(capability(&refused), Some("os_shell_hotkey"));
    assert_eq!(
        inputs(&adb),
        Vec::<String>::new(),
        "the tap before it was not sent"
    );
    // A second contact the plan only sees once the first is down.
    let two = [
        tap(PointerDevice::Touch, AT)[..2].to_vec(),
        tap(PointerDevice::Mouse, AT)[..2].to_vec(),
    ]
    .concat();
    assert!(lane.check(&two).is_err());
}

#[test]
fn a_pause_is_waited_out_and_cut_short_by_the_persons_input() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    let started = Instant::now();
    lane.send(&OsEvent::Pause(Duration::from_millis(120)))
        .expect("a pause");
    assert!(started.elapsed() >= Duration::from_millis(120));
    let device = scripted(false, true);
    let mut lane = connected(&device);
    lane.guard().expect("guarded").arm().expect("armed");
    device.push(
        "shell -tt getevent",
        "/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000045",
    );
    let started = Instant::now();
    let interrupted = lane
        .send(&OsEvent::Pause(Duration::from_secs(5)))
        .expect_err("someone touched it");
    assert!(
        matches!(interrupted, Error::InterruptedByUser { .. }),
        "{interrupted}"
    );
    assert!(started.elapsed() < Duration::from_secs(4), "cut short");
}

#[test]
fn a_pen_whose_device_failed_registers_a_new_one_and_releases_nothing_through_the_old() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    let events = tap(PointerDevice::Pen, AT);
    lane.send(&events[0]).expect("the pen enters");
    lane.send(&events[1]).expect("the pen presses");
    adb.end_stream("shell uinput -");
    let opened = |adb: &ScriptedAdb| {
        adb.log()
            .iter()
            .filter(|line| line.as_str() == "shell uinput -")
            .count()
    };
    let before = opened(&adb);
    lane.send(&events[3]).expect_err("the device is gone");
    lane.release_all()
        .expect("the kernel took its contact away");
    assert_eq!(
        opened(&adb),
        before,
        "the release opened no device to lift a pen that is gone"
    );
    lane::send_guarded(&mut lane, &events).expect("a new device takes the pen");
    assert_eq!(opened(&adb), before + 1);
}

#[test]
fn a_late_answer_to_an_earlier_sync_does_not_answer_the_next() {
    /// Answers the registration's sync twice and no other.
    fn stale(line: &str) -> Vec<String> {
        let answers = answer_syncs(line);
        if line.contains("\"syncToken\":\"1\"") {
            answers.iter().chain(answers.iter()).cloned().collect()
        } else {
            Vec::new()
        }
    }
    // Each device answers its registration's sync twice and no later one.
    let adb = scripted_with(true, Some(stale));
    let mut lane = connected(&adb);
    let failed = lane
        .send(&tap(PointerDevice::Pen, AT)[0])
        .expect_err("no answer to the second sync");
    assert!(matches!(failed, Error::Lane { .. }), "{failed}");
}

#[test]
fn a_probe_whose_command_ends_at_once_finds_no_uinput() {
    let adb = scripted_with(true, None)
        .ending_stream("shell uinput -", Vec::new())
        .reply(
            "shell cat /proc/bus/input/devices",
            AdbReply::new(0, PROBED),
        );
    let lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    assert!(
        !lane.capabilities().pointers.contains(&PointerDevice::Pen),
        "a device the kernel lists from before is not the probe's"
    );
}

#[test]
fn a_lane_that_started_no_app_stops_nothing_when_it_ends() {
    let adb = scripted(true, true);
    drop(connected(&adb));
    assert_eq!(adb.commands("shell 'am'"), Vec::<String>::new());
    assert_eq!(adb.commands("forward"), Vec::<String>::new());
    // A forward `adb` refused for another reason is no rule of the lane's: nothing is removed, and
    // the app it started is still stopped.
    let adb = scripted(true, true)
        .reply("shell 'am'", AdbReply::new(0, ""))
        .reply("forward --remove", AdbReply::new(0, ""))
        .reply(
            "forward --no-rebind",
            AdbReply::new(1, "").stderr("error: device offline"),
        );
    let app = AndroidApp::try_new(PACKAGE, "a.B").expect("an app");
    assert_eq!(app.package(), PACKAGE);
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    assert!(lane.start(&app).is_err());
    assert_eq!(adb.commands("forward --no-rebind").len(), 1);
    drop(lane);
    assert_eq!(adb.commands("shell 'am' 'force-stop'").len(), 2);
    assert_eq!(adb.commands("forward --remove"), Vec::<String>::new());
    // A forward that timed out may have been made: it is removed.
    let adb = scripted(true, true)
        .reply("shell 'am'", AdbReply::new(0, ""))
        .reply("forward --remove", AdbReply::new(0, ""))
        .timeout_once("forward --no-rebind");
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    assert!(matches!(
        lane.start(&app),
        Err(Error::Timeout {
            awaited: Awaited::Host,
            ..
        })
    ));
    let local = local_spec(&adb.commands("forward --no-rebind")[0]);
    drop(lane);
    assert_eq!(
        adb.commands("forward --remove"),
        vec![format!("forward --remove {local}")]
    );
}

/// The local spec (`tcp:N`) of a logged `forward --no-rebind tcp:N localabstract:…`.
fn local_spec(forward: &str) -> String {
    forward
        .split_whitespace()
        .nth(2)
        .expect("the local spec")
        .to_owned()
}

#[test]
fn a_forward_never_takes_over_another_sessions_port() {
    let taken = |said: &str| AdbReply::new(1, "").stderr(said);
    // The first two ports are another session's: the lane asks for a fresh one each time and
    // removes only the rule it made.
    let adb = scripted(true, true)
        .reply("shell 'am'", AdbReply::new(0, ""))
        .reply("forward --remove", AdbReply::new(0, ""))
        .reply_once(
            "forward --no-rebind",
            taken("error: cannot rebind existing socket"),
        )
        .reply_once(
            "forward --no-rebind",
            taken("error: cannot bind listener: Address already in use"),
        )
        .reply("forward --no-rebind", AdbReply::new(0, ""));
    let app = AndroidApp::try_new(PACKAGE, "a.B").expect("an app");
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let started = lane.start(&app).expect("the third port is free");
    let asked = adb.commands("forward --no-rebind");
    assert_eq!(asked.len(), 3, "{asked:?}");
    let made = local_spec(&asked[2]);
    let flui_driver::Endpoint::Forwarded(address) = started.endpoint else {
        panic!("a forwarded endpoint");
    };
    assert_eq!(made, format!("tcp:{}", address.port()));
    drop(lane);
    assert_eq!(
        adb.commands("forward --remove"),
        vec![format!("forward --remove {made}")],
        "the refused ports' rules are another session's"
    );
    // Every port taken: the lane gives up after its attempts and removes nothing.
    let adb = scripted(true, true)
        .reply("shell 'am'", AdbReply::new(0, ""))
        .reply("forward --remove", AdbReply::new(0, ""))
        .reply(
            "forward --no-rebind",
            taken("error: cannot rebind existing socket"),
        );
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let refused = lane.start(&app).expect_err("every port taken");
    assert!(refused.to_string().contains("taken"), "{refused}");
    assert_eq!(
        adb.commands("forward --no-rebind").len(),
        flui_driver::lane::android::FORWARD_ATTEMPTS
    );
    drop(lane);
    assert_eq!(adb.commands("forward --remove"), Vec::<String>::new());
}

#[test]
fn a_rotated_display_turns_the_gesture_edges_with_it() {
    let turned = WINDOWS
        .replace("mRotation=0", "mRotation=1")
        .replace("frame=[0,60][1080,2400]", "frame=[0,0][2340,1080]");
    assert_ne!(turned, WINDOWS);
    // Turned when the lane locates the window and still when it checks the press.
    let adb = scripted(true, true)
        .reply_once("shell dumpsys window |", AdbReply::new(0, turned.clone()))
        .reply_once("shell dumpsys window |", AdbReply::new(0, turned));
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let window: flui_driver::protocol::tree::WindowId = "w1".parse().expect("a window");
    let placement = flui_driver::protocol::session::Placement::new(
        window,
        flui_driver::protocol::geometry::LogicalSize::new(1170.0, 540.0),
        2.0,
    );
    lane.locate_window(&placement).expect("the turned window");
    // At (1500, 600): inside a display 2400 wide, past the edge of one 1080 wide.
    lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(1500, 600)),
    )
    .expect("inside the turned display");
}

#[test]
fn an_app_in_front_without_a_surface_of_the_windows_size_is_no_refusal_of_focus() {
    let adb = scripted(true, true);
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let window: flui_driver::protocol::tree::WindowId = "w1".parse().expect("a window");
    let other_size = flui_driver::protocol::session::Placement::new(
        window,
        flui_driver::protocol::geometry::LogicalSize::new(100.0, 100.0),
        2.0,
    );
    match lane.locate_window(&other_size) {
        Err(Error::Lane { message, .. }) => {
            assert!(message.contains("200 x 200"), "{message}");
        }
        other => panic!("expected the size named, got {other:?}"),
    }
}

#[test]
fn a_sequence_that_ends_with_a_key_down_is_refused_naming_it() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    for (events, named) in [
        (vec![OsEvent::KeyDown("a".to_owned())], "KEYCODE_A"),
        (
            vec![OsEvent::KeyDown("Shift".to_owned())],
            "KEYCODE_SHIFT_LEFT",
        ),
        (
            vec![
                OsEvent::KeyDown("Control".to_owned()),
                OsEvent::KeyDown("a".to_owned()),
                OsEvent::KeyUp("a".to_owned()),
            ],
            "KEYCODE_CTRL_LEFT",
        ),
    ] {
        let refused = lane::send_guarded(&mut lane, &events).expect_err("a key left down");
        assert_eq!(capability(&refused), Some("os_held_key"), "{refused}");
        assert!(refused.to_string().contains(named), "{refused}");
    }
    assert_eq!(inputs(&adb), Vec::<String>::new(), "nothing was sent");
    // A sequence that releases what it pressed is taken.
    lane::send_guarded(
        &mut lane,
        &[
            OsEvent::KeyDown("a".to_owned()),
            OsEvent::KeyUp("a".to_owned()),
        ],
    )
    .expect("a tap");
}

#[test]
fn a_key_an_aborted_sequence_left_held_is_forgotten_with_it() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    // Sent event by event, as a run cut short leaves it: the key is held here and never sent.
    lane.send(&OsEvent::KeyDown("Shift".to_owned()))
        .expect("held here alone");
    lane.release_all().expect("nothing is down on the device");
    assert_eq!(inputs(&adb), Vec::<String>::new());
    lane::send_guarded(&mut lane, &tap(PointerDevice::Touch, AT))
        .expect("the next sequence holds no key");
}
