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

/// A window's hash, the id `Window{hash uU title}` names it by (`WindowState`'s identity hash):
/// two windows of one title (an activity's window leaving as it is started again, split screen)
/// differ in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WindowHash(String);

impl std::fmt::Display for WindowHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A display's id, as `mDisplayId=` prints it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayId(u32);

impl DisplayId {
    /// The display of the id `id`.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// The display an `mDisplayId=` word's `value` names: the digits before any trailing mark
    /// (`0,`); `None` for a value that starts with no digit (`-1`, `INVALID_DISPLAY`).
    fn parse(value: &str) -> Option<Self> {
        let digits = value.split(|c: char| !c.is_ascii_digit()).next()?;
        digits.parse::<u32>().ok().map(Self)
    }
}

impl std::fmt::Display for DisplayId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A window as `Window{hash uU title}` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WindowName {
    /// Its hash, which tells it from another window of its title.
    pub hash: WindowHash,
    /// Its title: `package/activity` for an activity's window, `NotificationShade` for the shade.
    pub title: String,
}

impl WindowName {
    /// The window `text` names as `Window{hash uU title}`, wherever in `text` it stands; `None`
    /// when it names no hash or no title.
    fn parse(text: &str) -> Option<Self> {
        let inside = text.split_once("Window{")?.1.rsplit_once('}')?.0;
        // `Window{<hash> u<user> <title>}`: the title is the rest after the user.
        let mut words = inside.splitn(3, ' ');
        let hash = words.next()?;
        let _user = words.next()?;
        let title = words.next()?.trim();
        (!hash.is_empty() && !title.is_empty()).then(|| Self {
            hash: WindowHash(hash.to_owned()),
            title: title.to_owned(),
        })
    }
}

/// The line that prints where the windows are, from one `dumpsys window -a`, which the window
/// manager dumps in one `synchronized (mGlobalLock)` block (AOSP `WindowManagerService.doDump`),
/// so the reply is of one moment: the parts' headers (`WINDOW MANAGER LAST ANR`,
/// `… DISPLAY CONTENTS`, `… WINDOWS`, …); in the displays' part each display's header
/// (`Display: mDisplayId=N`), the window holding its input focus (`mCurrentFocus=`) and its
/// rotation (`DisplayRotation`'s `mRotation=`); in the windows' part each window's header
/// (`Window #N Window{hash uU title}:`), its display (`mDisplayId=`, the first word of its dump),
/// its frame (`frame=[l,t][r,b]` on its `Frames:` line from Android 11, `mFrame=[l,t][r,b]`
/// before) and the display holding the device's input focus (`mTopFocusedDisplayId=`, from
/// Android 10).
///
/// `-a` is what prints the frames: `doDump` passes `dumpAll` only for it, and
/// `WindowState.dump` prints a window's frames only `if (dumpAll)` (Android 9 to 15;
/// `WindowState.java` line 4152 in android-14.0.0_r1), where the `windows` sub-command passed
/// `true` itself. The cost is the whole dump, every part, under the window manager's lock, for
/// every locate and before every press, key, text and scroll: the device's window changes wait
/// for it, and `grep` keeps the reply small. It reads no accessibility, so it pauses no
/// accessibility service.
pub const WINDOWS: &str = concat!(
    "dumpsys window -a | grep -E ",
    "'WINDOW MANAGER |mDisplayId=|mCurrentFocus=|mTopFocusedDisplayId=|",
    "mRotation=|Window #|rame=\\['"
);

/// The display the lane drives, the default one: the one `input`, `screencap` and
/// `uiautomator dump` address, whose rotation the lane reads, and where the app's window must be.
pub const DEFAULT_DISPLAY: DisplayId = DisplayId::new(0);

/// The most bytes of the [`WINDOWS`] reply the lane reads.
pub const MAX_WINDOWS_REPLY: usize = 256 * 1024;

/// The most windows the lane reads of one [`WINDOWS`] reply.
pub const MAX_WINDOWS: usize = 512;

/// One window as `dumpsys window` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListedWindow {
    /// Its hash and title.
    pub name: WindowName,
    /// The display it is on (its `mDisplayId=`); `None` when the dump named none, or named it by
    /// no number.
    pub display: Option<DisplayId>,
    /// Its frame in display pixels as the display is turned; `None` when no frame was read.
    pub frame: Option<Bounds>,
}

