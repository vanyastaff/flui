//! Tool arguments: the JSON shapes agents send, and their validation into
//! the domain types the desktop worker takes.
//!
//! Doc comments on fields become the JSON-schema descriptions agents read.

use std::collections::BTreeMap;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::a11y::Query;
use crate::capture::ShotTarget;
use crate::error::{ToolError, ToolResult};
use crate::input::MouseButton;
use crate::keys::KeyCombo;

/// A window, or every top-level window of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// One window by id.
    Window(u32),
    /// All top-level windows of a process.
    Pid(u32),
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Window(id) => write!(f, "window {id}"),
            Self::Pid(pid) => write!(f, "process {pid}"),
        }
    }
}

/// Exactly one of `window_id` / `pid`.
pub fn required_target(window_id: Option<u32>, pid: Option<u32>) -> ToolResult<Target> {
    optional_target(window_id, pid)?.ok_or_else(|| {
        ToolError::InvalidArgument("pass `window_id` or `pid` (from list_windows)".into())
    })
}

/// At most one of `window_id` / `pid`.
pub fn optional_target(window_id: Option<u32>, pid: Option<u32>) -> ToolResult<Option<Target>> {
    match (window_id, pid) {
        (Some(_), Some(_)) => Err(ToolError::InvalidArgument(
            "pass either `window_id` or `pid`, not both".into(),
        )),
        (Some(id), None) => Ok(Some(Target::Window(id))),
        (None, Some(pid)) => Ok(Some(Target::Pid(pid))),
        (None, None) => Ok(None),
    }
}

/// `list_windows` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListWindowsParams {
    /// Keep only windows whose title contains this text (case-insensitive).
    pub title_contains: Option<String>,
    /// Keep only windows of this process.
    pub pid: Option<u32>,
}

/// `launch` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LaunchParams {
    /// Executable path, or a name found on PATH.
    pub program: String,
    /// Command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: Option<String>,
    /// Extra environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// If set, wait up to this many ms (at most 120000) for the process to
    /// show a window and return it. Some apps (Windows 11 Notepad) hand off to
    /// another process; then no window appears for this pid and list_windows
    /// finds it.
    pub wait_for_window_ms: Option<u64>,
}

/// The longest `launch` waits for a window.
pub const MAX_LAUNCH_WAIT_MS: u64 = 120_000;

impl LaunchParams {
    /// Rejects a window wait above [`MAX_LAUNCH_WAIT_MS`] before anything is
    /// started, rather than shortening it silently.
    pub fn validate(&self) -> ToolResult<()> {
        match self.wait_for_window_ms {
            Some(ms) if ms > MAX_LAUNCH_WAIT_MS => Err(ToolError::InvalidArgument(format!(
                "wait_for_window_ms {ms} is above {MAX_LAUNCH_WAIT_MS}"
            ))),
            _ => Ok(()),
        }
    }
}

/// `kill` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KillParams {
    /// A pid returned by launch in this session.
    pub pid: u32,
}

/// `screenshot` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotParams {
    /// Capture this window (works even when other windows cover it, where
    /// the OS allows).
    pub window_id: Option<u32>,
    /// Capture this process's frontmost window.
    pub pid: Option<u32>,
    /// Capture this monitor (0-based index). With no target at all, the
    /// primary monitor is captured.
    pub monitor: Option<u32>,
    /// Downscale so neither side exceeds this many pixels; the reply's
    /// `scale` maps image pixels back to screen pixels.
    pub max_side: Option<u32>,
}

/// What a screenshot request resolves to, before window lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotTarget {
    /// Already a concrete capture target.
    Direct(ShotTarget),
    /// A process's frontmost window, looked up on the worker.
    Pid(u32),
}

impl ScreenshotParams {
    /// At most one target.
    pub fn target(&self) -> ToolResult<ScreenshotTarget> {
        match (self.window_id, self.pid, self.monitor) {
            (None, None, None) => Ok(ScreenshotTarget::Direct(ShotTarget::Primary)),
            (Some(id), None, None) => Ok(ScreenshotTarget::Direct(ShotTarget::Window(id))),
            (None, Some(pid), None) => Ok(ScreenshotTarget::Pid(pid)),
            (None, None, Some(m)) => Ok(ScreenshotTarget::Direct(ShotTarget::Monitor(m as usize))),
            _ => Err(ToolError::InvalidArgument(
                "pass at most one of `window_id`, `pid`, `monitor`".into(),
            )),
        }
    }
}

