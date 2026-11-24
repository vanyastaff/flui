//! The Android lane on every host: its tables and parsers against what each `adb` command prints
//! (the fixtures in `tests/android/` follow the AOSP sources that print them: `InputShellCommand`'s
//! usage, `getevent.c`'s lines, `uiautomator`'s dump), and the lane itself against a scripted
//! device (`ScriptedAdb`): what it sends for each event, what it refuses before any event, and
//! what it releases on every exit.

use std::time::Duration;

use flui_driver::coordinates::DevicePoint;
use flui_driver::lane::android::adb::{self, AdbReply, Printed, Splitter};
use flui_driver::lane::android::device::{self, DeviceEntry};
use flui_driver::lane::android::getevent::{self, Line, Watch};
use flui_driver::lane::android::held::{
    Abilities, Command, HeldInput, MotionAction, MotionSource, Part, PenChange,
};
use flui_driver::lane::android::keys::{self, KeyName, TextStroke};
use flui_driver::lane::android::safety::{self, Display, Refusal};
use flui_driver::lane::android::screencap::{self, Raw};
use flui_driver::lane::android::uiautomator::{Bounds, UiDump};
use flui_driver::lane::android::uinput;
use flui_driver::lane::android::{AndroidApp, AndroidLane};
use flui_driver::lane::{self, Lane as _, PositionFidelity};
use flui_driver::protocol::geometry::LogicalSize;
use flui_driver::protocol::input::{PointerButton, PointerDevice};
use flui_driver::protocol::session::{Isolation, Placement};
use flui_driver::protocol::tree::WindowId;
use flui_driver::steps::OsEvent;
use flui_driver::testing::{Responder, ScriptedAdb};
use flui_driver::{Awaited, Endpoint, Error};

pub(crate) const PACKAGE: &str = "dev.flui.scenario";
pub(crate) const USAGE: &str = include_str!("android/input_usage.txt");
pub(crate) const WINDOW: &str = include_str!("android/window.xml");
/// What `dumpsys window` prints of the windows, as the lane's line keeps it (Android 14's
/// `DisplayRotation` and `WindowState` dumps): the app's window below a 60-pixel status bar.
pub(crate) const WINDOWS: &str = include_str!("android/windows.txt");

/// [`WINDOWS`] with the app's window `width` by `height` pixels at the display's top left: an
/// edge-to-edge window.
pub(crate) fn windows_edge_to_edge(width: i32, height: i32) -> String {
    WINDOWS.replace(
        "frame=[0,60][1080,2400]",
        &format!("frame=[0,0][{width},{height}]"),
    )
}
pub(crate) const GETEVENT: &str = include_str!("android/getevent.txt");

/// The kernel's device list while the probe's device is registered.
pub(crate) const PROBED: &str = "I: Bus=0003 Vendor=18d1 Product=abcc Version=0001\n\
                      N: Name=\"FLUI Driver Probe\"\n\
                      P: Phys=\n";

/// The accessibility settings of a device whose user turned no service on.
pub(crate) const NO_SCREEN_READER: &str = "null\n0\n";

pub(crate) const FOCUSED: &str =
    "  mCurrentFocus=Window{4f2a1b u0 dev.flui.scenario/android.app.NativeActivity}\n";

/// `uinput`'s answer to each sync written to it.
pub(crate) fn answer_syncs(line: &str) -> Vec<String> {
    let Ok(command) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    if command["command"] != "sync" {
        return Vec::new();
    }
    vec![format!(
        "{{\"reason\":\"sync\",\"id\":{},\"syncToken\":{}}}",
        command["id"], command["syncToken"]
    )]
}

/// A device with the app in front, its `input` the fixture's: an emulator when `emulator`, a
/// device in someone's hands otherwise; its shell may open `uinput` when `uinput`.
pub(crate) fn scripted(emulator: bool, uinput: bool) -> ScriptedAdb {
    if uinput {
        scripted_with(emulator, Some(answer_syncs))
    } else {
        scripted_with(emulator, None).refuse_stream("shell uinput -")
    }
}

/// [`scripted`], its `uinput` devices answering what they are written with `uinput` (the kernel
/// then lists the probe's), or with no rule for `uinput` at all when `None`.
pub(crate) fn scripted_with(emulator: bool, uinput: Option<Responder>) -> ScriptedAdb {
    scripted_showing(emulator, uinput, WINDOW, WINDOWS)
}

/// [`scripted_with`], its window dumped as `window` and its windows listed as `windows`.
pub(crate) fn scripted_showing(
    emulator: bool,
    uinput: Option<Responder>,
    window: &str,
    windows: &str,
) -> ScriptedAdb {
    let adb = ScriptedAdb::new()
        .reply(
            "shell getprop",
            AdbReply::new(0, if emulator { "34\n\n1\n" } else { "34\n\n\n" }),
        )
        .reply("shell input help", AdbReply::new(0, USAGE))
        .reply(
            "shell wm size",
            AdbReply::new(0, "Physical size: 1080x2400\nPhysical density: 420\n"),
        )
        .reply("shell dumpsys window displays", AdbReply::new(0, windows))
        .reply("shell dumpsys window", AdbReply::new(0, FOCUSED))
        .reply(
            "shell settings get secure",
            AdbReply::new(0, NO_SCREEN_READER),
        )
        .reply("exec-out uiautomator", AdbReply::new(0, window))
        .reply("shell input ", AdbReply::new(0, ""))
        .stream(
            "shell -tt getevent",
            GETEVENT.lines().map(str::to_owned).collect(),
            None,
        );
    match uinput {
        Some(responder) => adb
            .reply(
                "shell cat /proc/bus/input/devices",
                AdbReply::new(0, PROBED),
            )
            .stream("shell uinput -", Vec::new(), Some(responder)),
        None => adb,
    }
}

/// The app's surface in the fixture's window: 1080 by 2340 pixels, 60 below the display's top.
pub(crate) fn placement() -> Placement {
    let window: WindowId = "w1".parse().expect("a window handle");
    Placement::new(window, LogicalSize::new(540.0, 1170.0), 2.0)
}

pub(crate) fn connected(adb: &ScriptedAdb) -> AndroidLane {
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    lane.locate_window(&placement())
        .expect("the window is found");
    lane
}

pub(crate) fn tap(device: PointerDevice, at: DevicePoint) -> Vec<OsEvent> {
    let pointer = format!("scenario.{}", device.name());
    vec![
        OsEvent::PointerMove {
            pointer: pointer.clone(),
            device,
            to: at,
        },
        OsEvent::PointerDown {
            pointer: pointer.clone(),
            device,
            button: PointerButton::Primary,
        },
        OsEvent::Pause(Duration::from_millis(1)),
        OsEvent::PointerUp {
            pointer,
            device,
            button: PointerButton::Primary,
        },
    ]
}

pub(crate) fn display() -> Display {
    Display {
        width: 1080,
        height: 2400,
        density: 420,
    }
}

pub(crate) fn abilities() -> Abilities {
    Abilities {
        input: device::parse_input_usage(USAGE),
        uinput: true,
        display: Some(display()),
    }
}

pub(crate) fn capability(error: &Error) -> Option<&str> {
    match error {
        Error::NotSupported { capability, .. } => capability.as_deref(),
        _ => None,
    }
}

/// Whether the logged command `line` sends input: every `input` command but the usage the lane
/// reads when it connects.
pub(crate) fn is_input(line: &str) -> bool {
    line.starts_with("shell input ") && !line.starts_with("shell input help")
}

