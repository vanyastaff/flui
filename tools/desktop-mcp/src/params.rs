//! Tool arguments: the JSON shapes agents send, and their validation into
//! the domain types the desktop worker takes.
//!
//! Doc comments on fields become the JSON-schema descriptions agents read.
//! Every tool that sends input takes a safety target, `window` or `pid`,
//! and every pointer tool takes one [`Location`] shape: an element, a screen
//! point, or a point in a screenshot.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::Duration;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::a11y::{Checked, Node, Query};
use crate::cache::parse_handle;
use crate::error::{HandleKind, ToolError, ToolResult};
use crate::input::MouseButton;
use crate::keys::KeyCombo;

/// A tool's arguments, or why they do not parse. rmcp answers a failed
/// `Parameters<T>` with a JSON-RPC error, which an agent does not see as the
/// tool's result; kept here, the failure becomes a tool error the agent reads
/// and corrects, like every other refusal. The schema is `T`'s.
#[derive(Debug)]
pub struct Args<T>(pub ToolResult<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Args<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self(
            serde_json::from_value(value).map_err(|e| ToolError::InvalidArgument(e.to_string())),
        ))
    }
}

impl<T: JsonSchema> JsonSchema for Args<T> {
    fn inline_schema() -> bool {
        T::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        T::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        T::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        T::json_schema(generator)
    }
}

/// The most characters one `type_text` call types: each is a guarded OS
/// event on the one desktop thread, which every other tool waits behind.
pub const MAX_TEXT_CHARS: usize = 10_000;

/// A window or process as the agent names it: a window handle this
/// session issued (`w3`), or a pid it handed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArg {
    /// A window, by the number in its handle.
    Window(u64),
    /// A process, by pid.
    Pid(u32),
}

impl std::fmt::Display for TargetArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Window(n) => write!(f, "window w{n}"),
            Self::Pid(pid) => write!(f, "process {pid}"),
        }
    }
}

/// A target as the OS knows it, once the session has resolved the handle:
/// the window's native id with the handle it was named by, or a pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// One window: its native id and the handle number the caller used.
    Window(u32, u64),
    /// All top-level windows of a process.
    Pid(u32),
}

impl Target {
    /// The argument this target resolved from.
    pub fn arg(self) -> TargetArg {
        match self {
            Self::Window(_, n) => TargetArg::Window(n),
            Self::Pid(pid) => TargetArg::Pid(pid),
        }
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.arg().fmt(f)
    }
}

/// Exactly one of `window` / `pid`.
pub fn required_target(window: Option<&str>, pid: Option<u32>) -> ToolResult<TargetArg> {
    optional_target(window, pid)?.ok_or_else(|| {
        ToolError::InvalidArgument(
            "pass `window` (a w-id from list_windows) or `pid`: input is only sent with a safety target"
                .into(),
        )
    })
}

/// At most one of `window` / `pid`.
pub fn optional_target(window: Option<&str>, pid: Option<u32>) -> ToolResult<Option<TargetArg>> {
    match (window, pid) {
        (Some(_), Some(_)) => Err(ToolError::InvalidArgument(
            "pass either `window` or `pid`, not both".into(),
        )),
        (Some(w), None) => Ok(Some(TargetArg::Window(parse_handle(
            w,
            HandleKind::Window,
        )?))),
        (None, Some(pid)) => Ok(Some(TargetArg::Pid(pid))),
        (None, None) => Ok(None),
    }
}

/// Where a pointer action lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// An element's clickable point.
    Element(String),
    /// A screen point.
    Point(i32, i32),
    /// A pixel of a screenshot this session took, mapped back to the screen
    /// by the server.
    InScreenshot {
        /// The screenshot's handle number.
        shot: u64,
        /// Image x.
        x: i32,
        /// Image y.
        y: i32,
    },
}

/// One location from the three optional spellings: `element`, `x`+`y`, or
/// `screenshot`+`x`+`y`.
pub fn location(
    element: Option<&str>,
    x: Option<i32>,
    y: Option<i32>,
    screenshot: Option<&str>,
) -> ToolResult<Location> {
    match (element, x, y, screenshot) {
        (Some(e), None, None, None) => {
            parse_handle(e, HandleKind::Element)?;
            Ok(Location::Element(e.to_owned()))
        }
        (None, Some(x), Some(y), None) => Ok(Location::Point(x, y)),
        (None, Some(x), Some(y), Some(s)) => Ok(Location::InScreenshot {
            shot: parse_handle(s, HandleKind::Screenshot)?,
            x,
            y,
        }),
        (Some(_), _, _, _) => Err(ToolError::InvalidArgument(
            "pass either `element` or a point (`x` and `y`, with `screenshot` for image pixels), not both".into(),
        )),
        _ => Err(ToolError::InvalidArgument(
            "pass `element`, or both `x` and `y` (screen coordinates, or image pixels with `screenshot`)".into(),
        )),
    }
}

/// A location as an object, for a tool that takes two (`drag`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocationArg {
    /// An element id (`e12`): its clickable point.
    pub element: Option<String>,
    /// Screen x, or image x with `screenshot`.
    pub x: Option<i32>,
    /// Screen y, or image y with `screenshot`.
    pub y: Option<i32>,
    /// A screenshot id (`s2`): then `x`/`y` are pixels of that image.
    pub screenshot: Option<String>,
}

