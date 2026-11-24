//! What the lane reads of the device, as pure parsers over what `adb` printed: the devices the adb
//! server lists and which one a run takes, whether it is an emulator, its display, the commands
//! its `input` has, the windows' frames and the one that holds the input focus, and which
//! accessibility services run.
//!
//! Everything here is the device's output, so each parser reads it as untrusted: a line it cannot
//! read is skipped or answers `None`, never a panic, and what it collects is bounded.

use flui_protocol::session::Isolation;

use super::adb::unavailable;
use super::uiautomator::Bounds;
use crate::error::{Error, Result};

/// The most devices read from one listing.
pub const MAX_DEVICES: usize = 64;

/// The environment variable naming the device a run takes, by its `adb` serial: a run takes no
/// device nobody named, physical or emulated.
pub const SERIAL_ENV: &str = "FLUI_ANDROID_SERIAL";

/// One device of `adb devices -l`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceEntry {
    /// Its serial: `emulator-5554`, `R5CR10ABCDE`.
    pub serial: String,
    /// Its state: `device` when it can be driven, else `unauthorized`, `offline`, …
    pub state: String,
    /// Its model, when listed.
    pub model: Option<String>,
}

/// The devices `adb devices -l` printed as `listing`, at most [`MAX_DEVICES`].
#[must_use]
pub fn parse_devices(listing: &str) -> Vec<DeviceEntry> {
    listing
        .lines()
        .filter(|line| !line.starts_with("List of devices") && !line.starts_with('*'))
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let serial = words.next()?;
            let state = words.next()?;
            let model = words
                .find_map(|word| word.strip_prefix("model:"))
                .map(str::to_owned);
            Some(DeviceEntry {
                serial: serial.to_owned(),
                state: state.to_owned(),
                model,
            })
        })
        .take(MAX_DEVICES)
        .collect()
}

/// The device a run takes: the one `serial` names, which must be listed and ready.
///
/// # Errors
///
/// [`Error::Lane`] marked unavailable, which a report makes cannot-verify: no serial was named
/// (the lane never picks a device by itself), the named device is not listed, or it is listed in
/// a state other than `device`, with what to do about it.
pub fn choose(entries: &[DeviceEntry], serial: Option<&str>) -> Result<DeviceEntry> {
    let Some(serial) = serial else {
        return Err(unnamed());
    };
    let Some(entry) = entries.iter().find(|entry| entry.serial == serial) else {
        let listed: Vec<&str> = entries.iter().map(|entry| entry.serial.as_str()).collect();
        return Err(unavailable(format!(
            "adb lists no device {serial:?}; it lists {listed:?}"
        )));
    };
    match entry.state.as_str() {
        "device" => Ok(entry.clone()),
        "unauthorized" => Err(unavailable(format!(
            "{serial} has not authorized this computer: accept the prompt on the device"
        ))),
        other => Err(unavailable(format!(
            "{serial} is {other}, not ready to be driven"
        ))),
    }
}

/// The refusal of a run that named no device.
#[must_use]
pub fn unnamed() -> Error {
    unavailable(format!(
        "no Android device is named: set {SERIAL_ENV} to the serial `adb devices` lists for the \
         emulator or the device the run may take"
    ))
}

/// What the lane reads of a device once, when it connects.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceFacts {
    /// Its API level (`ro.build.version.sdk`), 0 when unread.
    pub sdk: u32,
    /// An emulator, or a device in someone's hands.
    pub isolation: Isolation,
    /// The commands its `input` has.
    pub input: InputCommands,
    /// Its display in its natural orientation.
    pub display: DisplaySize,
}

/// The shell line the lane reads the device's properties with: the API level, then the two
/// properties an emulator sets (`ro.kernel.qemu` on the older images, `ro.boot.qemu` on the
/// newer), one a line.
pub const PROPERTIES: &str =
    "getprop ro.build.version.sdk; getprop ro.kernel.qemu; getprop ro.boot.qemu";