/// Where the windows are, as the [`WINDOWS`] line's reply shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Windows {
    /// The default display's rotation from its natural orientation, in quarter turns
    /// (`Surface.ROTATION_0` to `ROTATION_270`).
    pub rotation: u8,
    /// The windows, in the order `dumpsys` lists them, at most [`MAX_WINDOWS`].
    pub windows: Vec<ListedWindow>,
    /// The window holding the default display's input focus ([`DEFAULT_DISPLAY`]), the display
    /// the lane drives; `None` when no window holds it there, a window holding another display's
    /// focus included. Before Android 10, where no display's section prints a focus, the one
    /// focus the windows' part prints.
    pub focus: Option<WindowName>,
    /// The display holding the device's input focus, where keys and text go
    /// (`mTopFocusedDisplayId=`, Android 10 and later); `None` where the reply names none or
    /// names it by no number.
    pub focused_display: Option<DisplayId>,
}

impl Windows {
    /// The window holding the default display's input focus ([`Self::focus`]), found by its hash
    /// in the list: a window of the same title is another window.
    #[must_use]
    pub fn focused_window(&self) -> Option<&ListedWindow> {
        let focus = self.focus.as_ref()?;
        self.windows
            .iter()
            .find(|window| window.name.hash == focus.hash)
    }

    /// The frame of the window holding the input focus, when that window is on the default
    /// display ([`DEFAULT_DISPLAY`]), the one the rotation, `input` and `screencap` are of.
    #[must_use]
    pub fn focused_frame(&self) -> Option<Bounds> {
        self.focused_window()
            .filter(|window| window.display == Some(DEFAULT_DISPLAY))
            .and_then(|window| window.frame)
    }
}

/// A part of the [`WINDOWS`] reply, as its header names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartName {
    /// `WINDOW MANAGER LAST ANR (dumpsys window lastanr)`.
    LastAnr,
    /// `WINDOW MANAGER DISPLAY CONTENTS (dumpsys window displays)`.
    DisplayContents,
    /// `WINDOW MANAGER WINDOWS (dumpsys window windows)`.
    Windows,
    /// Any other part's header.
    Other,
}

impl PartName {
    /// The part `line` heads, when it is a part's header as the window manager prints one: a
    /// whole line from its first column, `WINDOW MANAGER <NAME> (dumpsys window <command>)`, the
    /// name in capitals and the command in small letters. A line of any other shape heads no
    /// part.
    fn of_header(line: &str) -> Option<Self> {
        let rest = line.strip_prefix("WINDOW MANAGER ")?;
        let (name, command) = rest.split_once(" (dumpsys window ")?;
        let command = command.strip_suffix(')')?;
        let shaped = !name.is_empty()
            && name.chars().all(|c| c.is_ascii_uppercase() || c == ' ')
            && !command.is_empty()
            && command.chars().all(|c| c.is_ascii_lowercase());
        shaped.then_some(match (name, command) {
            ("LAST ANR", "lastanr") => Self::LastAnr,
            ("DISPLAY CONTENTS", "displays") => Self::DisplayContents,
            ("WINDOWS", "windows") => Self::Windows,
            _ => Self::Other,
        })
    }
}

/// Which part of the [`WINDOWS`] reply a line is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Before any part's header: a reply that heads no part, read whole.
    Unheaded,
    /// The live displays' part: each display's header, focus and rotation.
    Displays,
    /// The live windows' part: each window's header, display and frame.
    Windows,
    /// Any other part, the `LAST ANR` part and the displays' part it prints included: not read.
    Other,
}

/// Where the live parts of the [`WINDOWS`] reply begin, by the line of their header.
///
/// `dumpsys window -a` prints each part's header once, `LAST ANR` first: that part holds, when an
/// ANR happened, the windows and a displays' part of the ANR's moment, under a displays' header
/// of its own; then the live displays' part, and later the one windows' part. A window's title
/// can hold line breaks and print a header's whole line, so the headers are counted against that
/// shape before any part is read: a reply with a `LAST ANR` or windows' header more than once,
/// or a displays' header more or fewer times than its `LAST ANR` part accounts for, holds a
/// forged one, and is refused rather than read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveParts {
    displays: usize,
    windows: usize,
}