impl LocationArg {
    /// The location it spells.
    pub fn resolve(&self) -> ToolResult<Location> {
        location(
            self.element.as_deref(),
            self.x,
            self.y,
            self.screenshot.as_deref(),
        )
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
}

/// The most bytes a `launch` command line and environment may carry, all
/// strings together: about what Windows accepts for one command line.
pub const MAX_LAUNCH_BYTES: usize = 32 * 1024;

impl LaunchParams {
    /// Rejects, before anything is started rather than by shortening it
    /// silently: a command line and environment above [`MAX_LAUNCH_BYTES`],
    /// and an environment name the OS would read as something else (`=`
    /// ends a name, NUL a string).
    pub fn validate(&self) -> ToolResult<()> {
        let strings = std::iter::once(&self.program)
            .chain(&self.args)
            .chain(&self.cwd)
            .chain(self.env.iter().flat_map(|(k, v)| [k, v]));
        // Counted as encoded, conservatively: each string can gain a
        // separator and a pair of quotes, and each quote or backslash an
        // escape (Windows' command-line quoting), so an empty argument or a
        // run of backslashes cannot slip a line past the limit.
        let mut bytes = 0_usize;
        for s in strings {
            if s.contains('\0') {
                return Err(ToolError::InvalidArgument(
                    "launch arguments must not contain NUL characters".into(),
                ));
            }
            let escapes = s.bytes().filter(|&b| b == b'"' || b == b'\\').count();
            bytes = bytes.saturating_add(s.len() + escapes + 3);
        }
        if bytes > MAX_LAUNCH_BYTES {
            return Err(ToolError::InvalidArgument(format!(
                "program, args, cwd and env take {bytes} bytes; at most {MAX_LAUNCH_BYTES}"
            )));
        }
        if let Some(bad) = self.env.keys().find(|k| k.is_empty() || k.contains('=')) {
            return Err(ToolError::InvalidArgument(format!(
                "environment name {bad:?} is empty or contains `=`"
            )));
        }
        Ok(())
    }
}

/// `wait_for_window` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitForWindowParams {
    /// The process whose window to wait for (from launch or list_windows).
    pub pid: u32,
    /// Keep only windows whose title contains this text (case-insensitive).
    pub title_contains: Option<String>,
    /// Give up after this many ms (default 10000, at most 120000).
    pub timeout_ms: Option<u64>,
}

const DEFAULT_WINDOW_WAIT_MS: u64 = 10_000;
const MAX_WAIT_MS: u64 = 120_000;

impl WaitForWindowParams {
    /// The pid, the folded title filter and the timeout.
    pub fn validate(&self) -> ToolResult<(u32, Option<String>, Duration)> {
        let ms = self.timeout_ms.unwrap_or(DEFAULT_WINDOW_WAIT_MS);
        if ms > MAX_WAIT_MS {
            return Err(ToolError::InvalidArgument(format!(
                "timeout_ms {ms} is above {MAX_WAIT_MS}"
            )));
        }
        let needle = self.title_contains.as_deref().map(crate::a11y::fold);
        Ok((self.pid, needle, Duration::from_millis(ms)))
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
    pub window: Option<String>,
    /// Capture this process's frontmost window.
    pub pid: Option<u32>,
    /// Capture this monitor (0-based index). With no target at all, the
    /// primary monitor is captured.
    pub monitor: Option<u32>,
    /// Downscale so neither side exceeds this many pixels (default 1920,
    /// at most 4096); the reply's `scale_x`/`scale_y` map image pixels back
    /// to the screen, or pass the reply's id with image pixels to click.
    pub max_side: Option<u32>,
}

/// What a screenshot request captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotTarget {
    /// A window this session issued a handle for.
    Window(u64),
    /// A process's frontmost window.
    Pid(u32),
    /// A monitor, by index.
    Monitor(usize),
    /// The primary monitor.
    Primary,
}

/// The longer side a screenshot is downscaled to when `max_side` is not
/// given: a full 4K or 8K capture as one base64 message is tens of MB, more
/// than MCP clients and models take for an image.
pub const DEFAULT_MAX_SIDE: u32 = 1920;

/// The largest `max_side`: the shrunk image is at most 64 MiB as RGBA, its
/// PNG about as much for content that does not compress, and the base64
/// reply a third more (about 85 MiB); the bitmap read before shrinking is
/// bounded apart (an 8K display, about 127 MiB).
pub const MAX_SIDE: u32 = 4096;

impl ScreenshotParams {
    /// The longer side to downscale to.
    pub fn max_side(&self) -> u32 {
        self.max_side.unwrap_or(DEFAULT_MAX_SIDE)
    }

    /// At most one target.
    pub fn target(&self) -> ToolResult<ScreenshotTarget> {
        if self.max_side == Some(0) {
            return Err(ToolError::InvalidArgument(
                "`max_side` must be at least 1".into(),
            ));
        }
        if self.max_side.is_some_and(|m| m > MAX_SIDE) {
            return Err(ToolError::InvalidArgument(format!(
                "`max_side` is at most {MAX_SIDE}"
            )));
        }
        match (self.window.as_deref(), self.pid, self.monitor) {
            (None, None, None) => Ok(ScreenshotTarget::Primary),
            (Some(w), None, None) => Ok(ScreenshotTarget::Window(parse_handle(
                w,
                HandleKind::Window,
            )?)),
            (None, Some(pid), None) => Ok(ScreenshotTarget::Pid(pid)),
            (None, None, Some(m)) => Ok(ScreenshotTarget::Monitor(m as usize)),
            _ => Err(ToolError::InvalidArgument(
                "pass at most one of `window`, `pid`, `monitor`".into(),
            )),
        }
    }
}

/// How a tree or a list of matches is reported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// One line per element (`- button "Save" [ref=e12]`), a fraction of the
    /// JSON's size.
    #[default]
    Outline,
    /// The full node objects.
    Json,
}

/// What a read covers: a window, every window of a process, or one
/// element's subtree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// A window or a process.
    Target(TargetArg),
    /// The subtree under an element this session issued.
    Element(String),
}