/// The input commands the lane sent, in order.
pub(crate) fn inputs(adb: &ScriptedAdb) -> Vec<String> {
    adb.log()
        .into_iter()
        .filter(|line| is_input(line))
        .collect()
}

#[test]
fn a_key_name_maps_to_its_key_code_and_a_system_key_is_refused() {
    let rows: &[(&str, Option<(&str, u16)>)] = &[
        ("Enter", Some(("KEYCODE_ENTER", 66))),
        ("Tab", Some(("KEYCODE_TAB", 61))),
        ("Backspace", Some(("KEYCODE_DEL", 67))),
        ("Delete", Some(("KEYCODE_FORWARD_DEL", 112))),
        ("Escape", Some(("KEYCODE_ESCAPE", 111))),
        ("Control", Some(("KEYCODE_CTRL_LEFT", 113))),
        ("Meta", Some(("KEYCODE_META_LEFT", 117))),
        ("ArrowLeft", Some(("KEYCODE_DPAD_LEFT", 21))),
        ("Home", Some(("KEYCODE_MOVE_HOME", 122))),
        ("End", Some(("KEYCODE_MOVE_END", 123))),
        ("F12", Some(("KEYCODE_F12", 142))),
        ("a", Some(("KEYCODE_A", 29))),
        ("z", Some(("KEYCODE_Z", 54))),
        ("0", Some(("KEYCODE_0", 7))),
        ("9", Some(("KEYCODE_9", 16))),
        ("/", Some(("KEYCODE_SLASH", 76))),
        (" ", Some(("KEYCODE_SPACE", 62))),
        ("A", None),
        ("ü", None),
        ("Hyper", None),
    ];
    for &(name, expected) in rows {
        match (keys::resolve(name), expected) {
            (KeyName::Key(code), Some((key, value))) => {
                assert_eq!((code.name, code.code), (key, value), "{name}");
            }
            (KeyName::Unknown, None) => {}
            (other, _) => panic!("{name}: {other:?}"),
        }
    }
    for name in [
        "GoHome",
        "AppSwitch",
        "GoBack",
        "Power",
        "AudioVolumeUp",
        "Notification",
    ] {
        assert!(
            matches!(keys::resolve(name), KeyName::System(_)),
            "{name} leaves the app or changes the device"
        );
    }
}

#[test]
fn text_is_typed_as_ascii_runs_and_line_breaks_as_keys() {
    let run = |text: &str| TextStroke::Run(text.to_owned());
    assert_eq!(
        keys::text_strokes("Hi there\r\nok\tdone\rx\n"),
        Ok(vec![
            run("Hi there"),
            TextStroke::Key(keys::ENTER),
            run("ok"),
            TextStroke::Key(keys::TAB),
            run("done"),
            TextStroke::Key(keys::ENTER),
            run("x"),
            TextStroke::Key(keys::ENTER),
        ])
    );
    // `input text` reads `%s` as a space: a run never holds one.
    assert_eq!(
        keys::text_strokes("100%sure %"),
        Ok(vec![run("100%"), run("sure %")])
    );
    assert_eq!(keys::text_strokes("Grüße"), Err('ü'));
    assert_eq!(keys::text_strokes(""), Ok(Vec::new()));
}

#[test]
fn a_value_from_outside_the_lane_is_one_word_of_the_device_shell() {
    assert_eq!(adb::quote("plain"), "'plain'");
    assert_eq!(adb::quote("it's"), "'it'\\''s'");
    assert_eq!(adb::quote("a; reboot $(id) `x`"), "'a; reboot $(id) `x`'");
    assert_eq!(
        adb::shell_line(&["am", "force-stop", "x y"]),
        "'am' 'force-stop' 'x y'"
    );
    // Typed text goes through the same quoting.
    assert_eq!(
        Command::Text("a'b; c".to_owned()).line().as_deref(),
        Some("input text 'a'\\''b; c'")
    );
}

#[test]
fn a_stream_is_cut_into_lines_and_json_objects_and_a_record_is_bounded() {
    let cut = |framing: adb::Framing, bytes: &[u8]| -> Result<Vec<String>, String> {
        let mut splitter = Splitter::new(framing);
        let mut records = Vec::new();
        for &byte in bytes {
            if let Some(record) = splitter.push(byte)? {
                records.push(record);
            }
        }
        Ok(records)
    };
    assert_eq!(
        cut(adb::Framing::Lines, b"one\r\ntwo\nthree"),
        Ok(vec!["one".to_owned(), "two".to_owned()])
    );
    assert_eq!(
        cut(
            adb::Framing::JsonObjects,
            br#"noise{"a":"}{"}{"b":{"c":"\"}"}}trailing"#
        ),
        Ok(vec![
            r#"{"a":"}{"}"#.to_owned(),
            r#"{"b":{"c":"\"}"}}"#.to_owned()
        ])
    );
    let long = vec![b'x'; adb::MAX_RECORD + 1];
    assert!(
        cut(adb::Framing::Lines, &long).is_err(),
        "a record past the bound is refused"
    );
}

#[test]
fn a_command_fails_on_its_exit_code_or_on_what_the_device_printed() {
    for printed in [Printed::Status, Printed::Data] {
        assert_eq!(AdbReply::new(0, "ok").failure(printed), None);
        assert!(AdbReply::new(1, "").failure(printed).is_some());
        assert!(
            AdbReply::new(0, "")
                .stderr("java.lang.IllegalArgumentException: Unknown action: HOVER")
                .failure(printed)
                .is_some()
        );
    }
    // `am` and Java commands report some failures with exit 0 in their status.
    let said = AdbReply::new(0, "Error: Activity class {x/y} does not exist.\n");
    assert!(said.failure(Printed::Status).is_some());
    // Data may hold any text: a window whose button reads so is no failure.
    let dump = AdbReply::new(0, "<node text=\"NullPointerException\" />\nError: none\n");
    assert_eq!(dump.failure(Printed::Data), None);
}

