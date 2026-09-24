//! The MCP surface: one `#[tool]` per operation, each validating its
//! arguments and handing the work to the desktop thread.

use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::Serialize;
use serde_json::{Value, json};

use crate::a11y::{self, Action};
use crate::desktop::Desktop;
use crate::error::{ToolError, ToolResult};
use crate::params::{
    ActivateParams, ClickParams, DragParams, ElementParams, FindParams, KeyParams, KillParams,
    LaunchParams, ListWindowsParams, MoveMouseParams, ScreenshotParams, ScrollParams,
    SetValueParams, Target, TreeParams, TypeTextParams, WaitForParams, required_target,
};
use crate::process::{Children, LaunchSpec};
use crate::worker::Worker;

/// How often `wait_for` and `launch` re-check.
const POLL: Duration = Duration::from_millis(250);

const INSTRUCTIONS: &str = "\
Drives desktop applications like a person with a screen reader. Coordinates are physical \
screen pixels everywhere (window rects, element rects, screenshots at scale 1, input).

Typical loop: list_windows (or launch) -> activate_window -> accessibility_tree / find / \
screenshot -> invoke / toggle / set_value (preferred: no pointer, works when covered) or \
click / type_text / key -> read the tree again to confirm.

Safety: always pass window_id or pid to click, drag, scroll, type_text and key. The server \
then refuses the input unless that window is in front (and, for coordinates, holds the \
point uncovered), so keystrokes never land in another application. Element clicks always \
require the element's own window to be in front.

Element ids (e12) are session handles from accessibility_tree, find and wait_for; the same \
element keeps its id across reads. Accessibility tools use UI Automation and are Windows-only \
for now. Window listing, screenshots and input work on Windows and macOS (on macOS, \
coordinate input with a safety target is refused: the server cannot yet verify what covers a \
point); on Linux none of them is available yet.";

/// The MCP server.
#[derive(Debug, Clone)]
pub struct DesktopServer {
    worker: Worker,
    children: Arc<Children>,
    tool_router: ToolRouter<Self>,
}

/// Wraps a result for the agent: structured JSON on success, a readable
/// tool error (not a protocol error) on failure.
fn respond<T: Serialize>(result: ToolResult<T>) -> CallToolResult {
    match result.and_then(|v| {
        serde_json::to_value(v).map_err(|e| ToolError::platform("serializing the reply", e))
    }) {
        Ok(value) => CallToolResult::structured(value),
        Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
    }
}

fn element_reply(result: ToolResult<a11y::Node>) -> CallToolResult {
    respond(result.map(|node| json!({ "element": node })))
}

impl DesktopServer {
    /// A server over a running desktop thread.
    pub fn new(worker: Worker, children: Arc<Children>) -> Self {
        Self {
            worker,
            children,
            tool_router: Self::tool_router(),
        }
    }

    async fn act(&self, element: String, action: Action) -> CallToolResult {
        element_reply(self.worker.run(move |d| d.act(&element, &action)).await)
    }
}