fn scope(window: Option<&str>, pid: Option<u32>, root: Option<&str>) -> ToolResult<Scope> {
    match (optional_target(window, pid)?, root) {
        (Some(_), Some(_)) => Err(ToolError::InvalidArgument(
            "pass `root` alone: it names the element whose subtree to read".into(),
        )),
        (None, Some(root)) => {
            parse_handle(root, HandleKind::Element)?;
            Ok(Scope::Element(root.to_owned()))
        }
        (Some(target), None) => Ok(Scope::Target(target)),
        (None, None) => Err(ToolError::InvalidArgument(
            "pass `window`, `pid` or `root` (an element id)".into(),
        )),
    }
}

/// `accessibility_tree` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TreeParams {
    /// Read this window's tree.
    pub window: Option<String>,
    /// Read the trees of all this process's top-level windows and popups.
    pub pid: Option<u32>,
    /// Read the subtree under this element (`e12`) instead.
    pub root: Option<String>,
    /// Levels below each root to include (default 30, at most 200); deeper
    /// children are counted in `omitted_children`.
    pub max_depth: Option<u32>,
    /// Elements to report at most (default 500, at most 5000); the rest are
    /// counted in `omitted_children` and the reply says `truncated`.
    pub max_nodes: Option<u32>,
    /// `outline` (default) or `json`.
    #[serde(default)]
    pub format: Format,
}

/// Default for `max_depth` ([`MAX_DEPTH`] is its ceiling).
pub const DEFAULT_DEPTH: usize = 30;
/// The deepest tree any tool reads: `max_depth`'s ceiling, and `find`'s bound.
pub const MAX_DEPTH: u32 = 200;
/// Default for `max_nodes`: about what one reply can carry before an MCP
/// client truncates it.
pub const DEFAULT_NODES: usize = 500;
/// The most elements one read fetches.
pub const MAX_NODES: u32 = 5_000;

fn depth(max_depth: Option<u32>) -> ToolResult<usize> {
    match max_depth {
        None => Ok(DEFAULT_DEPTH),
        Some(d) if d <= MAX_DEPTH => Ok(d as usize),
        Some(d) => Err(ToolError::InvalidArgument(format!(
            "max_depth {d} is above {MAX_DEPTH}"
        ))),
    }
}

fn nodes(max_nodes: Option<u32>, default: usize) -> ToolResult<usize> {
    match max_nodes {
        None => Ok(default),
        Some(0) => Err(ToolError::InvalidArgument(
            "max_nodes must be at least 1".into(),
        )),
        Some(n) if n <= MAX_NODES => Ok(n as usize),
        Some(n) => Err(ToolError::InvalidArgument(format!(
            "max_nodes {n} is above {MAX_NODES}"
        ))),
    }
}

impl TreeParams {
    /// The scope, depth, node budget and format.
    pub fn validate(&self) -> ToolResult<(Scope, usize, usize, Format)> {
        Ok((
            scope(self.window.as_deref(), self.pid, self.root.as_deref())?,
            depth(self.max_depth)?,
            nodes(self.max_nodes, DEFAULT_NODES)?,
            self.format,
        ))
    }
}

/// `find` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindParams {
    /// Search this window.
    pub window: Option<String>,
    /// Search all this process's top-level windows and popups.
    pub pid: Option<u32>,
    /// Search under this element (`e12`) instead.
    pub root: Option<String>,
    /// Exact accessible name.
    pub name: Option<String>,
    /// Case-insensitive substring of the accessible name.
    pub name_contains: Option<String>,
    /// A role (`button`, `check_box`, `text_input`, ...) or the OS's own
    /// name for it (`Edit`), case-insensitive.
    pub role: Option<String>,
    /// Exact automation id.
    pub automation_id: Option<String>,
    /// Matches to return at most (default 50, at most 1000).
    pub limit: Option<u32>,
    /// `outline` (default) or `json`.
    #[serde(default)]
    pub format: Format,
}

/// Default for `find`'s `limit`.
pub const DEFAULT_LIMIT: usize = 50;
/// The most matches `find` returns.
pub const MAX_LIMIT: u32 = 1_000;

impl FindParams {
    fn query(&self) -> ToolResult<Query> {
        let query = Query {
            name: self.name.clone(),
            name_contains: self.name_contains.clone(),
            role: self.role.clone(),
            automation_id: self.automation_id.clone(),
            element: None,
        };
        if query.is_empty() {
            return Err(ToolError::InvalidArgument(
                "pass at least one of `name`, `name_contains`, `role`, `automation_id`".into(),
            ));
        }
        query.prepared()
    }

    /// The scope, a non-empty query, the limit and the format.
    pub fn validate(&self) -> ToolResult<(Scope, Query, usize, Format)> {
        let limit = match self.limit {
            None => DEFAULT_LIMIT,
            Some(0) => {
                return Err(ToolError::InvalidArgument(
                    "limit must be at least 1".into(),
                ));
            }
            Some(n) if n <= MAX_LIMIT => n as usize,
            Some(n) => {
                return Err(ToolError::InvalidArgument(format!(
                    "limit {n} is above {MAX_LIMIT}"
                )));
            }
        };
        Ok((
            scope(self.window.as_deref(), self.pid, self.root.as_deref())?,
            self.query()?,
            limit,
            self.format,
        ))
    }
}

/// The state `wait_for` waits for on a matching element; every given
/// field must hold.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StateArg {
    /// `true`, `false` or `"mixed"`.
    pub checked: Option<Checked>,
    /// Expanded (`true`) or collapsed.
    pub expanded: Option<bool>,
    /// Selected or not.
    pub selected: Option<bool>,
    /// Holding keyboard focus or not.
    pub focused: Option<bool>,
    /// Disabled (`true`) or enabled.
    pub disabled: Option<bool>,
    /// The exact value.
    pub value: Option<String>,
    /// A case-insensitive substring of the value.
    pub value_contains: Option<String>,
}