#[test]
fn the_devices_listing_is_read_and_only_a_named_ready_device_is_taken() {
    let listing = "List of devices attached\n\
                   emulator-5554          device product:sdk_gphone64 model:sdk_gphone64_x86_64 transport_id:1\n\
                   R5CR10ABCDE            unauthorized usb:1-1 transport_id:2\n\
                   0123456789             offline\n";
    let entries = device::parse_devices(listing);
    let read: Vec<(&str, &str, Option<&str>)> = entries
        .iter()
        .map(|entry| {
            (
                entry.serial.as_str(),
                entry.state.as_str(),
                entry.model.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        read,
        vec![
            ("emulator-5554", "device", Some("sdk_gphone64_x86_64")),
            ("R5CR10ABCDE", "unauthorized", None),
            ("0123456789", "offline", None),
        ]
    );
    let unavailable = |result: Result<DeviceEntry, Error>| match result {
        Err(Error::Lane {
            unavailable: true, ..
        }) => {}
        other => panic!("expected the lane unavailable, got {other:?}"),
    };
    // No device is taken unless one is named, whatever adb lists.
    unavailable(device::choose(&entries, None));
    unavailable(device::choose(&entries, Some("R5CR10ABCDE")));
    unavailable(device::choose(&entries, Some("0123456789")));
    unavailable(device::choose(&entries, Some("absent")));
    assert_eq!(
        device::choose(&entries, Some("emulator-5554"))
            .expect("a ready device")
            .serial,
        "emulator-5554"
    );
    let bounded = "x device\n".repeat(device::MAX_DEVICES + 10);
    assert_eq!(device::parse_devices(&bounded).len(), device::MAX_DEVICES);
}

#[test]
fn what_the_device_prints_reads_into_its_facts() {
    assert_eq!(
        device::parse_properties("34\n\n1\n"),
        (34, Isolation::Emulator)
    );
    assert_eq!(
        device::parse_properties("30\n1\n\n"),
        (30, Isolation::Emulator)
    );
    assert_eq!(
        device::parse_properties("35\n\n\n"),
        (35, Isolation::Device)
    );
    assert_eq!(device::parse_properties("garbage"), (0, Isolation::Device));
    let display = |printed: &str| {
        device::parse_display(printed).map(|size| (size.physical, size.natural, size.density))
    };
    assert_eq!(
        display("Physical size: 1080x2400\nPhysical density: 420\n"),
        Some(((1080, 2400), (1080, 2400), 420))
    );
    assert_eq!(
        display(
            "Physical size: 1440x3200\nOverride size: 1080x2400\nPhysical density: 560\n\
             Override density: 420\n"
        ),
        Some(((1440, 3200), (1080, 2400), 420))
    );
    assert_eq!(display("Physical size: 0x2400\nPhysical density: 1"), None);
    let commands = |usage: &str| {
        let read = device::parse_input_usage(usage);
        (read.motion_event, read.key_combination, read.pointer_scroll)
    };
    assert_eq!(commands(USAGE), (true, true, true));
    let older: String = USAGE
        .lines()
        .filter(|line| {
            !line.contains("motionevent")
                && !line.contains("keycombination")
                && !line.contains("scroll")
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(commands(&older), (false, false, false));
    assert_eq!(
        device::parse_focus(FOCUSED).as_deref(),
        Some("dev.flui.scenario/android.app.NativeActivity")
    );
    assert_eq!(
        device::parse_focus("  mCurrentFocus=Window{9e u0 NotificationShade}\n").as_deref(),
        Some("NotificationShade")
    );
    assert_eq!(device::parse_focus("  mCurrentFocus=null\n"), None);
    assert!(device::check_component("dev.flui.scenario", "id").is_ok());
    assert!(device::check_component("a/.B$C", "id").is_ok());
    for bad in ["", "a b", "a;reboot", "a'b", "pkg\n"] {
        assert!(device::check_component(bad, "id").is_err(), "{bad:?}");
    }
}

#[test]
fn the_windows_give_the_rotation_and_the_focused_apps_frame() {
    let windows = device::parse_windows(WINDOWS).expect("the fixture lists windows");
    assert_eq!(windows.rotation, 0);
    assert_eq!(windows.windows.len(), 4);
    assert_eq!(
        windows.focus.as_deref(),
        Some("dev.flui.scenario/android.app.NativeActivity")
    );
    assert_eq!(
        windows.focused_frame(),
        Some(Bounds {
            left: 0,
            top: 60,
            right: 1080,
            bottom: 2400
        }),
        "the frame, never the parent or display frame before it"
    );
    // Android 10 and before write the frame as `mFrame=`, a turned display's rotation by name.
    let older = "    mRotation=ROTATION_90 mDeferredRotationPauseCount=0\n\
                 \x20 Window #2 Window{abc u0 dev.flui.scenario/android.app.NativeActivity}:\n\
                 \x20   Frames: containing=[0,0][2400,1080] parent=[0,0][2400,1080]\n\
                 \x20   mFrame=[60,0][2400,1080] last=[60,0][2400,1080]\n\
                 \x20 mCurrentFocus=Window{abc u0 dev.flui.scenario/android.app.NativeActivity}\n";
    let windows = device::parse_windows(older).expect("an older device's windows");
    assert_eq!(windows.rotation, 1);
    assert_eq!(
        windows.focused_frame(),
        Some(Bounds {
            left: 60,
            top: 0,
            right: 2400,
            bottom: 1080
        })
    );
    // No rotation, or one `Surface` does not have, is no reply the lane reads.
    for refused in [
        WINDOWS.replace("mRotation=0", "mRotation=4"),
        WINDOWS.replace("    mRotation=0 mDeferredRotationPauseCount=0\n", ""),
    ] {
        assert!(device::parse_windows(&refused).is_err(), "{refused}");
    }
    // The focus on a window the list does not hold, or none, frames nothing.
    let unlisted = WINDOWS.replace(
        "mCurrentFocus=Window{4f2a1b u0 dev.flui.scenario",
        "mCurrentFocus=Window{9999 u0 dev.flui.scenario.other",
    );
    assert_eq!(
        device::parse_windows(&unlisted)
            .expect("windows")
            .focused_frame(),
        None
    );
    // The windows read are bounded.
    let many = format!(
        "mRotation=0\n{}",
        "  Window #0 Window{1 u0 Toast}:\n    Frames: frame=[0,0][1,1]\n"
            .repeat(device::MAX_WINDOWS + 10)
    );
    assert_eq!(
        device::parse_windows(&many).expect("windows").windows.len(),
        device::MAX_WINDOWS
    );
    let dump = UiDump::parse(WINDOW).expect("the fixture is a dump");
    assert_eq!(dump.rotation, 0);
    assert_eq!(
        Bounds::parse("[1,2][3,4]").map(|bounds| bounds.width()),
        Some(2)
    );
    assert_eq!(Bounds::parse("[1,2][3]"), None);
    for refused in [
        "<hierarchy><node/></hierarchy>".to_owned(),
        "<window rotation=\"0\"/>".to_owned(),
        "<hierarchy rotation=\"7\"/>".to_owned(),
        "not xml".to_owned(),
        format!(
            "<hierarchy rotation=\"0\">{}{}</hierarchy>",
            "<node>".repeat(300),
            "</node>".repeat(300)
        ),
        "x".repeat(flui_driver::lane::android::uiautomator::MAX_DUMP + 1),
    ] {
        assert!(
            UiDump::parse(&refused).is_err(),
            "{}",
            &refused[..refused.len().min(40)]
        );
    }
}

/// A raw capture of `width` by `height` pixels in `format`, its header `header` bytes long.
/// A `width` by `height` PNG of `rgba`, four bytes a pixel.
pub(crate) fn png_of(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a PNG header");
    writer.write_image_data(rgba).expect("the PNG's pixels");
    writer.finish().expect("a PNG");
    bytes
}

pub(crate) fn raw_capture(width: u32, height: u32, format: u32, header: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    bytes.extend(format.to_le_bytes());
    if header == 16 {
        bytes.extend(0_u32.to_le_bytes());
    }
    for index in 0..width * height {
        let value = u8::try_from(index % 251).expect("below 251");
        bytes.extend([value, 1, 2, 3]);
    }
    bytes
}

#[test]
fn a_raw_capture_is_read_in_both_header_forms_and_refused_when_it_lies() {
    for header in [12, 16] {
        let Ok(Raw::Rgba {
            width,
            height,
            rgba,
        }) = screencap::parse_raw(raw_capture(3, 2, 1, header))
        else {
            panic!("a {header}-byte header reads");
        };
        assert_eq!((width, height, rgba.len()), (3, 2, 24));
        assert_eq!(&rgba[..4], &[0, 1, 2, 3]);
    }
    // RGBX: the fourth byte is no alpha.
    let Ok(Raw::Rgba { rgba, .. }) = screencap::parse_raw(raw_capture(2, 1, 2, 16)) else {
        panic!("RGBX reads");
    };
    assert_eq!(&rgba[..4], &[0, 1, 2, 255]);
    assert_eq!(
        screencap::parse_raw(raw_capture(2, 1, 4, 16)),
        Ok(Raw::OtherFormat(4))
    );
    let mut short = raw_capture(3, 2, 1, 16);
    short.pop();
    assert!(screencap::parse_raw(short).is_err());
    assert!(screencap::parse_raw(vec![1, 0, 0]).is_err());
    let huge = [
        (screencap::MAX_SIDE + 1).to_le_bytes(),
        1_u32.to_le_bytes(),
        1_u32.to_le_bytes(),
    ]
    .concat();
    assert!(screencap::parse_raw(huge).is_err(), "a side past the bound");
    assert!(screencap::parse_raw(raw_capture(0, 2, 1, 16)).is_err());
}

#[test]
fn a_png_capture_decodes_and_a_window_is_cut_from_it() {
    let pixels: Vec<u8> = (0..4 * 3)
        .flat_map(|index| [u8::try_from(index).expect("small"), 0, 0, 255])
        .collect();
    let png = png_of(4, 3, &pixels);
    let (width, height, rgba) = screencap::parse_png(&png).expect("the PNG decodes");
    assert_eq!((width, height), (4, 3));
    assert_eq!(rgba, pixels);
    assert!(screencap::parse_png(b"not a png").is_err());
    let window = Bounds {
        left: 1,
        top: 1,
        right: 3,
        bottom: 3,
    };
    let cut = screencap::crop(width, height, rgba.clone(), window).expect("inside");
    assert_eq!((cut.width(), cut.height()), (2, 2));
    assert_eq!(cut.pixel(0, 0), Some([5, 0, 0, 255]));
    assert_eq!(cut.pixel(1, 1), Some([10, 0, 0, 255]));
    let past = Bounds { right: 5, ..window };
    assert!(screencap::crop(width, height, rgba, past).is_err());
}

#[test]
fn getevent_lines_tell_the_lanes_devices_from_everyone_elses() {
    assert_eq!(
        getevent::parse_line("/dev/input/event5: EV_KEY       BTN_TOUCH            DOWN    "),
        Line::Event {
            path: "/dev/input/event5".to_owned(),
            kind: "EV_KEY".to_owned(),
            code: "BTN_TOUCH".to_owned(),
            value: "DOWN".to_owned(),
        }
    );
    assert_eq!(
        getevent::parse_line("  name:     \"sec_touchscreen\""),
        Line::Name("sec_touchscreen".to_owned())
    );
    assert_eq!(
        getevent::parse_line("could not get driver version"),
        Line::Other
    );
    let mut watch = Watch::new();
    assert!(!watch.started());
    for line in GETEVENT.lines() {
        watch.read(line);
    }
    assert!(watch.started());
    assert_eq!(
        watch.foreign(),
        None,
        "nothing counts before the watch is armed"
    );
    watch.arm();
    // A sensor's axes, a switch, a repeat and the lane's own pen are no one's input.
    for line in GETEVENT.lines().skip(6) {
        watch.read(line);
    }
    assert_eq!(watch.foreign(), None);
    watch.read("/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000045");
    assert_eq!(
        watch.foreign(),
        Some("EV_ABS ABS_MT_TRACKING_ID 00000045 from event5")
    );
    // A key press, and input from a device the watch never saw named, count too.
    for line in [
        "/dev/input/event2: EV_KEY       KEY_VOLUMEDOWN       DOWN",
        "/dev/input/event11: EV_REL       REL_X                00000003",
    ] {
        let mut watch = Watch::new();
        for fixture in GETEVENT.lines() {
            watch.read(fixture);
        }
        watch.arm();
        watch.read(line);
        assert!(watch.foreign().is_some(), "{line}");
    }
    // A device that is gone and added again under another name is read by its new name.
    let mut watch = Watch::new();
    for line in GETEVENT.lines() {
        watch.read(line);
    }
    watch.read("remove device 4: /dev/input/event9");
    watch.read("add device 5: /dev/input/event9");
    watch.read("  name:     \"usb_mouse\"");
    watch.arm();
    watch.read("/dev/input/event9: EV_REL       REL_X                00000001");
    assert!(watch.foreign().is_some());
}

#[test]
fn uinput_lines_register_inject_and_sync() {
    let axes = uinput::stylus_axes(1080, 2400);
    let register = uinput::stylus(&axes).command();
    let parsed: serde_json::Value = serde_json::from_str(&register).expect("JSON");
    assert_eq!(parsed["command"], "register");
    assert_eq!(parsed["name"], uinput::STYLUS_NAME);
    assert_eq!(parsed["id"], uinput::STYLUS_ID);
    assert_eq!(parsed["abs_info"][0]["info"]["maximum"], 1079);
    assert_eq!(parsed["abs_info"][1]["info"]["maximum"], 2399);
    // The command reads a register's fields in order: the name before the configuration.
    let name_at = register.find("\"name\"").expect("a name");
    let configuration_at = register.find("\"configuration\"").expect("a configuration");
    assert!(name_at < configuration_at);
    let probe: serde_json::Value = serde_json::from_str(&uinput::probe().command()).expect("JSON");
    assert_eq!(
        probe["configuration"][0]["data"],
        serde_json::json!(["EV_KEY"])
    );
    let inject: serde_json::Value =
        serde_json::from_str(&uinput::inject(2, &[("EV_KEY", "BTN_TOUCH", 1)])).expect("JSON");
    assert_eq!(
        inject["events"],
        serde_json::json!(["EV_KEY", "BTN_TOUCH", 1, "EV_SYN", "SYN_REPORT", 0])
    );
    let sync = uinput::sync(2, 7);
    assert_eq!(answer_syncs(&sync).len(), 1);
    assert!(uinput::answers_sync(&answer_syncs(&sync)[0], 2, 7));
    assert!(!uinput::answers_sync(&answer_syncs(&sync)[0], 2, 8));
    assert!(!uinput::answers_sync(&answer_syncs(&sync)[0], 1, 7));
    assert!(!uinput::answers_sync(
        r#"{"reason":"vibrating","id":2,"status":1}"#,
        2,
        7
    ));
    assert!(uinput::lists_device(PROBED, uinput::PROBE_NAME));
    assert!(!uinput::lists_device(PROBED, uinput::STYLUS_NAME));
}

#[test]
fn a_display_point_lands_on_the_pens_axes_at_every_rotation() {
    let natural = (1080, 2400);
    let at =
        |x, y, rotation| uinput::natural_point(DevicePoint::new(x, y), rotation, natural, natural);
    // The natural top-left corner, wherever each rotation puts it on the display.
    assert_eq!(at(0, 0, 0), Some(DevicePoint::new(0, 0)));
    assert_eq!(at(0, 1079, 1), Some(DevicePoint::new(0, 0)));
    assert_eq!(at(1079, 2399, 2), Some(DevicePoint::new(0, 0)));
    assert_eq!(at(2399, 0, 3), Some(DevicePoint::new(0, 0)));
    // The natural bottom-right corner.
    assert_eq!(at(1079, 2399, 0), Some(DevicePoint::new(1079, 2399)));
    assert_eq!(at(2399, 0, 1), Some(DevicePoint::new(1079, 2399)));
    assert_eq!(at(0, 0, 2), Some(DevicePoint::new(1079, 2399)));
    assert_eq!(at(0, 1079, 3), Some(DevicePoint::new(1079, 2399)));
    // Off the display as it is turned.
    assert_eq!(at(1080, 0, 0), None);
    assert_eq!(at(1100, 10, 0), None);
    assert_eq!(at(-1, 10, 1), None);
    assert_eq!(at(2000, 10, 1), Some(DevicePoint::new(1069, 2000)));
    // An overridden size is a scale of the panel.
    assert_eq!(
        uinput::natural_point(DevicePoint::new(540, 1200), 0, (1080, 2400), (1440, 3200)),
        Some(DevicePoint::new(720, 1600))
    );
}

#[test]
fn each_safety_rule_refuses_with_its_code() {
    let rows = [
        (
            Refusal::SystemKey {
                name: "GoHome".to_owned(),
                effect: "goes home",
            },
            "os_shell_hotkey",
        ),
        (
            Refusal::SystemChord {
                chord: "META_LEFT+H".to_owned(),
                effect: "the window manager",
            },
            "os_shell_hotkey",
        ),
        (
            Refusal::SystemGesture {
                point: DevicePoint::new(5, 5),
                gesture: "back",
            },
            "os_system_gesture",
        ),
        (
            Refusal::UnknownKey {
                name: "Hyper".to_owned(),
            },
            "os_key",
        ),
        (Refusal::UnsentText { character: 'ü' }, "os_text"),
        (
            Refusal::UnsentInput {
                what: "x".to_owned(),
            },
            "os_input_device",
        ),
        (
            Refusal::NeedsUinput {
                what: "a pen".to_owned(),
            },
            "uinput_device",
        ),
        (
            Refusal::NeedsCommand {
                command: "motionevent",
                what: "x".to_owned(),
            },
            "android_input_motionevent",
        ),
    ];
    for (refusal, expected) in rows {
        let error = refusal.clone().into_error();
        assert_eq!(capability(&error), Some(expected), "{refusal:?}");
        assert!(error.is_cannot_verify(), "{refusal:?}");
    }
    assert!(matches!(
        Refusal::NotForeground { focus: None }.into_error(),
        Error::NotForeground { .. }
    ));
    assert!(safety::admit_isolation(Isolation::Emulator).is_ok());
    assert!(safety::admit_isolation(Isolation::Device).is_ok());
    for isolation in [Isolation::Shared, Isolation::Xvfb, Isolation::InProcess] {
        assert!(safety::admit_isolation(isolation).is_err(), "{isolation:?}");
    }
    assert!(safety::check_focus(Some("dev.flui.scenario/a.B"), PACKAGE).is_ok());
    assert!(safety::check_focus(Some("dev.flui.scenario.evil/a.B"), PACKAGE).is_err());
    assert!(safety::check_focus(Some("NotificationShade"), PACKAGE).is_err());
    assert!(safety::check_focus(None, PACKAGE).is_err());
}

#[test]
fn a_contact_going_down_at_an_edge_of_the_display_is_the_systems_gesture() {
    let display = display();
    // 48 dp at 420 dpi is 126 pixels.
    assert_eq!(display.gesture_edge(), 126);
    for (x, y) in [(500, 125), (500, 2274), (125, 1200), (954, 1200)] {
        assert!(
            safety::gesture_edge(DevicePoint::new(x, y), &display).is_some(),
            "({x}, {y})"
        );
    }
    for (x, y) in [(500, 126), (500, 2273), (126, 1200), (953, 1200)] {
        assert!(
            safety::gesture_edge(DevicePoint::new(x, y), &display).is_none(),
            "({x}, {y})"
        );
    }
}

#[test]
fn a_key_is_sent_at_its_release_alone_or_as_a_chord() {
    let abilities = abilities();
    let plan = |events: &[OsEvent]| -> Result<Vec<Command>, Refusal> {
        let mut held = HeldInput::new();
        let mut commands = Vec::new();
        for event in events {
            let planned = held.plan(event, &abilities)?;
            for command in &planned {
                held.took(command);
            }
            commands.extend(planned);
        }
        Ok(commands)
    };
    let down = |name: &str| OsEvent::KeyDown(name.to_owned());
    let up = |name: &str| OsEvent::KeyUp(name.to_owned());
    assert_eq!(
        plan(&[down("Enter"), up("Enter")]),
        Ok(vec![Command::Key(keys::ENTER)])
    );
    let a = match keys::resolve("a") {
        KeyName::Key(code) => code,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        plan(&[down("Control"), down("a"), up("a"), up("Control")]),
        Ok(vec![Command::Chord(vec![keys::CONTROL, a])])
    );
    assert_eq!(
        Command::Chord(vec![keys::CONTROL, a]).line().as_deref(),
        Some("input keycombination KEYCODE_CTRL_LEFT KEYCODE_A")
    );
    // A modifier tapped alone is sent alone; one held through a chord is not sent again.
    assert_eq!(
        plan(&[down("Shift"), up("Shift")]),
        Ok(vec![Command::Key(keys::SHIFT)])
    );
    assert!(matches!(
        plan(&[down("Meta"), up("Meta")]),
        Err(Refusal::SystemKey { .. })
    ));
    assert!(matches!(
        plan(&[down("Meta"), down("h"), up("h")]),
        Err(Refusal::SystemChord { .. })
    ));
    assert!(matches!(
        plan(&[down("Alt"), down("Tab"), up("Tab")]),
        Err(Refusal::SystemChord { .. })
    ));
    assert!(matches!(
        plan(&[down("Control"), down(" "), up(" ")]),
        Err(Refusal::SystemChord { .. })
    ));
    assert!(matches!(
        plan(&[down("GoHome")]),
        Err(Refusal::SystemKey { .. })
    ));
    assert!(matches!(
        plan(&[down("a"), down("b")]),
        Err(Refusal::UnsentInput { .. })
    ));
    let mut older = abilities;
    older.input.key_combination = false;
    let mut held = HeldInput::new();
    held.plan(&down("Control"), &older)
        .expect("a modifier is held here");
    held.plan(&down("a"), &older).expect("a key is held here");
    assert!(matches!(
        held.plan(&up("a"), &older),
        Err(Refusal::NeedsCommand {
            command: "keycombination",
            ..
        })
    ));
    assert_eq!(
        plan(&[OsEvent::Text("a\nb".to_owned())]),
        Ok(vec![
            Command::Text("a".to_owned()),
            Command::Key(keys::ENTER),
            Command::Text("b".to_owned()),
        ])
    );
    assert!(matches!(
        plan(&[down("Shift"), OsEvent::Text("a".to_owned())]),
        Err(Refusal::UnsentInput { .. })
    ));
}

#[test]
fn a_contact_is_held_only_once_the_device_took_it() {
    let abilities = abilities();
    let at = DevicePoint::new(300, 600);
    let events = tap(PointerDevice::Touch, at);
    let mut held = HeldInput::new();
    assert_eq!(
        held.plan(&events[0], &abilities),
        Ok(Vec::new()),
        "a move places the finger"
    );
    let down = held.plan(&events[1], &abilities).expect("a press");
    assert_eq!(
        down,
        vec![Command::Motion {
            pointer: "scenario.touch".to_owned(),
            source: MotionSource::Touchscreen,
            action: MotionAction::Down,
            at,
        }]
    );
    assert_eq!(
        held.held_by(Part::Contact),
        Vec::<String>::new(),
        "nothing is held before the device took it"
    );
    assert_eq!(held.release_plan_of(Part::Contact), Vec::new());
    held.took(&down[0]);
    assert_eq!(
        held.held_by(Part::Contact),
        vec!["the touch contact scenario.touch"]
    );
    assert_eq!(
        held.release_plan_of(Part::Contact),
        vec![Command::Motion {
            pointer: "scenario.touch".to_owned(),
            source: MotionSource::Touchscreen,
            action: MotionAction::Cancel,
            at,
        }]
    );
    // A press whose command timed out may have gone down: it is held, to be cancelled.
    let mut unsure = HeldInput::new();
    unsure.plan(&events[0], &abilities).expect("a move");
    let down = unsure.plan(&events[1], &abilities).expect("a press");
    unsure.unsure(&down[0]);
    assert_eq!(unsure.held_by(Part::Contact).len(), 1);
    // A key a timed-out command may have left down is named once, never pressed again.
    let mut keys_held = HeldInput::new();
    keys_held.unsure(&Command::Key(keys::ENTER));
    assert_eq!(keys_held.release_plan_of(Part::Contact), Vec::new());
    assert_eq!(keys_held.take_unsure(), vec!["KEYCODE_ENTER"]);
    assert_eq!(keys_held.take_unsure(), Vec::<String>::new());
}

#[test]
fn a_second_contact_a_press_with_a_key_held_and_other_buttons_are_refused() {
    let abilities = abilities();
    let at = DevicePoint::new(300, 600);
    let mut held = HeldInput::new();
    for event in &tap(PointerDevice::Touch, at)[..2] {
        for command in held.plan(event, &abilities).expect("the first contact") {
            held.took(&command);
        }
    }
    let second = tap(PointerDevice::Mouse, at);
    held.plan(&second[0], &abilities)
        .expect("a move places the mouse");
    assert!(matches!(
        held.plan(&second[1], &abilities),
        Err(Refusal::UnsentInput { .. })
    ));
    let mut keyed = HeldInput::new();
    keyed
        .plan(&OsEvent::KeyDown("Shift".to_owned()), &abilities)
        .expect("held here");
    keyed.plan(&second[0], &abilities).expect("a move");
    assert!(matches!(
        keyed.plan(&second[1], &abilities),
        Err(Refusal::UnsentInput { .. })
    ));
    let mut buttons = HeldInput::new();
    buttons.plan(&second[0], &abilities).expect("a move");
    assert!(matches!(
        buttons.plan(
            &OsEvent::PointerDown {
                pointer: "scenario.mouse".to_owned(),
                device: PointerDevice::Mouse,
                button: PointerButton::Secondary,
            },
            &abilities
        ),
        Err(Refusal::UnsentInput { .. })
    ));
    let mut edge = HeldInput::new();
    let at_edge = tap(PointerDevice::Touch, DevicePoint::new(500, 2390));
    edge.plan(&at_edge[0], &abilities).expect("a move");
    assert!(matches!(
        edge.plan(&at_edge[1], &abilities),
        Err(Refusal::SystemGesture { .. })
    ));
    let mut no_motion = abilities;
    no_motion.input.motion_event = false;
    let mut older = HeldInput::new();
    older.plan(&second[0], &no_motion).expect("a move");
    assert!(matches!(
        older.plan(&second[1], &no_motion),
        Err(Refusal::NeedsCommand {
            command: "motionevent",
            ..
        })
    ));
}

#[test]
fn a_pen_needs_uinput_and_hovers_presses_and_leaves() {
    let at = DevicePoint::new(300, 600);
    let events = tap(PointerDevice::Pen, at);
    let mut without = abilities();
    without.uinput = false;
    assert!(matches!(
        HeldInput::new().plan(&events[0], &without),
        Err(Refusal::NeedsUinput { .. })
    ));
    let abilities = abilities();
    let mut held = HeldInput::new();
    let mut changes = Vec::new();
    for event in &events {
        for command in held.plan(event, &abilities).expect("a pen") {
            if let Command::Pen { change, .. } = command {
                changes.push(change);
            }
            held.took(&command);
        }
    }
    assert_eq!(
        changes,
        [PenChange::Enter, PenChange::Press, PenChange::Lift]
    );
    assert_eq!(held.held_by(Part::Pen), vec!["the pen, in range"]);
    assert!(matches!(
        held.release_plan_of(Part::Pen).as_slice(),
        [Command::Pen {
            change: PenChange::Leave,
            ..
        }]
    ));
}

#[test]
fn the_lane_connects_reading_the_device_and_probing_uinput() {
    let adb = scripted(true, true);
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("the lane connects");
    assert_eq!(lane.capabilities().isolation, Isolation::Emulator);
    assert!(lane.capabilities().pointers.contains(&PointerDevice::Pen));
    let capabilities = lane.capabilities();
    assert_eq!(capabilities.fidelity, PositionFidelity::OrderOnly);
    assert_eq!(capabilities.isolation, Isolation::Emulator);
    assert_eq!(
        capabilities.pointers,
        vec![
            PointerDevice::Touch,
            PointerDevice::Mouse,
            PointerDevice::Pen
        ]
    );
    assert!(lane.guard().is_none(), "nobody holds an emulator");
    // The probe registered its device, synced, and ended, which removes it.
    let probe = adb.commands("shell uinput -");
    assert!(
        probe.iter().any(|line| line.contains("\"register\"")),
        "{probe:?}"
    );
    assert!(
        probe.iter().any(|line| line.contains("\"sync\"")),
        "{probe:?}"
    );
    assert!(
        probe.iter().any(|line| line.ends_with("ended")),
        "{probe:?}"
    );
    let device = scripted(false, true);
    let mut lane = AndroidLane::connect(device.device(), PACKAGE).expect("the lane connects");
    assert_eq!(lane.capabilities().isolation, Isolation::Device);
    let guard = lane
        .guard()
        .expect("a device in someone's hands is guarded");
    guard.arm().expect("getevent named its devices");
    assert!(guard.foreign_input().is_none());
    assert!(matches!(
        AndroidLane::connect(scripted(true, true).device(), "bad name"),
        Err(Error::InvalidArgument { .. })
    ));
}

#[test]
fn a_device_without_root_uinput_reports_the_capability_missing_and_refuses_the_pen() {
    let adb = scripted(true, false);
    let mut lane = connected(&adb);
    assert!(!lane.capabilities().pointers.contains(&PointerDevice::Pen));
    assert!(!lane.capabilities().pointers.contains(&PointerDevice::Pen));
    let refused = lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Pen, DevicePoint::new(300, 600)),
    )
    .expect_err("a pen needs uinput");
    assert_eq!(capability(&refused), Some("uinput_device"), "{refused}");
    assert!(refused.is_cannot_verify());
    assert!(inputs(&adb).is_empty(), "nothing was sent");
    // A touch still goes through `input`.
    lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect("a touch needs no uinput");
}

#[test]
fn a_tap_goes_out_as_motion_events_each_press_after_the_focus_check() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    let geometry = lane.locate_window(&placement()).expect("located");
    // The surface's top-left is 60 pixels below the display's.
    let origin = flui_driver::coordinates::to_device(
        &geometry,
        flui_driver::protocol::geometry::LogicalPoint::new(0.0, 0.0),
    )
    .expect("inside");
    assert_eq!(origin, DevicePoint::new(0, 60));
    lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect("a tap");
    let sent: Vec<String> = adb
        .log()
        .into_iter()
        .filter(|line| is_input(line) || line.starts_with("shell dumpsys window windows"))
        .collect();
    assert_eq!(
        sent,
        vec![
            "shell dumpsys window windows | grep -m1 'mCurrentFocus='",
            "shell input touchscreen motionevent DOWN 300 600",
            "shell input touchscreen motionevent UP 300 600",
        ]
    );
    assert!(
        adb.commands("shell input touchscreen motionevent CANCEL")
            .is_empty(),
        "nothing was held at the end"
    );
}

#[test]
fn keys_text_and_a_scroll_go_out_as_input_commands() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    let events = [
        OsEvent::KeyDown("Control".to_owned()),
        OsEvent::KeyDown("a".to_owned()),
        OsEvent::KeyUp("a".to_owned()),
        OsEvent::KeyUp("Control".to_owned()),
        OsEvent::Text("it's".to_owned()),
        OsEvent::Scroll {
            at: DevicePoint::new(300, 600),
            delta_x: 0,
            delta_y: 40,
        },
    ];
    lane::send_guarded(&mut lane, &events).expect("keys, text and a scroll");
    assert_eq!(
        inputs(&adb),
        vec![
            "shell input keycombination KEYCODE_CTRL_LEFT KEYCODE_A",
            "shell input text 'it'\\''s'",
            "shell input mouse scroll 300 600 --axis VSCROLL,-1 --axis HSCROLL,0",
        ]
    );
}

