//! Native recordings (ADR-0215 §4): checked-in journals of each backend's
//! plain readings, replayed through the shared decoder on any host, so
//! Linux CI exercises Win32, Apple, Android, winit and browser translation.

#[cfg(all(debug_assertions, feature = "native-capture"))]
pub mod capture;
pub mod codec;

use std::fmt::Write as _;

use flui_platform::traits::PlatformInput;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{
    ButtonChange, PanZoomPhase, PenTool, PointerEvent, PointerInfo, PointerKind, PointerRole,
    PointerSample,
};

use codec::{CodecError, NativeRecording};

/// A checked-in recording, read.
fn recording(text: &str) -> NativeRecording {
    NativeRecording::read(text.as_bytes()).expect("a checked-in recording reads")
}

/// `inputs` one event per line, in the terms a gesture consumer sees.
fn transcript(inputs: &[PlatformInput]) -> Vec<String> {
    inputs.iter().map(line).collect()
}

fn line(input: &PlatformInput) -> String {
    let Some(event) = input.as_pointer() else {
        return format!("{input:?}");
    };
    let mut out = String::new();
    match event {
        PointerEvent::Down(press) => {
            write_head(&mut out, "down", &press.pointer);
            write!(
                out,
                " b{} {:?}",
                press.button().number(),
                held(press.buttons())
            )
            .expect("BUG: a String accepts writes");
            write_clicks(&mut out, press.click_count, press.modifiers);
            write_sample(&mut out, &press.sample);
        }
        PointerEvent::Up(release) => {
            write_head(&mut out, "up", &release.pointer);
            write!(
                out,
                " b{} {:?}",
                release.button().number(),
                held(release.buttons())
            )
            .expect("BUG: a String accepts writes");
            write_clicks(&mut out, None, release.modifiers);
            write_sample(&mut out, &release.sample);
        }
        PointerEvent::ButtonChange(ButtonChange::Pressed(press)) => {
            write_head(&mut out, "press", &press.pointer);
            write!(
                out,
                " b{} {:?}",
                press.button().number(),
                held(press.buttons())
            )
            .expect("BUG: a String accepts writes");
            write_clicks(&mut out, press.click_count, press.modifiers);
            write_sample(&mut out, &press.sample);
        }
        PointerEvent::ButtonChange(ButtonChange::Released(release)) => {
            write_head(&mut out, "release", &release.pointer);
            write!(
                out,
                " b{} {:?}",
                release.button().number(),
                held(release.buttons())
            )
            .expect("BUG: a String accepts writes");
            write_clicks(&mut out, None, release.modifiers);
            write_sample(&mut out, &release.sample);
        }
        PointerEvent::Move(movement) => {
            write_head(&mut out, "move", &movement.pointer);
            write!(out, " {:?}", held(movement.buttons)).expect("BUG: a String accepts writes");
            write_clicks(&mut out, None, movement.modifiers);
            write_sample(&mut out, movement.current());
            for (label, samples) in [
                ("coalesced", movement.coalesced()),
                ("predicted", movement.predicted()),
            ] {
                if samples.is_empty() {
                    continue;
                }
                write!(out, " {label}[").expect("BUG: a String accepts writes");
                for (index, sample) in samples.iter().enumerate() {
                    if index > 0 {
                        out.push(';');
                    }
                    write_sample(&mut out, sample);
                }
                out.push_str(" ]");
            }
        }
        PointerEvent::Cancel(cancel) => {
            write_head(&mut out, "cancel", &cancel.pointer);
            write!(out, " {:?} t={}", cancel.reason, cancel.time.as_nanos())
                .expect("BUG: a String accepts writes");
        }
        PointerEvent::Enter(signal) | PointerEvent::Leave(signal) => {
            let verb = if matches!(event, PointerEvent::Enter(_)) {
                "enter"
            } else {
                "leave"
            };
            write_head(&mut out, verb, &signal.pointer);
            write!(out, " t={}", signal.time.as_nanos()).expect("BUG: a String accepts writes");
            if let Some(position) = signal.position {
                let point = position.get();
                write!(out, " ({}, {})", point.x, point.y).expect("BUG: a String accepts writes");
            }
        }
        PointerEvent::PanZoom(gesture) => {
            write_head(&mut out, "panzoom", gesture.pointer());
            match gesture.phase {
                PanZoomPhase::Start => out.push_str(" start"),
                PanZoomPhase::Update(transform) => write!(
                    out,
                    " update scale={:.3} rotation={:.3}",
                    transform.scale(),
                    transform.rotation()
                )
                .expect("BUG: a String accepts writes"),
                PanZoomPhase::End => out.push_str(" end"),
                PanZoomPhase::Cancelled => out.push_str(" cancelled"),
                _ => out.push_str(" ?"),
            }
            write_clicks(&mut out, None, gesture.modifiers);
            let point = gesture.position.get();
            write!(
                out,
                " t={} ({}, {})",
                gesture.time.as_nanos(),
                point.x,
                point.y
            )
            .expect("BUG: a String accepts writes");
        }
        PointerEvent::DeviceAdded(change) | PointerEvent::DeviceRemoved(change) => {
            let verb = if matches!(event, PointerEvent::DeviceAdded(_)) {
                "device+"
            } else {
                "device-"
            };
            write!(
                out,
                "{verb} d{} {} t={}",
                change.device.get(),
                kind(change.kind),
                change.time.as_nanos()
            )
            .expect("BUG: a String accepts writes");
        }
        other => write!(out, "{other:?}").expect("BUG: a String accepts writes"),
    }
    out
}