impl StateArg {
    /// Whether every given field holds for `node`.
    pub fn holds(&self, node: &Node) -> bool {
        // A default bool is not evidence that a failed property read was
        // false. Each requested state needs its own successful observation.
        for (field, requested) in [
            ("checked", self.checked.is_some()),
            ("expanded", self.expanded.is_some()),
            ("selected", self.selected.is_some()),
            ("focused", self.focused.is_some()),
            ("disabled", self.disabled.is_some()),
            (
                "value",
                self.value.is_some() || self.value_contains.is_some(),
            ),
        ] {
            if requested && node.unread_states.contains(&field) {
                return false;
            }
        }
        // A value predicate needs a value that was read: an element with
        // none, or whose value could not be read, matches neither
        // `value: ""` nor any substring. `value_contains` is folded already
        // ([`Self::prepared`]).
        let value = node.value.as_deref();
        self.checked.is_none_or(|c| node.checked == Some(c))
            && self.expanded.is_none_or(|e| node.expanded == Some(e))
            && self.selected.is_none_or(|s| node.selected == Some(s))
            && self.focused.is_none_or(|f| node.focused == f)
            && self.disabled.is_none_or(|d| node.disabled == d)
            && self.value.as_deref().is_none_or(|v| value == Some(v))
            && self
                .value_contains
                .as_deref()
                .is_none_or(|v| value.is_some_and(|value| crate::a11y::fold(value).contains(v)))
    }

    /// The state ready to test: value criteria at most
    /// [`crate::a11y::CLIPPED_CHARS`] characters (no reported value is
    /// longer), `value_contains` not empty and folded once here rather than
    /// for every node on every poll.
    fn prepared(mut self) -> ToolResult<Self> {
        for (field, value) in [
            ("value", &self.value),
            ("value_contains", &self.value_contains),
        ] {
            if value
                .as_deref()
                .is_some_and(|v| v.chars().count() > crate::a11y::CLIPPED_CHARS)
            {
                return Err(ToolError::InvalidArgument(format!(
                    "`state.{field}` is longer than {} characters, longer than any value a read reports",
                    crate::a11y::CLIPPED_CHARS
                )));
            }
        }
        if self.value_contains.as_deref() == Some("") {
            return Err(ToolError::InvalidArgument(
                "`state.value_contains` must not be empty; every value contains it".into(),
            ));
        }
        self.value_contains = self.value_contains.as_deref().map(crate::a11y::fold);
        Ok(self)
    }

    /// Whether any field is given.
    pub fn is_empty(&self) -> bool {
        self.checked.is_none()
            && self.expanded.is_none()
            && self.selected.is_none()
            && self.focused.is_none()
            && self.disabled.is_none()
            && self.value.is_none()
            && self.value_contains.is_none()
    }

    /// The fields, formatted for messages.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(c) = self.checked {
            parts.push(format!("checked: {c}"));
        }
        if let Some(e) = self.expanded {
            parts.push(format!("expanded: {e}"));
        }
        if let Some(s) = self.selected {
            parts.push(format!("selected: {s}"));
        }
        if let Some(f) = self.focused {
            parts.push(format!("focused: {f}"));
        }
        if let Some(d) = self.disabled {
            parts.push(format!("disabled: {d}"));
        }
        if let Some(v) = &self.value {
            parts.push(format!("value == {v:?}"));
        }
        if let Some(v) = &self.value_contains {
            parts.push(format!("value contains {v:?}"));
        }
        parts.join(" and ")
    }
}

/// `wait_for` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaitForParams {
    /// Watch this window.
    pub window: Option<String>,
    /// Watch all this process's top-level windows (and wait for one to
    /// exist); windows that appear or close meanwhile are followed.
    pub pid: Option<u32>,
    /// Watch the subtree under this element (`e12`) instead.
    pub root: Option<String>,
    /// One element by id (`e12`), instead of or as well as the criteria
    /// below: wait for its state, or for it to be gone.
    pub element: Option<String>,
    /// Exact accessible name.
    pub name: Option<String>,
    /// Case-insensitive substring of the accessible name.
    pub name_contains: Option<String>,
    /// A role (`button`, ...) or the OS's own name, case-insensitive.
    pub role: Option<String>,
    /// Exact automation id.
    pub automation_id: Option<String>,
    /// The state the matching element must be in.
    pub state: Option<StateArg>,
    /// Wait until no element matches (a dialog closed, an item deleted)
    /// instead of until one does.
    #[serde(default)]
    pub gone: bool,
    /// Give up after this many ms (default 5000, at most 120000).
    pub timeout_ms: Option<u64>,
}

const DEFAULT_WAIT_MS: u64 = 5_000;

/// What `wait_for` waits for.
#[derive(Debug, Clone)]
pub struct Wait {
    /// Where to look.
    pub scope: Scope,
    /// Which elements count.
    pub query: Query,
    /// The state a counting element must be in, if any.
    pub state: Option<StateArg>,
    /// Whether the wait ends when nothing counts, rather than when
    /// something does.
    pub gone: bool,
    /// The budget.
    pub timeout: Duration,
}

impl Wait {
    /// The condition, formatted for messages.
    pub fn describe(&self) -> String {
        let mut text = self.query.describe();
        if let Some(state) = &self.state {
            text = format!("{text} with {}", state.describe());
        }
        if self.gone {
            text = format!("no element matching {text}");
        }
        text
    }
}