#[test]
fn the_app_without_the_focus_is_refused_before_its_press() {
    let adb = scripted(true, true).reply_once(
        "shell dumpsys window windows",
        AdbReply::new(0, "  mCurrentFocus=Window{9e u0 NotificationShade}\n"),
    );
    let mut lane = connected(&adb);
    let refused = lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect_err("the shade is in front");
    assert!(matches!(refused, Error::NotForeground { .. }), "{refused}");
    assert!(inputs(&adb).is_empty(), "nothing was sent");
}

#[test]
fn a_release_failure_names_the_contact_still_held() {
    let adb = scripted(true, true)
        .reply_once(
            "shell input touchscreen motionevent UP",
            AdbReply::new(1, "").stderr("Error: the device refused it"),
        )
        .reply_once(
            "shell input touchscreen motionevent CANCEL",
            AdbReply::new(1, "").stderr("Error: the device refused it again"),
        );
    let mut lane = connected(&adb);
    let events = tap(PointerDevice::Touch, DevicePoint::new(300, 600));
    lane.check(&events).expect("a tap");
    for event in &events[..3] {
        lane.send(event).expect("the press goes down");
    }
    let refused = lane.send(&events[3]).expect_err("the lift was refused");
    assert!(matches!(refused, Error::Lane { .. }), "{refused}");
    match lane.release_all() {
        Err(Error::ReleaseFailed {
            lane: name,
            held,
            message,
            ..
        }) => {
            assert_eq!(name, "android");
            assert_eq!(held, vec!["the touch contact scenario.touch".to_owned()]);
            assert!(message.contains("refused it again"), "{message}");
        }
        other => panic!("expected the contact named as held, got {other:?}"),
    }
    // A later release that the device takes lets go of it.
    lane.release_all().expect("the device takes the cancel now");
    assert_eq!(
        adb.commands("shell input touchscreen motionevent CANCEL")
            .len(),
        2
    );
}