impl LiveParts {
    /// The live parts of the reply whose lines are `lines`; `None` for a reply that heads no
    /// part, which is read whole.
    fn of(lines: &[&str]) -> std::result::Result<Option<Self>, String> {
        let headers: Vec<(usize, PartName)> = lines
            .iter()
            .enumerate()
            .filter_map(|(at, line)| PartName::of_header(line).map(|name| (at, name)))
            .collect();
        if headers.is_empty() {
            return Ok(None);
        }
        let named = |wanted: PartName| {
            headers
                .iter()
                .filter(move |(_, name)| *name == wanted)
                .map(|(at, _)| *at)
        };
        let refused = |why: &str| {
            Err(format!(
                "`dumpsys` printed {why}, which the window manager does not: a window's title \
                 may have forged a part's header"
            ))
        };
        let last_anr: Vec<usize> = named(PartName::LastAnr).collect();
        let windows: Vec<usize> = named(PartName::Windows).collect();
        let displays: Vec<usize> = named(PartName::DisplayContents).collect();
        if last_anr.len() > 1 {
            return refused("the `LAST ANR` part's header more than once");
        }
        let [windows] = windows[..] else {
            return refused("the windows' part's header other than once");
        };
        // The `LAST ANR` part prints lines (the ANR's windows) only when an ANR happened, and a
        // displays' part of the ANR's moment after them.
        let anr_printed = last_anr.first().is_some_and(|&at| {
            let next = headers
                .iter()
                .map(|(header, _)| *header)
                .find(|&header| header > at)
                .unwrap_or(lines.len());
            lines[at + 1..next]
                .iter()
                .any(|line| !line.trim().is_empty())
        });
        let expected = if anr_printed { 2 } else { 1 };
        if displays.len() != expected {
            return refused("a displays' part's header more or fewer times than its parts hold");
        }
        let Some(&live) = displays.last() else {
            return refused("no displays' part");
        };
        if live > windows {
            return refused("the displays' part after the windows' part");
        }
        Ok(Some(Self {
            displays: live,
            windows,
        }))
    }

    /// The part the header at line `at` opens.
    fn part_at(self, at: usize) -> Part {
        if at == self.displays {
            Part::Displays
        } else if at == self.windows {
            Part::Windows
        } else {
            Part::Other
        }
    }
}

impl Part {
    /// Whether the part lists the displays.
    fn lists_displays(self) -> bool {
        matches!(self, Self::Unheaded | Self::Displays)
    }

    /// Whether the part lists the windows.
    fn lists_windows(self) -> bool {
        matches!(self, Self::Unheaded | Self::Windows)
    }
}

/// Which display's section of the [`WINDOWS`] reply a line is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// Before any display's header: a reply that heads no display.
    Unheaded,
    /// After the header of a display, `None` when the header named it by no number.
    Display(Option<DisplayId>),
}

impl Section {
    /// Whether the section's lines are the default display's ([`DEFAULT_DISPLAY`]).
    fn is_default(self) -> bool {
        match self {
            Self::Unheaded => true,
            Self::Display(display) => display == Some(DEFAULT_DISPLAY),
        }
    }
}

/// The first `mCurrentFocus=` line a part of the [`WINDOWS`] reply printed, if any.
#[derive(Debug, Default)]
enum FocusLine {
    /// The part printed none.
    #[default]
    Unprinted,
    /// The window the line named; `None` for a line naming none (`mCurrentFocus=null`).
    Printed(Option<WindowName>),
}

impl FocusLine {
    /// Keep `named` when no line was printed before it.
    fn print(&mut self, named: Option<WindowName>) {
        if matches!(self, Self::Unprinted) {
            *self = Self::Printed(named);
        }
    }

    /// The window this line names, or, where none was printed, the one `fallback` names.
    fn or(self, fallback: Self) -> Option<WindowName> {
        match self {
            Self::Printed(named) => named,
            Self::Unprinted => match fallback {
                Self::Printed(named) => named,
                Self::Unprinted => None,
            },
        }
    }
}

/// What the live displays' part of the [`WINDOWS`] reply says of the default display: the first
/// rotation and the first focus line of its section.
#[derive(Debug, Default)]
struct DisplaysReading {
    rotation: Option<u8>,
    focus: FocusLine,
}

/// What the live windows' part of the [`WINDOWS`] reply lists: the windows, and the first focus
/// line outside any display's section (before Android 10, which prints the focus with the
/// windows).
#[derive(Debug, Default)]
struct WindowsReading {
    windows: Vec<ListedWindow>,
    focus: FocusLine,
    /// The display the first `mTopFocusedDisplayId=` line of its own names.
    focused_display: Option<DisplayId>,
    /// Whether that line was read.
    focused_display_read: bool,
}