impl WaitForParams {
    /// The wait's condition and budget.
    pub fn validate(&self) -> ToolResult<Wait> {
        let scope = scope(self.window.as_deref(), self.pid, self.root.as_deref())?;
        if let Some(e) = &self.element {
            parse_handle(e, HandleKind::Element)?;
        }
        let query = Query {
            name: self.name.clone(),
            name_contains: self.name_contains.clone(),
            role: self.role.clone(),
            automation_id: self.automation_id.clone(),
            element: self.element.clone(),
        };
        if query.is_empty() {
            return Err(ToolError::InvalidArgument(
                "pass `element`, or at least one of `name`, `name_contains`, `role`, `automation_id`"
                    .into(),
            ));
        }
        let state = self
            .state
            .clone()
            .filter(|s| !s.is_empty())
            .map(StateArg::prepared)
            .transpose()?;
        if self.gone && state.is_some() {
            return Err(ToolError::InvalidArgument(
                "`gone` waits for no match at all; pass either `gone` or `state`".into(),
            ));
        }
        let ms = self.timeout_ms.unwrap_or(DEFAULT_WAIT_MS);
        if ms > MAX_WAIT_MS {
            return Err(ToolError::InvalidArgument(format!(
                "timeout_ms {ms} is above {MAX_WAIT_MS}"
            )));
        }
        Ok(Wait {
            scope,
            query: query.prepared()?,
            state,
            gone: self.gone,
            timeout: Duration::from_millis(ms),
        })
    }
}

/// Arguments of the element actions (`invoke`, `toggle`, `focus`, `select`,
/// `expand`, `collapse`, `scroll_into_view`).
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
    /// The new value (at most 100000 characters); a number for range
    /// controls (sliders).
    pub value: String,
}

/// The most characters one `set_value` writes: the provider copies it
/// across processes, and the target application stores it, on the one
/// desktop thread.
pub const MAX_VALUE_CHARS: usize = 100_000;

impl SetValueParams {
    /// Refuses a value above [`MAX_VALUE_CHARS`], or one with a NUL (which
    /// the provider would cut at), before anything is sent.
    pub fn validate(&self) -> ToolResult<()> {
        let count = self.value.chars().count();
        if count > MAX_VALUE_CHARS {
            return Err(ToolError::InvalidArgument(format!(
                "value has {count} characters; at most {MAX_VALUE_CHARS}"
            )));
        }
        if self.value.contains('\0') {
            return Err(ToolError::InvalidArgument(
                "value must not contain NUL characters".into(),
            ));
        }
        Ok(())
    }
}

/// Mouse button names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, serde::Serialize, JsonSchema)]
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

/// `click` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClickParams {
    /// Click this element's clickable point. Its process must be in front
    /// and its own window under the point (a popup counts).
    pub element: Option<String>,
    /// Screen x (physical pixels on Windows, points on macOS), or image x
    /// with `screenshot`; with `y`, instead of `element`.
    pub x: Option<i32>,
    /// Screen y, or image y with `screenshot`; with `x`.
    pub y: Option<i32>,
    /// A screenshot id (`s2`): then `x`/`y` are pixels of that image, and
    /// the server maps them to the screen.
    pub screenshot: Option<String>,
    /// left (default), right or middle.
    #[serde(default)]
    pub button: ButtonArg,
    /// Double-click.
    #[serde(default)]
    pub double: bool,
    /// Safety target: refuse unless this window is in front and holds the
    /// point. Exactly one of `window` / `pid`.
    pub window: Option<String>,
    /// Safety target: refuse unless this process owns the foreground window
    /// and one of its windows holds the point.
    pub pid: Option<u32>,
}

impl ClickParams {
    /// Where to click, and the safety target.
    pub fn validate(&self) -> ToolResult<(Location, TargetArg)> {
        let target = required_target(self.window.as_deref(), self.pid)?;
        let at = location(
            self.element.as_deref(),
            self.x,
            self.y,
            self.screenshot.as_deref(),
        )?;
        Ok((at, target))
    }
}

/// `move_mouse` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveMouseParams {
    /// Move to this element's clickable point.
    pub element: Option<String>,
    /// Screen x, or image x with `screenshot`; with `y`.
    pub x: Option<i32>,
    /// Screen y, or image y with `screenshot`; with `x`.
    pub y: Option<i32>,
    /// A screenshot id (`s2`): then `x`/`y` are pixels of that image.
    pub screenshot: Option<String>,
}

impl MoveMouseParams {
    /// Where to move.
    pub fn validate(&self) -> ToolResult<Location> {
        location(
            self.element.as_deref(),
            self.x,
            self.y,
            self.screenshot.as_deref(),
        )
    }
}

/// `drag` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DragParams {
    /// Press here: `{"element": "e12"}`, `{"x": .., "y": ..}` or
    /// `{"screenshot": "s2", "x": .., "y": ..}`.
    pub from: LocationArg,
    /// Release here, in the same shape.
    pub to: LocationArg,
    /// How long the move takes (default 300 ms, at most 10000).
    pub duration_ms: Option<u64>,
    /// Safety target: refuse unless this window is in front and holds every
    /// point of the drag. Exactly one of `window` / `pid`.
    pub window: Option<String>,
    /// Safety target: refuse unless this process owns the foreground window
    /// and its windows hold every point of the drag.
    pub pid: Option<u32>,
}

impl DragParams {
    /// The endpoints, the safety target and the duration.
    pub fn validate(&self) -> ToolResult<(Location, Location, TargetArg, Duration)> {
        let target = required_target(self.window.as_deref(), self.pid)?;
        let ms = self.duration_ms.unwrap_or(300);
        if ms > 10_000 {
            return Err(ToolError::InvalidArgument(format!(
                "duration_ms {ms} is above 10000"
            )));
        }
        Ok((
            self.from.resolve()?,
            self.to.resolve()?,
            target,
            Duration::from_millis(ms),
        ))
    }
}

/// `scroll` arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScrollParams {
    /// Scroll with the pointer over this element.
    pub element: Option<String>,
    /// Screen x to scroll at, or image x with `screenshot`; with `y`.
    pub x: Option<i32>,
    /// Screen y to scroll at, or image y with `screenshot`; with `x`.
    pub y: Option<i32>,
    /// A screenshot id (`s2`): then `x`/`y` are pixels of that image.
    pub screenshot: Option<String>,
    /// Horizontal wheel notches (at most 100); positive scrolls right.
    #[serde(default)]
    pub dx: i32,
    /// Vertical wheel notches (at most 100); positive scrolls down.
    #[serde(default)]
    pub dy: i32,
    /// Safety target: refuse unless this window is in front and holds the
    /// point. Exactly one of `window` / `pid`.
    pub window: Option<String>,
    /// Safety target: refuse unless this process owns the foreground window
    /// and one of its windows holds the point.
    pub pid: Option<u32>,
}