fn held(buttons: flui_platform_api::pointer::PointerButtons) -> Vec<u8> {
    buttons.iter().map(|button| button.number().get()).collect()
}

fn kind(kind: PointerKind) -> &'static str {
    match kind {
        PointerKind::Mouse => "mouse",
        PointerKind::Touch => "touch",
        PointerKind::Pen {
            tool: PenTool::Eraser,
        } => "eraser",
        PointerKind::Pen { .. } => "pen",
        PointerKind::Trackpad => "trackpad",
        PointerKind::Virtual => "virtual",
        _ => "unknown",
    }
}

/// `verb p<id><*primary> <kind> [d<device>]`.
fn write_head(out: &mut String, verb: &str, pointer: &PointerInfo) {
    let primary = if pointer.role == PointerRole::Primary {
        "*"
    } else {
        ""
    };
    write!(
        out,
        "{verb} p{}{primary} {}",
        pointer.id.get(),
        kind(pointer.kind)
    )
    .expect("BUG: a String accepts writes");
    if let Some(device) = pointer.device {
        write!(out, " d{}", device.get()).expect("BUG: a String accepts writes");
    }
}

fn write_clicks(out: &mut String, clicks: Option<std::num::NonZeroU8>, modifiers: Modifiers) {
    if let Some(count) = clicks {
        write!(out, " x{count}").expect("BUG: a String accepts writes");
    }
    if modifiers != Modifiers::NONE {
        write!(out, " {modifiers:?}").expect("BUG: a String accepts writes");
    }
}

/// ` t=<ns> (<x>, <y>)` and each sensor the sample carries.
fn write_sample(out: &mut String, sample: &PointerSample) {
    let point = sample.position.get();
    write!(
        out,
        " t={} ({}, {})",
        sample.time.as_nanos(),
        point.x,
        point.y
    )
    .expect("BUG: a String accepts writes");
    if let Some(pressure) = sample.pressure {
        write!(out, " pressure={:.3}", pressure.get()).expect("BUG: a String accepts writes");
    }
    if let Some(orientation) = sample.orientation {
        write!(
            out,
            " altitude={:.3} azimuth={:.3}",
            orientation.altitude().unwrap_or(f64::NAN),
            orientation.azimuth().unwrap_or(f64::NAN)
        )
        .expect("BUG: a String accepts writes");
    }
    if let Some(twist) = sample.twist {
        write!(out, " twist={:.3}", twist.radians()).expect("BUG: a String accepts writes");
    }
    if let Some(size) = sample.contact_size {
        let size = size.get();
        write!(out, " size={}x{}", size.width, size.height).expect("BUG: a String accepts writes");
    }
}

/// `text` replays to `expected`, one event per line.
fn assert_replays(text: &str, expected: &[&str]) {
    let replayed = transcript(&recording(text).replay());
    assert_eq!(replayed, expected, "replayed transcript");
}