#[test]
fn a_timed_out_press_is_held_and_cancelled() {
    let adb = scripted(true, true).timeout_once("shell input touchscreen motionevent DOWN");
    let mut lane = connected(&adb);
    let failed = lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect_err("the press timed out");
    assert!(
        matches!(
            failed,
            Error::Timeout {
                awaited: Awaited::Host,
                ..
            }
        ),
        "{failed}"
    );
    assert_eq!(
        adb.commands("shell input touchscreen motionevent CANCEL"),
        vec!["shell input touchscreen motionevent CANCEL 300 600"]
    );
}

#[test]
fn a_key_a_timed_out_command_may_have_left_down_is_named_once() {
    let adb = scripted(true, true).timeout_once("shell input keyevent");
    let mut lane = connected(&adb);
    let failed = lane::send_guarded(
        &mut lane,
        &[
            OsEvent::KeyDown("Enter".to_owned()),
            OsEvent::KeyUp("Enter".to_owned()),
        ],
    )
    .expect_err("the key timed out");
    match failed {
        Error::ReleaseFailed { held, message, .. } => {
            assert_eq!(held, vec!["KEYCODE_ENTER"]);
            assert!(message.contains("did not end within"), "{message}");
        }
        other => panic!("expected the key named as held, got {other}"),
    }
    assert_eq!(
        inputs(&adb),
        vec!["shell input keyevent KEYCODE_ENTER"],
        "never pressed again"
    );
    lane.release_all().expect("named once");
}

