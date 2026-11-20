//! The lane's `uinput` devices, as the JSON lines Android's `uinput` command reads, and what it
//! answers.
//!
//! `uinput -` reads commands from its input, one JSON object each: `register` makes a kernel
//! input device the system's `InputReader` reads as hardware (so a pen hovers, presses and tilts as
//! a pen does), `inject` sends one report, and `sync` makes it answer
//! `{"reason":"sync","id":…,"syncToken":…}` once everything before it was injected: the lane
//! records a report as taken only on that answer. The device lives as long as the command.
//!
//! Without root, the shell may open `/dev/uinput` only on some devices, so the lane probes once a
//! session: it registers [`PROBE_NAME`], asks for a sync, looks for the name among the kernel's
//! input devices, and ends the command, which removes the device. A device that fails any step
//! has no `uinput_device`, and its pen answers `not_supported` naming it.
//!
//! The command reads a register's fields in order and its configuration's `data` by its `type`,
//! so the lines are written in that order by hand; every name in them is a Linux input constant
//! or one of the lane's own, so none needs escaping.

use serde::Deserialize;

use crate::coordinates::DevicePoint;

/// The line that starts the command, reading from its input.
pub const COMMAND: &str = "uinput -";

/// The probe's device.
pub const PROBE_NAME: &str = "FLUI Driver Probe";

/// The lane's pen.
pub const STYLUS_NAME: &str = "FLUI Driver Stylus";

/// The line that lists the kernel's input devices.
pub const DEVICES: &str = "cat /proc/bus/input/devices";

/// The most bytes of the kernel's device list the lane reads.
pub const MAX_DEVICE_LIST: usize = 256 * 1024;

/// The pen's pressure range, from 0 to this.
pub const PRESSURE_MAX: i32 = 4095;

/// The pressure of a pen pressed down: half its range.
pub const PRESSED: i32 = PRESSURE_MAX / 2;

/// A device the command registers: its keys, its absolute axes as `(code, minimum, maximum,
/// resolution)`, and its properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registration<'a> {
    /// The id later commands name it by.
    pub id: u32,
    /// Its name, which the guard tells the lane's devices by.
    pub name: &'a str,
    /// Its USB product id.
    pub product: u32,
    /// Its keys.
    pub keys: &'a [&'a str],
    /// Its absolute axes.
    pub axes: &'a [(&'a str, i32, i32, i32)],
    /// Its properties.
    pub props: &'a [&'a str],
}

/// The probe's device: a button, enough for the kernel to make it a device.
#[must_use]
pub fn probe() -> Registration<'static> {
    Registration {
        id: 1,
        name: PROBE_NAME,
        product: 0xabcc,
        keys: &["BTN_TOUCH"],
        axes: &[],
        props: &[],
    }
}

/// The id the pen's device is registered with.
pub const STYLUS_ID: u32 = 2;

/// The pen's keys.
const STYLUS_KEYS: &[&str] = &["BTN_TOOL_PEN", "BTN_TOUCH", "BTN_STYLUS"];

/// The pen's properties: a pen on the display, not a tablet beside it.
const STYLUS_PROPS: &[&str] = &["INPUT_PROP_DIRECT"];