/// A Win32 pen burst whose coalesced history shares one `dwTime`, a touch
/// lost to a capture change, and legacy mouse clicks counted by the OS
/// double-click settings.
fn win32_pointer_info_recordings_decode_to_their_host_input() {
    assert_replays(
        include_str!("win32_pen_touch_mouse.jsonl"),
        &[
            "device+ d1 pen t=2100000000",
            "enter p2* pen d1 t=2100000000 (200, 134)",
            "down p2* pen d1 b1 [1] t=2110000000 (201, 135) pressure=0.500 altitude=1.047 azimuth=0.000 twist=1.571",
            "move p2* pen d1 [1] t=2135000000 (206, 140) pressure=0.750 altitude=1.047 azimuth=0.000 twist=1.571 coalesced[ t=2118333300 (202, 136) pressure=0.625 altitude=1.047 azimuth=0.000 twist=1.571; t=2126666600 (204, 138) pressure=0.688 altitude=1.047 azimuth=0.000 twist=1.571 ]",
            "up p2* pen d1 b1 [] t=2140000000 (206, 140) pressure=0.000",
            "move p2* pen d1 [] t=2145000000 (208, 140) pressure=0.000",
            "leave p2* pen d1 t=2150000000 (208, 140)",
            "device+ d2 touch t=2200000000",
            "down p3* touch d2 b1 [1] t=2200000000 (400, 300) pressure=0.250 size=20x30",
            "move p3* touch d2 [1] t=2210000000 (408, 300) pressure=0.250 size=20x30",
            "cancel p3* touch d2 CaptureLost t=2215000000",
            "down p1* mouse b1 [1] x1 t=2300000000 (100, 100)",
            "up p1* mouse b1 [] t=2350000000 (100, 100)",
            "down p1* mouse b1 [1] x2 {CONTROL} t=2400000000 (101, 101)",
            "up p1* mouse b1 [] t=2450000000 (101, 101)",
        ],
    );
    // Captured on a Windows 11 host from `InjectSyntheticPointerInput`
    // (`a_captured_native_pointer_session_replays_to_what_it_delivered`):
    // every reading is stamped from its performance count, 10 MHz from the
    // anchor, not from its `dwTime`.
    assert_replays(
        include_str!("win32_injected_touch_and_pen.jsonl"),
        &[
            "device+ d1 touch t=78000100",
            "enter p2* touch d1 t=87645800 (40, 40)",
            "down p2* touch d1 b1 [1] t=87645800 (40, 40) pressure=0.500 size=20x30",
            "move p2* touch d1 [1] t=387645800 (46, 40) pressure=0.750 size=20x30",
            "up p2* touch d1 b1 [] t=687645800 (46, 40) pressure=0.000 size=20x30",
            "leave p2* touch d1 t=687645800 (46, 40)",
            "device+ d2 pen t=984000100",
            "device- d1 touch t=984000100",
            "enter p3* pen d2 t=991498300 (40, 40)",
            "down p3* pen d2 b1 [1] t=991498300 (40, 40) pressure=0.500 altitude=1.047 azimuth=0.000 twist=0.000",
            "move p3* pen d2 [1] t=1292498300 (46, 40) pressure=0.750 altitude=1.047 azimuth=0.000 twist=0.000",
            "up p3* pen d2 b1 [] t=1593498300 (46, 40) pressure=0.000 altitude=1.047 azimuth=0.000 twist=0.000",
            "leave p3* pen d2 t=1593498300 (46, 40)",
        ],
    );
}

/// AppKit mouse clicks counted by the platform, a force-touch pressure move
/// and a pinch-and-rotate gesture session, stamped on system uptime.
fn appkit_recordings_decode_to_their_host_input() {
    assert_replays(
        include_str!("appkit_mouse_and_pinch.jsonl"),
        &[
            "enter p1* mouse t=3062500000 (50, 60)",
            "move p1* mouse [] t=3125000000 (52, 61)",
            "down p1* mouse b1 [1] x1 t=3250000000 (52, 61)",
            "up p1* mouse b1 [] t=3312500000 (52, 61)",
            "down p1* mouse b1 [1] x2 {SHIFT} t=3375000000 (52, 61)",
            "up p1* mouse b1 [] {SHIFT} t=3437500000 (52, 61)",
            "move p1* mouse [] t=3500000000 (53, 61) pressure=0.500",
            "panzoom p2* trackpad start t=3562500000 (100, 80)",
            "panzoom p2* trackpad update scale=1.250 rotation=0.000 t=3625000000 (100, 80)",
            "panzoom p2* trackpad update scale=1.250 rotation=0.500 t=3687500000 (102, 80)",
            "panzoom p2* trackpad update scale=1.500 rotation=0.500 t=3750000000 (102, 80)",
            "panzoom p2* trackpad end t=3937500000 (102, 80)",
            "leave p1* mouse t=4000000000 (53, 61)",
        ],
    );
}