/// The API level and isolation the [`PROPERTIES`] line printed as `printed`.
#[must_use]
pub fn parse_properties(printed: &str) -> (u32, Isolation) {
    let mut lines = printed.lines().map(str::trim);
    let sdk = lines
        .next()
        .and_then(|line| line.parse().ok())
        .unwrap_or_default();
    let emulator = lines.take(2).any(|line| line == "1");
    let isolation = if emulator {
        Isolation::Emulator
    } else {
        Isolation::Device
    };
    (sdk, isolation)
}

/// The display line: `wm size` and `wm density`.
pub const DISPLAY: &str = "wm size; wm density";

/// The display in its natural orientation, as `wm` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplaySize {
    /// Its panel, in physical pixels.
    pub physical: (i32, i32),
    /// The size apps see: the physical one unless `wm size` overrides it.
    pub natural: (i32, i32),
    /// The density apps see.
    pub density: u32,
}

/// The display, from what the [`DISPLAY`] line printed as `printed`; an override replaces the
/// physical value it overrides.
#[must_use]
pub fn parse_display(printed: &str) -> Option<DisplaySize> {
    let value = |prefix: &str| {
        printed
            .lines()
            .find_map(|line| line.trim().strip_prefix(prefix).map(str::trim))
    };
    let size = |text: &str| -> Option<(i32, i32)> {
        let (width, height) = text.split_once('x')?;
        let width: i32 = width.trim().parse().ok()?;
        let height: i32 = height.trim().parse().ok()?;
        (width > 0 && height > 0).then_some((width, height))
    };
    let physical = size(value("Physical size:")?)?;
    let natural = value("Override size:").and_then(size).unwrap_or(physical);
    let density = value("Override density:")
        .or_else(|| value("Physical density:"))
        .and_then(|text| text.parse().ok())
        .filter(|density: &u32| *density > 0)?;
    Some(DisplaySize {
        physical,
        natural,
        density,
    })
}

/// The commands of the device's `input` the lane needs beyond `keyevent` and `text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct InputCommands {
    /// `motionevent <DOWN|UP|MOVE|CANCEL> <x> <y>`: a contact held between commands.
    pub motion_event: bool,
    /// `keycombination`: keys held together.
    pub key_combination: bool,
    /// `scroll <x> <y> [axis_value]`: a wheel turned at a point.
    pub pointer_scroll: bool,
}

/// The line that prints the device's `input` usage.
pub const INPUT_USAGE: &str = "input help 2>&1";

/// The commands the usage `input help` printed as `usage` lists.
#[must_use]
pub fn parse_input_usage(usage: &str) -> InputCommands {
    let lists = |command: &str| {
        usage
            .lines()
            .map(str::trim_start)
            .any(|line| line.split_whitespace().next() == Some(command))
    };
    InputCommands {
        motion_event: lists("motionevent"),
        key_combination: lists("keycombination"),
        pointer_scroll: usage.contains("scroll <x> <y>"),
    }
}

/// The line that prints the window holding the input focus.
pub const FOCUS: &str = "dumpsys window windows | grep -m1 'mCurrentFocus='";