/// `accessibility_tree` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TreeParams {
    /// Read this window's tree.
    pub window_id: Option<u32>,
    /// Read the trees of all this process's top-level windows.
    pub pid: Option<u32>,
    /// Levels below each window to include (default 30); deeper children are
    /// counted in `omitted_children`.
    pub max_depth: Option<u32>,
}

/// Default and ceiling for `max_depth`.
pub const DEFAULT_DEPTH: usize = 30;
/// The deepest tree any tool reads: `max_depth`'s ceiling, and `find`'s bound.
pub const MAX_DEPTH: u32 = 200;

impl TreeParams {
    /// The target and depth.
    pub fn validate(&self) -> ToolResult<(Target, usize)> {
        let target = required_target(self.window_id, self.pid)?;
        let depth = match self.max_depth {
            None => DEFAULT_DEPTH,
            Some(d) if d <= MAX_DEPTH => d as usize,
            Some(d) => {
                return Err(ToolError::InvalidArgument(format!(
                    "max_depth {d} is above {MAX_DEPTH}"
                )));
            }
        };
        Ok((target, depth))
    }
}

/// `find` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindParams {
    /// Search this window.
    pub window_id: Option<u32>,
    /// Search all this process's top-level windows.
    pub pid: Option<u32>,
    /// Exact accessible name.
    pub name: Option<String>,
    /// Case-insensitive substring of the accessible name.
    pub name_contains: Option<String>,
    /// Control type, case-insensitive: Button, Text, Edit, CheckBox, ...
    pub role: Option<String>,
    /// Exact automation id.
    pub automation_id: Option<String>,
}

impl FindParams {
    /// The target and a non-empty query.
    pub fn validate(&self) -> ToolResult<(Target, Query)> {
        let target = required_target(self.window_id, self.pid)?;
        let query = Query {
            name: self.name.clone(),
            name_contains: self.name_contains.clone(),
            role: self.role.clone(),
            automation_id: self.automation_id.clone(),
        };
        if query.is_empty() {
            return Err(ToolError::InvalidArgument(
                "pass at least one of `name`, `name_contains`, `role`, `automation_id`".into(),
            ));
        }
        // Every name contains the empty string: it would match everything.
        if query.name_contains.as_deref() == Some("") {
            return Err(ToolError::InvalidArgument(
                "`name_contains` must not be empty".into(),
            ));
        }
        Ok((target, query))
    }
}

/// `wait_for` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitForParams {
    /// Watch this window.
    pub window_id: Option<u32>,
    /// Watch all this process's top-level windows (and wait for one to exist).
    pub pid: Option<u32>,
    /// Exact accessible name.
    pub name: Option<String>,
    /// Case-insensitive substring of the accessible name.
    pub name_contains: Option<String>,
    /// Control type, case-insensitive.
    pub role: Option<String>,
    /// Exact automation id.
    pub automation_id: Option<String>,
    /// Give up after this many ms (default 5000, at most 120000).
    pub timeout_ms: Option<u64>,
}

const DEFAULT_WAIT_MS: u64 = 5_000;
const MAX_WAIT_MS: u64 = 120_000;

impl WaitForParams {
    /// The target, query and timeout.
    pub fn validate(&self) -> ToolResult<(Target, Query, Duration)> {
        let (target, query) = FindParams {
            window_id: self.window_id,
            pid: self.pid,
            name: self.name.clone(),
            name_contains: self.name_contains.clone(),
            role: self.role.clone(),
            automation_id: self.automation_id.clone(),
        }
        .validate()?;
        let ms = self.timeout_ms.unwrap_or(DEFAULT_WAIT_MS);
        if ms > MAX_WAIT_MS {
            return Err(ToolError::InvalidArgument(format!(
                "timeout_ms {ms} is above {MAX_WAIT_MS}"
            )));
        }
        Ok((target, query, Duration::from_millis(ms)))
    }
}