impl ScrollParams {
    /// Where to scroll and the safety target; rejects a no-op scroll.
    pub fn validate(&self) -> ToolResult<(Location, TargetArg)> {
        let target = required_target(self.window.as_deref(), self.pid)?;
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
        let at = location(
            self.element.as_deref(),
            self.x,
            self.y,
            self.screenshot.as_deref(),
        )?;
        Ok((at, target))
    }
}

/// `type_text` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypeTextParams {
    /// Text to type into the focused control, as characters (at most 10000).
    /// A newline is sent as Enter and a tab as Tab.
    pub text: String,
    /// Safety target: refuse unless this window is in front and holds
    /// keyboard focus in its own process. Exactly one of `window` / `pid`.
    pub window: Option<String>,
    /// Safety target: refuse unless this process owns the foreground window
    /// and keyboard focus.
    pub pid: Option<u32>,
}

impl TypeTextParams {
    /// The safety target; the text is refused above [`MAX_TEXT_CHARS`]
    /// before anything is typed.
    pub fn validate(&self) -> ToolResult<TargetArg> {
        let target = required_target(self.window.as_deref(), self.pid)?;
        let count = self.text.chars().count();
        if count > MAX_TEXT_CHARS {
            return Err(ToolError::InvalidArgument(format!(
                "text has {count} characters; type at most {MAX_TEXT_CHARS} per call"
            )));
        }
        if self.text.contains('\0') {
            return Err(ToolError::InvalidArgument(
                "text must not contain NUL characters".into(),
            ));
        }
        Ok(target)
    }
}

/// `key` arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyParams {
    /// A key or combo: `enter`, `tab`, `ctrl+shift+s`, `alt+f4`, `cmd+q`.
    pub combo: String,
    /// Press it this many times (default 1, at most 100).
    pub repeat: Option<u32>,
    /// Safety target: refuse unless this window is in front and holds
    /// keyboard focus in its own process. Exactly one of `window` / `pid`.
    pub window: Option<String>,
    /// Safety target: refuse unless this process owns the foreground window
    /// and keyboard focus.
    pub pid: Option<u32>,
}

impl KeyParams {
    /// The parsed combo, repeat count and safety target.
    pub fn validate(&self) -> ToolResult<(KeyCombo, u32, TargetArg)> {
        let target = required_target(self.window.as_deref(), self.pid)?;
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
    pub window: Option<String>,
    /// Bring this process's frontmost window to the front.
    pub pid: Option<u32>,
}

impl ActivateParams {
    /// The target.
    pub fn validate(&self) -> ToolResult<TargetArg> {
        required_target(self.window.as_deref(), self.pid)
    }
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
        assert_eq!(
            required_target(Some("w7"), None).ok(),
            Some(TargetArg::Window(7))
        );
        assert_eq!(required_target(None, Some(9)).ok(), Some(TargetArg::Pid(9)));
        assert!(required_target(None, None).is_err());
        assert!(required_target(Some("w1"), Some(2)).is_err());
        assert!(
            required_target(Some("e1"), None).is_err(),
            "not a window id"
        );
        assert_eq!(optional_target(None, None).ok(), Some(None));
    }