/// The window holding the input focus, from the [`FOCUS`] line's `printed`: the window's title,
/// `package/activity` for an activity's window, `NotificationShade` for the shade; `None` when no
/// window holds it.
#[must_use]
pub fn parse_focus(printed: &str) -> Option<String> {
    let line = printed
        .lines()
        .find(|line| line.contains("mCurrentFocus="))?;
    let window = line.split_once("mCurrentFocus=")?.1.trim();
    let inside = window.strip_prefix("Window{")?.strip_suffix('}')?;
    // `Window{<hash> u<user> <title>}`: the title is the rest after the user.
    let mut words = inside.splitn(3, ' ');
    let _hash = words.next()?;
    let _user = words.next()?;
    let title = words.next()?.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// The line that prints where the windows are: the display's rotation (`DisplayRotation`'s
/// `mRotation=`), then each window's header (`Window #N Window{hash uU title}:`), its frame
/// (`frame=[l,t][r,b]` on its `Frames:` line from Android 11, `mFrame=[l,t][r,b]` before) and the
/// window holding the input focus (`mCurrentFocus=`), as `dumpsys window` writes them. It reads
/// no accessibility, so it pauses no accessibility service.
pub const WINDOWS: &str = "dumpsys window displays | grep -m1 'mRotation='; \
                           dumpsys window windows | grep -E 'Window #|mCurrentFocus=|rame=\\['";

/// The most bytes of the [`WINDOWS`] reply the lane reads.
pub const MAX_WINDOWS_REPLY: usize = 256 * 1024;

/// The most windows the lane reads of one [`WINDOWS`] reply.
pub const MAX_WINDOWS: usize = 512;

/// One window as `dumpsys window` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WindowFrame {
    /// Its title: `package/activity` for an activity's window, `StatusBar` for the status bar.
    pub title: String,
    /// Its frame in display pixels as the display is turned; `None` when no frame was read.
    pub frame: Option<Bounds>,
}

/// Where the windows are, as the [`WINDOWS`] line's reply shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Windows {
    /// The display's rotation from its natural orientation, in quarter turns
    /// (`Surface.ROTATION_0` to `ROTATION_270`).
    pub rotation: u8,
    /// The windows, in the order `dumpsys` lists them, at most [`MAX_WINDOWS`].
    pub windows: Vec<WindowFrame>,
    /// The title of the window holding the input focus; `None` when no window holds it.
    pub focus: Option<String>,
}

impl Windows {
    /// The frame of the window holding the input focus.
    #[must_use]
    pub fn focused_frame(&self) -> Option<Bounds> {
        let focus = self.focus.as_deref()?;
        self.windows
            .iter()
            .find(|window| window.title == focus)
            .and_then(|window| window.frame)
    }
}

/// The windows the [`WINDOWS`] line printed as `printed`: see [`Windows`]. A window's frame is
/// the first `frame=` or `mFrame=` after its header; a line the parser does not know is skipped.
///
/// # Errors
///
/// When the reply names no rotation from 0 to 3, so the lane cannot tell how the display is
/// turned.
pub fn parse_windows(printed: &str) -> std::result::Result<Windows, String> {
    let mut rotation = None;
    let mut windows: Vec<WindowFrame> = Vec::new();
    let mut in_window = false;
    for line in printed.lines() {
        let line = line.trim();
        if rotation.is_none()
            && let Some(value) = line.strip_prefix("mRotation=")
        {
            let value = value.split_whitespace().next().unwrap_or_default();
            rotation = match value {
                "0" | "ROTATION_0" => Some(0),
                "1" | "ROTATION_90" => Some(1),
                "2" | "ROTATION_180" => Some(2),
                "3" | "ROTATION_270" => Some(3),
                _ => return Err(format!("`dumpsys` printed the rotation {value:?}")),
            };
            continue;
        }
        if line.starts_with("Window #") {
            in_window = false;
            if windows.len() < MAX_WINDOWS
                && let Some(title) = window_title(line)
            {
                windows.push(WindowFrame { title, frame: None });
                in_window = true;
            }
            continue;
        }
        if in_window
            && let Some(window) = windows.last_mut()
            && window.frame.is_none()
        {
            window.frame = frame_of(line);
        }
    }
    let rotation = rotation.ok_or_else(|| "`dumpsys` printed no display rotation".to_owned())?;
    Ok(Windows {
        rotation,
        windows,
        focus: parse_focus(printed),
    })
}