/// `invoke` / `toggle` / `focus` / `select` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ElementParams {
    /// Element id (`e12`) from accessibility_tree, find or wait_for.
    pub element: String,
}

/// `set_value` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetValueParams {
    /// Element id (`e12`).
    pub element: String,
    /// The new value; a number for range controls (sliders).
    pub value: String,
}

/// Mouse button names.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ButtonArg {
    /// Primary button.
    #[default]
    Left,
    /// Secondary button.
    Right,
    /// Wheel button.
    Middle,
}

impl From<ButtonArg> for MouseButton {
    fn from(b: ButtonArg) -> Self {
        match b {
            ButtonArg::Left => Self::Left,
            ButtonArg::Right => Self::Right,
            ButtonArg::Middle => Self::Middle,
        }
    }
}

/// A point in physical screen pixels.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PointArg {
    /// Screen x.
    pub x: i32,
    /// Screen y.
    pub y: i32,
}

/// `click` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClickParams {
    /// Click this element's clickable point. Its window must be in front.
    pub element: Option<String>,
    /// Screen x (physical pixels), with `y`, instead of `element`.
    pub x: Option<i32>,
    /// Screen y (physical pixels), with `x`, instead of `element`.
    pub y: Option<i32>,
    /// left (default), right or middle.
    #[serde(default)]
    pub button: ButtonArg,
    /// Double-click.
    #[serde(default)]
    pub double: bool,
    /// Safety target: refuse unless this window is in front (and holds the point).
    pub window_id: Option<u32>,
    /// Safety target: refuse unless this process owns the foreground window.
    pub pid: Option<u32>,
}

/// Where a click goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickAt {
    /// An element's clickable point.
    Element(String),
    /// A screen point.
    Point(i32, i32),
}

impl ClickParams {
    /// Where to click, and the optional safety target.
    pub fn validate(&self) -> ToolResult<(ClickAt, Option<Target>)> {
        let target = optional_target(self.window_id, self.pid)?;
        let at = match (&self.element, self.x, self.y) {
            (Some(e), None, None) => ClickAt::Element(e.clone()),
            (None, Some(x), Some(y)) => ClickAt::Point(x, y),
            (Some(_), _, _) => {
                return Err(ToolError::InvalidArgument(
                    "pass either `element` or `x`+`y`, not both".into(),
                ));
            }
            _ => {
                return Err(ToolError::InvalidArgument(
                    "pass `element`, or both `x` and `y`".into(),
                ));
            }
        };
        Ok((at, target))
    }
}

/// `move_mouse` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveMouseParams {
    /// Screen x (physical pixels).
    pub x: i32,
    /// Screen y (physical pixels).
    pub y: i32,
}

/// `drag` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DragParams {
    /// Press here.
    pub from: PointArg,
    /// Release here.
    pub to: PointArg,
    /// How long the move takes (default 300 ms, at most 10000).
    pub duration_ms: Option<u64>,
    /// Safety target: refuse unless this window is in front and holds both points.
    pub window_id: Option<u32>,
    /// Safety target: refuse unless this process owns the foreground window.
    pub pid: Option<u32>,
}

impl DragParams {
    /// The safety target and the duration.
    pub fn validate(&self) -> ToolResult<(Option<Target>, Duration)> {
        let target = optional_target(self.window_id, self.pid)?;
        let ms = self.duration_ms.unwrap_or(300);
        if ms > 10_000 {
            return Err(ToolError::InvalidArgument(format!(
                "duration_ms {ms} is above 10000"
            )));
        }
        Ok((target, Duration::from_millis(ms)))
    }
}

/// `scroll` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScrollParams {
    /// Screen x to scroll at.
    pub x: i32,
    /// Screen y to scroll at.
    pub y: i32,
    /// Horizontal wheel notches; positive scrolls right.
    #[serde(default)]
    pub dx: i32,
    /// Vertical wheel notches; positive scrolls down.
    #[serde(default)]
    pub dy: i32,
    /// Safety target: refuse unless this window is in front and holds the point.
    pub window_id: Option<u32>,
    /// Safety target: refuse unless this process owns the foreground window.
    pub pid: Option<u32>,
}