/// The windows the [`WINDOWS`] line printed as `printed`: see [`Windows`].
///
/// The reply is read by its parts, each opened by a header as the window manager prints one
/// ([`PartName::of_header`]): the live displays' part and the live windows' part, which
/// [`LiveParts`] finds by counting the headers against the shape the window manager prints, and
/// refuses a reply a window's title forged a header into. No other part is read. In the displays'
/// part, the rotation is the first `mRotation=` of the default display's section
/// ([`DEFAULT_DISPLAY`]) and the focus its first `mCurrentFocus=`, which wins, a `null` one
/// included; where that section prints none (before Android 10), the focus is the windows'
/// part's. A window's frame is the first `frame=` or `mFrame=` after its header. The display
/// holding the device's focus is a line of its own in the windows' part,
/// `mTopFocusedDisplayId=<id>` whole, the first one read. A reply that heads no part is read
/// whole, and a line the parser does not know is skipped.
///
/// # Errors
///
/// When the headers do not have the window manager's shape, or the default display's section
/// names no rotation from 0 to 3, so the lane cannot tell how the display is turned.
pub fn parse_windows(printed: &str) -> std::result::Result<Windows, String> {
    let lines: Vec<&str> = printed.lines().collect();
    let live = LiveParts::of(&lines)?;
    let mut displays = DisplaysReading::default();
    let mut listed = WindowsReading::default();
    let mut part = Part::Unheaded;
    let mut section = Section::Unheaded;
    let mut in_window = false;
    for (at, printed_line) in lines.iter().enumerate() {
        if PartName::of_header(printed_line).is_some() {
            part = live.map_or(Part::Other, |live| live.part_at(at));
            section = Section::Unheaded;
            in_window = false;
            continue;
        }
        if part == Part::Other {
            continue;
        }
        let line = printed_line.trim();
        if part.lists_displays()
            && line.starts_with("Display:")
            && let Some(id) = word_value(line, "mDisplayId=")
        {
            section = Section::Display(DisplayId::parse(id));
            continue;
        }
        if part.lists_windows()
            && section == Section::Unheaded
            && let Some(value) = line.strip_prefix("mTopFocusedDisplayId=")
            && !value.is_empty()
            && value.chars().all(|c| c.is_ascii_digit() || c == '-')
        {
            if !listed.focused_display_read {
                listed.focused_display_read = true;
                listed.focused_display = DisplayId::parse(value);
            }
            continue;
        }
        if let Some((_, window)) = line.split_once("mCurrentFocus=") {
            let named = WindowName::parse(window.trim());
            match section {
                Section::Display(display) if display == Some(DEFAULT_DISPLAY) => {
                    displays.focus.print(named);
                }
                Section::Display(_) => {}
                Section::Unheaded => {
                    listed.focus.print(named);
                }
            }
            continue;
        }
        if displays.rotation.is_none()
            && part.lists_displays()
            && section.is_default()
            && let Some(value) = word_value(line, "mRotation=")
        {
            displays.rotation = Some(match value.trim_end_matches(',') {
                "0" | "ROTATION_0" => 0,
                "1" | "ROTATION_90" => 1,
                "2" | "ROTATION_180" => 2,
                "3" | "ROTATION_270" => 3,
                other => return Err(format!("`dumpsys` printed the rotation {other:?}")),
            });
            continue;
        }
        if part.lists_windows() && line.starts_with("Window #") {
            in_window = false;
            if listed.windows.len() < MAX_WINDOWS
                && let Some(name) = WindowName::parse(line)
            {
                listed.windows.push(ListedWindow {
                    name,
                    display: None,
                    frame: None,
                });
                in_window = true;
            }
            continue;
        }
        if in_window && let Some(window) = listed.windows.last_mut() {
            if window.display.is_none() {
                window.display = word_value(line, "mDisplayId=").and_then(DisplayId::parse);
            }
            if window.frame.is_none() {
                window.frame = frame_of(line);
            }
        }
    }
    let rotation = displays
        .rotation
        .ok_or_else(|| "`dumpsys` printed no rotation of the default display".to_owned())?;
    Ok(Windows {
        rotation,
        windows: listed.windows,
        focus: displays.focus.or(listed.focus),
        focused_display: listed.focused_display,
    })
}

/// The value of the word of `line` that starts with `key`, wherever in the line it stands.
fn word_value<'l>(line: &'l str, key: &str) -> Option<&'l str> {
    line.split_whitespace()
        .find_map(|word| word.strip_prefix(key))
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