#[tool_router]
impl DesktopServer {
    #[tool(
        description = "List top-level windows: id (pass as window_id), pid, app_name, title, rect (physical px), is_minimized, is_focused. Front to back.",
        annotations(read_only_hint = true)
    )]
    async fn list_windows(&self, Parameters(p): Parameters<ListWindowsParams>) -> CallToolResult {
        let windows = self
            .worker
            .run(move |d| d.list_windows(p.title_contains.as_deref(), p.pid))
            .await;
        respond(windows.map(|w| json!({ "windows": w })))
    }

    #[tool(
        description = "Start a program (stdio discarded). Returns its pid; with wait_for_window_ms (at most 120000), also its first window. The server kills every launched process when it exits."
    )]
    async fn launch(&self, Parameters(p): Parameters<LaunchParams>) -> CallToolResult {
        if let Err(e) = p.validate() {
            return respond::<Value>(Err(e));
        }
        let spec = LaunchSpec {
            program: p.program,
            args: p.args,
            cwd: p.cwd.map(Into::into),
            env: p.env.into_iter().collect(),
        };
        let pid = match self.children.launch(&spec) {
            Ok(pid) => pid,
            Err(e) => return respond::<Value>(Err(e)),
        };
        let Some(ms) = p.wait_for_window_ms else {
            return respond(Ok(json!({ "pid": pid })));
        };
        let deadline = Instant::now() + Duration::from_millis(ms);
        loop {
            let windows = self
                .worker
                .run(move |d| d.list_windows(None, Some(pid)))
                .await;
            match windows {
                Ok(w) if !w.is_empty() => {
                    return respond(Ok(json!({ "pid": pid, "window": w[0] })));
                }
                // The process is running and only this session can end it:
                // hand back its pid with the reason instead of losing it.
                Err(e) => {
                    return respond(Ok(json!({
                        "pid": pid,
                        "window": null,
                        "note": format!("started, but its window cannot be listed: {e}; kill it by pid if needed"),
                    })));
                }
                Ok(_) if Instant::now() >= deadline => {
                    return respond(Ok(json!({
                        "pid": pid,
                        "window": null,
                        "note": "no window for this pid yet; it may hand off to another process, see list_windows",
                    })));
                }
                Ok(_) => tokio::time::sleep(POLL).await,
            }
        }
    }

    #[tool(
        description = "End a process started by launch in this session (other pids are refused)."
    )]
    async fn kill(&self, Parameters(p): Parameters<KillParams>) -> CallToolResult {
        respond(self.children.kill(p.pid))
    }

    #[tool(
        description = "Capture a window (window_id or pid; covered windows are captured where the OS allows), a monitor (0-based index), or the primary monitor (no target). Returns a PNG plus its size, the captured screen rect, and scale (image px per screen px).",
        annotations(read_only_hint = true)
    )]
    async fn screenshot(&self, Parameters(p): Parameters<ScreenshotParams>) -> CallToolResult {
        let target = match p.target() {
            Ok(t) => t,
            Err(e) => return respond::<Value>(Err(e)),
        };
        let max_side = p.max_side;
        match self
            .worker
            .run(move |_| Desktop::screenshot(target, max_side))
            .await
        {
            Ok(shot) => {
                let meta = json!({
                    "width": shot.width,
                    "height": shot.height,
                    "source": shot.source,
                    "scale": shot.scale,
                });
                CallToolResult::success(vec![
                    ContentBlock::image(
                        base64::engine::general_purpose::STANDARD.encode(&shot.png),
                        "image/png",
                    ),
                    ContentBlock::text(meta.to_string()),
                ])
            }
            Err(e) => respond::<Value>(Err(e)),
        }
    }

    #[tool(
        description = "Read the accessibility tree of a window (window_id) or of all a process's windows (pid). Nodes: id (e12, for later calls), role, name, value, automation_id, class_name, rect, enabled, has_keyboard_focus, is_keyboard_focusable, toggle_state, patterns, children.",
        annotations(read_only_hint = true)
    )]
    async fn accessibility_tree(&self, Parameters(p): Parameters<TreeParams>) -> CallToolResult {
        let (target, depth) = match p.validate() {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        let roots = self.worker.run(move |d| d.tree(target, depth)).await;
        respond(roots.map(|r| json!({ "roots": r })))
    }

    #[tool(
        description = "Find elements in a window or process by name (exact), name_contains (case-insensitive), role (control type, e.g. Button) and/or automation_id; all given criteria must match. Returns a flat list of nodes without children.",
        annotations(read_only_hint = true)
    )]
    async fn find(&self, Parameters(p): Parameters<FindParams>) -> CallToolResult {
        let (target, query) = match p.validate() {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        let found = self.worker.run(move |d| d.find(target, &query)).await;
        respond(found.map(|(matches, _)| json!({ "matches": matches })))
    }

    #[tool(
        description = "Wait until an element matching the criteria (as for find) exists, polling every 250 ms. Returns the first match, or a timeout error listing the last tree seen."
    )]
    async fn wait_for(&self, Parameters(p): Parameters<WaitForParams>) -> CallToolResult {
        let (target, query, timeout) = match p.validate() {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        let deadline = Instant::now() + timeout;
        let mut summary = String::from("(no tree read: the target had no window)");
        loop {
            let q = query.clone();
            match self.worker.run(move |d| d.find(target, &q)).await {
                Ok((matches, roots)) => {
                    if let Some(first) = matches.into_iter().next() {
                        return element_reply(Ok(first));
                    }
                    summary = a11y::summarize(&roots, 60);
                }
                // A pid with no window yet is worth waiting for.
                Err(ToolError::NotFound(_)) if matches!(target, Target::Pid(_)) => {}
                Err(e) => return respond::<Value>(Err(e)),
            }
            if Instant::now() >= deadline {
                return respond::<Value>(Err(ToolError::Timeout {
                    timeout_ms: timeout.as_millis() as u64,
                    what: query.describe(),
                    summary,
                }));
            }
            tokio::time::sleep(POLL).await;
        }
    }

    #[tool(
        description = "Invoke (press) an element through its Invoke pattern. No pointer involved; works even when the window is covered."
    )]
    async fn invoke(&self, Parameters(p): Parameters<ElementParams>) -> CallToolResult {
        self.act(p.element, Action::Invoke).await
    }

    #[tool(
        description = "Flip a checkbox/switch through its Toggle pattern. Returns the element with its new toggle_state."
    )]
    async fn toggle(&self, Parameters(p): Parameters<ElementParams>) -> CallToolResult {
        self.act(p.element, Action::Toggle).await
    }

    #[tool(
        description = "Replace an element's value through its Value pattern (text fields), or RangeValue (sliders; value must be a number). Returns the element afterwards."
    )]
    async fn set_value(&self, Parameters(p): Parameters<SetValueParams>) -> CallToolResult {
        self.act(p.element, Action::SetValue(p.value)).await
    }

    #[tool(description = "Give an element keyboard focus through the accessibility API.")]
    async fn focus(&self, Parameters(p): Parameters<ElementParams>) -> CallToolResult {
        self.act(p.element, Action::Focus).await
    }

    #[tool(
        description = "Select an item (list item, tab, radio button) through its SelectionItem pattern."
    )]
    async fn select(&self, Parameters(p): Parameters<ElementParams>) -> CallToolResult {
        self.act(p.element, Action::Select).await
    }

    #[tool(
        description = "Real mouse click at an element's clickable point (element) or a screen point (x, y). Pass window_id or pid: the click is refused unless that window is in front and holds the point. button: left|right|middle; double for a double-click."
    )]
    async fn click(&self, Parameters(p): Parameters<ClickParams>) -> CallToolResult {
        let (at, target) = match p.validate() {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        let (button, double) = (p.button.into(), p.double);
        respond(
            self.worker
                .run(move |d| d.click(&at, button, double, target))
                .await,
        )
    }

    #[tool(
        description = "Move the pointer to a screen point (physical pixels). Returns where the pointer ended up."
    )]
    async fn move_mouse(&self, Parameters(p): Parameters<MoveMouseParams>) -> CallToolResult {
        respond(self.worker.run(move |d| d.move_mouse(p.x, p.y)).await)
    }

    #[tool(
        description = "Left-button drag from one screen point to another over duration_ms. Pass window_id or pid: refused unless that window is in front and holds both points."
    )]
    async fn drag(&self, Parameters(p): Parameters<DragParams>) -> CallToolResult {
        let (target, duration) = match p.validate() {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        let (from, to) = ((p.from.x, p.from.y), (p.to.x, p.to.y));
        respond(
            self.worker
                .run(move |d| d.drag(from, to, duration, target))
                .await,
        )
    }

    #[tool(
        description = "Scroll the wheel at a screen point: dy notches (positive = down), dx notches (positive = right). Pass window_id or pid: refused unless that window is in front and holds the point."
    )]
    async fn scroll(&self, Parameters(p): Parameters<ScrollParams>) -> CallToolResult {
        let target = match p.validate() {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        respond(
            self.worker
                .run(move |d| d.scroll(p.x, p.y, p.dx, p.dy, target))
                .await,
        )
    }

    #[tool(
        description = "Type text into the focused control as real keystrokes (layout-independent characters). Pass window_id or pid: refused unless that window is in front."
    )]
    async fn type_text(&self, Parameters(p): Parameters<TypeTextParams>) -> CallToolResult {
        let target = match crate::params::optional_target(p.window_id, p.pid) {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        respond(self.worker.run(move |d| d.type_text(&p.text, target)).await)
    }

    #[tool(
        description = "Press a key or combo as real keystrokes: enter, tab, esc, f5, ctrl+shift+s, alt+f4, cmd+q (modifiers: ctrl shift alt meta/win/cmd). repeat presses it N times. Pass window_id or pid: refused unless that window is in front."
    )]
    async fn key(&self, Parameters(p): Parameters<KeyParams>) -> CallToolResult {
        let (combo, repeat, target) = match p.validate() {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        respond(
            self.worker
                .run(move |d| d.key(&combo, repeat, target))
                .await,
        )
    }

    #[tool(
        description = "Bring a window (window_id) or a process's window (pid) to the foreground, restoring it if minimized. Reports became_foreground: Windows can refuse focus changes, so check it before sending input."
    )]
    async fn activate_window(&self, Parameters(p): Parameters<ActivateParams>) -> CallToolResult {
        let target = match required_target(p.window_id, p.pid) {
            Ok(v) => v,
            Err(e) => return respond::<Value>(Err(e)),
        };
        respond(self.worker.run(move |d| d.activate(target)).await)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DesktopServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                    .with_title("Desktop automation"),
            )
            .with_instructions(INSTRUCTIONS)
    }
}