impl ScrollParams {
    /// The safety target; rejects a no-op scroll.
    pub fn validate(&self) -> ToolResult<Option<Target>> {
        let target = optional_target(self.window_id, self.pid)?;
        if self.dx == 0 && self.dy == 0 {
            return Err(ToolError::InvalidArgument(
                "pass a non-zero `dx` or `dy`".into(),
            ));
        }
        if self.dx.unsigned_abs() > 100 || self.dy.unsigned_abs() > 100 {
            return Err(ToolError::InvalidArgument(
                "scroll at most 100 notches per call".into(),
            ));
        }
        Ok(target)
    }
}

/// `type_text` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeTextParams {
    /// Text to type into the focused control, as characters.
    pub text: String,
    /// Safety target: refuse unless this window is in front.
    pub window_id: Option<u32>,
    /// Safety target: refuse unless this process owns the foreground window.
    pub pid: Option<u32>,
}

/// `key` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyParams {
    /// A key or combo: `enter`, `tab`, `ctrl+shift+s`, `alt+f4`, `cmd+q`.
    pub combo: String,
    /// Press it this many times (default 1, at most 100).
    pub repeat: Option<u32>,
    /// Safety target: refuse unless this window is in front.
    pub window_id: Option<u32>,
    /// Safety target: refuse unless this process owns the foreground window.
    pub pid: Option<u32>,
}

impl KeyParams {
    /// The parsed combo, repeat count and safety target.
    pub fn validate(&self) -> ToolResult<(KeyCombo, u32, Option<Target>)> {
        let target = optional_target(self.window_id, self.pid)?;
        let combo = KeyCombo::parse(&self.combo)?;
        let repeat = self.repeat.unwrap_or(1);
        if !(1..=100).contains(&repeat) {
            return Err(ToolError::InvalidArgument(format!(
                "repeat {repeat} is outside 1..=100"
            )));
        }
        Ok((combo, repeat, target))
    }
}