/// UIKit touches stamped on system uptime, a second finger that is not
/// primary, a system cancel, and a Pencil whose hover is stamped on the
/// event epoch and whose contact keeps the reserved identity.
fn uikit_recordings_decode_to_their_host_input() {
    assert_replays(
        include_str!("uikit_touches_and_pencil.jsonl"),
        &[
            "down p2* touch b1 [1] t=4125000000 (100, 200) pressure=0.250 size=20x20",
            "down p3 touch b1 [1] t=4187500000 (150, 200) pressure=0.500 size=18x18",
            "move p2* touch [1] t=4250000000 (104, 200) pressure=0.250 size=20x20",
            "up p3 touch b1 [] t=4312500000 (150, 200) pressure=0.000 size=18x18",
            "cancel p2* touch Platform t=4375000000",
            "enter p1* pen t=4600000000 (300, 300)",
            "move p1* pen [] t=4600000000 (300, 300) altitude=1.000 azimuth=0.500",
            "down p1* pen b1 [1] t=4625000000 (301, 300) pressure=0.500 altitude=0.750 azimuth=0.500",
            "move p1* pen [1] t=4687500000 (303, 302) pressure=0.625 altitude=0.750 azimuth=0.500",
            "up p1* pen b1 [] t=4750000000 (303, 302) pressure=0.000 altitude=0.750 azimuth=0.500",
            "move p1* pen [] t=4800000000 (303, 302) altitude=0.750 azimuth=0.500",
            "leave p1* pen t=4850000000 (303, 302)",
        ],
    );
}

/// Android multi-touch with a historical batch, and a stylus whose device is
/// removed mid-contact.
fn android_recordings_decode_to_their_host_input() {
    assert_replays(
        include_str!("android_touches_and_stylus.jsonl"),
        &[
            "device+ d1 unknown t=1000000000",
            "down p1* touch d1 b1 [1] t=1000000000 (100, 200) pressure=0.500 size=20x20",
            "down p2 touch d1 b1 [1] t=1010000000 (300, 200) pressure=0.500 size=20x20",
            "move p1* touch d1 [1] t=1016000000 (104, 200) pressure=0.500 size=20x20 coalesced[ t=1012000000 (102, 200) pressure=0.500 size=20x20 ]",
            "up p2 touch d1 b1 [] t=1020000000 (300, 200) pressure=0.500 size=20x20",
            "device+ d2 unknown t=1030000000",
            "down p3* pen d2 b1 [1] t=1030000000 (50, 50) pressure=0.750 altitude=1.250 azimuth=0.500",
            "cancel p3* pen d2 DeviceRemoved t=1040000000",
            "device- d2 pen t=1040000000",
            "up p1* touch d1 b1 [] t=1050000000 (104, 200) pressure=0.500 size=20x20",
        ],
    );
}

/// The winit fallback: a mouse chord on a window surface, a touchpad
/// pressure reading without a position, a cancelled touch, and a pinch
/// centred on the device's cursor.
fn winit_recordings_decode_to_their_host_input() {
    assert_replays(
        include_str!("winit_mouse_touch_pinch.jsonl"),
        &[
            "device+ d1 mouse t=1000000000",
            "enter p1* mouse d1 t=1000000000",
            "move p1* mouse d1 [] t=1005000000 (100, 60)",
            "down p1* mouse d1 b1 [1] t=1010000000 (100, 60)",
            "press p1* mouse d1 b2 [1, 2] t=1020000000 (100, 60)",
            "release p1* mouse d1 b2 [1] t=1030000000 (100, 60)",
            "up p1* mouse d1 b1 [] t=1040000000 (100, 60)",
            "move p1* mouse d1 [] t=1045000000 (100, 60) pressure=0.500",
            "leave p1* mouse d1 t=1050000000 (100, 60)",
            "device+ d2 touch t=1100000000",
            "down p2* touch d2 b1 [1] t=1100000000 (200, 200)",
            "move p2* touch d2 [1] t=1110000000 (202, 200)",
            "cancel p2* touch d2 Platform t=1120000000",
            "panzoom p3* trackpad d1 start t=1200000000 (100, 60)",
            "panzoom p3* trackpad d1 update scale=1.500 rotation=0.000 t=1200000000 (100, 60)",
            "panzoom p3* trackpad d1 end t=1210000000 (100, 60)",
        ],
    );
}