/// The title of the window a `Window #N Window{hash uU title}:` header names.
fn window_title(header: &str) -> Option<String> {
    let inside = header.split_once("Window{")?.1.rsplit_once('}')?.0;
    let mut words = inside.splitn(3, ' ');
    let _hash = words.next()?;
    let _user = words.next()?;
    let title = words.next()?.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// The frame a window's line gives: its `frame=[l,t][r,b]` (Android 11 and later, on the
/// `Frames:` line) or `mFrame=[l,t][r,b]` (before), never `parent=`, `display=` or another frame.
fn frame_of(line: &str) -> Option<Bounds> {
    let at = line.split_whitespace().find_map(|word| {
        word.strip_prefix("frame=")
            .or_else(|| word.strip_prefix("mFrame="))
    })?;
    Bounds::parse(at)
}

/// The line that prints the accessibility services the user turned on and whether touch
/// exploration is on, one a line (`Settings.Secure`).
pub const ACCESSIBILITY_SETTINGS: &str = "settings get secure enabled_accessibility_services; settings get secure touch_exploration_enabled";

/// The most bytes of the [`ACCESSIBILITY_SETTINGS`] reply the lane reads, which bounds the list
/// of services it scans.
pub const MAX_ACCESSIBILITY_SETTINGS: usize = 16 * 1024;

/// The accessibility services the device runs, as its settings show them: what a
/// `uiautomator dump` suspends while it runs. Every service counts, not only a screen reader:
/// Voice Access, Switch Access, Samsung's Universal Switch and Select to Speak turn on no touch
/// exploration and are suspended all the same.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct EnabledServices {
    /// The services the user turned on, each as Android lists it (`package/class`), in its order.
    pub services: Vec<String>,
    /// Whether touch exploration is on, which a screen reader asks for.
    pub touch_exploration: bool,
}

impl EnabledServices {
    /// Whether nothing would be suspended: no service on and no touch exploration.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.services.is_empty() && !self.touch_exploration
    }

    /// What is on, for people.
    #[must_use]
    pub fn describe(&self) -> String {
        match (self.services.as_slice(), self.touch_exploration) {
            ([], true) => "touch exploration, which a screen reader turns on,".to_owned(),
            ([], false) => "no accessibility service".to_owned(),
            ([service], _) => format!("the accessibility service {service}"),
            (services, _) => format!("the accessibility services {}", services.join(", ")),
        }
    }
}

/// The accessibility services the [`ACCESSIBILITY_SETTINGS`] line's `printed` shows on: the
/// colon-separated list of `enabled_accessibility_services`, and touch exploration.
///
/// `settings get` prints a line for every key: `null` for one never set, and an empty line for a
/// list emptied when its last service was turned off, so a reply is exactly two lines, the list
/// (`null`, empty, or components) and `0`, `1` or `null`. The shell reports only the second
/// command's status, so a first command that failed shows only in the reply's shape: it prints
/// no line.
///
/// # Errors
///
/// What is wrong with a reply of any other shape: the lane cannot tell whether a service runs.
pub fn parse_enabled_services(printed: &str) -> std::result::Result<EnabledServices, String> {
    let lines: Vec<&str> = printed.lines().map(str::trim).collect();
    let [services, touch_exploration] = lines.as_slice() else {
        return Err(format!(
            "`settings` printed {} lines where it prints two",
            lines.len()
        ));
    };
    let touch_exploration = match *touch_exploration {
        "1" => true,
        "0" | "null" => false,
        other => {
            return Err(format!(
                "`settings` printed {other:?} for touch exploration, where it prints 0, 1 or null"
            ));
        }
    };
    let services = match *services {
        "null" => Vec::new(),
        list => list
            .split(':')
            .map(str::trim)
            .filter(|component| !component.is_empty())
            .map(str::to_owned)
            .collect(),
    };
    Ok(EnabledServices {
        services,
        touch_exploration,
    })
}

/// A name `am`, `pm` and `input` take whole: an application id or an activity class, its
/// characters Java's and `/` alone.
///
/// # Errors
///
/// [`Error::InvalidArgument`] for an empty name, or one holding any other character.
pub fn check_component(name: &str, what: &str) -> Result<()> {
    let fits = !name.is_empty()
        && name.len() <= 255
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._$/".contains(character));
    if fits {
        Ok(())
    } else {
        Err(Error::invalid_argument(format!(
            "{name:?} is no {what}: it holds letters, digits, `.`, `_`, `$` and `/` only"
        )))
    }
}