/// `activate_window` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActivateParams {
    /// Bring this window to the front.
    pub window_id: Option<u32>,
    /// Bring this process's frontmost window to the front.
    pub pid: Option<u32>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
        serde_json::from_value(v).expect("BUG: test arguments deserialize")
    }

    #[test]
    fn target_needs_exactly_one_selector() {
        assert_eq!(required_target(Some(7), None).ok(), Some(Target::Window(7)));
        assert_eq!(required_target(None, Some(9)).ok(), Some(Target::Pid(9)));
        assert!(required_target(None, None).is_err());
        assert!(required_target(Some(1), Some(2)).is_err());
        assert_eq!(optional_target(None, None).ok(), Some(None));
        assert!(optional_target(Some(1), Some(2)).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let r: Result<KeyParams, _> =
            serde_json::from_value(json!({"combo": "enter", "windowId": 3}));
        assert!(r.is_err(), "a misspelled safety target must not be ignored");
    }

    #[test]
    fn click_needs_element_or_full_point() {
        let (at, target) = parse::<ClickParams>(json!({"element": "e3", "pid": 5}))
            .validate()
            .expect("BUG: element click is valid");
        assert_eq!(at, ClickAt::Element("e3".into()));
        assert_eq!(target, Some(Target::Pid(5)));
        let (at, _) = parse::<ClickParams>(json!({"x": 10, "y": -4}))
            .validate()
            .expect("BUG: point click is valid");
        assert_eq!(at, ClickAt::Point(10, -4));
        for bad in [
            json!({}),
            json!({"x": 1}),
            json!({"element": "e1", "x": 1, "y": 2}),
            json!({"element": "e1", "window_id": 1, "pid": 2}),
        ] {
            assert!(
                parse::<ClickParams>(bad.clone()).validate().is_err(),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn click_button_names() {
        let p: ClickParams = parse(json!({"x": 0, "y": 0, "button": "right", "double": true}));
        assert!(matches!(p.button, ButtonArg::Right));
        assert!(p.double);
        assert!(serde_json::from_value::<ClickParams>(json!({"button": "RIGHT"})).is_err());
    }

    #[test]
    fn find_requires_a_criterion() {
        assert!(parse::<FindParams>(json!({"pid": 1})).validate().is_err());
        let (target, query) = parse::<FindParams>(json!({"window_id": 4, "role": "Button"}))
            .validate()
            .expect("BUG: a role is a criterion");
        assert_eq!(target, Target::Window(4));
        assert_eq!(query.role.as_deref(), Some("Button"));
    }

    /// A window wait above the ceiling is refused, not shortened.
    #[test]
    fn a_launch_wait_above_the_ceiling_is_refused() {
        assert!(
            parse::<LaunchParams>(json!({"program": "x", "wait_for_window_ms": 120_001}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<LaunchParams>(json!({"program": "x", "wait_for_window_ms": 120_000}))
                .validate()
                .is_ok()
        );
    }

    /// Every name contains "", so an empty substring would match everything.
    #[test]
    fn an_empty_name_contains_is_refused() {
        assert!(
            parse::<FindParams>(json!({"pid": 1, "name_contains": ""}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<WaitForParams>(json!({"pid": 1, "name_contains": ""}))
                .validate()
                .is_err()
        );
    }

    /// `i32::MIN` has no `i32` magnitude; it is refused, not wrapped.
    #[test]
    fn an_i32_min_scroll_is_refused() {
        assert!(
            parse::<ScrollParams>(json!({"x": 0, "y": 0, "dy": i32::MIN}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn wait_for_bounds_the_timeout() {
        let (_, _, t) = parse::<WaitForParams>(json!({"pid": 1, "name": "OK"}))
            .validate()
            .expect("BUG: default timeout is valid");
        assert_eq!(t, Duration::from_millis(DEFAULT_WAIT_MS));
        assert!(
            parse::<WaitForParams>(json!({"pid": 1, "name": "OK", "timeout_ms": 999_999}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn tree_depth_defaults_and_caps() {
        let (_, d) = parse::<TreeParams>(json!({"pid": 1}))
            .validate()
            .expect("BUG: default depth is valid");
        assert_eq!(d, DEFAULT_DEPTH);
        assert!(
            parse::<TreeParams>(json!({"pid": 1, "max_depth": 10_000}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn screenshot_takes_at_most_one_target() {
        assert_eq!(
            ScreenshotParams::default().target().ok(),
            Some(ScreenshotTarget::Direct(ShotTarget::Primary))
        );
        let p: ScreenshotParams = parse(json!({"monitor": 1}));
        assert_eq!(
            p.target().ok(),
            Some(ScreenshotTarget::Direct(ShotTarget::Monitor(1)))
        );
        let p: ScreenshotParams = parse(json!({"monitor": 1, "window_id": 3}));
        assert!(p.target().is_err());
    }

    #[test]
    fn scroll_rejects_no_op_and_huge_scrolls() {
        assert!(
            parse::<ScrollParams>(json!({"x": 0, "y": 0}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<ScrollParams>(json!({"x": 0, "y": 0, "dy": 1000}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<ScrollParams>(json!({"x": 0, "y": 0, "dy": -3}))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn key_validates_combo_and_repeat() {
        let (combo, repeat, _) = parse::<KeyParams>(json!({"combo": "ctrl+s", "repeat": 3}))
            .validate()
            .expect("BUG: valid key args");
        assert_eq!(combo.modifiers.len(), 1);
        assert_eq!(repeat, 3);
        assert!(
            parse::<KeyParams>(json!({"combo": "ctrl+s", "repeat": 0}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<KeyParams>(json!({"combo": "ctrl+"}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn drag_bounds_the_duration() {
        let p: DragParams = parse(json!({"from": {"x": 0, "y": 0}, "to": {"x": 5, "y": 5}}));
        assert_eq!(
            p.validate().ok().map(|(_, d)| d),
            Some(Duration::from_millis(300))
        );
        let p: DragParams =
            parse(json!({"from": {"x": 0, "y": 0}, "to": {"x": 5, "y": 5}, "duration_ms": 60_000}));
        assert!(p.validate().is_err());
    }
}