#[test]
fn foreign_input_on_a_device_ends_the_run_and_releases_its_contact() {
    let adb = scripted(false, true).on_command(
        "shell input touchscreen motionevent DOWN",
        "shell -tt getevent",
        "/dev/input/event5: EV_ABS       ABS_MT_TRACKING_ID   00000045",
    );
    let mut lane = connected(&adb);
    let interrupted = lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect_err("someone touched the screen");
    match &interrupted {
        Error::InterruptedByUser { lane, event } => {
            assert_eq!(lane, "android");
            assert!(event.contains("event5"), "{event}");
        }
        other => panic!("{other}"),
    }
    assert!(interrupted.is_cannot_verify());
    assert_eq!(
        adb.commands("shell input touchscreen motionevent UP"),
        Vec::<String>::new()
    );
    assert_eq!(
        adb.commands("shell input touchscreen motionevent CANCEL")
            .len(),
        1,
        "the contact was released"
    );
}

#[test]
fn a_guard_that_stopped_watching_ends_the_run() {
    let adb = scripted(false, true);
    let mut lane = connected(&adb);
    let guard = lane.guard().expect("guarded");
    guard.arm().expect("watching");
    adb.end_stream("shell -tt getevent");
    let stopped = guard
        .foreign_input()
        .expect("a guard that stopped reports it");
    assert!(
        stopped
            .description
            .starts_with(flui_driver::lane::android::guard::STOPPED_WATCHING),
        "{stopped:?}"
    );
}