/// DOM pointer events: a pen move carrying the browser's coalesced and
/// predicted events, a mouse chord, and a touch the browser cancelled.
fn web_recordings_decode_to_their_host_input() {
    assert_replays(
        include_str!("web_pen_mouse_touch.jsonl"),
        &[
            "enter p1* pen t=5000000000 (10, 10)",
            "down p1* pen b1 [1] t=5010000000 (10, 10) pressure=0.500 altitude=1.250 azimuth=0.500 twist=0.785",
            "move p1* pen [1] t=5018000000 (14, 10) pressure=0.500 coalesced[ t=5014000000 (12, 10) pressure=0.500 ] predicted[ t=5022000000 (16, 10) pressure=0.500 ]",
            "up p1* pen b1 [] t=5030000000 (14, 10) pressure=0.000",
            "leave p1* pen t=5040000000 (14, 10)",
            "down p2* mouse b1 [1] t=5100000000 (50, 50)",
            "press p2* mouse b2 [1, 2] t=5110000000 (50, 50)",
            "release p2* mouse b1 [2] t=5120000000 (51, 50)",
            "up p2* mouse b2 [] t=5130000000 (51, 50)",
            "down p3* touch b1 [1] t=5200000000 (70, 70)",
            "cancel p3* touch Platform t=5210000000",
        ],
    );
}

/// Every checked-in recording, by name.
const RECORDINGS: [(&str, &str); 7] = [
    (
        "win32_pen_touch_mouse",
        include_str!("win32_pen_touch_mouse.jsonl"),
    ),
    (
        "win32_injected_touch_and_pen",
        include_str!("win32_injected_touch_and_pen.jsonl"),
    ),
    (
        "appkit_mouse_and_pinch",
        include_str!("appkit_mouse_and_pinch.jsonl"),
    ),
    (
        "uikit_touches_and_pencil",
        include_str!("uikit_touches_and_pencil.jsonl"),
    ),
    (
        "android_touches_and_stylus",
        include_str!("android_touches_and_stylus.jsonl"),
    ),
    (
        "winit_mouse_touch_pinch",
        include_str!("winit_mouse_touch_pinch.jsonl"),
    ),
    (
        "web_pen_mouse_touch",
        include_str!("web_pen_mouse_touch.jsonl"),
    ),
];

/// The codec writes each checked-in recording back byte for byte: the files
/// are in the written form, every field read is written, and the header's
/// device list is the one the rows imply.
fn native_recordings_write_back_byte_for_byte() {
    for (name, text) in RECORDINGS {
        let mut written = Vec::new();
        recording(text)
            .write(&mut written)
            .expect("a read recording writes");
        assert_eq!(
            String::from_utf8(written).expect("JSON is UTF-8"),
            text.replace("\r\n", "\n"),
            "{name}"
        );
    }
}

fn read_error(text: &str) -> CodecError {
    NativeRecording::read(text.as_bytes()).expect_err("the recording is refused")
}

const HEADER: &str = r#"{"flui_native_recording":1,"backend":"web","epoch_ns":0,"uptime_ns":0,"devices":[{"device":4,"kinds":["touch"]}]}"#;