    /// Input tools refuse to run without a safety target.
    #[test]
    fn input_tools_require_a_target() {
        assert!(
            parse::<ClickParams>(json!({"x": 1, "y": 2}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<TypeTextParams>(json!({"text": "x"}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<KeyParams>(json!({"combo": "enter"}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<ScrollParams>(json!({"x": 1, "y": 2, "dy": 1}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<DragParams>(json!({"from": {"x": 0, "y": 0}, "to": {"x": 5, "y": 5}}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<MoveMouseParams>(json!({"x": 1, "y": 2}))
                .validate()
                .is_ok(),
            "a move sends no press"
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let r: Result<KeyParams, _> =
            serde_json::from_value(json!({"combo": "enter", "windowId": 3}));
        assert!(r.is_err(), "a misspelled safety target must not be ignored");
    }

    /// Malformed arguments are kept as a readable tool error, with the same
    /// schema the arguments themselves have.
    #[test]
    fn args_keep_a_parse_failure_as_a_tool_error() {
        let bad: Args<KeyParams> = parse(json!({"combo": "enter", "windowId": 3}));
        let err = bad.0.expect_err("BUG: an unknown field fails");
        assert!(matches!(err, ToolError::InvalidArgument(_)), "{err}");
        assert!(err.to_string().contains("windowId"), "{err}");
        let good: Args<KeyParams> = parse(json!({"combo": "enter"}));
        assert!(good.0.is_ok());
        assert_eq!(
            schemars::schema_for!(Args<KeyParams>),
            schemars::schema_for!(KeyParams)
        );
    }

    /// The launch limit counts what the command line becomes: empty
    /// arguments still take separators and quotes, and backslashes their
    /// escapes.
    #[test]
    fn launch_limit_counts_encoding() {
        let empties: Vec<String> = vec![String::new(); MAX_LAUNCH_BYTES / 2];
        assert!(
            parse::<LaunchParams>(json!({"program": "x", "args": empties}))
                .validate()
                .is_err()
        );
        let slashes = "\\".repeat(MAX_LAUNCH_BYTES / 2 + 10);
        assert!(
            parse::<LaunchParams>(json!({"program": "x", "args": [slashes]}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<LaunchParams>(json!({"program": "x", "args": ["a", "b"]}))
                .validate()
                .is_ok()
        );
    }

    /// Oversized text is refused before anything is typed.
    #[test]
    fn type_text_is_capped() {
        let long = "a".repeat(MAX_TEXT_CHARS + 1);
        assert!(
            parse::<TypeTextParams>(json!({"text": long, "pid": 1}))
                .validate()
                .is_err()
        );
        let most = "я".repeat(MAX_TEXT_CHARS);
        assert!(
            parse::<TypeTextParams>(json!({"text": most, "pid": 1}))
                .validate()
                .is_ok(),
            "the cap counts characters, not bytes"
        );
    }

    #[test]
    fn a_location_is_an_element_a_point_or_a_screenshot_pixel() {
        let (at, target) = parse::<ClickParams>(json!({"element": "e3", "pid": 5}))
            .validate()
            .expect("BUG: element click is valid");
        assert_eq!(at, Location::Element("e3".into()));
        assert_eq!(target, TargetArg::Pid(5));
        let (at, _) = parse::<ClickParams>(json!({"x": 10, "y": -4, "window": "w2"}))
            .validate()
            .expect("BUG: point click is valid");
        assert_eq!(at, Location::Point(10, -4));
        let (at, _) =
            parse::<ClickParams>(json!({"screenshot": "s4", "x": 10, "y": 20, "window": "w2"}))
                .validate()
                .expect("BUG: screenshot pixel click is valid");
        assert_eq!(
            at,
            Location::InScreenshot {
                shot: 4,
                x: 10,
                y: 20
            }
        );
        for bad in [
            json!({"pid": 1}),
            json!({"x": 1, "pid": 1}),
            json!({"element": "e1", "x": 1, "y": 2, "pid": 1}),
            json!({"element": "w1", "pid": 1}),
            json!({"screenshot": "s1", "pid": 1}),
            json!({"element": "e1", "window": "w1", "pid": 2}),
        ] {
            assert!(
                parse::<ClickParams>(bad.clone()).validate().is_err(),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn click_button_names() {
        let p: ClickParams =
            parse(json!({"x": 0, "y": 0, "button": "right", "double": true, "pid": 1}));
        assert!(matches!(p.button, ButtonArg::Right));
        assert!(p.double);
        assert!(serde_json::from_value::<ClickParams>(json!({"button": "RIGHT"})).is_err());
    }

    #[test]
    fn find_requires_a_criterion_and_one_scope() {
        assert!(parse::<FindParams>(json!({"pid": 1})).validate().is_err());
        let (scope, query, limit, format) =
            parse::<FindParams>(json!({"window": "w4", "role": "Button"}))
                .validate()
                .expect("BUG: a role is a criterion");
        assert_eq!(scope, Scope::Target(TargetArg::Window(4)));
        assert_eq!(query.role.as_deref(), Some("Button"));
        assert_eq!((limit, format), (DEFAULT_LIMIT, Format::Outline));
        let (scope, ..) = parse::<FindParams>(json!({"root": "e9", "name": "OK"}))
            .validate()
            .expect("BUG: a subtree is a scope");
        assert_eq!(scope, Scope::Element("e9".into()));
        assert!(
            parse::<FindParams>(json!({"root": "e9", "pid": 1, "name": "OK"}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<FindParams>(json!({"pid": 1, "name": "OK", "limit": 0}))
                .validate()
                .is_err()
        );
    }

    /// A zero size limit is refused rather than read as "no limit".
    #[test]
    fn a_zero_screenshot_limit_is_refused() {
        assert!(
            parse::<ScreenshotParams>(json!({"max_side": 0}))
                .target()
                .is_err()
        );
        assert!(
            parse::<ScreenshotParams>(json!({"max_side": 4097}))
                .target()
                .is_err()
        );
        assert!(
            parse::<ScreenshotParams>(json!({"max_side": 1}))
                .target()
                .is_ok()
        );
    }

    /// A criterion longer than any reported string is refused; the
    /// substring is folded once.
    #[test]
    fn criteria_are_bounded_and_folded_once() {
        let long = "x".repeat(crate::a11y::CLIPPED_CHARS + 1);
        assert!(
            parse::<FindParams>(json!({"pid": 1, "name_contains": long}))
                .validate()
                .is_err()
        );
        let (_, query, ..) = parse::<FindParams>(json!({"pid": 1, "name_contains": "OK"}))
            .validate()
            .expect("BUG: a short substring is valid");
        assert_eq!(query.name_contains.as_deref(), Some("ok"));
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
            parse::<ScrollParams>(json!({"x": 0, "y": 0, "dy": i32::MIN, "pid": 1}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn wait_for_bounds_the_timeout_and_takes_a_state_or_gone() {
        let wait = parse::<WaitForParams>(json!({"pid": 1, "name": "OK"}))
            .validate()
            .expect("BUG: default timeout is valid");
        assert_eq!(wait.timeout, Duration::from_millis(DEFAULT_WAIT_MS));
        assert!(
            parse::<WaitForParams>(json!({"pid": 1, "name": "OK", "timeout_ms": 999_999}))
                .validate()
                .is_err()
        );
        let wait = parse::<WaitForParams>(
            json!({"window": "w1", "element": "e5", "state": {"checked": true, "value_contains": "Ab"}}),
        )
        .validate()
        .expect("BUG: an element with a state is valid");
        assert_eq!(wait.query.element.as_deref(), Some("e5"));
        assert!(
            wait.describe().contains("checked: checked"),
            "{}",
            wait.describe()
        );
        let wait = parse::<WaitForParams>(json!({"pid": 1, "role": "dialog", "gone": true}))
            .validate()
            .expect("BUG: gone is valid");
        assert!(wait.gone);
        assert!(
            wait.describe().starts_with("no element"),
            "{}",
            wait.describe()
        );
        assert!(
            parse::<WaitForParams>(
                json!({"pid": 1, "role": "dialog", "gone": true, "state": {"focused": true}})
            )
            .validate()
            .is_err(),
            "gone and a state contradict"
        );
        assert!(
            parse::<WaitForParams>(json!({"element": "e5"}))
                .validate()
                .is_err(),
            "a scope is still needed"
        );
    }

    /// A value predicate needs a value that was read, and its criteria are
    /// bounded and folded once.
    #[test]
    fn state_value_needs_a_read_value() {
        let wait = parse::<WaitForParams>(
            json!({"pid": 1, "role": "text_input", "state": {"value_contains": "STRASSE"}}),
        )
        .validate()
        .expect("BUG: valid");
        let state = wait.state.expect("BUG: a state");
        assert_eq!(state.value_contains.as_deref(), Some("strasse"));
        let mut node = crate::a11y::tests_node();
        assert!(!state.holds(&node), "no value read");
        node.value = Some("Hauptstraße".into());
        assert!(state.holds(&node));
        let empty = StateArg {
            value: Some(String::new()),
            ..StateArg::default()
        };
        node.value = None;
        assert!(!empty.holds(&node), "an unread value is not the empty one");
        let long = "x".repeat(crate::a11y::CLIPPED_CHARS + 1);
        assert!(
            parse::<WaitForParams>(
                json!({"pid": 1, "role": "x", "state": {"value_contains": long}})
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn state_predicates_require_the_requested_observation() {
        let mut node = crate::a11y::tests_node();
        node.focused = false;
        node.disabled = false;
        node.unread_states = vec!["focused"];
        let unfocused = StateArg {
            focused: Some(false),
            ..StateArg::default()
        };
        let enabled = StateArg {
            disabled: Some(false),
            ..StateArg::default()
        };
        assert!(!unfocused.holds(&node), "an unread focus flag is not false");
        assert!(
            enabled.holds(&node),
            "the independent enabled observation is known"
        );
        node.unread_states = vec!["disabled"];
        assert!(unfocused.holds(&node));
        assert!(!enabled.holds(&node));
        node.value = Some("clipped…".into());
        node.unread_states = vec!["value"];
        let substring = StateArg {
            value_contains: Some("…".into()),
            ..StateArg::default()
        };
        assert!(
            !substring.holds(&node),
            "a clipping marker is not the control's value"
        );
    }

    #[test]
    fn tree_depth_and_nodes_default_and_cap() {
        let (_, d, n, f) = parse::<TreeParams>(json!({"pid": 1}))
            .validate()
            .expect("BUG: defaults are valid");
        assert_eq!((d, n, f), (DEFAULT_DEPTH, DEFAULT_NODES, Format::Outline));
        assert!(
            parse::<TreeParams>(json!({"pid": 1, "max_depth": 10_000}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<TreeParams>(json!({"pid": 1, "max_nodes": 10_000}))
                .validate()
                .is_err()
        );
        let (_, _, _, f) = parse::<TreeParams>(json!({"window": "w1", "format": "json"}))
            .validate()
            .expect("BUG: json is a format");
        assert_eq!(f, Format::Json);
    }

    #[test]
    fn screenshot_takes_at_most_one_target() {
        assert_eq!(
            ScreenshotParams::default().target().ok(),
            Some(ScreenshotTarget::Primary)
        );
        let p: ScreenshotParams = parse(json!({"monitor": 1}));
        assert_eq!(p.target().ok(), Some(ScreenshotTarget::Monitor(1)));
        let p: ScreenshotParams = parse(json!({"window": "w3"}));
        assert_eq!(p.target().ok(), Some(ScreenshotTarget::Window(3)));
        let p: ScreenshotParams = parse(json!({"monitor": 1, "window": "w3"}));
        assert!(p.target().is_err());
    }

    #[test]
    fn scroll_rejects_no_op_and_huge_scrolls() {
        assert!(
            parse::<ScrollParams>(json!({"x": 0, "y": 0, "pid": 1}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<ScrollParams>(json!({"x": 0, "y": 0, "dy": 1000, "pid": 1}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<ScrollParams>(json!({"element": "e2", "dy": -3, "pid": 1}))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn key_validates_combo_and_repeat() {
        let (combo, repeat, _) =
            parse::<KeyParams>(json!({"combo": "ctrl+s", "repeat": 3, "window": "w1"}))
                .validate()
                .expect("BUG: valid key args");
        assert_eq!(combo.modifiers.len(), 1);
        assert_eq!(repeat, 3);
        assert!(
            parse::<KeyParams>(json!({"combo": "ctrl+s", "repeat": 0, "window": "w1"}))
                .validate()
                .is_err()
        );
        assert!(
            parse::<KeyParams>(json!({"combo": "ctrl+", "window": "w1"}))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn drag_bounds_the_duration_and_takes_elements() {
        let p: DragParams =
            parse(json!({"from": {"x": 0, "y": 0}, "to": {"element": "e4"}, "pid": 1}));
        let (from, to, _, d) = p.validate().expect("BUG: valid drag");
        assert_eq!(from, Location::Point(0, 0));
        assert_eq!(to, Location::Element("e4".into()));
        assert_eq!(d, Duration::from_millis(300));
        let p: DragParams = parse(
            json!({"from": {"x": 0, "y": 0}, "to": {"x": 5, "y": 5}, "duration_ms": 60_000, "pid": 1}),
        );
        assert!(p.validate().is_err());
    }

    #[test]
    fn wait_for_window_defaults() {
        let (pid, needle, timeout) = parse::<WaitForWindowParams>(json!({"pid": 7}))
            .validate()
            .expect("BUG: valid");
        assert_eq!((pid, needle), (7, None));
        assert_eq!(timeout, Duration::from_millis(DEFAULT_WINDOW_WAIT_MS));
        let (_, needle, _) =
            parse::<WaitForWindowParams>(json!({"pid": 7, "title_contains": "Straße"}))
                .validate()
                .expect("BUG: valid");
        assert_eq!(needle.as_deref(), Some("strasse"));
    }
}