#[test]
fn a_pen_goes_through_its_uinput_device_and_leaves_at_the_end() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Pen, DevicePoint::new(300, 600)),
    )
    .expect("a pen");
    let written: Vec<String> = adb
        .log()
        .into_iter()
        .filter_map(|line| line.strip_prefix("shell uinput - <- ").map(str::to_owned))
        .collect();
    let commands: Vec<String> = written
        .iter()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value["id"] == uinput::STYLUS_ID)
        .map(|value| match value["command"].as_str() {
            Some("inject") => format!("inject {}", value["events"]),
            other => other.unwrap_or_default().to_owned(),
        })
        .collect();
    assert_eq!(
        commands,
        vec![
            "register".to_owned(),
            "sync".to_owned(),
            r#"inject ["EV_KEY","BTN_TOOL_PEN",1,"EV_ABS","ABS_X",300,"EV_ABS","ABS_Y",600,"EV_SYN","SYN_REPORT",0]"#.to_owned(),
            "sync".to_owned(),
            r#"inject ["EV_ABS","ABS_X",300,"EV_ABS","ABS_Y",600,"EV_KEY","BTN_TOUCH",1,"EV_ABS","ABS_PRESSURE",2047,"EV_SYN","SYN_REPORT",0]"#.to_owned(),
            "sync".to_owned(),
            r#"inject ["EV_KEY","BTN_TOUCH",0,"EV_ABS","ABS_PRESSURE",0,"EV_SYN","SYN_REPORT",0]"#.to_owned(),
            "sync".to_owned(),
            r#"inject ["EV_KEY","BTN_TOOL_PEN",0,"EV_SYN","SYN_REPORT",0]"#.to_owned(),
            "sync".to_owned(),
        ]
    );
    assert!(
        inputs(&adb)
            .iter()
            .all(|line| !line.contains("motionevent"))
    );
}

#[test]
fn a_pen_whose_device_stopped_answering_is_released_with_it() {
    let adb = scripted(true, true);
    let mut lane = connected(&adb);
    let events = tap(PointerDevice::Pen, DevicePoint::new(300, 600));
    lane.check(&events).expect("a pen");
    lane.send(&events[0]).expect("the pen enters");
    lane.send(&events[1]).expect("the pen presses");
    adb.end_stream("shell uinput -");
    let failed = lane.send(&events[3]).expect_err("the device is gone");
    assert!(matches!(failed, Error::Lane { .. }), "{failed}");
    // The kernel took the pen's contact away with its device: nothing stays held.
    lane.release_all().expect("nothing is held");
}