/// An unknown row, action or kind is a typed error naming its line and
/// spelling, never a skipped row; an unknown field is skipped.
fn an_unknown_native_variant_is_a_typed_error_not_a_skipped_row() {
    let touch = r#""contact":1,"kind":"touch","action":"move","time":{"epoch_ns":5}"#;
    for (case, row, tag) in [
        ("row", r#"{"row":"teleport"}"#, "teleport"),
        (
            "action",
            r#"{"row":"reading","reading":{"contact":1,"kind":"touch","action":"hover","time":{"epoch_ns":5}}}"#,
            "hover",
        ),
        (
            "kind",
            r#"{"row":"reading","reading":{"contact":1,"kind":"glove","action":"move","time":{"epoch_ns":5}}}"#,
            "glove",
        ),
        (
            "clock",
            r#"{"row":"reading","reading":{"contact":1,"kind":"touch","action":"move","time":{"sundial":5}}}"#,
            "sundial",
        ),
    ] {
        let text = format!("{HEADER}\n{{\"row\":\"reading\",\"reading\":{{{touch}}}}}\n{row}\n");
        assert!(
            matches!(
                read_error(&text),
                CodecError::UnknownVariant { line: 3, tag: found } if found == tag
            ),
            "{case}"
        );
    }
    let skipped = format!(
        "{HEADER}\n{{\"row\":\"reading\",\"later\":1,\"reading\":{{{touch},\"altitude_hint\":2}}}}\n"
    );
    assert_eq!(
        NativeRecording::read(skipped.as_bytes())
            .expect("unknown fields are skipped")
            .rows
            .len(),
        1
    );
}

/// Another major version, a value outside its range and a device the header
/// does not declare are refused.
fn a_native_recording_outside_its_format_is_refused() {
    let version = HEADER.replace(
        r#""flui_native_recording":1"#,
        r#""flui_native_recording":2"#,
    );
    assert!(matches!(
        read_error(&version),
        CodecError::Version {
            found: 2,
            supported: 1
        }
    ));
    let zero_frequency = HEADER.replace(
        r#""uptime_ns":0"#,
        r#""uptime_ns":0,"counter":{"count":1,"frequency":0}"#,
    );
    assert!(matches!(
        read_error(&zero_frequency),
        CodecError::Invalid { line: 1, .. }
    ));
    for (case, device, kind) in [
        ("unknown device", 5, "touch"),
        ("undeclared kind", 4, "pen"),
    ] {
        let text = format!(
            "{HEADER}\n{{\"row\":\"reading\",\"reading\":{{\"device\":{device},\"contact\":1,\"kind\":\"{kind}\",\"action\":\"enter\",\"time\":{{\"epoch_ns\":5}}}}}}\n"
        );
        assert!(
            matches!(
                read_error(&text),
                CodecError::UndeclaredDevice { line: 2, device: found } if found == device
            ),
            "{case}"
        );
    }
    let button = format!(
        "{HEADER}\n{{\"row\":\"reading\",\"reading\":{{\"contact\":1,\"kind\":\"touch\",\"action\":{{\"press\":0}},\"time\":{{\"epoch_ns\":5}}}}}}\n"
    );
    assert!(matches!(
        read_error(&button),
        CodecError::Invalid { line: 2, .. }
    ));
}

#[test]
fn native_recording_contracts() {
    let cases: Vec<(&str, fn())> = vec![
        (
            "win32_pointer_info_recordings_decode_to_their_host_input",
            win32_pointer_info_recordings_decode_to_their_host_input,
        ),
        (
            "appkit_recordings_decode_to_their_host_input",
            appkit_recordings_decode_to_their_host_input,
        ),
        (
            "uikit_recordings_decode_to_their_host_input",
            uikit_recordings_decode_to_their_host_input,
        ),
        (
            "android_recordings_decode_to_their_host_input",
            android_recordings_decode_to_their_host_input,
        ),
        (
            "winit_recordings_decode_to_their_host_input",
            winit_recordings_decode_to_their_host_input,
        ),
        (
            "web_recordings_decode_to_their_host_input",
            web_recordings_decode_to_their_host_input,
        ),
        (
            "native_recordings_write_back_byte_for_byte",
            native_recordings_write_back_byte_for_byte,
        ),
        (
            "an_unknown_native_variant_is_a_typed_error_not_a_skipped_row",
            an_unknown_native_variant_is_a_typed_error_not_a_skipped_row,
        ),
        (
            "a_native_recording_outside_its_format_is_refused",
            a_native_recording_outside_its_format_is_refused,
        ),
    ];
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    let cases = [cases, capture::CASES.to_vec()].concat();
    crate::run_table("native_recording_contracts", &cases);
}