/// The pen's axes on a display of `width` by `height` physical pixels in its natural orientation:
/// one unit a pixel, the pressure and the tilt of a pen.
#[must_use]
pub fn stylus_axes(width: i32, height: i32) -> [(&'static str, i32, i32, i32); 5] {
    [
        ("ABS_X", 0, width.saturating_sub(1), 0),
        ("ABS_Y", 0, height.saturating_sub(1), 0),
        ("ABS_PRESSURE", 0, PRESSURE_MAX, 0),
        ("ABS_TILT_X", -90, 90, 0),
        ("ABS_TILT_Y", -90, 90, 0),
    ]
}

/// The pen, with `axes` from [`stylus_axes`].
#[must_use]
pub fn stylus<'a>(axes: &'a [(&'a str, i32, i32, i32)]) -> Registration<'a> {
    Registration {
        id: STYLUS_ID,
        name: STYLUS_NAME,
        product: 0xabcd,
        keys: STYLUS_KEYS,
        axes,
        props: STYLUS_PROPS,
    }
}

impl Registration<'_> {
    /// The register command.
    #[must_use]
    pub fn command(&self) -> String {
        let names = |names: &[&str]| {
            names
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(",")
        };
        let codes: Vec<&str> = self.axes.iter().map(|axis| axis.0).collect();
        let infos = self
            .axes
            .iter()
            .map(|(code, minimum, maximum, resolution)| {
                format!(
                    "{{\"code\":\"{code}\",\"info\":{{\"value\":0,\"minimum\":{minimum},\
                     \"maximum\":{maximum},\"fuzz\":0,\"flat\":0,\"resolution\":{resolution}}}}}"
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut configuration = vec![
            format!(
                "{{\"type\":\"UI_SET_EVBIT\",\"data\":[\"EV_KEY\"{}]}}",
                if self.axes.is_empty() {
                    ""
                } else {
                    ",\"EV_ABS\""
                }
            ),
            format!(
                "{{\"type\":\"UI_SET_KEYBIT\",\"data\":[{}]}}",
                names(self.keys)
            ),
        ];
        if !self.axes.is_empty() {
            configuration.push(format!(
                "{{\"type\":\"UI_SET_ABSBIT\",\"data\":[{}]}}",
                names(&codes)
            ));
        }
        if !self.props.is_empty() {
            configuration.push(format!(
                "{{\"type\":\"UI_SET_PROPBIT\",\"data\":[{}]}}",
                names(self.props)
            ));
        }
        format!(
            "{{\"id\":{},\"command\":\"register\",\"name\":\"{}\",\"vid\":6353,\"pid\":{},\
             \"bus\":\"usb\",\"configuration\":[{}],\"abs_info\":[{infos}]}}",
            self.id,
            self.name,
            self.product,
            configuration.join(","),
        )
    }
}

/// One report to device `id`: `events` as `(type, code, value)`, then `SYN_REPORT`.
#[must_use]
pub fn inject(id: u32, events: &[(&str, &str, i32)]) -> String {
    let events = events
        .iter()
        .chain([&("EV_SYN", "SYN_REPORT", 0)])
        .map(|(kind, code, value)| format!("\"{kind}\",\"{code}\",{value}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"id\":{id},\"command\":\"inject\",\"events\":[{events}]}}")
}

/// A sync of device `id` with `token`.
#[must_use]
pub fn sync(id: u32, token: u64) -> String {
    format!("{{\"id\":{id},\"command\":\"sync\",\"syncToken\":\"{token}\"}}")
}

/// An answer of the command.
#[derive(Debug, Deserialize)]
struct Answer {
    reason: String,
    id: Option<u32>,
    #[serde(rename = "syncToken")]
    sync_token: Option<String>,
}

/// Whether `record`, one object the command printed, answers the sync of device `id` with
/// `token`.
#[must_use]
pub fn answers_sync(record: &str, id: u32, token: u64) -> bool {
    serde_json::from_str::<Answer>(record).is_ok_and(|answer| {
        answer.reason == "sync"
            && answer.id == Some(id)
            && answer.sync_token.as_deref() == Some(token.to_string().as_str())
    })
}

/// Whether the kernel's device list `list` (`/proc/bus/input/devices`) names a device `name`.
#[must_use]
pub fn lists_device(list: &str, name: &str) -> bool {
    list.lines()
        .filter_map(|line| line.trim().strip_prefix("N: Name="))
        .any(|listed| listed.trim_matches('"') == name)
}

/// The pen's report to move to `at`, entering range first when it is out of it.
#[must_use]
pub fn pen_move(at: DevicePoint, in_range: bool) -> Vec<(&'static str, &'static str, i32)> {
    let mut events = Vec::with_capacity(3);
    if !in_range {
        events.push(("EV_KEY", "BTN_TOOL_PEN", 1));
    }
    events.push(("EV_ABS", "ABS_X", at.x));
    events.push(("EV_ABS", "ABS_Y", at.y));
    events
}

/// The pen's report to touch down or lift.
#[must_use]
pub fn pen_touch(down: bool) -> Vec<(&'static str, &'static str, i32)> {
    vec![
        ("EV_KEY", "BTN_TOUCH", i32::from(down)),
        ("EV_ABS", "ABS_PRESSURE", if down { PRESSED } else { 0 }),
    ]
}

/// The pen's report to leave range.
#[must_use]
pub fn pen_leave() -> Vec<(&'static str, &'static str, i32)> {
    vec![("EV_KEY", "BTN_TOOL_PEN", 0)]
}

/// The `size` of a display in its natural orientation, as it is turned `rotation` quarter turns:
/// a quarter or three quarters swap its sides.
#[must_use]
pub fn turned(size: (i32, i32), rotation: u8) -> (i32, i32) {
    if rotation.is_multiple_of(2) {
        size
    } else {
        (size.1, size.0)
    }
}

/// Where `point`, in display pixels as the display is turned `rotation` quarter turns
/// counter-clockwise from its natural orientation, lies on a direct device whose axes span the
/// `physical` display in its natural orientation, apps seeing it as `natural` (a size `wm size`
/// overrides): the inverse of `InputReader`'s turn of a touch screen's raw axes. `None` when the
/// point is off the display.
#[must_use]
pub fn natural_point(
    point: DevicePoint,
    rotation: u8,
    natural: (i32, i32),
    physical: (i32, i32),
) -> Option<DevicePoint> {
    let (width, height) = natural;
    let (turned_width, turned_height) = turned(natural, rotation);
    let inside = (0..turned_width).contains(&point.x) && (0..turned_height).contains(&point.y);
    if !inside {
        return None;
    }
    let (x, y) = match rotation {
        0 => (point.x, point.y),
        // The natural top-left corner lands at the bottom left.
        1 => (width - 1 - point.y, point.x),
        2 => (width - 1 - point.x, height - 1 - point.y),
        3 => (point.y, height - 1 - point.x),
        _ => return None,
    };
    // An overridden size is a scale of the panel; both sizes are positive and at most i32.
    let scale = |value: i32, from: i32, to: i32| -> Option<i32> {
        let scaled = i64::from(value) * i64::from(to) / i64::from(from);
        i32::try_from(scaled).ok()
    };
    Some(DevicePoint::new(
        scale(x, width, physical.0)?,
        scale(y, height, physical.1)?,
    ))
}