#[test]
fn the_window_is_captured_from_screencap_and_cut_to_the_surface() {
    let raw = raw_capture(1080, 2400, 1, 16);
    let adb = scripted(true, true).reply("exec-out screencap", AdbReply::new(0, raw));
    let mut lane = connected(&adb);
    let capture = lane.capture_window(&placement()).expect("a capture");
    assert_eq!((capture.width(), capture.height()), (1080, 2340));
    // Its top-left pixel is the display's pixel 60 rows down.
    let index = 60 * 1080;
    assert_eq!(
        capture.pixel(0, 0),
        Some([u8::try_from(index % 251).expect("small"), 1, 2, 3])
    );
    assert_eq!(adb.commands("exec-out screencap -p"), Vec::<String>::new());
}

#[test]
fn a_capture_in_another_pixel_format_is_taken_again_as_a_png() {
    let png = png_of(1080, 2400, &[9, 8, 7, 255].repeat(1080 * 2400));
    let adb = scripted(true, true)
        .reply("exec-out screencap -p", AdbReply::new(0, png))
        .reply(
            "exec-out screencap",
            AdbReply::new(0, raw_capture(2, 2, 4, 16)),
        );
    let mut lane = connected(&adb);
    let capture = lane.capture_window(&placement()).expect("a capture");
    assert_eq!(capture.pixel(10, 10), Some([9, 8, 7, 255]));
}

#[test]
fn a_window_the_device_does_not_show_is_not_in_front() {
    let shade = WINDOWS.replace(
        "mCurrentFocus=Window{4f2a1b u0 dev.flui.scenario/android.app.NativeActivity}",
        "mCurrentFocus=Window{77aa u0 NotificationShade}",
    );
    assert_ne!(shade, WINDOWS);
    let adb =
        scripted(true, true).reply_once("shell dumpsys window displays", AdbReply::new(0, shade));
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let refused = lane
        .locate_window(&placement())
        .expect_err("the shade is in front");
    assert!(matches!(refused, Error::NotForeground { .. }), "{refused}");
}

/// A device whose user runs an accessibility service (a password manager, an automation app)
/// still has its window located, pressed on and captured: none of them takes a `uiautomator
/// dump`, which would pause the service. Only the read of the tree is refused, naming it.
#[test]
fn a_service_on_a_device_refuses_the_tree_and_nothing_else() {
    let service = "com.example.passwords/.AutofillService\n0\n";
    let adb = scripted(false, true)
        .reply_once("shell settings get secure", AdbReply::new(0, service))
        .reply_once("shell settings get secure", AdbReply::new(0, service))
        .reply(
            "exec-out screencap",
            AdbReply::new(0, raw_capture(1080, 2400, 1, 16)),
        );
    let mut lane = connected(&adb);
    lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect("the press goes out");
    lane.capture_window(&placement())
        .expect("the window is captured");
    let empty = flui_driver::protocol::tree::Tree::new(Vec::new(), false);
    let refused = lane
        .read_tree(&placement(), &empty)
        .expect_err("no tree while the service runs");
    assert_eq!(
        refused.missing_capability().as_deref(),
        Some(safety::WINDOW_DUMP)
    );
    assert!(
        refused.to_string().contains("com.example.passwords"),
        "{refused}"
    );
    assert_eq!(adb.commands("exec-out uiautomator"), Vec::<String>::new());
}

#[test]
fn starting_the_app_hands_it_its_endpoint_and_token_and_forwards_a_port() {
    let adb = scripted(true, true)
        .reply("shell 'am' 'force-stop'", AdbReply::new(0, ""))
        .reply("shell 'am' 'start'", AdbReply::new(0, "Status: ok\n"))
        .reply("install", AdbReply::new(0, "Success\n"))
        .reply("forward --remove", AdbReply::new(0, ""))
        .reply("forward --no-rebind tcp:", AdbReply::new(0, ""));
    let app = AndroidApp::try_new(PACKAGE, "android.app.NativeActivity")
        .expect("an app")
        .apk("app.apk");
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let started = lane.start(&app).expect("started");
    // The lane forwards a loopback port it picked: the one its endpoint names.
    let Endpoint::Forwarded(address) = &started.endpoint else {
        panic!("{:?}", started.endpoint);
    };
    assert!(address.ip().is_loopback());
    let port = address.port();
    assert_ne!(port, 0);
    assert!(started.token.len() >= 32);
    let shown = format!("{started:?}");
    assert!(
        !shown.contains(&started.token) && shown.contains(&port.to_string()),
        "the token stays out of the debug form: {shown}"
    );
    let log = adb.log();
    let at = |prefix: &str| {
        log.iter()
            .position(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("no `{prefix}` in {log:#?}"))
    };
    assert!(at("install -r -t app.apk") < at("shell 'am' 'force-stop'"));
    assert!(at("shell 'am' 'force-stop'") < at("shell 'am' 'start'"));
    let start = &log[at("shell 'am' 'start'")];
    let name = log[at(&format!("forward --no-rebind tcp:{port} localabstract:"))]
        .rsplit(':')
        .next()
        .expect("a name")
        .to_owned();
    assert!(
        start.contains(&format!("'flui.agent.endpoint' '@{name}'")),
        "{start}"
    );
    assert!(
        start.contains(&format!("'flui.agent.token' '{}'", started.token)),
        "{start}"
    );
    assert!(
        start.contains("'flui.agent.exit_on_disconnect' '1'"),
        "{start}"
    );
    drop(lane);
    let ended = adb.log();
    assert_eq!(
        ended
            .iter()
            .filter(|line| line.starts_with("shell 'am' 'force-stop'"))
            .count(),
        2
    );
    assert!(
        ended
            .iter()
            .any(|line| *line == format!("forward --remove tcp:{port}"))
    );
    assert!(AndroidApp::try_new("a b", "x").is_err());
    let other = AndroidApp::try_new("other.app", "x").expect("an app");
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    assert!(matches!(
        lane.start(&other),
        Err(Error::InvalidArgument { .. })
    ));
}

#[test]
fn an_app_that_does_not_start_is_stopped_and_reports_why() {
    let adb = scripted(true, true)
        .reply("shell 'am' 'force-stop'", AdbReply::new(0, ""))
        .reply(
            "shell 'am' 'start'",
            AdbReply::new(
                0,
                "Error: Activity class {dev.flui.scenario/x} does not exist.\n",
            ),
        );
    let app = AndroidApp::try_new(PACKAGE, "x").expect("an app");
    let mut lane = AndroidLane::connect(adb.device(), PACKAGE).expect("connects");
    let failed = lane.start(&app).expect_err("no such activity");
    assert!(failed.to_string().contains("does not exist"), "{failed}");
    assert_eq!(adb.commands("forward"), Vec::<String>::new());
}

#[test]
fn a_device_gone_mid_run_is_unavailable() {
    let adb = scripted(true, true).reply_once(
        "shell input touchscreen motionevent DOWN",
        AdbReply::new(1, "").stderr("error: device 'emulator-5554' not found"),
    );
    let mut lane = connected(&adb);
    let failed = lane::send_guarded(
        &mut lane,
        &tap(PointerDevice::Touch, DevicePoint::new(300, 600)),
    )
    .expect_err("the device went");
    assert!(failed.is_cannot_verify(), "{failed}");
    assert!(
        adb.commands("shell input touchscreen motionevent CANCEL")
            .is_empty(),
        "a press the device refused is not held"
    );
}
